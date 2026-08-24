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

use crate::dotnet::read_string;
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
}

/// Reads one frame.
///
/// # Errors
///
/// Returns [`ReadError`] when a read the frame cannot do without fails — the client
/// pointer, or the game state. Everything else degrades to a default rather than failing
/// the frame, because a missing camera should not silence a lobby.
#[allow(clippy::too_many_lines)]
// A frame has twenty fields and reading them is what this function is. Splitting it
// further would move the reads away from the comments that explain why each one is
// hedged, which is the part a reader of this code actually needs.
pub fn read_state(
    memory: &dyn ProcessMemory,
    offsets: &Offsets,
    context: &ReadContext,
) -> Result<AmongUsState, ReadError> {
    let base = context.module_base;

    let inner_net = follow(memory, base, offsets_chain(offsets, "innerNetClient.base"))?;
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

    if !game_code.is_empty() || is_local_game {
        let all_players_ptr = follow(memory, base, top_level_chain(offsets, "allPlayersPtr"))?;
        let all_players = follow(
            memory,
            all_players_ptr,
            top_level_chain(offsets, "allPlayers"),
        )?;
        let count = read_u32_at(
            memory,
            all_players_ptr,
            first_offset(offsets, "playerCount"),
        )
        .unwrap_or(0) as usize;

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
    let light_radius = local_player
        .as_ref()
        .and_then(|player| {
            read_f32_chain(
                memory,
                player.object_ptr,
                top_level_chain(offsets, "lightRadius"),
            )
        })
        .unwrap_or(-1.0);

    // The game options pointer does not resolve on every build: on Among Us 17.4.0 x86 it
    // comes back as zero, and every read through it is undefined. That went out as an
    // undefined map, where the collider lookup found nothing and reported that no wall was
    // ever in the way — so walls-block-audio silently did nothing for everyone on that
    // build. ShipStatus carries the same value from a different signature.
    let options_ptr =
        follow(memory, base, top_level_chain(offsets, "gameoptionsData")).unwrap_or(0);
    let max_players = read_u8_at(
        memory,
        options_ptr,
        first_offset(offsets, "gameOptions_MaxPLayers"),
    )
    .unwrap_or(0);
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

    // A local game has no code to decode -- `lobby_code_int` is the sentinel 32 -- so the
    // Electron reader shows the host's name hash instead, and players read it to each
    // other. Reproduced rather than skipped: without it the two readers disagree on
    // `lobbyCode` for every LAN game, which is precisely the sort of divergence gate G1
    // exists to catch.
    let lobby_code = lobby_code_for(&players, host_id, is_local_game, game_code);

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
        old_game_state: context
            .previous
            .as_ref()
            .map_or(GameState::Unknown, |state| state.game_state),
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
    if rounded != 0.0 {
        rounded
    } else if raw != 0.0 {
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
    let object_ptr = struct_pointer(memory, record, offsets, "objectPtr")?;
    let task_ptr = struct_pointer(memory, record, offsets, "taskPtr").unwrap_or(0);
    let outfits_ptr = struct_pointer(memory, record, offsets, "outfitsPtr").unwrap_or(0);
    let role_ptr = struct_pointer(memory, record, offsets, "rolePtr").unwrap_or(0);

    // The in-game player id, which meetings and votes are keyed by. Carried as zero for
    // every player until 2026-08-24.
    let id = struct_value(memory, record, offsets, "id").unwrap_or(0);
    let client_id = read_u32_at(memory, object_ptr, first_player_offset(offsets, "clientId"))?;
    let disconnected = struct_value(memory, record, offsets, "disconnected").unwrap_or(0) != 0;
    let is_local = client_id == local_client_id && !disconnected;

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
    let x = position(read_f32_chain(
        memory,
        object_ptr,
        player_chain(offsets, x_field),
    ));
    let y = position(read_f32_chain(
        memory,
        object_ptr,
        player_chain(offsets, y_field),
    ));

    let current_outfit = read_u32_at(
        memory,
        object_ptr,
        first_player_offset(offsets, "currentOutfit"),
    )
    .unwrap_or(0);
    let is_dummy =
        read_u8_at(memory, object_ptr, first_player_offset(offsets, "isDummy")).unwrap_or(0) != 0;
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

    // A build with a direct name pointer uses it; the rest read the name off outfit zero.
    let name = match player_chain(offsets, "nameText") {
        Some(chain) if !chain.is_empty() => follow(memory, object_ptr, Some(chain))
            .ok()
            .and_then(|at| read_pointer(memory, at).ok())
            .and_then(|pointer| read_string(memory, pointer, 1000).ok())
            .unwrap_or_else(|| outfit.name.clone()),
        _ => outfit.name.clone(),
    };

    let role_team = read_u32_at(memory, role_ptr, first_player_offset(offsets, "roleTeam"));

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
        bugged: false,
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
            .find(|player| player.client_id == host_id)
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
            client_id,
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

    fn context() -> ReadContext {
        ReadContext {
            module_base: 0x1000_0000,
            previous: None,
            loaded_mod: Mod::None,
            current_server: String::new(),
        }
    }

    #[test]
    fn an_empty_process_fails_the_frame_rather_than_inventing_one() {
        // The client pointer is what a frame cannot do without. Everything downstream of
        // it degrades; this does not.
        let empty = SparseProcess::new(false);
        assert!(read_state(&empty, &offsets(), &context()).is_err());
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
                let _ = read_state(&process, &offsets, &context());
                let _ = round;
            }
        }
    }

    #[test]
    fn a_self_referential_chain_is_refused_rather_than_followed_forever() {
        // Reachable from a modded or corrupted game today.
        let offsets = offsets();
        let process = SparseProcess::new(false).with_pointer(0x1000_0000, 0x1000_0000);
        assert!(read_state(&process, &offsets, &context()).is_err());
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
        let mut process = SparseProcess::new(false);
        // Walk the chain, laying down a pointer at each step it dereferences.
        let mut address = 0x1000_0000u64;
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

        let state = read_state(&process, &offsets, &context()).expect("a frame");
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
