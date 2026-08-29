//! What a player should sound like: `calculateVoiceAudio()`, as a function of its inputs.
//!
//! The Electron original is 210 lines in `Voice.tsx` that decide a gain and a pan position
//! and, in the same breath, connect and disconnect nodes on a live Web Audio graph. The
//! deciding is what this module is. Connecting is the caller's, and keeping the two apart
//! is what makes roughly 150 table-driven cases cheap enough to be worth writing — the
//! original can only be tested by building a graph and listening to it.
//!
//! # Faithfulness over tidiness
//!
//! This reproduces the original's behaviour, including where that behaviour looks wrong,
//! because gate G2 compares the two on recorded tuples from real sessions. Two quirks are
//! deliberate and are named where they happen: the biquad's filter type is set once by the
//! impostor-radio branch and never reset, and a listener already within earshot is not
//! treated as being on a camera even when they are.

use acl_types::collider::pose_collide;
use acl_types::map::{CameraLocation, MapType, Vector2, camera, nearest};

/// The game state, as far as this decision is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    /// In a lobby, before the round starts.
    Lobby,
    /// Playing.
    Tasks,
    /// A meeting.
    Discussion,
    /// Not in a game.
    Menu,
    /// A state this build does not know.
    Unknown,
}

/// One player, as far as this decision is concerned.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one field per flag the game reports, so the correspondence stays readable"
)]
pub struct Player {
    /// The network client id, which the impostor radio is keyed by.
    pub client_id: u32,
    /// Where they are.
    pub position: Vector2,
    /// Whether they are dead.
    pub is_dead: bool,
    /// Whether they are an impostor.
    pub is_impostor: bool,
    /// Whether they are inside a vent.
    pub in_vent: bool,
    /// Whether they have left.
    pub disconnected: bool,
    /// Whether they are a freeplay dummy rather than a person.
    pub is_dummy: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            client_id: 0,
            position: Vector2 { x: 0.0, y: 0.0 },
            is_dead: false,
            is_impostor: false,
            in_vent: false,
            disconnected: false,
            is_dummy: false,
        }
    }
}

/// The parts of the game state this decision reads.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    /// Which state the game is in.
    pub game_state: GameState,
    /// Which map, for the collider and the camera table.
    pub map: MapType,
    /// Which doors are shut, for the collider.
    pub closed_doors: Vec<u32>,
    /// Whether communications are sabotaged.
    pub coms_sabotaged: bool,
    /// Which camera the local player is looking through.
    pub current_camera: CameraLocation,
    /// Whether the light radius changed this frame.
    pub light_radius_changed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            game_state: GameState::Tasks,
            map: MapType::TheSkeld,
            closed_doors: Vec::new(),
            coms_sabotaged: false,
            current_camera: CameraLocation::None,
            light_radius_changed: false,
        }
    }
}

/// The lobby settings the host chooses, as they reach this decision.
///
/// `ILobbySettings` minus the three public-lobby fields, which are about listing a lobby
/// and never reach the audio path.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one field per setting in ILobbySettings; packing them would hide the               correspondence that gate G2 is checked against"
)]
pub struct LobbySettings {
    /// Impostors haunt: a living impostor hears the dead, with reverb.
    pub haunting: bool,
    /// Everyone hears an impostor who is in a vent.
    pub hear_impostors_in_vents: bool,
    /// An impostor in a vent hears other impostors in vents.
    pub impostors_hear_impostors_in_vent: bool,
    /// Impostors have a radio to each other, at any distance.
    pub impostor_radio_enabled: bool,
    /// Sabotaged comms cut positional audio for the crew.
    pub coms_sabotage: bool,
    /// Only the dead hear anything.
    pub dead_only: bool,
    /// Only ghosts talk in meetings.
    pub meeting_ghost_only: bool,
    /// A player watching cameras hears what the camera sees.
    pub hear_through_cameras: bool,
    /// Walls block audio.
    pub walls_block_audio: bool,
    /// Hearing range follows the crew's vision rather than the fixed setting.
    pub vision_hearing: bool,
    /// The fixed hearing range, in the game's units.
    ///
    /// Not a `bool` like its neighbours, so [`LobbySettings::default`] is not the shipped
    /// default: this is zero there, and the client's is 5.32. The tests build from the
    /// derived value rather than from this.
    pub max_distance: f64,
}

/// The client's own settings that reach this decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClientSettings {
    /// How loud a haunted ghost is to an impostor, as a percentage.
    pub ghost_volume_as_impostor: f32,
    /// Whether panning is applied at all.
    pub enable_spatial_audio: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            ghost_volume_as_impostor: 100.0,
            enable_spatial_audio: true,
        }
    }
}

/// Which biquad shape the muffle filter should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// The shape the node is created with.
    LowPass,
    /// The impostor radio's shape.
    HighPass,
}

/// The muffle filter's settings, when it should be in the path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Muffle {
    /// The filter shape.
    ///
    /// Always set, and that is the correction of 2026-08-28. It was `Option`, and `None`
    /// meant "whatever this node already is" — which was an accurate reading of
    /// `Voice.tsx` when this was written at 17:23 on 2026-08-24, and stopped being one at
    /// 22:20 the same day, when `651d7ae9` gave the vent and camera branch an explicit
    /// `muffle.type = 'lowpass'`.
    ///
    /// The bug it fixed there is the reason it matters here: the impostor radio borrows
    /// the same node and leaves it a high pass, and nothing put it back. One use of the
    /// radio turned every later vent and camera into a high pass at the *low* pass corner
    /// frequency — stripping out everything below 2 kHz, which is where speech lives — for
    /// as long as that peer existed.
    ///
    /// The recorded corpus could not catch this: all 1,035 of its tuples have no muffle at
    /// all, because nobody in that session was in a vent, on a camera or on the radio.
    pub kind: FilterKind,
    /// The corner frequency, in hertz.
    pub frequency: f32,
    /// The filter's Q.
    pub q: f32,
}

/// What one peer should sound like this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceParams {
    /// The gain to apply, from 0 to 1.
    pub gain: f32,
    /// Where to place them, relative to the listener.
    pub pan: Vector2,
    /// The muffle filter, or `None` if the direct path should be restored.
    pub muffle: Option<Muffle>,
    /// Whether the reverb path should be connected.
    pub reverb: bool,
    /// Whether the panner's `maxDistance` should be rewritten this frame.
    pub update_max_distance: bool,
    /// Whether [`Self::pan`] is an answer or an absence.
    ///
    /// The Electron original returns 0 from three places without touching the graph, so
    /// the panner keeps the position the previous frame left on it. `false` says this is
    /// one of those: the peer is silent, and where they would have been placed is not
    /// something this call decided.
    ///
    /// Audibly it makes no difference — a gain of zero is a gain of zero — but it is the
    /// difference between comparing an answer and comparing a leftover, which is what
    /// gate G2's tuples do.
    pub placed: bool,
}

impl VoiceParams {
    /// Silence, with nothing in the path.
    ///
    /// The original's three bare `return 0` statements leave the graph exactly as it was,
    /// which matters: a peer that goes silent because a wall moved between you keeps its
    /// muffle and its reverb, and gets them back unchanged when the wall moves away.
    const fn silent() -> Self {
        Self {
            gain: 0.0,
            pan: Vector2 { x: 0.0, y: 0.0 },
            muffle: None,
            reverb: false,
            update_max_distance: false,
            placed: false,
        }
    }
}

/// The frequency and Q the vent muffle uses.
const VENT_MUFFLE: (f32, f32) = (2000.0, 20.0);

/// The frequency and Q the camera muffle uses.
const CAMERA_MUFFLE: (f32, f32) = (2300.0, -15.0);

/// The impostor radio's filter: a high pass well above speech fundamentals.
const RADIO_MUFFLE: (f32, f32) = (1000.0, 10.0);

/// How loud a muffled peer is when they would otherwise be at full gain.
const VENT_GAIN: f32 = 0.5;

/// The same, through a camera.
const CAMERA_GAIN: f32 = 0.8;

/// A hearing range at or below this is treated as broken and replaced.
///
/// The light radius is read from the game and can arrive as zero or negative — during a
/// blackout, or on a frame where the read failed. Without this floor everyone would go
/// silent, which reads as the app being broken rather than as the lights being out.
const MINIMUM_RANGE: f64 = 0.6;

/// The range a broken one is replaced with.
const FALLBACK_RANGE: f64 = 1.0;

/// How far the local player can hear, before any of the per-peer decisions.
///
/// `Voice.tsx` computes this beside the peer list rather than inside
/// `calculateVoiceAudio`, but it is the same decision and it is where two of the eleven
/// lobby settings live:
///
/// ```text
/// maxDistanceRef.current = lobbySettings.visionHearing
///     ? myPlayer.isImpostor ? lobbySettings.maxDistance : gameState.lightRadius + 0.5
///     : lobbySettings.maxDistance;
/// if (maxDistanceRef.current <= 0.6) maxDistanceRef.current = 1;
/// ```
///
/// With `visionHearing` on, the crew hear as far as they can see and an impostor keeps the
/// fixed range — which is the asymmetry the setting exists for, since an impostor's vision
/// is not reduced by the lights going out.
#[must_use]
pub fn hearing_range(lobby: &LobbySettings, me: &Player, light_radius: f64) -> f64 {
    let range = if lobby.vision_hearing && !me.is_impostor {
        // Half a unit past what the crewmate can see, so a voice arrives just before its
        // owner does.
        light_radius + 0.5
    } else {
        lobby.max_distance
    };
    // `!is_finite()` as well as too small, and it is not defensive programming for its own
    // sake. `light_radius` comes out of the game's memory through `position`-style reads,
    // and a NaN there makes this a NaN, which `Panner::distance_gain` then hands to
    // `f64::clamp` -- and `clamp` asserts `min <= max`, which a NaN fails. On the mixing
    // thread of a `panic = "abort"` build that is the whole client gone, from one unlucky
    // frame. The reader stops NaNs at source since 2026-08-29; this is the second line,
    // because the first one is a hundred lines away in another crate.
    if !range.is_finite() || range <= MINIMUM_RANGE {
        FALLBACK_RANGE
    } else {
        range
    }
}

/// Decides what one peer sounds like.
///
/// `max_distance` is the hearing range, which the client derives from the lobby setting
/// and the light radius before calling. `impostor_radio` is the client id the local
/// impostor's radio is tuned to, if any.
#[must_use]
pub fn voice_params(
    state: &State,
    settings: &ClientSettings,
    lobby: &LobbySettings,
    me: &Player,
    other: &Player,
    max_distance: f64,
    impostor_radio: Option<u32>,
) -> VoiceParams {
    // Nobody to hear.
    if other.disconnected || other.is_dummy {
        return VoiceParams::silent();
    }

    let mut pan = Vector2 {
        x: other.position.x - me.position.x,
        y: other.position.y - me.position.y,
    };
    let mut collided = false;
    let mut skip_distance_check = false;
    let mut radio_muffle = false;
    let mut reverb = false;

    let mut gain = match state.game_state {
        GameState::Menu => return VoiceParams::silent(),
        GameState::Lobby => 1.0,
        GameState::Discussion => {
            // Everyone is in the same room, so nobody is placed anywhere.
            pan = Vector2 { x: 0.0, y: 0.0 };
            if !me.is_dead && other.is_dead {
                0.0
            } else {
                1.0
            }
        }
        GameState::Tasks => during_a_round(
            state,
            *settings,
            lobby,
            me,
            other,
            impostor_radio,
            &mut collided,
            &mut skip_distance_check,
            &mut radio_muffle,
            &mut reverb,
        ),
        GameState::Unknown => 0.0,
    };

    if lobby.dead_only {
        pan = Vector2 { x: 0.0, y: 0.0 };
        if !me.is_dead || !other.is_dead {
            gain = 0.0;
        }
    }

    // Set before the distance check and cleared by it: a peer already within earshot is
    // not treated as being on a camera even when the listener is watching one. That is
    // what decides whether the camera muffle applies, and it is the original's behaviour.
    let mut on_camera = state.current_camera != CameraLocation::None;

    match reach(
        state,
        lobby,
        other,
        pan,
        max_distance,
        skip_distance_check,
        collided,
    ) {
        Reach::Silent => return VoiceParams::silent(),
        Reach::Direct => on_camera = false,
        Reach::ThroughCamera(from) => pan = from,
    }

    // The muffle. Two writes to one filter, in the order the original makes them, and the
    // order is the whole of it.
    //
    // `Voice.tsx` sets the radio's high pass at line 483 and the vent's low pass at line
    // 588, and the second is a separate `if` on the same node rather than an `else if`. So
    // an impostor who is on the radio *and* in a vent hears the vent: low pass at 2 kHz,
    // Q 20, and the gain brought down from 1.0. This was an `else if` until 2026-08-29,
    // which gave the radio the win it does not have.
    let mut muffle = radio_muffle.then_some(Muffle {
        kind: FilterKind::HighPass,
        frequency: RADIO_MUFFLE.0,
        q: RADIO_MUFFLE.1,
    });
    if state.game_state == GameState::Tasks
        && ((me.in_vent && !me.is_dead) || (other.in_vent && !other.is_dead) || on_camera)
    {
        let (frequency, q) = if on_camera {
            CAMERA_MUFFLE
        } else {
            VENT_MUFFLE
        };
        // Full gain through a vent or a camera is too loud, so it is brought down — but
        // only from exactly 1.0, which leaves a haunted ghost's volume alone.
        #[allow(
            clippy::float_cmp,
            reason = "`if (endGain === 1)` is the original, and an epsilon here would                       catch a ghost volume of 0.999 that the client leaves alone"
        )]
        if gain == 1.0 {
            gain = if on_camera { CAMERA_GAIN } else { VENT_GAIN };
        }
        muffle = Some(Muffle {
            // Explicit, every time. See `Muffle::kind`: the radio leaves this node a high
            // pass, and only writing it back makes the next vent a low pass again.
            kind: FilterKind::LowPass,
            frequency,
            q,
        });
    }

    if !settings.enable_spatial_audio || skip_distance_check {
        pan = Vector2 { x: 0.0, y: 0.0 };
    }

    VoiceParams {
        gain,
        pan,
        muffle,
        reverb,
        update_max_distance: state.light_radius_changed,
        placed: true,
    }
}

/// The gain during a round, and the four decisions that go with it.
///
/// Split out of [`voice_params`] because it is most of it: every lobby setting except the
/// two that make the hearing range is read here. The four `&mut` flags are what the
/// original sets as local variables and reads again further down, after the branch has
/// ended — they are the branch's other outputs, not state.
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per thing the original reads or writes at this point"
)]
fn during_a_round(
    state: &State,
    settings: ClientSettings,
    lobby: &LobbySettings,
    me: &Player,
    other: &Player,
    impostor_radio: Option<u32>,
    collided: &mut bool,
    skip_distance_check: &mut bool,
    radio_muffle: &mut bool,
    reverb: &mut bool,
) -> f32 {
    let mut gain = 1.0f32;

    if lobby.meeting_ghost_only {
        gain = 0.0;
    }
    // Sabotaged comms cut the crew's positional audio; impostors keep theirs, and the
    // dead are past caring.
    if !me.is_dead && lobby.coms_sabotage && state.coms_sabotaged && !me.is_impostor {
        gain = 0.0;
    }
    // A player in a vent is silent unless the lobby says otherwise — either to everyone,
    // or to another impostor who is also in a vent.
    if other.in_vent
        && !(lobby.hear_impostors_in_vents
            || (lobby.impostors_hear_impostors_in_vent && me.in_vent))
    {
        gain = 0.0;
    }
    if lobby.walls_block_audio
        && !me.is_dead
        && pose_collide(me.position, other.position, state.map, &state.closed_doors)
    {
        *collided = true;
    }
    if me.is_impostor
        && other.is_impostor
        && lobby.impostor_radio_enabled
        && impostor_radio == Some(other.client_id)
    {
        *skip_distance_check = true;
        *radio_muffle = true;
    }
    if !me.is_dead && other.is_dead && me.is_impostor && lobby.haunting {
        *reverb = true;
        // Haunting reaches through walls: that is the point of it.
        *collided = false;
        gain = settings.ghost_volume_as_impostor / 100.0;
    } else if other.is_dead && !me.is_dead {
        gain = 0.0;
    }

    gain
}

/// Whether a peer is audible at all, and from where.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reach {
    /// Out of earshot, or behind a wall.
    Silent,
    /// Close enough to hear directly.
    Direct,
    /// Out of earshot, but a camera reaches them; this is the pan to use.
    ThroughCamera(Vector2),
}

/// The distance check, and the camera that can undo it.
///
/// Split out of [`voice_params`] to keep it under the line limit, but the shape is the
/// original's: a peer already within earshot takes the `Direct` arm, which is what clears
/// `isOnCamera` and stops a nearby speaker being muffled because the listener happens to
/// be watching a screen.
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per thing the original reads at this point"
)]
fn reach(
    state: &State,
    lobby: &LobbySettings,
    other: &Player,
    pan: Vector2,
    max_distance: f64,
    skip_distance_check: bool,
    collided: bool,
) -> Reach {
    if skip_distance_check || length(pan) <= max_distance {
        // Within earshot. A wall still stops them, unless the radio skipped the check.
        return if collided && !skip_distance_check {
            Reach::Silent
        } else {
            Reach::Direct
        };
    }

    // Out of earshot. A camera can bring them back, if the lobby allows it and the round
    // is running.
    if !(lobby.hear_through_cameras && state.game_state == GameState::Tasks) {
        return Reach::Silent;
    }
    let from = camera_position(state, other.position).map_or(pan, |at| Vector2 {
        x: other.position.x - at.x,
        y: other.position.y - at.y,
    });
    if length(from) > max_distance {
        Reach::Silent
    } else {
        Reach::ThroughCamera(from)
    }
}

/// Where a peer is heard from when a camera is what reaches them.
///
/// The Skeld's console shows four rooms at once, so the camera nearest the speaker is the
/// one they are heard from rather than one the watcher chose. Every other map reports the
/// camera being watched.
fn camera_position(state: &State, other: Vector2) -> Option<Vector2> {
    match state.current_camera {
        CameraLocation::None => None,
        CameraLocation::Skeld => nearest(state.map, other),
        at => camera(state.map, at),
    }
}

/// The length of a pan vector.
fn length(pan: Vector2) -> f64 {
    pan.y.mul_add(pan.y, pan.x * pan.x).sqrt()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    /// The local player: alive, a crewmate, standing at the origin.
    fn me() -> Player {
        Player::default()
    }

    /// Someone else, close enough to hear.
    fn other() -> Player {
        Player {
            client_id: 2,
            position: Vector2 { x: 1.0, y: 0.0 },
            ..Player::default()
        }
    }

    /// Far enough away to be out of earshot at the default range.
    fn far() -> Player {
        Player {
            position: Vector2 { x: 100.0, y: 0.0 },
            ..other()
        }
    }

    /// Everything off, which is not the shipped default — it is the base a test adds to.
    fn lobby() -> LobbySettings {
        LobbySettings::default()
    }

    /// The hearing range the tests use, in the game's units.
    const RANGE: f64 = 5.0;

    fn run(state: &State, lobby: &LobbySettings, me: &Player, other: &Player) -> VoiceParams {
        voice_params(
            state,
            &ClientSettings::default(),
            lobby,
            me,
            other,
            RANGE,
            None,
        )
    }

    // ---------------------------------------------------------------- early returns

    #[test]
    fn a_disconnected_peer_is_silent() {
        let peer = Player {
            disconnected: true,
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby(), &me(), &peer).gain, 0.0);
    }

    #[test]
    fn a_freeplay_dummy_is_silent() {
        // Not a person, so there is no stream to place anywhere.
        let peer = Player {
            is_dummy: true,
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby(), &me(), &peer).gain, 0.0);
    }

    #[test]
    fn nobody_is_audible_in_the_menu() {
        let state = State {
            game_state: GameState::Menu,
            ..State::default()
        };
        assert_eq!(run(&state, &lobby(), &me(), &other()).gain, 0.0);
    }

    #[test]
    fn an_unknown_state_is_silent_rather_than_a_panic() {
        // A new game build adding a state should not make everyone shout or crash.
        let state = State {
            game_state: GameState::Unknown,
            ..State::default()
        };
        assert_eq!(run(&state, &lobby(), &me(), &other()).gain, 0.0);
    }

    #[test]
    fn a_silent_return_leaves_the_graph_alone() {
        // The original's bare `return 0` touches no node, so a peer silenced by a wall
        // keeps its muffle and gets it back unchanged when the wall moves away.
        let params = run(&State::default(), &lobby(), &me(), &far());
        assert_eq!(params.gain, 0.0);
        assert!(params.muffle.is_none());
        assert!(!params.reverb);
    }

    // ---------------------------------------------------------------- lobby

    #[test]
    fn everyone_is_audible_in_the_lobby() {
        let state = State {
            game_state: GameState::Lobby,
            ..State::default()
        };
        assert_eq!(run(&state, &lobby(), &me(), &other()).gain, 1.0);
    }

    #[test]
    fn the_lobby_still_places_people() {
        let state = State {
            game_state: GameState::Lobby,
            ..State::default()
        };
        let params = run(&state, &lobby(), &me(), &other());
        assert_eq!(params.pan.x, 1.0);
    }

    #[test]
    fn distance_still_applies_in_the_lobby() {
        let state = State {
            game_state: GameState::Lobby,
            ..State::default()
        };
        assert_eq!(run(&state, &lobby(), &me(), &far()).gain, 0.0);
    }

    // ---------------------------------------------------------------- discussion

    #[test]
    fn a_meeting_puts_everyone_in_the_same_place() {
        // Panning a meeting would place people by where their bodies happen to be.
        let state = State {
            game_state: GameState::Discussion,
            ..State::default()
        };
        let params = run(&state, &lobby(), &me(), &far());
        assert_eq!(params.gain, 1.0);
        assert_eq!(params.pan, Vector2 { x: 0.0, y: 0.0 });
    }

    #[test]
    fn the_living_do_not_hear_the_dead_in_a_meeting() {
        let state = State {
            game_state: GameState::Discussion,
            ..State::default()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        assert_eq!(run(&state, &lobby(), &me(), &ghost).gain, 0.0);
    }

    #[test]
    fn the_dead_hear_each_other_in_a_meeting() {
        let state = State {
            game_state: GameState::Discussion,
            ..State::default()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let dead_me = Player {
            is_dead: true,
            ..me()
        };
        assert_eq!(run(&state, &lobby(), &dead_me, &ghost).gain, 1.0);
    }

    // ---------------------------------------------------------------- ghosts

    #[test]
    fn the_living_do_not_hear_the_dead_during_a_round() {
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby(), &me(), &ghost).gain, 0.0);
    }

    #[test]
    fn the_dead_hear_the_living() {
        // One-way: being dead is a listening position, not a silence.
        let dead_me = Player {
            is_dead: true,
            ..me()
        };
        assert_eq!(
            run(&State::default(), &lobby(), &dead_me, &other()).gain,
            1.0
        );
    }

    // ---------------------------------------------------------------- haunting

    #[test]
    fn haunting_lets_an_impostor_hear_a_ghost() {
        let lobby = LobbySettings {
            haunting: true,
            ..lobby()
        };
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let params = run(&State::default(), &lobby, &impostor, &ghost);
        assert_eq!(params.gain, 1.0);
        assert!(params.reverb, "a haunting ghost is heard through reverb");
    }

    #[test]
    fn haunting_uses_its_own_volume() {
        let lobby = LobbySettings {
            haunting: true,
            ..lobby()
        };
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let settings = ClientSettings {
            ghost_volume_as_impostor: 40.0,
            ..ClientSettings::default()
        };
        let params = voice_params(
            &State::default(),
            &settings,
            &lobby,
            &impostor,
            &ghost,
            RANGE,
            None,
        );
        assert_eq!(params.gain, 0.4);
    }

    #[test]
    fn haunting_reaches_through_walls() {
        // The whole point of it. `collided` is cleared by the haunting branch.
        let lobby = LobbySettings {
            haunting: true,
            walls_block_audio: true,
            ..lobby()
        };
        let impostor = Player {
            is_impostor: true,
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert!(run(&State::default(), &lobby, &impostor, &ghost).gain > 0.0);
    }

    #[test]
    fn a_crewmate_does_not_haunt() {
        let lobby = LobbySettings {
            haunting: true,
            ..lobby()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let params = run(&State::default(), &lobby, &me(), &ghost);
        assert_eq!(params.gain, 0.0);
        assert!(!params.reverb);
    }

    #[test]
    fn a_dead_impostor_does_not_haunt() {
        // Two ghosts hear each other the ordinary way, without the reverb.
        let lobby = LobbySettings {
            haunting: true,
            ..lobby()
        };
        let dead_impostor = Player {
            is_impostor: true,
            is_dead: true,
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let params = run(&State::default(), &lobby, &dead_impostor, &ghost);
        assert!(!params.reverb);
        assert_eq!(params.gain, 1.0);
    }

    // ---------------------------------------------------------------- vents

    #[test]
    fn a_player_in_a_vent_is_silent_by_default() {
        let vented = Player {
            in_vent: true,
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby(), &me(), &vented).gain, 0.0);
    }

    #[test]
    fn hear_impostors_in_vents_makes_them_audible_to_everyone() {
        let lobby = LobbySettings {
            hear_impostors_in_vents: true,
            ..lobby()
        };
        let vented = Player {
            in_vent: true,
            ..other()
        };
        let params = run(&State::default(), &lobby, &me(), &vented);
        assert!(params.gain > 0.0);
        assert!(params.muffle.is_some(), "and muffled");
    }

    #[test]
    fn impostors_hear_impostors_in_vents_needs_both_in_a_vent() {
        let lobby = LobbySettings {
            impostors_hear_impostors_in_vent: true,
            ..lobby()
        };
        let vented = Player {
            in_vent: true,
            ..other()
        };
        // Standing outside the vent: still nothing.
        assert_eq!(run(&State::default(), &lobby, &me(), &vented).gain, 0.0);

        let vented_me = Player {
            in_vent: true,
            ..me()
        };
        assert!(run(&State::default(), &lobby, &vented_me, &vented).gain > 0.0);
    }

    #[test]
    fn the_vent_muffle_brings_full_gain_down() {
        // Full volume through a vent is too loud.
        let lobby = LobbySettings {
            hear_impostors_in_vents: true,
            ..lobby()
        };
        let vented = Player {
            in_vent: true,
            ..other()
        };
        let params = run(&State::default(), &lobby, &me(), &vented);
        assert_eq!(params.gain, VENT_GAIN);
        let muffle = params.muffle.unwrap();
        assert_eq!(muffle.frequency, VENT_MUFFLE.0);
        assert_eq!(muffle.q, VENT_MUFFLE.1);
        assert_eq!(
            muffle.kind,
            FilterKind::LowPass,
            "a vent is a low pass, and saying so is what stops the radio's high pass \
             surviving into it"
        );
    }

    #[test]
    fn a_listener_in_a_vent_hears_everyone_muffled() {
        // The muffle is about where the *listener* is as much as the speaker.
        let vented_me = Player {
            in_vent: true,
            ..me()
        };
        let params = run(&State::default(), &lobby(), &vented_me, &other());
        assert!(params.muffle.is_some());
        assert_eq!(params.gain, VENT_GAIN);
    }

    #[test]
    fn a_dead_players_vent_does_not_muffle() {
        // `!me.isDead` and `!other.isDead` guard both halves.
        let dead_me = Player {
            in_vent: true,
            is_dead: true,
            ..me()
        };
        let params = run(&State::default(), &lobby(), &dead_me, &other());
        assert!(params.muffle.is_none());
        assert_eq!(params.gain, 1.0);
    }

    #[test]
    fn vents_do_not_muffle_outside_a_round() {
        let state = State {
            game_state: GameState::Lobby,
            ..State::default()
        };
        let vented_me = Player {
            in_vent: true,
            ..me()
        };
        assert!(run(&state, &lobby(), &vented_me, &other()).muffle.is_none());
    }

    // ---------------------------------------------------------------- walls and doors

    #[test]
    fn a_wall_silences_a_neighbour() {
        let lobby = LobbySettings {
            walls_block_audio: true,
            ..lobby()
        };
        // The Skeld's first vertical wall sits at world x -6.35, spanning y 2.43 to
        // 4.68. These two are either side of it and 1.5 apart, well within earshot.
        let listener = Player {
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let speaker = Player {
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert_eq!(
            run(&State::default(), &lobby, &listener, &speaker).gain,
            0.0
        );
    }

    #[test]
    fn walls_do_not_block_when_the_setting_is_off() {
        let listener = Player {
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let speaker = Player {
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert!(run(&State::default(), &lobby(), &listener, &speaker).gain > 0.0);
    }

    #[test]
    fn the_dead_hear_through_walls() {
        // `!me.isDead` guards the collision check: ghosts pass through everything.
        let lobby = LobbySettings {
            walls_block_audio: true,
            ..lobby()
        };
        let listener = Player {
            position: Vector2 { x: -7.0, y: 3.5 },
            is_dead: true,
            ..me()
        };
        let speaker = Player {
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert!(run(&State::default(), &lobby, &listener, &speaker).gain > 0.0);
    }

    #[test]
    fn walls_do_not_block_outside_a_round() {
        let state = State {
            game_state: GameState::Lobby,
            ..State::default()
        };
        let lobby = LobbySettings {
            walls_block_audio: true,
            ..lobby()
        };
        let listener = Player {
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let speaker = Player {
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert!(run(&state, &lobby, &listener, &speaker).gain > 0.0);
    }

    // ---------------------------------------------------------------- comms sabotage

    #[test]
    fn sabotaged_comms_silence_the_crew() {
        let state = State {
            coms_sabotaged: true,
            ..State::default()
        };
        let lobby = LobbySettings {
            coms_sabotage: true,
            ..lobby()
        };
        assert_eq!(run(&state, &lobby, &me(), &other()).gain, 0.0);
    }

    #[test]
    fn sabotaged_comms_leave_impostors_talking() {
        let state = State {
            coms_sabotaged: true,
            ..State::default()
        };
        let lobby = LobbySettings {
            coms_sabotage: true,
            ..lobby()
        };
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        assert_eq!(run(&state, &lobby, &impostor, &other()).gain, 1.0);
    }

    #[test]
    fn sabotaged_comms_leave_the_dead_alone() {
        let state = State {
            coms_sabotaged: true,
            ..State::default()
        };
        let lobby = LobbySettings {
            coms_sabotage: true,
            ..lobby()
        };
        let dead_me = Player {
            is_dead: true,
            ..me()
        };
        assert_eq!(run(&state, &lobby, &dead_me, &other()).gain, 1.0);
    }

    #[test]
    fn sabotage_needs_the_lobby_setting() {
        let state = State {
            coms_sabotaged: true,
            ..State::default()
        };
        assert_eq!(run(&state, &lobby(), &me(), &other()).gain, 1.0);
    }

    // ---------------------------------------------------------------- dead only

    #[test]
    fn dead_only_silences_the_living() {
        let lobby = LobbySettings {
            dead_only: true,
            ..lobby()
        };
        assert_eq!(run(&State::default(), &lobby, &me(), &other()).gain, 0.0);
    }

    #[test]
    fn dead_only_needs_both_dead() {
        let lobby = LobbySettings {
            dead_only: true,
            ..lobby()
        };
        let dead_me = Player {
            is_dead: true,
            ..me()
        };
        assert_eq!(run(&State::default(), &lobby, &dead_me, &other()).gain, 0.0);

        let ghost = Player {
            is_dead: true,
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby, &dead_me, &ghost).gain, 1.0);
    }

    #[test]
    fn dead_only_stops_placing_people() {
        let lobby = LobbySettings {
            dead_only: true,
            ..lobby()
        };
        let dead_me = Player {
            is_dead: true,
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        assert_eq!(
            run(&State::default(), &lobby, &dead_me, &ghost).pan,
            Vector2 { x: 0.0, y: 0.0 }
        );
    }

    // ---------------------------------------------------------------- meeting ghosts

    #[test]
    fn meeting_ghost_only_silences_a_round() {
        // It applies during the round, not during the meeting: the branch is in `Tasks`.
        let lobby = LobbySettings {
            meeting_ghost_only: true,
            ..lobby()
        };
        assert_eq!(run(&State::default(), &lobby, &me(), &other()).gain, 0.0);
    }

    #[test]
    fn meeting_ghost_only_leaves_the_meeting_alone() {
        let state = State {
            game_state: GameState::Discussion,
            ..State::default()
        };
        let lobby = LobbySettings {
            meeting_ghost_only: true,
            ..lobby()
        };
        assert_eq!(run(&state, &lobby, &me(), &other()).gain, 1.0);
    }

    // ---------------------------------------------------------------- impostor radio

    fn radio_lobby() -> LobbySettings {
        LobbySettings {
            impostor_radio_enabled: true,
            ..lobby()
        }
    }

    #[test]
    fn the_impostor_radio_reaches_across_the_map() {
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let distant = Player {
            is_impostor: true,
            ..far()
        };
        let params = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &radio_lobby(),
            &impostor,
            &distant,
            RANGE,
            Some(distant.client_id),
        );
        assert_eq!(params.gain, 1.0);
    }

    #[test]
    fn the_radio_is_a_high_pass() {
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let distant = Player {
            is_impostor: true,
            ..far()
        };
        let muffle = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &radio_lobby(),
            &impostor,
            &distant,
            RANGE,
            Some(distant.client_id),
        )
        .muffle
        .unwrap();
        assert_eq!(muffle.kind, FilterKind::HighPass);
        assert_eq!(muffle.frequency, RADIO_MUFFLE.0);
        assert_eq!(muffle.q, RADIO_MUFFLE.1);
    }

    #[test]
    fn the_radio_is_not_placed_anywhere() {
        // It skips the distance check, and a skipped check means no panning.
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let distant = Player {
            is_impostor: true,
            ..far()
        };
        assert_eq!(
            voice_params(
                &State::default(),
                &ClientSettings::default(),
                &radio_lobby(),
                &impostor,
                &distant,
                RANGE,
                Some(distant.client_id),
            )
            .pan,
            Vector2 { x: 0.0, y: 0.0 }
        );
    }

    #[test]
    fn the_radio_reaches_through_walls() {
        let lobby = LobbySettings {
            walls_block_audio: true,
            ..radio_lobby()
        };
        let impostor = Player {
            is_impostor: true,
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let partner = Player {
            is_impostor: true,
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert!(
            voice_params(
                &State::default(),
                &ClientSettings::default(),
                &lobby,
                &impostor,
                &partner,
                RANGE,
                Some(partner.client_id),
            )
            .gain
                > 0.0
        );
    }

    #[test]
    fn the_radio_is_tuned_to_one_client() {
        // Two impostors, one radio: the other one is heard the ordinary way, or not.
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let distant = Player {
            is_impostor: true,
            ..far()
        };
        let params = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &radio_lobby(),
            &impostor,
            &distant,
            RANGE,
            Some(distant.client_id + 1),
        );
        assert_eq!(params.gain, 0.0);
    }

    #[test]
    fn a_crewmate_has_no_radio() {
        let distant = Player {
            is_impostor: true,
            ..far()
        };
        let params = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &radio_lobby(),
            &me(),
            &distant,
            RANGE,
            Some(distant.client_id),
        );
        assert_eq!(params.gain, 0.0);
    }

    // ---------------------------------------------------------------- cameras

    fn camera_state(at: CameraLocation, map: MapType) -> State {
        State {
            current_camera: at,
            map,
            ..State::default()
        }
    }

    #[test]
    fn a_camera_brings_a_distant_player_into_earshot() {
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        // Standing beside Polus's central camera, far from the listener.
        let speaker = Player {
            position: Vector2 { x: 16.0, y: -15.4 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        let params = run(&state, &lobby, &me(), &speaker);
        assert!(params.gain > 0.0);
        assert_eq!(params.gain, CAMERA_GAIN);
    }

    #[test]
    fn the_camera_muffle_is_its_own() {
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let speaker = Player {
            position: Vector2 { x: 16.0, y: -15.4 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        let muffle = run(&state, &lobby, &me(), &speaker).muffle.unwrap();
        assert_eq!(muffle.frequency, CAMERA_MUFFLE.0);
        assert_eq!(muffle.q, CAMERA_MUFFLE.1);
    }

    #[test]
    fn a_camera_places_the_speaker_relative_to_the_camera() {
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let speaker = Player {
            position: Vector2 { x: 16.0, y: -15.4 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        let params = run(&state, &lobby, &me(), &speaker);
        // Polus central sits at 15.4, so the speaker is 0.6 to its right.
        assert!((params.pan.x - 0.6).abs() < 1e-9);
    }

    #[test]
    fn the_skeld_uses_whichever_camera_is_nearest() {
        // Its console shows four rooms at once, so the watcher did not pick one.
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let speaker = Player {
            position: Vector2 { x: 13.5, y: -4.348 },
            ..other()
        };
        let state = camera_state(CameraLocation::Skeld, MapType::TheSkeld);
        let params = run(&state, &lobby, &me(), &speaker);
        assert!(params.gain > 0.0);
        assert!((params.pan.x - (13.5 - 13.2417)).abs() < 1e-6);
    }

    #[test]
    fn a_camera_does_not_reach_someone_far_from_it_either() {
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        // Nowhere near any Polus camera.
        let speaker = Player {
            position: Vector2 { x: -200.0, y: 0.0 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        assert_eq!(run(&state, &lobby, &me(), &speaker).gain, 0.0);
    }

    #[test]
    fn cameras_need_the_lobby_setting() {
        let speaker = Player {
            position: Vector2 { x: 16.0, y: -15.4 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        assert_eq!(run(&state, &lobby(), &me(), &speaker).gain, 0.0);
    }

    #[test]
    fn cameras_only_work_during_a_round() {
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let speaker = Player {
            position: Vector2 { x: 16.0, y: -15.4 },
            ..other()
        };
        let mut state = camera_state(CameraLocation::Central, MapType::Polus);
        state.game_state = GameState::Lobby;
        assert_eq!(run(&state, &lobby, &me(), &speaker).gain, 0.0);
    }

    #[test]
    fn a_map_without_cameras_does_not_place_the_listener_at_the_origin() {
        // The lookup returns nothing rather than a zero vector, so the second distance
        // check still refuses. A `(0, 0)` fallback would put the listener in the middle
        // of the map and let them hear whoever stood there.
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let speaker = Player {
            position: Vector2 { x: 100.0, y: 0.0 },
            ..other()
        };
        let state = camera_state(CameraLocation::Central, MapType::MiraHq);
        assert_eq!(run(&state, &lobby, &me(), &speaker).gain, 0.0);
    }

    #[test]
    fn a_player_already_in_earshot_is_not_treated_as_being_on_camera() {
        // The original clears `isOnCamera` in the branch where the distance check passed,
        // so a nearby speaker is not muffled just because the listener is watching. It
        // reads like an oversight and it is what ships.
        let lobby = LobbySettings {
            hear_through_cameras: true,
            ..lobby()
        };
        let state = camera_state(CameraLocation::Central, MapType::Polus);
        let params = run(&state, &lobby, &me(), &other());
        assert!(params.muffle.is_none());
        assert_eq!(params.gain, 1.0);
    }

    // ---------------------------------------------------------------- spatial audio

    #[test]
    fn turning_off_spatial_audio_stops_placing_people() {
        let settings = ClientSettings {
            enable_spatial_audio: false,
            ..ClientSettings::default()
        };
        let params = voice_params(
            &State::default(),
            &settings,
            &lobby(),
            &me(),
            &other(),
            RANGE,
            None,
        );
        assert_eq!(params.pan, Vector2 { x: 0.0, y: 0.0 });
        assert_eq!(params.gain, 1.0, "and it stays audible");
    }

    #[test]
    fn turning_off_spatial_audio_does_not_change_who_is_audible() {
        // The distance check runs on the real positions, before panning is discarded.
        let settings = ClientSettings {
            enable_spatial_audio: false,
            ..ClientSettings::default()
        };
        let params = voice_params(
            &State::default(),
            &settings,
            &lobby(),
            &me(),
            &far(),
            RANGE,
            None,
        );
        assert_eq!(params.gain, 0.0);
    }

    // ---------------------------------------------------------------- light radius

    #[test]
    fn a_changed_light_radius_asks_for_the_range_to_be_rewritten() {
        let state = State {
            light_radius_changed: true,
            ..State::default()
        };
        assert!(run(&state, &lobby(), &me(), &other()).update_max_distance);
        assert!(!run(&State::default(), &lobby(), &me(), &other()).update_max_distance);
    }

    // ---------------------------------------------------------------- distance

    #[test]
    fn the_range_is_a_boundary_not_a_slope() {
        // Gain does not fall off here — the panner's distance model does that. This only
        // decides audible or not.
        let inside = Player {
            position: Vector2 {
                x: RANGE - 0.001,
                y: 0.0,
            },
            ..other()
        };
        let outside = Player {
            position: Vector2 {
                x: RANGE + 0.001,
                y: 0.0,
            },
            ..other()
        };
        assert_eq!(run(&State::default(), &lobby(), &me(), &inside).gain, 1.0);
        assert_eq!(run(&State::default(), &lobby(), &me(), &outside).gain, 0.0);
    }

    #[test]
    fn distance_is_measured_in_both_axes() {
        let diagonal = Player {
            position: Vector2 { x: 4.0, y: 4.0 },
            ..other()
        };
        // 5.66 away, outside a range of 5.
        assert_eq!(run(&State::default(), &lobby(), &me(), &diagonal).gain, 0.0);
    }

    // ---------------------------------------------------------------- interactions

    #[test]
    fn a_vented_impostor_on_the_radio_hears_the_vent_and_not_the_radio() {
        // Both branches apply, and the vent wins because it runs *second*. `Voice.tsx`
        // sets the radio's high pass at line 483 and the vent's low pass at line 588, and
        // the second is a separate `if` on the same filter node rather than an `else if`.
        //
        // This test asserted the opposite until 2026-08-29, which is worse than having no
        // test: it was written from a reading of the branch structure rather than of the
        // order, and it then defended the mistake.
        let lobby = LobbySettings {
            hear_impostors_in_vents: true,
            ..radio_lobby()
        };
        let impostor = Player {
            is_impostor: true,
            in_vent: true,
            ..me()
        };
        let partner = Player {
            is_impostor: true,
            in_vent: true,
            ..far()
        };
        let params = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &lobby,
            &impostor,
            &partner,
            RANGE,
            Some(partner.client_id),
        );
        let muffle = params.muffle.unwrap();
        assert_eq!(muffle.kind, FilterKind::LowPass);
        assert_eq!(muffle.frequency, VENT_MUFFLE.0);
        assert_eq!(muffle.q, VENT_MUFFLE.1);
        // And the vent's gain reduction with it, from the same block.
        assert!((params.gain - VENT_GAIN).abs() < f32::EPSILON);
    }

    #[test]
    fn a_radio_with_no_vent_is_still_the_radios_filter() {
        // The other half of the same rule: with only one of the two blocks running, the
        // one that runs decides. A fix that made the vent win unconditionally would pass
        // the test above and break this.
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        let partner = Player {
            is_impostor: true,
            ..far()
        };
        let muffle = voice_params(
            &State::default(),
            &ClientSettings::default(),
            &radio_lobby(),
            &impostor,
            &partner,
            RANGE,
            Some(partner.client_id),
        )
        .muffle
        .unwrap();
        assert_eq!(muffle.kind, FilterKind::HighPass);
        assert_eq!(muffle.frequency, RADIO_MUFFLE.0);
        assert_eq!(muffle.q, RADIO_MUFFLE.1);
    }

    #[test]
    fn a_muffle_does_not_lift_a_gain_that_is_already_down() {
        // `if (endGain === 1)` — a haunted ghost's volume is not overwritten by the vent
        // gain, and neither is a silenced player's zero.
        let lobby = LobbySettings {
            haunting: true,
            hear_impostors_in_vents: true,
            ..lobby()
        };
        let impostor = Player {
            is_impostor: true,
            in_vent: true,
            ..me()
        };
        let ghost = Player {
            is_dead: true,
            ..other()
        };
        let settings = ClientSettings {
            ghost_volume_as_impostor: 30.0,
            ..ClientSettings::default()
        };
        let params = voice_params(
            &State::default(),
            &settings,
            &lobby,
            &impostor,
            &ghost,
            RANGE,
            None,
        );
        assert_eq!(params.gain, 0.3);
        assert!(params.muffle.is_some());
    }

    #[test]
    fn silence_survives_the_muffle_branch() {
        let lobby = LobbySettings {
            meeting_ghost_only: true,
            ..lobby()
        };
        let vented_me = Player {
            in_vent: true,
            ..me()
        };
        assert_eq!(
            run(&State::default(), &lobby, &vented_me, &other()).gain,
            0.0
        );
    }

    #[test]
    fn dead_only_and_a_wall_do_not_argue() {
        // `deadOnly` zeroes the gain and the wall would return silence; both agree.
        let lobby = LobbySettings {
            dead_only: true,
            walls_block_audio: true,
            ..lobby()
        };
        let listener = Player {
            position: Vector2 { x: -7.0, y: 3.5 },
            ..me()
        };
        let speaker = Player {
            position: Vector2 { x: -5.5, y: 3.5 },
            ..other()
        };
        assert_eq!(
            run(&State::default(), &lobby, &listener, &speaker).gain,
            0.0
        );
    }

    #[test]
    fn every_lobby_setting_is_reachable_from_the_default() {
        // A guard on the struct: a setting added without a branch here would leave this
        // count stale, and the next person would find out from a bug report.
        let all = LobbySettings {
            haunting: true,
            hear_impostors_in_vents: true,
            impostors_hear_impostors_in_vent: true,
            impostor_radio_enabled: true,
            coms_sabotage: true,
            dead_only: true,
            meeting_ghost_only: true,
            hear_through_cameras: true,
            walls_block_audio: true,
            vision_hearing: true,
            max_distance: SHIPPED,
        };
        // Eleven settings: nine booleans this decision reads, plus the two that decide
        // the hearing range. The three public-lobby fields never reach the audio path.
        assert_ne!(all, LobbySettings::default());
    }

    // ---------------------------------------------------------------- hearing range

    /// The client's shipped fixed range.
    const SHIPPED: f64 = 5.32;

    fn ranged() -> LobbySettings {
        LobbySettings {
            max_distance: SHIPPED,
            ..lobby()
        }
    }

    #[test]
    fn the_fixed_range_is_used_when_vision_hearing_is_off() {
        assert_eq!(hearing_range(&ranged(), &me(), 3.0), SHIPPED);
    }

    #[test]
    fn vision_hearing_ties_the_crew_to_the_light_radius() {
        let lobby = LobbySettings {
            vision_hearing: true,
            ..ranged()
        };
        // Half a unit past what they can see, so a voice arrives just before its owner.
        assert_eq!(hearing_range(&lobby, &me(), 3.0), 3.5);
    }

    #[test]
    fn vision_hearing_leaves_impostors_on_the_fixed_range() {
        // The asymmetry the setting exists for: an impostor's vision is not cut by the
        // lights going out, so their hearing is not either.
        let lobby = LobbySettings {
            vision_hearing: true,
            ..ranged()
        };
        let impostor = Player {
            is_impostor: true,
            ..me()
        };
        assert_eq!(hearing_range(&lobby, &impostor, 0.5), SHIPPED);
    }

    #[test]
    fn a_collapsed_range_becomes_one_rather_than_silence() {
        // A blackout, or a frame where the light radius failed to read. Everyone going
        // silent reads as the app being broken rather than as the lights being out.
        let lobby = LobbySettings {
            vision_hearing: true,
            ..ranged()
        };
        assert_eq!(hearing_range(&lobby, &me(), 0.0), FALLBACK_RANGE);
        assert_eq!(hearing_range(&lobby, &me(), -1.0), FALLBACK_RANGE);
        // 0.1 + 0.5 = 0.6, which is the boundary and is replaced.
        assert_eq!(hearing_range(&lobby, &me(), 0.1), FALLBACK_RANGE);
    }

    #[test]
    fn a_fixed_range_of_zero_is_also_replaced() {
        // The floor applies to both branches, so a lobby that sends nonsense still works.
        let lobby = LobbySettings {
            max_distance: 0.0,
            ..lobby()
        };
        assert_eq!(hearing_range(&lobby, &me(), 3.0), FALLBACK_RANGE);
    }

    #[test]
    fn just_above_the_floor_is_kept() {
        let lobby = LobbySettings {
            max_distance: 0.61,
            ..lobby()
        };
        assert_eq!(hearing_range(&lobby, &me(), 3.0), 0.61);
    }
}
