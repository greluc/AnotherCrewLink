//! One frame, read out of a running game.
//!
//! This is the function gate G1 measures: given the same bytes the Electron reader saw,
//! it has to produce the same [`AmongUsState`], field for field, with float positions
//! within 1e-6.
//!
//! It takes `&dyn ProcessMemory` and returns `Result`, which is the whole reason the
//! parity run and the fuzzer can both drive it. No `unwrap`, no `as` on a value that came
//! out of the target, and every count bounded before it sizes anything.

use acl_types::map::MapType;

use crate::memory::{ProcessMemory, ReadError, read_pointer, resolve_chain};
use crate::mods::Mod;
use crate::offsets::Offsets;
use crate::state::{
    AmongUsState, GameState, OutfitOffsets, Player, hash_name, lobby_code_to_string, player_chain,
    read_outfit, top_level_chain,
};
use crate::systems::{self, SystemOffsets, Systems};

/// The most players a frame will read.
///
/// The Electron reader caps its loop at forty against a game that allows fifteen. The
/// same number, for the same reason: the count comes out of the target and a mod can
/// write anything there.
pub const MAX_PLAYERS_PER_FRAME: usize = 40;

/// The lobby code the game uses for a local practice game.
const LOCAL_GAME_CODE: i32 = 32;

/// What the reader needs to know that is not in the bundle.
#[derive(Debug, Clone)]
pub struct ReadContext {
    /// Where `GameAssembly` is loaded.
    pub module_base: u64,
    /// The state from the previous frame, for the fields that are differences.
    pub previous: Option<AmongUsState>,
    /// Which mod is loaded.
    pub loaded_mod: Mod,
    /// Which server the game is on. Read rarely and carried between frames.
    pub current_server: String,
    /// How many more frames the menu may be held for.
    ///
    /// `GameReader.menuUpdateTimer`, and it starts at 20 there. See the hold in
    /// [`read_state`] for what it is holding against.
    pub menu_update_timer: i32,
    /// The player table this reader last accepted as new.
    ///
    /// `GameReader.lastPlayerPtr`, starting at zero. Part of the same hold: an unchanged
    /// table is the signal that the game has not rebuilt its player list yet.
    pub last_player_ptr: u64,
}

impl ReadContext {
    /// A context for the first frame.
    ///
    /// The two hold fields start where `GameReader.ts` starts them, which is not at their
    /// defaults: the timer begins full, so the very first transition out of a menu gets
    /// the whole allowance.
    #[must_use]
    pub fn new(module_base: u64, loaded_mod: Mod) -> Self {
        Self {
            module_base,
            previous: None,
            loaded_mod,
            current_server: String::new(),
            menu_update_timer: MENU_HOLD_FRAMES,
            last_player_ptr: 0,
        }
    }
}

/// How long the reader may keep reporting a menu after the game says lobby.
///
/// `GameReader.menuUpdateTimer`'s reset value.
pub const MENU_HOLD_FRAMES: i32 = 20;

/// Reads one frame.
///
/// # Errors
///
/// Returns [`ReadError`] when the module base cannot be read, which is the one thing that
/// distinguishes a game that is running from a game that has closed. Everything else
/// degrades to a default rather than failing the frame, because a missing camera should
/// not silence a lobby.
#[allow(clippy::too_many_lines)]
// A frame has twenty fields and reading them is what this function is. Splitting it
// further would move the reads away from the comments that explain why each one is
// hedged, which is the part a reader of this code actually needs.
pub fn read_state(
    memory: &dyn ProcessMemory,
    offsets: &Offsets,
    context: &mut ReadContext,
) -> Result<AmongUsState, ReadError> {
    let base = context.module_base;

    // The one read that is allowed to fail the frame, and it is a fix of 2026-08-29.
    //
    // Every other read below is hedged, deliberately, because the Electron reader never
    // fails a frame either. The consequence was that this function was infallible for any
    // `ProcessMemory` at all -- including one whose every read fails -- and the doc comment
    // above promised errors it could not produce.
    //
    // What that cost: after the game exits, the reads all fall back, `gameState` lands on
    // 4, the lobby code reads as -1 and becomes `MENU`, and the helper emitted well-formed
    // menu frames at five hertz for ever. `Sampler::sample` therefore never returned
    // `None`, so the helper's re-attach arm was dead code and the game was opened exactly
    // once per helper process. A player who restarted Among Us had no proximity voice for
    // the rest of the session, and nothing said so -- the client simply believed they were
    // in the menu, left the voice room, and did not go back.
    //
    // One byte at the module base: that is the PE header, mapped for as long as the
    // process lives and unreadable the moment it does not. It costs one
    // `ReadProcessMemory` per frame against the several dozen below it. A process-table
    // scan would answer the same question for eighteen milliseconds, which is most of a
    // frame -- and would answer it wrongly for a game that has restarted into the same pid,
    // where the handle is stale but the name is back.
    let mut mapped = [0u8; 1];
    memory.read_exact(base, &mut mapped)?;

    // Not `?`. The Electron reader never fails a frame: a read that goes nowhere returns
    // undefined, the value falls back, and the frame goes out as a menu frame. A reader
    // that gives up instead disagrees with it on every frame where the game is starting,
    // closing or between rounds — which in a real session is thousands of them.
    let inner_net =
        follow(memory, base, offsets_chain(offsets, "innerNetClient.base")).unwrap_or(0);
    // Whether an in-game frame is a discussion is a second reading, of the meeting hud.
    // The cache pointer is what says a meeting is actually open rather than last used, and
    // 4 is the value the Electron reader falls back to for "no meeting".
    let meeting_hud = follow(memory, base, top_level_chain(offsets, "meetingHud")).unwrap_or(0);
    let meeting_hud_cache = if meeting_hud == 0 {
        0
    } else {
        follow(
            memory,
            meeting_hud,
            top_level_chain(offsets, "objectCachePtr"),
        )
        .unwrap_or(0)
    };
    let meeting_hud_state = if meeting_hud_cache == 0 {
        4
    } else {
        read_i32_at(
            memory,
            meeting_hud,
            first_offset(offsets, "meetingHudState"),
        )
        .unwrap_or(4)
    };

    let game_state = GameState::from_game(
        read_u32_at(memory, inner_net, inner_client_offset(offsets, "gameState")).unwrap_or(4),
        meeting_hud_state,
    );

    let lobby_code_int = if game_state == GameState::Menu {
        -1
    } else {
        read_i32_at(memory, inner_net, inner_client_offset(offsets, "gameId")).unwrap_or(-1)
    };
    // `GameReader.gameCode`, not yet the state's `lobbyCode`. The two differ: a local
    // game overwrites this from the host's name hash further down, and the state field
    // falls back to the literal "MENU" when this is empty.
    let game_code = if game_state == GameState::Menu {
        String::new()
    } else {
        lobby_code_to_string(lobby_code_int)
    };

    let host_id =
        read_u32_at(memory, inner_net, inner_client_offset(offsets, "hostId")).unwrap_or(0);
    let client_id =
        read_u32_at(memory, inner_net, inner_client_offset(offsets, "clientId")).unwrap_or(0);
    let is_local_game = lobby_code_int == LOCAL_GAME_CODE;

    let mut players = Vec::new();
    let mut local_player: Option<Player> = None;
    // Outside the block, because the menu hold below compares it against the previous
    // frame's. `GameReader.ts` reads it unconditionally for the same reason; here the
    // reads are inside a guard, so the value has to be hoisted rather than the reads.
    // Read unconditionally, and that is a fix of 2026-08-29 rather than a preference.
    // `GameReader.ts:270-272` reads all three before its guard, and the menu hold below
    // compares `all_players` against the previous frame's. Inside the guard the value was
    // zero on every menu frame, so `context.last_player_ptr == all_players` was `0 == 0`
    // on the first menu frame after a lobby and could never distinguish "the same player
    // table as before" from "no player table at all" -- which is exactly the question it
    // was written to ask.
    //
    // A player table that cannot be reached is an empty lobby, not a failed frame. The
    // Electron reader walks a garbage pointer, gets nothing back and pushes no players.
    let all_players_ptr =
        follow(memory, base, top_level_chain(offsets, "allPlayersPtr")).unwrap_or(0);
    let all_players = follow(
        memory,
        all_players_ptr,
        top_level_chain(offsets, "allPlayers"),
    )
    .unwrap_or(0);
    let count = read_u32_at(
        memory,
        all_players_ptr,
        first_offset(offsets, "playerCount"),
    )
    .unwrap_or(0) as usize;

    // Whether the block below ran at all. Two fields keep a starting value rather than a
    // read value when it does not, and the two are different numbers.
    //
    // `&& playerCount` is part of the Electron condition, so a lobby the reader can reach
    // but which reports nobody leaves the two starting values standing.
    let read_players = (!game_code.is_empty() || is_local_game) && count > 0;

    if read_players {
        let stride = memory.pointer_size() as u64;
        let mut entry =
            all_players.saturating_add_signed(first_offset(offsets, "playerAddrPtr").unwrap_or(0));

        for _ in 0..count.min(MAX_PLAYERS_PER_FRAME) {
            let chain = player_chain(offsets, "offsets").unwrap_or_default();
            let record = match resolve_chain(memory, entry, &chain) {
                Ok(address) if address != 0 => address,
                // A slot that is not filled in is skipped, not fatal: a lobby of four in a
                // list sized for fifteen has eleven of them.
                _ => {
                    entry = entry.saturating_add(stride);
                    continue;
                }
            };
            entry = entry.saturating_add(stride);

            if game_state == GameState::Menu {
                continue;
            }
            if let Some(player) = read_player(memory, offsets, record, client_id) {
                if player.is_local {
                    local_player = Some(player.clone());
                }
                players.push(player);
            }
        }
    }

    // `lightRadius` is a two-step chain in every bundle in the corpus.
    // `let lightRadius = 1;` — overwritten only inside the player block, and only when
    // there is a local player to read it from. A reader that falls back to -1 in all three
    // cases reports a blackout on every menu frame, and `lightRadiusChanged` with it.
    let light_radius = if read_players {
        local_player.as_ref().map_or(1.0, |player| {
            read_f32_chain(
                memory,
                player.object_ptr,
                top_level_chain(offsets, "lightRadius"),
            )
            // The read's own default, not the starting value: `readMemory(..., -1)`.
            .unwrap_or(-1.0)
        })
    } else {
        1.0
    };

    // The game options pointer does not resolve on every build: on Among Us 17.4.0 x86 it
    // comes back as zero, and every read through it is undefined. That went out as an
    // undefined map, where the collider lookup found nothing and reported that no wall was
    // ever in the way — so walls-block-audio silently did nothing for everyone on that
    // build. ShipStatus carries the same value from a different signature.
    let options_ptr =
        follow(memory, base, top_level_chain(offsets, "gameoptionsData")).unwrap_or(0);
    // `let maxPlayers = 10;`, overwritten by `read ?? 0` inside the player block. Outside
    // it the ten stands, which is why a menu frame reports ten rather than nobody.
    let max_players = if read_players {
        read_u8_at(
            memory,
            options_ptr,
            first_offset(offsets, "gameOptions_MaxPLayers"),
        )
        .unwrap_or(0)
    } else {
        10
    };
    let map = read_u8_at(
        memory,
        options_ptr,
        first_offset(offsets, "gameOptions_MapId"),
    )
    .or_else(|| {
        let ship = follow(memory, base, top_level_chain(offsets, "shipStatus")).ok()?;
        read_u8_at(memory, ship, first_offset(offsets, "shipStatus_map"))
    })
    // Never absent. An undefined map is what made the collider lookup silently
    // useless, so the unknown value is a value.
    .unwrap_or(6);

    let previous_light = context
        .previous
        .as_ref()
        .map_or(f32::NAN, |state| state.light_radius);

    let old_game_state = context
        .previous
        .as_ref()
        .map_or(GameState::Unknown, |state| state.game_state);

    // Keep reporting the menu for a few frames after the game says lobby.
    //
    // Leaving the menu into a lobby is not one event in memory. The client reports the
    // new state before it has rebuilt the player table, so for a handful of frames the
    // table is either the previous one -- same pointer -- or a new one the local player
    // is not in yet. Announcing a lobby then announces a lobby full of the last game's
    // players, or one with nobody in it.
    //
    // So `GameReader.ts` holds the menu until the table actually changes, for at most
    // twenty frames so that a game which never rebuilds it is not held forever. Found by
    // the corpus: without this the two readers disagreed on `gameState`, `lobbyCode` and
    // `oldGameState` across every menu-to-lobby transition in a recording, which is
    // sixteen frames of a client showing the wrong thing rather than a rounding argument.
    let game_state = if old_game_state == GameState::Menu
        && game_state == GameState::Lobby
        && context.menu_update_timer > 0
        && (context.last_player_ptr == all_players || !players.iter().any(|player| player.is_local))
    {
        context.menu_update_timer -= 1;
        GameState::Menu
    } else {
        context.menu_update_timer = MENU_HOLD_FRAMES;
        context.last_player_ptr = all_players;
        game_state
    };

    // A local game has no code to decode -- `lobby_code_int` is the sentinel 32 -- so the
    // Electron reader shows the host's name hash instead, and players read it to each
    // other. Reproduced rather than skipped: without it the two readers disagree on
    // `lobbyCode` for every LAN game, which is precisely the sort of divergence gate G1
    // exists to catch.
    //
    // Held menus take the literal, whatever the code decoded to: `lobbyCode = state !==
    // MENU ? gameCode || 'MENU' : 'MENU'`, and by this point `state` may have been
    // overridden above.
    let lobby_code = if game_state == GameState::Menu {
        "MENU".to_owned()
    } else {
        lobby_code_for(&players, host_id, is_local_game, game_code)
    };

    // `gameState: lobbyCode === 'MENU' ? GameState.MENU : state`. The Electron reader
    // overrides the state it read when there is no code to show, and a reader that does
    // not differs on every frame between leaving a game and reaching the menu.
    let game_state = if lobby_code == "MENU" {
        GameState::Menu
    } else {
        game_state
    };

    // Sabotage, doors and cameras are read only during a round, as `GameReader.ts` does:
    // the systems dictionary and the door table say nothing in a lobby or a meeting.
    let systems = if game_state == GameState::Tasks {
        let ship = follow(memory, base, top_level_chain(offsets, "shipStatus")).unwrap_or(0);
        let minigame = follow(memory, base, top_level_chain(offsets, "miniGame")).unwrap_or(0);
        systems::read_systems(
            memory,
            ship,
            minigame,
            MapType::from_game(u32::from(map)),
            local_player.as_ref().map(|player| (player.x, player.y)),
            &system_offsets(offsets),
        )
    } else {
        Systems::default()
    };

    Ok(AmongUsState {
        game_state,
        old_game_state,
        lobby_code_int,
        lobby_code,
        players,
        is_host: host_id == client_id && host_id != 0,
        client_id,
        host_id,
        coms_sabotaged: systems.coms_sabotaged,
        current_camera: systems.current_camera,
        map: u32::from(map),
        light_radius,
        // `lightRadius != this.lastState?.lightRadius` — an exact comparison, not a
        // tolerance. A tolerance here would swallow a change the Electron reader reports,
        // and the two would disagree on the frame the lights come back up. NaN compares
        // unequal to itself, so the first frame reports a change, which is what comparing
        // against an absent previous state does in JavaScript too.
        #[allow(
            clippy::float_cmp,
            reason = "the Electron reader's `!=` is the specification"
        )]
        light_radius_changed: light_radius != previous_light,
        closed_doors: systems.closed_doors,
        current_server: context.current_server.clone(),
        max_players: u32::from(max_players),
        mod_id: context.loaded_mod.id().to_owned(),
        old_meeting_hud: offsets.old_meeting_hud,
    })
}

/// Walks a chain the bundle may not have.
fn follow(
    memory: &dyn ProcessMemory,
    base: u64,
    chain: Option<Vec<i64>>,
) -> Result<u64, ReadError> {
    let chain = chain.ok_or(ReadError::Chain {
        step: 0,
        reason: "this build's offsets do not have the field",
    })?;
    let at = resolve_chain(memory, base, &chain)?;
    read_pointer(memory, at)
}

fn read_u32_at(memory: &dyn ProcessMemory, base: u64, offset: Option<i64>) -> Option<u32> {
    let at = base.checked_add_signed(offset?)?;
    let mut raw = [0u8; 4];
    memory.read_exact(at, &mut raw).ok()?;
    Some(u32::from_le_bytes(raw))
}

fn read_i32_at(memory: &dyn ProcessMemory, base: u64, offset: Option<i64>) -> Option<i32> {
    // The lobby code is stored as a signed value and read as one; reinterpreting the bits
    // is the point rather than a conversion.
    read_u32_at(memory, base, offset).map(|value| i32::from_ne_bytes(value.to_ne_bytes()))
}

fn read_u8_at(memory: &dyn ProcessMemory, base: u64, offset: Option<i64>) -> Option<u8> {
    let at = base.checked_add_signed(offset?)?;
    let mut raw = [0u8; 1];
    memory.read_exact(at, &mut raw).ok()?;
    raw.first().copied()
}

/// One coordinate, rounded the way the Electron reader rounds it.
///
/// ```text
/// const x_round = parseFloat(x?.toFixed(4));
/// x: x_round || x || 999,
/// ```
///
/// The rounding is not cosmetic for the gate: four decimal places move a coordinate by up
/// to 5e-5, and the tolerance is 1e-6.
///
/// The `||` chain is reproduced rather than tidied, quirk included. `x_round` is falsy
/// when it is zero, so a player standing at exactly 0.0 falls through to the raw value,
/// which is also falsy, and is reported at 999. That is what the Electron reader does, and
/// the gate compares the two exactly.
fn position(read: Option<f32>) -> f64 {
    let raw = f64::from(read.unwrap_or(0.0));
    let rounded = (raw * 10_000.0).round() / 10_000.0;
    #[allow(
        clippy::float_cmp,
        reason = "JavaScript truthiness is the specification here"
    )]
    // `is_finite` on each, and that is the other half of the same quirk. A NaN read out of
    // a half-written player record is falsy in JavaScript -- `if (NaN)` is false -- so the
    // Electron reader falls through both branches and reports 999, which is the "nowhere
    // near you" sentinel every distance check already understands. Rust's `!=` says a NaN
    // is not zero, so the port returned the NaN.
    //
    // It does not stay a coordinate. The difference between two positions is a NaN, the
    // hearing range derived from it is a NaN, and `Panner::distance_gain` clamps against
    // it -- `f64::clamp` asserts `min <= max`, which a NaN fails, and it runs on the mixing
    // thread of a `panic = "abort"` build. A single unlucky frame took the whole client
    // down. Infinity does not panic and is no better: every gain becomes zero and that
    // player is silent for as long as it lasts.
    if rounded.is_finite() && rounded != 0.0 {
        rounded
    } else if raw.is_finite() && raw != 0.0 {
        raw
    } else {
        999.0
    }
}

/// Walks a chain and reads a float from where it lands.
///
/// The single-offset readers beside this one are for fields the bundles really do give as
/// one step. Everything with a longer chain has to come through here: resolving only the
/// first step of `[140, 16]` reads the pointer, not the value.
fn read_f32_chain(memory: &dyn ProcessMemory, base: u64, chain: Option<Vec<i64>>) -> Option<f32> {
    let at = resolve_chain(memory, base, &chain?).ok()?;
    let mut bytes = [0u8; 4];
    memory.read_exact(at, &mut bytes).ok()?;
    Some(f32::from_le_bytes(bytes))
}

/// The byte offset of a named member of the player struct, and how wide it is.
///
/// `objectPtr`, `taskPtr`, `outfitsPtr`, `rolePtr`, `disconnected`, `dead` and `id` are
/// **not** fields of `offsets.player`. They are entries of `offsets.player.struct`, a
/// structron layout the Electron reader parses out of the record buffer, and until
/// 2026-08-24 this reader looked for them in the wrong place. `player_chain` returned
/// `None` for every one of them, `follow` turned that into an error, and `read_player`
/// returned `None` for every record — so against any real bundle the player list came out
/// empty. No test caught it because no test had ever populated one.
///
/// The layout is sequential, as structron builds it: `SKIP` advances by its `skip`, and a
/// typed member occupies its own width. Across all 44 bundles in the corpus only three
/// types appear — `SKIP`, `UINT` and `BYTE` — but the widths of structron's whole set are
/// here so an older or newer bundle is laid out rather than silently mismeasured.
fn struct_member(offsets: &Offsets, name: &str) -> Option<(i64, usize)> {
    let mut at: i64 = 0;
    for field in &offsets.player.fields {
        let width = match field.kind.as_str() {
            "SKIP" => field.skip.unwrap_or(0),
            "BYTE" | "CHAR" => 1,
            "SHORT" | "SHORT_BE" | "USHORT" | "USHORT_BE" => 2,
            // INT, UINT, FLOAT and their big-endian twins.
            _ => 4,
        };
        if field.kind != "SKIP" && field.name == name {
            return Some((at, usize::try_from(width).ok()?));
        }
        at = at.checked_add(width)?;
    }
    None
}

/// Reads a named member of the player struct as an unsigned integer of its declared width.
fn struct_value(
    memory: &dyn ProcessMemory,
    record: u64,
    offsets: &Offsets,
    name: &str,
) -> Option<u32> {
    let (offset, width) = struct_member(offsets, name)?;
    let address = record.checked_add_signed(offset)?;
    let mut bytes = [0u8; 4];
    memory.read_exact(address, bytes.get_mut(..width)?).ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Reads a named member of the player struct as a pointer.
///
/// The struct declares these as `UINT` even in a 64-bit bundle, and the Electron reader
/// re-reads them at the same offset as pointers when the game is 64-bit. This does the
/// same: the offset comes from the layout, the width from the process.
fn struct_pointer(
    memory: &dyn ProcessMemory,
    record: u64,
    offsets: &Offsets,
    name: &str,
) -> Option<u64> {
    let (offset, _) = struct_member(offsets, name)?;
    read_pointer(memory, record.checked_add_signed(offset)?).ok()
}

/// Reads one player record.
///
/// Returns `None` rather than an error: a record that cannot be read is a slot the game
/// has not filled in, and one bad player should not cost the frame.
fn read_player(
    memory: &dyn ProcessMemory,
    offsets: &Offsets,
    record: u64,
    local_client_id: u32,
) -> Option<Player> {
    // Zero rather than a bail-out. The Electron reader's `parsePlayer` gives up only when
    // it has no struct definition at all; a record whose object pointer is zero still
    // becomes a player, with every read through that pointer failing and the player coming
    // out bugged. Dropping it here loses a player the other reader keeps, which is one
    // frame of the corpus and would be a missing voice in a lobby being torn down.
    let object_ptr = struct_pointer(memory, record, offsets, "objectPtr").unwrap_or(0);
    let task_ptr = struct_pointer(memory, record, offsets, "taskPtr").unwrap_or(0);
    let outfits_ptr = struct_pointer(memory, record, offsets, "outfitsPtr").unwrap_or(0);
    let role_ptr = struct_pointer(memory, record, offsets, "rolePtr").unwrap_or(0);

    // The in-game player id, which meetings and votes are keyed by. Carried as zero for
    // every player until 2026-08-24.
    let id = struct_value(memory, record, offsets, "id").unwrap_or(0);
    let client_id = read_u32_at(memory, object_ptr, first_player_offset(offsets, "clientId"));
    let disconnected = struct_value(memory, record, offsets, "disconnected").unwrap_or(0) != 0;
    // `clientId === LocalclientId`, where an unread id is `undefined` and matches nothing.
    let is_local = client_id == Some(local_client_id) && !disconnected;

    // The local player's position lives in a different field from everyone else's: theirs
    // is authoritative, the others are interpolated from the network.
    let (x_field, y_field) = if is_local {
        ("localX", "localY")
    } else {
        ("remoteX", "remoteY")
    };
    // Two steps in most bundles and four in some, so taking only the first lands on a
    // pointer rather than on the coordinate it points at. Positions are what proximity
    // chat is, which makes this the most expensive place in the reader to get wrong.
    // Kept as `Option`s rather than collapsed: a read that failed is one of the four
    // conditions that make a player bugged, and `position` cannot tell the caller which
    // of its answers came from nothing.
    let read_x = read_f32_chain(memory, object_ptr, player_chain(offsets, x_field));
    let read_y = read_f32_chain(memory, object_ptr, player_chain(offsets, y_field));

    let current_outfit = read_u32_at(
        memory,
        object_ptr,
        first_player_offset(offsets, "currentOutfit"),
    )
    .unwrap_or(0);
    let is_dummy = read_u8_at(memory, object_ptr, first_player_offset(offsets, "isDummy"))
        .map(|byte| byte != 0);
    let in_vent =
        read_u8_at(memory, object_ptr, first_player_offset(offsets, "inVent")).unwrap_or(0) != 0;

    let outfit = read_outfit(
        memory,
        outfits_ptr,
        current_outfit,
        OutfitOffsets {
            player_name: outfit_offset(offsets, "playerName"),
            color_id: outfit_offset(offsets, "colorId"),
            hat_id: outfit_offset(offsets, "hatId"),
            skin_id: outfit_offset(offsets, "skinId"),
            visor_id: outfit_offset(offsets, "visorId"),
        },
    )
    .ok()?;

    // The name comes off the outfit, and only off the outfit.
    //
    // The bundle also carries a `player.nameText` chain and this used to prefer it. **The
    // Electron reader does not**: `GameReader.ts:1006-1009` is commented out, and even as
    // written it applied only to one mod and only to a player whose colour had not
    // resolved. Following it unconditionally on Among Us 17.4.0 x86 read the same
    // non-string for every player -- five identical pages of mojibake where five names
    // should have been -- because the chain does not land on a name on a stock build.
    //
    // If a mod ever needs it back, it needs the two conditions with it.
    let name = outfit.name.clone();

    let role_team = read_u32_at(memory, role_ptr, first_player_offset(offsets, "roleTeam"));

    // A player the reader could not make sense of, parked off the map.
    //
    // `GameReader.ts` sets both coordinates to 9999 and raises this flag, and 9999 is not
    // a coordinate any map reaches -- so proximity puts them silently out of everyone's
    // range instead of at the origin, which is inside Cafeteria on the Skeld. Found by
    // the corpus: the Rust reader had the flag hard-coded false and reported the fallback
    // 999 instead, which is also off the map but is not the same number, and the gate
    // compares numbers.
    //
    // Two of the Electron reader's four conditions. The other two are both about the
    // colour -- `color < 0 || color > playercolors.length` -- and neither can be asked
    // here yet. The upper bound needs the colour table this reader does not read:
    // `readPlayerColors` in `GameReader.ts`, which also supplies the rainbow-colour
    // substitution that is missing for the same reason. The lower bound needs nothing but
    // a signed colour, and `color_id` is carried unsigned, so a negative colour arrives as
    // a very large one -- which is to say it turns into the upper-bound case, and the two
    // are owed together or not at all. No recording in the corpus produces either.
    let bugged = read_x.is_none() || read_y.is_none() || disconnected;
    let (x, y) = if bugged {
        (9999.0, 9999.0)
    } else {
        (position(read_x), position(read_y))
    };

    Some(Player {
        ptr: record,
        id: u8::try_from(id).unwrap_or(u8::MAX),
        client_id,
        name_hash: hash_name(&name),
        name,
        color_id: outfit.color_id,
        hat_id: outfit.hat_id,
        // Only the eight pre-outfit bundles carry a pet in the struct; on every other
        // build the Electron reader leaves it unset. Zero either way.
        pet_id: struct_value(memory, record, offsets, "pet").unwrap_or(0),
        skin_id: outfit.skin_id,
        visor_id: outfit.visor_id,
        disconnected,
        // `data.impostor == 1`, and the Electron reader assigns the role team to that
        // field. Exactly one, not merely non-zero: a modded role on a third team would
        // otherwise be announced as an impostor. Eight bundles in the corpus carry
        // `impostor` in the struct instead, from before roles existed, and those win.
        is_impostor: struct_value(memory, record, offsets, "impostor")
            .or(role_team)
            .is_some_and(|team| team == 1),
        is_dead: struct_value(memory, record, offsets, "dead").unwrap_or(0) != 0,
        task_ptr,
        object_ptr,
        is_local,
        shifted_color: outfit.shifted_color,
        bugged,
        x,
        y,
        in_vent,
        is_dummy,
    })
}

/// A chain from the bundle's `innerNetClient` block.
fn offsets_chain(offsets: &Offsets, dotted: &str) -> Option<Vec<i64>> {
    let (block, field) = dotted.split_once('.')?;
    let value = offsets.rest.get(block)?.get(field)?;
    match value {
        serde_json::Value::Array(steps) => steps.iter().map(serde_json::Value::as_i64).collect(),
        serde_json::Value::Number(number) => number.as_i64().map(|one| vec![one]),
        _ => None,
    }
}

/// A single offset from the bundle's `innerNetClient` block.
fn inner_client_offset(offsets: &Offsets, field: &str) -> Option<i64> {
    offsets.rest.get("innerNetClient")?.get(field)?.as_i64()
}

/// The first step of a top-level chain, for a field the reader uses as one offset.
fn first_offset(offsets: &Offsets, field: &str) -> Option<i64> {
    top_level_chain(offsets, field)?.first().copied()
}

/// As [`first_offset`], for the player block.
fn first_player_offset(offsets: &Offsets, field: &str) -> Option<i64> {
    player_chain(offsets, field)?.first().copied()
}

/// An offset from the player's `outfit` block, or -1 when the build has no such field.
fn outfit_offset(offsets: &Offsets, field: &str) -> i64 {
    offsets
        .player
        .rest
        .get("outfit")
        .and_then(|outfit| outfit.get(field))
        .and_then(|value| match value {
            serde_json::Value::Array(steps) => steps.first().and_then(serde_json::Value::as_i64),
            serde_json::Value::Number(number) => number.as_i64(),
            _ => None,
        })
        .unwrap_or(-1)
}

/// Gathers the offsets [`systems::read_systems`] needs out of the bundle.
///
/// Each is optional: a bundle for an older build may not carry all of them, and a missing
/// one skips that reading rather than failing the frame. `playerCount` and `playerAddrPtr`
/// are reused for the door table's length and first element, which is what
/// `GameReader.ts` does — the door table has the same shape as the player table.
fn system_offsets(offsets: &Offsets) -> SystemOffsets {
    SystemOffsets {
        systems: top_level_chain(offsets, "shipStatus_systems"),
        hud_override_active: top_level_chain(offsets, "HudOverrideSystemType_isActive"),
        mira_completed_consoles: top_level_chain(offsets, "hqHudSystemType_CompletedConsoles"),
        decon_lower_open: top_level_chain(offsets, "deconDoorLowerOpen"),
        decon_upper_open: top_level_chain(offsets, "deconDoorUpperOpen"),
        all_doors: top_level_chain(offsets, "shipstatus_allDoors"),
        count: top_level_chain(offsets, "playerCount"),
        first_element: top_level_chain(offsets, "playerAddrPtr"),
        door_is_open: top_level_chain(offsets, "door_isOpen"),
        object_cache: top_level_chain(offsets, "objectCachePtr"),
        current_camera: top_level_chain(offsets, "planetSurveillanceMinigame_currentCamera"),
        camera_count: top_level_chain(offsets, "planetSurveillanceMinigame_camarasCount"),
        filtered_rooms: top_level_chain(offsets, "surveillanceMinigame_FilteredRoomsCount"),
    }
}

/// The state's `lobbyCode`, from the decoded code and the players.
///
/// Two Electron behaviours live here, and both were missing from this port until
/// 2026-08-24.
///
/// A local game has no code to decode — `lobbyCodeInt` is the sentinel 32 — so the
/// Electron reader shows the host's name hash instead, and players read that to each
/// other. Without it the two readers disagree on `lobbyCode` for every LAN game.
///
/// And the empty string never reaches the state: `state !== MENU ? this.gameCode ||
/// 'MENU' : 'MENU'` ends at the literal on both branches.
fn lobby_code_for(
    players: &[Player],
    host_id: u32,
    is_local_game: bool,
    game_code: String,
) -> String {
    let code = if is_local_game {
        players
            .iter()
            .find(|player| player.client_id == Some(host_id))
            // JavaScript's `%` takes the sign of the dividend, and so does Rust's, so a
            // negative hash gives a negative code on both sides.
            .map_or(game_code, |host| (host.name_hash % 99_999).to_string())
    } else {
        game_code
    };
    if code.is_empty() {
        "MENU".to_owned()
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn player(client_id: u32, name: &str) -> Player {
        Player {
            client_id: Some(client_id),
            name: name.to_owned(),
            name_hash: crate::state::hash_name(name),
            ..Player::default()
        }
    }

    #[test]
    fn an_ordinary_game_keeps_its_decoded_code() {
        let players = vec![player(1, "Player1")];
        assert_eq!(
            lobby_code_for(&players, 1, false, "ABCDEF".to_owned()),
            "ABCDEF"
        );
    }

    #[test]
    fn an_empty_code_becomes_the_literal_menu() {
        // `this.gameCode || 'MENU'`. An empty string never reaches the state, so a reader
        // that leaves it empty differs from the Electron one on every menu frame.
        assert_eq!(lobby_code_for(&[], 0, false, String::new()), "MENU");
    }

    #[test]
    fn a_local_game_shows_the_hosts_name_hash() {
        // What players read to each other on a LAN, because there is no code to decode.
        let expected = (crate::state::hash_name("Player1") % 99_999).to_string();
        let players = vec![player(3, "Someone"), player(7, "Player1")];
        assert_eq!(
            lobby_code_for(&players, 7, true, "whatever".to_owned()),
            expected
        );
    }

    #[test]
    fn a_local_game_without_its_host_keeps_what_it_had() {
        // The host can be missing from the table for a frame. The Electron reader leaves
        // `gameCode` as it was rather than blanking it, so this does too.
        let players = vec![player(3, "Someone")];
        assert_eq!(lobby_code_for(&players, 7, true, "kept".to_owned()), "kept");
    }

    #[test]
    fn a_negative_name_hash_gives_a_negative_code() {
        // JavaScript's `%` takes the sign of the dividend. A Cyrillic name overflows into
        // the top bit, so this is reachable rather than theoretical.
        let name = "\u{43d}\u{435}\u{433}\u{43e}\u{434}\u{44f}\u{439}";
        assert!(crate::state::hash_name(name) < 0);
        let code = lobby_code_for(&[player(1, name)], 1, true, String::new());
        assert!(code.starts_with('-'), "got {code}");
    }
    use crate::sparse::SparseProcess;

    fn offsets() -> Offsets {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/offsets/offsets__x86__V2026.8.18__offsets.json"),
        )
        .expect("a fixture");
        serde_json::from_str(&text).expect("parses")
    }

    const BASE: u64 = 0x1000_0000;

    fn context() -> ReadContext {
        ReadContext::new(BASE, Mod::None)
    }

    /// A process that is running and has nothing else mapped.
    ///
    /// The module base carries a byte because a live process always has its PE header
    /// mapped, and since 2026-08-29 that is the one read `read_state` will fail a frame
    /// for -- it is what separates a game that is between rounds from a game that has
    /// closed. A fixture with nothing at the base describes a process that does not exist.
    fn running(is_64bit: bool) -> SparseProcess {
        SparseProcess::new(is_64bit).with_region(BASE, vec![0x4d])
    }

    #[test]
    fn an_empty_process_reads_as_a_menu_frame_rather_than_failing() {
        // This test asserted the opposite until 2026-08-24, and the opposite was wrong.
        // The Electron reader never fails a frame: a read that goes nowhere returns
        // undefined, the value falls back, the lobby code comes out as "MENU" and that
        // forces the state. A reader that gives up instead disagrees with it on every
        // frame where the game is starting, closing or between rounds — thousands of them
        // in a real session, and gate G1 counted every one.
        let empty = running(false);
        let state = read_state(&empty, &offsets(), &mut context()).expect("a frame, not an error");
        assert_eq!(state.game_state, GameState::Menu);
        assert_eq!(state.lobby_code, "MENU");
        assert!(state.players.is_empty());
        // And the two fields that keep a starting value rather than a read one.
        assert_eq!(state.max_players, 10);
        assert!((state.light_radius - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_process_that_is_gone_fails_the_frame_rather_than_reading_as_a_menu() {
        // The distinction this whole `Result` exists for, and it did not exist until
        // 2026-08-29: every read in `read_state` was hedged, so the function was infallible
        // for any process at all. A game that had closed read as `gameState` 4, a lobby
        // code of -1, and therefore `MENU` -- a well-formed menu frame, emitted at five
        // hertz for ever.
        //
        // What that cost is not a blank overlay. `Sampler::sample` never returned `None`,
        // so the helper's re-attach arm was dead code and the game was opened exactly once
        // per helper process; and the client reads `MENU` as "not in a lobby", so it left
        // the voice room and never went back. A player who restarted Among Us had no
        // proximity voice for the rest of the session, with nothing anywhere to say why.
        let gone = SparseProcess::new(false);
        assert!(
            read_state(&gone, &offsets(), &mut context()).is_err(),
            "a process with nothing mapped at its module base is not a running game"
        );

        // And the case it must not be confused with: a game that is running and simply not
        // in a lobby yet. One byte at the base is the difference.
        assert!(read_state(&running(false), &offsets(), &mut context()).is_ok());
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        // Item 7 of the plan, and the reason the whole reader takes a trait: a fuzzer that
        // has to open a process finds nothing. This is a deterministic stand-in for one,
        // and it runs on stable in CI. The libfuzzer target in fuzz/ explores further.
        let offsets = offsets();
        let mut seed = 0x1234_5678_9abc_def0u64;
        for round in 0..2000 {
            // xorshift, so the corpus is the same on every machine and a failure is
            // reproducible from the round number alone.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            // Truncation is the point here: this is a corpus generator, not a decoder.
            #[allow(clippy::cast_possible_truncation)]
            let length = 64 + usize::try_from(seed % 4096).unwrap_or(0);
            #[allow(clippy::cast_possible_truncation)]
            let bytes: Vec<u8> = (0..length)
                .map(|index| {
                    let shifted = (seed >> (index % 56)) as u8;
                    shifted.wrapping_add(index as u8)
                })
                .collect();

            for is_64 in [true, false] {
                let process = SparseProcess::from_arbitrary(&bytes, is_64);
                // The only requirement is that it returns. A panic here is the bug this
                // exists to find.
                let _ = read_state(&process, &offsets, &mut context());
                let _ = round;
            }
        }
    }

    #[test]
    fn a_self_referential_chain_is_refused_rather_than_followed_forever() {
        // Reachable from a modded or corrupted game today. The refusal happens in
        // `resolve_chain`, which is what stops the walk; the frame then degrades to a
        // menu frame rather than failing, because that is what the Electron reader does
        // with a pointer that goes nowhere.
        let offsets = offsets();
        let process = SparseProcess::new(false).with_pointer(0x1000_0000, 0x1000_0000);
        let state = read_state(&process, &offsets, &mut context()).expect("a frame, not an error");
        assert_eq!(state.game_state, GameState::Menu);
        assert!(state.players.is_empty());
    }

    #[test]
    #[allow(clippy::items_after_statements)]
    fn reads_the_fields_that_do_not_need_a_player_list() {
        // A minimal process: the client pointer resolves, the state says menu, and that is
        // enough for a frame. It is what the reader sees between games.
        let offsets = offsets();
        // The bundle ships `innerNetClient.base` as [-1, …]: the first step is a hole the
        // pattern scanner fills. Filling it here is what the real path does, and building
        // the chain without doing so is how this test first failed.
        let mut offsets = offsets;
        let scanned = 0x1000_0100u64;
        assert!(crate::resolve::fill_first_step_for_test(
            &mut offsets,
            "innerNetClient.base",
            scanned
        ));

        let chain = offsets_chain(&offsets, "innerNetClient.base").expect("the bundle has it");
        let state_offset = inner_client_offset(&offsets, "gameState").expect("the bundle has it");

        let inner = 0x2000u64;
        // `running`, so the module base is mapped: `read_state` reads one byte there to
        // tell a game that is between rounds from one that has closed.
        let mut process = running(false);
        // Walk the chain, laying down a pointer at each step it dereferences.
        let mut address = BASE;
        for (index, step) in chain.iter().enumerate() {
            address = address.checked_add_signed(*step).expect("in range");
            if index + 1 < chain.len() {
                let next = 0x3000 + (index as u64) * 0x100;
                process = process.with_pointer(address, next);
                address = next;
            }
        }
        process = process.with_pointer(address, inner);
        process = process.with_region(
            inner.checked_add_signed(state_offset).expect("in range"),
            3u32.to_le_bytes(),
        );

        let state = read_state(&process, &offsets, &mut context()).expect("a frame");
        assert_eq!(state.game_state, GameState::Menu);
        // In the menu the Electron reader reports the literal "MENU" rather than an
        // empty string, and the state field is compared exactly.
        assert_eq!(state.lobby_code, "MENU");
        assert_eq!(state.lobby_code_int, -1);
        assert!(state.players.is_empty());
        // And the map is never absent, because an undefined map is what made the collider
        // lookup silently report that no wall was ever in the way.
        assert_eq!(state.map, 6);
    }
}
