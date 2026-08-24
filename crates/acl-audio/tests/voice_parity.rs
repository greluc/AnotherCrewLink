#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    // One field per setting the client sends, so the correspondence stays readable.
    clippy::struct_excessive_bools
)]
//! Gate G2, second criterion: the voice decision against the Electron client's.
//!
//! > `voice_params` matches the Electron implementation on every recorded tuple.
//!
//! The tuples come from `src/renderer/voiceRecorder.ts`, which records what the decision
//! was asked and what it answered — the answer read back off the Web Audio nodes rather
//! than captured inside the function, so it is what reached the graph.
//!
//! The criterion names gain and pan position, and those are compared exactly. The muffle
//! and the reverb are compared too, and reported separately: they are useful signal, but
//! the client carries them as node state that outlives one call, so a difference there is
//! a question about stickiness rather than about the decision.
//!
//! # Running it
//!
//! Put `.ndjson` or `.ndjson.gz` files in `test/voice/` and run `cargo test -p acl-audio`.
//! Without any it skips, loudly: a gate that quietly passes having compared nothing is
//! worse than one that fails.

use std::io::Read;
use std::path::{Path, PathBuf};

use acl_audio::voice::{ClientSettings, GameState, LobbySettings, Player, State, voice_params};
use acl_types::map::{CameraLocation, MapType, Vector2};
use flate2::read::GzDecoder;
use serde::Deserialize;

/// How far a gain or a coordinate may differ. JSON round-trips a float through decimal,
/// and the client's own values are `f32` widened to `f64` on the way out.
const TOLERANCE: f64 = 1e-6;

#[derive(Debug, Deserialize)]
struct Tuple {
    inputs: Inputs,
    outputs: Outputs,
}

#[derive(Debug, Deserialize)]
struct Inputs {
    #[serde(rename = "gameState")]
    game_state: u32,
    map: u32,
    #[serde(rename = "closedDoors")]
    closed_doors: Vec<u32>,
    #[serde(rename = "comsSabotaged")]
    coms_sabotaged: bool,
    #[serde(rename = "currentCamera")]
    current_camera: u32,
    #[serde(rename = "lightRadiusChanged")]
    light_radius_changed: bool,
    #[serde(rename = "maxDistance")]
    max_distance: f64,
    /// The client id the radio is tuned to.
    ///
    /// Signed, and read as an `i64`: the client carries -1 for "nobody" rather than a
    /// null, and a `u32` would refuse the tuple over a sentinel.
    #[serde(rename = "impostorRadio")]
    impostor_radio: Option<i64>,
    #[serde(rename = "ghostVolumeAsImpostor")]
    ghost_volume_as_impostor: f32,
    #[serde(rename = "enableSpatialAudio")]
    enable_spatial_audio: bool,
    lobby: Lobby,
    me: RecordedPlayer,
    other: RecordedPlayer,
}

#[derive(Debug, Deserialize)]
struct Lobby {
    haunting: bool,
    #[serde(rename = "hearImpostorsInVents")]
    hear_impostors_in_vents: bool,
    #[serde(rename = "impostersHearImpostersInvent")]
    impostors_hear_impostors_in_vent: bool,
    #[serde(rename = "impostorRadioEnabled")]
    impostor_radio_enabled: bool,
    #[serde(rename = "commsSabotage")]
    coms_sabotage: bool,
    #[serde(rename = "deadOnly")]
    dead_only: bool,
    #[serde(rename = "meetingGhostOnly")]
    meeting_ghost_only: bool,
    #[serde(rename = "hearThroughCameras")]
    hear_through_cameras: bool,
    #[serde(rename = "wallsBlockAudio")]
    walls_block_audio: bool,
    #[serde(rename = "visionHearing")]
    vision_hearing: bool,
    #[serde(rename = "maxDistance")]
    max_distance: f64,
}

#[derive(Debug, Deserialize)]
struct RecordedPlayer {
    #[serde(rename = "clientId")]
    client_id: u32,
    x: f64,
    y: f64,
    #[serde(rename = "isDead")]
    is_dead: bool,
    #[serde(rename = "isImpostor")]
    is_impostor: bool,
    #[serde(rename = "inVent")]
    in_vent: bool,
    disconnected: bool,
    #[serde(rename = "isDummy")]
    is_dummy: bool,
}

#[derive(Debug, Deserialize)]
struct Outputs {
    gain: f64,
    /// Null when the decision returned before placing the peer.
    #[serde(rename = "panX")]
    pan_x: Option<f64>,
    #[serde(rename = "panY")]
    pan_y: Option<f64>,
    muffle: Option<RecordedMuffle>,
    reverb: bool,
}

#[derive(Debug, Deserialize)]
struct RecordedMuffle {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    kind: String,
    frequency: f64,
    q: f64,
}

impl From<&RecordedPlayer> for Player {
    fn from(recorded: &RecordedPlayer) -> Self {
        Self {
            client_id: recorded.client_id,
            position: Vector2 {
                x: recorded.x,
                y: recorded.y,
            },
            is_dead: recorded.is_dead,
            is_impostor: recorded.is_impostor,
            in_vent: recorded.in_vent,
            disconnected: recorded.disconnected,
            is_dummy: recorded.is_dummy,
        }
    }
}

/// The client's own numbering, which is not the game's — see `GameState::from_repr`.
fn game_state(value: u32) -> GameState {
    match value {
        0 => GameState::Lobby,
        1 => GameState::Tasks,
        2 => GameState::Discussion,
        3 => GameState::Menu,
        _ => GameState::Unknown,
    }
}

fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/voice")
}

fn files() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "ndjson" || extension == "gz")
        })
        .collect();
    found.sort();
    found
}

fn read(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("a recording");
    if path.extension().is_some_and(|extension| extension == "gz") {
        let mut text = String::new();
        GzDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .expect("a gzipped recording");
        text
    } else {
        String::from_utf8(bytes).expect("a recording is UTF-8")
    }
}

#[test]
fn the_voice_decision_agrees_with_the_electron_one() {
    let paths = files();
    if paths.is_empty() {
        eprintln!(
            "skipping gate G2's second criterion: no tuples in {}.\n\
             Record with `set ACL_RECORD=<name>` before starting the Electron client, then \
             copy userData/recordings/*.voice.ndjson there. The same variable records the \
             memory frames gate G1 needs, so one session produces both.",
            directory().display()
        );
        return;
    }

    let mut compared = 0usize;
    let mut differing = 0usize;
    let mut effects_differ = 0usize;
    // Of those, how many are frames where the peer went silent — the case the
    // stickiness explanation predicts, and the one that would make it a wrong guess if
    // the numbers did not agree.
    let mut effects_differ_while_silent = 0usize;
    let mut first = String::new();

    for path in &paths {
        for line in read(path).lines().filter(|line| !line.trim().is_empty()) {
            let recorded: Tuple = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));

            let state = State {
                game_state: game_state(recorded.inputs.game_state),
                map: MapType::from_game(recorded.inputs.map),
                closed_doors: recorded.inputs.closed_doors.clone(),
                coms_sabotaged: recorded.inputs.coms_sabotaged,
                current_camera: CameraLocation::from_state(recorded.inputs.current_camera),
                light_radius_changed: recorded.inputs.light_radius_changed,
            };
            let settings = ClientSettings {
                ghost_volume_as_impostor: recorded.inputs.ghost_volume_as_impostor,
                enable_spatial_audio: recorded.inputs.enable_spatial_audio,
            };
            let lobby = LobbySettings {
                haunting: recorded.inputs.lobby.haunting,
                hear_impostors_in_vents: recorded.inputs.lobby.hear_impostors_in_vents,
                impostors_hear_impostors_in_vent: recorded
                    .inputs
                    .lobby
                    .impostors_hear_impostors_in_vent,
                impostor_radio_enabled: recorded.inputs.lobby.impostor_radio_enabled,
                coms_sabotage: recorded.inputs.lobby.coms_sabotage,
                dead_only: recorded.inputs.lobby.dead_only,
                meeting_ghost_only: recorded.inputs.lobby.meeting_ghost_only,
                hear_through_cameras: recorded.inputs.lobby.hear_through_cameras,
                walls_block_audio: recorded.inputs.lobby.walls_block_audio,
                vision_hearing: recorded.inputs.lobby.vision_hearing,
                max_distance: recorded.inputs.lobby.max_distance,
            };

            let ours = voice_params(
                &state,
                &settings,
                &lobby,
                &Player::from(&recorded.inputs.me),
                &Player::from(&recorded.inputs.other),
                recorded.inputs.max_distance,
                recorded
                    .inputs
                    .impostor_radio
                    .filter(|id| *id >= 0)
                    .and_then(|id| u32::try_from(id).ok()),
            );

            compared += 1;

            let mut problems = Vec::new();
            if (f64::from(ours.gain) - recorded.outputs.gain).abs() > TOLERANCE {
                problems.push(format!(
                    "gain: electron={} rust={}",
                    recorded.outputs.gain, ours.gain
                ));
            }
            // Both sides say whether they placed the peer at all, so a disagreement about
            // *that* is caught rather than papered over by comparing a leftover.
            match (recorded.outputs.pan_x, recorded.outputs.pan_y) {
                (Some(x), Some(y)) if ours.placed => {
                    if (ours.pan.x - x).abs() > TOLERANCE || (ours.pan.y - y).abs() > TOLERANCE {
                        problems.push(format!(
                            "pan: electron=({x}, {y}) rust=({}, {})",
                            ours.pan.x, ours.pan.y
                        ));
                    }
                }
                (None, None) if !ours.placed => {}
                (theirs_x, _) => problems.push(format!(
                    "placed: electron={} rust={}",
                    theirs_x.is_some(),
                    ours.placed
                )),
            }

            if problems.is_empty() {
                // The criterion is gain and pan. These two are node state that outlives a
                // call in the client, so they are counted apart rather than failing it.
                let muffle_differs = match (&ours.muffle, &recorded.outputs.muffle) {
                    (None, None) => false,
                    (Some(ours), Some(theirs)) => {
                        (f64::from(ours.frequency) - theirs.frequency).abs() > TOLERANCE
                            || (f64::from(ours.q) - theirs.q).abs() > TOLERANCE
                    }
                    _ => true,
                };
                if muffle_differs || ours.reverb != recorded.outputs.reverb {
                    effects_differ += 1;
                    if !ours.placed {
                        effects_differ_while_silent += 1;
                    }
                }
                continue;
            }

            differing += 1;
            if first.is_empty() {
                first = format!(
                    "{}: {}\n  inputs: {:?}",
                    path.display(),
                    problems.join("; "),
                    recorded.inputs
                );
            }
        }
    }

    assert!(compared > 0, "the recordings held no tuples");
    assert_eq!(
        differing, 0,
        "gate G2: {differing} of {compared} tuples differ in gain or pan.\n{first}"
    );

    // The explanation, asserted rather than offered. An effect difference on a frame the
    // decision ran to the end of is not stickiness, and would want looking at.
    assert_eq!(
        effects_differ,
        effects_differ_while_silent,
        "{} effect differences are on frames the decision completed, which stickiness does not explain",
        effects_differ - effects_differ_while_silent
    );

    eprintln!(
        "gate G2 (voice): {compared} tuples, no difference in gain or pan. {effects_differ} differ in the muffle or reverb, all of them on a frame where the decision returned early -- the client leaves the graph alone there and keeps the effect it had."
    );
}
