//! What the parity corpus actually reaches, measured rather than described.
//!
//! `test/recordings/README.md` says which situations each recording covers, and
//! `docs/rust-port/04-implementation-plan.md` says which are still missing. Both are
//! prose, and prose about a corpus goes stale the moment a recording is added or
//! replaced — this project has already had a status note say "the corpus is empty" for a
//! day after it stopped being empty.
//!
//! So this reads the recordings and reports. It exists for two moments:
//!
//! **Before a recording session**, to say what is worth going after. **After one**, to say
//! whether the session actually captured it — which is otherwise only discoverable by
//! reading a parity diff that says nothing, because a branch neither reader reaches
//! compares equal on both sides.
//!
//! Run it with output:
//!
//! ```text
//! cargo test -p acl-game --test corpus_coverage -- --nocapture
//! ```
//!
//! It asserts only what the corpus reaches **today**, so a recording removed or replaced
//! by a weaker one fails here. It does not assert what is missing: that is the work, and
//! a test that failed until the work was done would be a red suite for months.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// What one pass over the corpus found.
// One flag per situation, named. A bitfield or a set of strings would make the report
// shorter and the compiler's help with a typo disappear, and this file exists to be read
// by somebody deciding what to record.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
struct Coverage {
    frames: usize,
    game_states: BTreeSet<&'static str>,
    maps: BTreeSet<i64>,
    /// Situations that only a live round produces.
    comms_sabotaged: bool,
    doors_closed: bool,
    /// Distinct `currentCamera` values. One means the field never varied, so it is
    /// compared but never exercised.
    cameras: BTreeSet<i64>,
    anybody_dead: bool,
    anybody_in_vent: bool,
    anybody_an_impostor: bool,
    light_radius_varied: bool,
}

fn recordings_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/recordings")
        .canonicalize()
        .expect("the recordings directory is beside the crates")
}

fn read_recording(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("a readable recording");
    if path.extension().is_some_and(|e| e == "gz") {
        let mut text = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .expect("a readable gzip stream");
        text
    } else {
        String::from_utf8(bytes).expect("a UTF-8 recording")
    }
}

fn scan() -> Coverage {
    let mut coverage = Coverage::default();
    let mut light_radii: BTreeSet<String> = BTreeSet::new();

    let mut files: Vec<PathBuf> = std::fs::read_dir(recordings_directory())
        .expect("a readable directory")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.to_string_lossy().ends_with(".ndjson")
                || path.to_string_lossy().ends_with(".ndjson.gz")
        })
        .collect();
    files.sort();

    for file in &files {
        for line in read_recording(file).lines() {
            let Ok(frame) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(state) = frame.get("state") else {
                continue;
            };
            coverage.frames += 1;

            // Already the derived state, not the raw memory value: the recorder writes
            // the `AmongUsState` the Electron reader produced. `LOBBY = 0, TASKS = 1,
            // DISCUSSION = 2, MENU = 3, UNKNOWN = 4` on both sides. Reading it as raw and
            // re-deriving reports MENU for freeplay and LOBBY for the menu, which is how
            // this was nearly written down backwards.
            if let Some(game_state) = state.get("gameState").and_then(serde_json::Value::as_i64) {
                coverage.game_states.insert(match game_state {
                    0 => "LOBBY",
                    1 => "TASKS",
                    2 => "DISCUSSION",
                    3 => "MENU",
                    _ => "UNKNOWN",
                });
            }
            if let Some(map) = state.get("map").and_then(serde_json::Value::as_i64) {
                coverage.maps.insert(map);
            }
            if state.get("comsSabotaged") == Some(&serde_json::Value::Bool(true)) {
                coverage.comms_sabotaged = true;
            }
            if state
                .get("closedDoors")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|doors| !doors.is_empty())
            {
                coverage.doors_closed = true;
            }
            if let Some(camera) = state
                .get("currentCamera")
                .and_then(serde_json::Value::as_i64)
            {
                coverage.cameras.insert(camera);
            }
            if let Some(radius) = state.get("lightRadius") {
                light_radii.insert(radius.to_string());
            }

            for player in state
                .get("players")
                .and_then(serde_json::Value::as_array)
                .unwrap_or(&Vec::new())
            {
                for (field, seen) in [
                    ("isDead", &mut coverage.anybody_dead),
                    ("inVent", &mut coverage.anybody_in_vent),
                    ("isImpostor", &mut coverage.anybody_an_impostor),
                ] {
                    if player.get(field) == Some(&serde_json::Value::Bool(true)) {
                        *seen = true;
                    }
                }
            }
        }
    }

    coverage.light_radius_varied = light_radii.len() > 1;
    coverage
}

#[test]
fn the_corpus_covers_what_it_is_claimed_to_and_reports_what_it_does_not() {
    let coverage = scan();

    let situations = [
        (
            "a round in progress (TASKS)",
            coverage.game_states.contains("TASKS"),
        ),
        (
            "a meeting (DISCUSSION)",
            coverage.game_states.contains("DISCUSSION"),
        ),
        ("somebody in a vent", coverage.anybody_in_vent),
        ("an impostor", coverage.anybody_an_impostor),
        ("somebody dead", coverage.anybody_dead),
        ("comms sabotaged", coverage.comms_sabotaged),
        ("a closed door", coverage.doors_closed),
        ("the camera changing", coverage.cameras.len() > 1),
        ("the light radius changing", coverage.light_radius_varied),
    ];

    eprintln!("\ncorpus coverage: {} frames", coverage.frames);
    eprintln!(
        "  game states reached: {}",
        coverage
            .game_states
            .iter()
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!(
        "  currentCamera values: {}",
        coverage
            .cameras
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!(
        "  maps: {}",
        coverage
            .maps
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (name, seen) in situations {
        eprintln!("  [{}] {name}", if seen { "x" } else { " " });
    }
    let missing: Vec<&str> = situations
        .iter()
        .filter(|(_, seen)| !seen)
        .map(|(name, _)| *name)
        .collect();
    if missing.is_empty() {
        eprintln!("\n  nothing on this list is missing.");
    } else {
        eprintln!(
            "\n  still to record: {}\n  see test/recordings/README.md",
            missing.join(", ")
        );
    }

    // Only what is true today. A recording removed, or replaced by a weaker one, fails
    // here — which is the regression this test can actually catch. What is missing is the
    // work, and asserting it would leave a red suite standing for months.
    assert!(coverage.frames > 12_000, "{} frames", coverage.frames);
    assert!(
        coverage.game_states.contains("LOBBY") && coverage.game_states.contains("MENU"),
        "the corpus no longer reaches both LOBBY and MENU: {:?}",
        coverage.game_states
    );
    assert!(
        coverage.anybody_in_vent,
        "no frame has anybody in a vent; freeplay used to cover this"
    );
    assert!(!coverage.maps.is_empty(), "no frame carried a map");
    // Every map, and by number rather than by count: a corpus that reached five maps
    // because Fungle was replaced by Submerged would pass a count and would have lost the
    // one thing this asserts, which is that the four non-Skeld branches of the reader are
    // exercised at all.
    //
    // Recorded from an online lobby's settings and not from freeplay, which cannot do it:
    // the reader takes the map from the game options, and freeplay does not write its map
    // there. Choosing a map in the menu leaves the field on whatever the last lobby set,
    // so a whole freeplay session on Polus arrives labelled Skeld.
    for (map, name) in [
        (0, "The Skeld"),
        (1, "Mira HQ"),
        (2, "Polus"),
        (4, "Airship"),
        (5, "Fungle"),
    ] {
        assert!(
            coverage.maps.contains(&map),
            "no frame is on {name} ({map}); the corpus reaches {:?}",
            coverage.maps
        );
    }
    for (name, seen) in [
        ("an impostor", coverage.anybody_an_impostor),
        ("somebody dead", coverage.anybody_dead),
        ("a light radius that changes", coverage.light_radius_varied),
    ] {
        assert!(seen, "the corpus no longer covers {name}");
    }
}

#[test]
fn every_recording_is_committed_compressed() {
    // A session is roughly 10 KB per frame and gzips by a factor of 130, because 99.8% of
    // a frame's regions are byte-identical to the one before. An uncompressed one is a
    // repository that grows by tens of megabytes per recording.
    for entry in std::fs::read_dir(recordings_directory()).expect("a readable directory") {
        let path = entry.expect("a readable entry").path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        assert!(
            !name.ends_with(".ndjson"),
            "{name} is not compressed; gzip it before committing"
        );
    }
}
