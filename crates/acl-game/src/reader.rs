//! One frame, read out of a running game.
//!
//! This is the function gate G1 measures: given the same bytes the Electron reader saw,
//! it has to produce the same [`AmongUsState`], field for field, with float positions
//! within 1e-6.
//!
//! It takes `&dyn ProcessMemory` and returns `Result`, which is the whole reason the
//! parity run and the fuzzer can both drive it. No `unwrap`, no `as` on a value that came
//! out of the target, and every count bounded before it sizes anything.

use crate::dotnet::read_string;
use crate::memory::{ProcessMemory, ReadError, read_pointer, resolve_chain};
use crate::mods::Mod;
use crate::offsets::Offsets;
use crate::state::{
    AmongUsState, GameState, OutfitOffsets, Player, hash_name, lobby_code_to_string, player_chain,
    read_outfit, top_level_chain,
};

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
    let game_state = GameState::from_game(
        read_u32_at(memory, inner_net, inner_client_offset(offsets, "gameState")).unwrap_or(4),
    );

    let lobby_code_int = if game_state == GameState::Menu {
        -1
    } else {
        read_i32_at(memory, inner_net, inner_client_offset(offsets, "gameId")).unwrap_or(-1)
    };
    let lobby_code = if game_state == GameState::Menu {
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

    if !lobby_code.is_empty() || is_local_game {
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

    let light_radius = local_player
        .as_ref()
        .and_then(|player| {
            read_f32_at(
                memory,
                player.object_ptr,
                first_offset(offsets, "lightRadius"),
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
        coms_sabotaged: false,
        current_camera: 0,
        map: u32::from(map),
        light_radius,
        // NaN compares unequal to itself, so the first frame reports a change — which is
        // what the Electron reader does too, comparing against an absent previous state.
        light_radius_changed: (light_radius - previous_light).abs() > f32::EPSILON,
        closed_doors: Vec::new(),
        current_server: context.current_server.clone(),
        max_players: u32::from(max_players),
        mod_id: context.loaded_mod.id().to_owned(),
        old_meeting_hud: offsets.old_meeting_hud,
    })
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
    let object_ptr = follow(memory, record, player_chain(offsets, "objectPtr")).ok()?;
    let task_ptr = follow(memory, record, player_chain(offsets, "taskPtr")).unwrap_or(0);
    let outfits_ptr = follow(memory, record, player_chain(offsets, "outfitsPtr")).unwrap_or(0);
    let role_ptr = follow(memory, record, player_chain(offsets, "rolePtr")).unwrap_or(0);

    let client_id = read_u32_at(memory, object_ptr, first_player_offset(offsets, "clientId"))?;
    let disconnected =
        read_u8_at(memory, record, first_player_offset(offsets, "disconnected")).unwrap_or(0) != 0;
    let is_local = client_id == local_client_id && !disconnected;

    // The local player's position lives in a different field from everyone else's: theirs
    // is authoritative, the others are interpolated from the network.
    let (x_field, y_field) = if is_local {
        ("localX", "localY")
    } else {
        ("remoteX", "remoteY")
    };
    let x = read_f32_at(memory, object_ptr, first_player_offset(offsets, x_field)).unwrap_or(0.0);
    let y = read_f32_at(memory, object_ptr, first_player_offset(offsets, y_field)).unwrap_or(0.0);

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
        id: 0,
        client_id,
        name_hash: hash_name(&name),
        name,
        color_id: outfit.color_id,
        hat_id: outfit.hat_id,
        pet_id: 0,
        skin_id: outfit.skin_id,
        visor_id: outfit.visor_id,
        disconnected,
        // The role team is what the game calls the impostor side. A build without the
        // field reports crewmate, which is the safe direction: it does not reveal anyone.
        is_impostor: role_team.is_some_and(|team| team != 0),
        is_dead: read_u8_at(memory, record, first_player_offset(offsets, "isDead")).unwrap_or(0)
            != 0,
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

fn read_f32_at(memory: &dyn ProcessMemory, base: u64, offset: Option<i64>) -> Option<f32> {
    let at = base.checked_add_signed(offset?)?;
    let mut raw = [0u8; 4];
    memory.read_exact(at, &mut raw).ok()?;
    Some(f32::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
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
        // In the menu there is no lobby code, whatever the field happens to hold.
        assert_eq!(state.lobby_code, "");
        assert_eq!(state.lobby_code_int, -1);
        assert!(state.players.is_empty());
        // And the map is never absent, because an undefined map is what made the collider
        // lookup silently report that no wall was ever in the way.
        assert_eq!(state.map, 6);
    }
}
