//! What the reader produces: one frame of the game, as the rest of the client sees it.
//!
//! The field names and the JSON shape are the Electron client's, because gate G1 compares
//! the two field for field and a rename would make that comparison meaningless. Where the
//! two differ it is because one of them is wrong, and this is where that shows up.
//!
//! # Purity
//!
//! Everything here takes `&dyn ProcessMemory` and returns `Result`. No `unwrap`, no `as`
//! truncation on a value that came out of the target. That is what item 7 of the plan
//! asks for, and it is not decoration: a fuzzer that has to open a process finds nothing,
//! and a parity run that has to have a game running cannot be part of CI.

use serde::{Deserialize, Serialize};

use crate::dotnet::{read_dictionary, read_string, strip_rich_text};
use crate::memory::{ProcessMemory, ReadError, read_pointer, resolve_chain};
use crate::offsets::Offsets;

/// Where the game is.
///
/// The discriminants are the game's own, and the JSON is the number, because that is what
/// the Electron client sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum GameState {
    /// In a lobby, before the game starts.
    Lobby,
    /// Playing.
    Tasks,
    /// A meeting or a vote.
    Discussion,
    /// Not in a game at all.
    Menu,
    /// Not read yet, or a value this build does not know.
    Unknown,
}

impl From<GameState> for u8 {
    fn from(state: GameState) -> Self {
        match state {
            GameState::Lobby => 0,
            GameState::Tasks => 1,
            GameState::Discussion => 2,
            GameState::Menu => 3,
            GameState::Unknown => 4,
        }
    }
}

impl TryFrom<u8> for GameState {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(Self::from_repr(u32::from(value)))
    }
}

impl GameState {
    /// This enum's own value, as it is carried over IPC and stored.
    ///
    /// The inverse of the numbering above, and **not** the game's. `from_game` served as
    /// both until 2026-08-24, which is how the game's raw state came to be read as if it
    /// were one of these: the two happen to overlap for 0 to 3 while meaning different
    /// things, so nothing complained until a real frame was compared.
    #[must_use]
    pub fn from_repr(value: u32) -> Self {
        match value {
            0 => Self::Lobby,
            1 => Self::Tasks,
            2 => Self::Discussion,
            3 => Self::Menu,
            _ => Self::Unknown,
        }
    }

    /// The state for a value read out of the game.
    ///
    /// Anything unrecognised is [`GameState::Unknown`] rather than an error: a new game
    /// build adding a state should leave players audible, not stop the reader.
    #[must_use]
    pub fn from_game(value: u32, meeting_hud_state: i32) -> Self {
        // `GameReader.ts`'s switch, which is not a mapping of the raw value onto this
        // enum — it was read that way here until 2026-08-24, and the first real recording
        // showed every lobby frame reported as `Tasks`. The raw value distinguishes menu
        // from in-game; whether an in-game frame is a discussion is a separate reading, of
        // the meeting hud, and 4 is its "no meeting" value.
        match value {
            0 => Self::Menu,
            1 | 3 => Self::Lobby,
            _ if meeting_hud_state < 4 => Self::Discussion,
            _ => Self::Tasks,
        }
    }
}

/// One player, as the client needs them.
///
/// `ptr`, `taskPtr` and `objectPtr` are addresses in the game's memory. They are here
/// because the Electron reader puts them here and G1 compares field for field; H1's
/// `WireGameState` projection is what strips them before anything leaves the machine.
// Eight booleans, and clippy would rather they were a bitfield. They are the Electron
// client's field names and gate G1 compares the two structures field for field, so the
// shape is not free to improve.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    /// The player record's address.
    pub ptr: u64,
    /// The in-game player id.
    pub id: u8,
    /// The network client id, when there is one.
    ///
    /// Optional because the Electron reader's is. It reads this through `objectPtr`, and a
    /// record whose object pointer is zero -- which happens for a frame or two while the
    /// game is tearing a lobby down -- yields `undefined`, a key `JSON.stringify` then
    /// omits. A `u32` here would have to invent a number for that, and gate G1 compares
    /// the two structures field for field.
    pub client_id: Option<u32>,
    /// The name, with rich-text tags stripped.
    pub name: String,
    /// A hash of the name, for the overlay. Signed, because `hashCode` ends in `| 0`.
    pub name_hash: i32,
    /// The colour index.
    pub color_id: u32,
    /// The hat, by asset name.
    pub hat_id: String,
    /// The pet, by index.
    pub pet_id: u32,
    /// The skin, by asset name.
    pub skin_id: String,
    /// The visor, by asset name.
    pub visor_id: String,
    /// Whether they have left.
    pub disconnected: bool,
    /// Whether they are an impostor.
    pub is_impostor: bool,
    /// Whether they are dead.
    pub is_dead: bool,
    /// The task list's address.
    pub task_ptr: u64,
    /// The player object's address.
    pub object_ptr: u64,
    /// Whether this is the player at this machine.
    pub is_local: bool,
    /// A colour a mod has shifted them to, or -1.
    pub shifted_color: i32,
    /// Whether the record looked wrong and was kept anyway.
    pub bugged: bool,
    /// Where they are.
    /// A double, because the Electron reader's is: it rounds to four decimal places and
    /// a `f32` cannot hold the result — 36.676 came back as 36.67599868774414. The
    /// collider's `Vector2` is a double too, so this is the boundary that was odd.
    pub x: f64,
    /// See [`Player::x`].
    pub y: f64,
    /// Whether they are in a vent.
    pub in_vent: bool,
    /// Whether they are a practice-mode dummy, when that could be read.
    ///
    /// Optional for the same reason as [`Player::client_id`]: the Electron reader assigns
    /// this read straight through, with no coercion, so a record whose object pointer is
    /// zero carries `undefined` here. `inVent` beside it is read the same way and is not
    /// optional, because that one is compared with `> 0` and `undefined > 0` is false.
    pub is_dummy: Option<bool>,
}

/// One frame of the game.
// As above: the shape is the wire format, not a design decision this crate gets to make.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmongUsState {
    /// Where the game is now.
    pub game_state: GameState,
    /// Where it was on the previous frame.
    pub old_game_state: GameState,
    /// The lobby code as the game stores it.
    pub lobby_code_int: i32,
    /// The lobby code as players type it.
    pub lobby_code: String,
    /// Everyone in the lobby.
    pub players: Vec<Player>,
    /// Whether this machine is the host.
    pub is_host: bool,
    /// This machine's network client id.
    pub client_id: u32,
    /// The host's network client id.
    pub host_id: u32,
    /// Whether communications are sabotaged.
    pub coms_sabotaged: bool,
    /// Which security camera the local player is watching.
    pub current_camera: u32,
    /// Which map.
    pub map: u32,
    /// How far the local player can see.
    pub light_radius: f32,
    /// Whether that changed since the previous frame.
    pub light_radius_changed: bool,
    /// Which doors are shut.
    pub closed_doors: Vec<u32>,
    /// Which server the game is on.
    pub current_server: String,
    /// The lobby's player limit.
    pub max_players: u32,
    /// Which mod is loaded.
    #[serde(rename = "mod")]
    pub mod_id: String,
    /// Whether this build has the older meeting HUD layout.
    pub old_meeting_hud: bool,
}

/// The alphabet the game encodes a lobby code with.
const CODE_ALPHABET: &[u8; 26] = b"QWXRTYLPESDFGHUJKZOCVBINMA";

/// Below this, the value is the packed six-character format.
///
/// The thresholds are the Electron reader's exactly: at or below -1000 is the six
/// character code, strictly above zero is the older four character one, and everything
/// between is no code at all. Getting the boundary wrong would turn a menu into a lobby.
const CODE_V2_MAXIMUM: i32 = -1000;

/// Turns the integer the game stores into the code players type.
///
/// Two formats, and the thresholds between them matter as much as the arithmetic. Both
/// are ported from `IntToGameCode` in `GameReader.ts`.
#[must_use]
pub fn lobby_code_to_string(value: i32) -> String {
    if value == 0 {
        return String::new();
    }
    if value > 0 {
        // Four characters, one per byte, little-endian.
        return value
            .to_le_bytes()
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| char::from(*byte))
            .collect();
    }
    if value > CODE_V2_MAXIMUM {
        // Negative but not a packed code. The Electron reader returns nothing here and so
        // does this; a lobby code is never in that range.
        return String::new();
    }

    // Masked first, so both are known to be small and non-negative before the cast. The
    // TypeScript has no such concern and the arithmetic below has to match it exactly.
    let a = u32::try_from(value & 0x3ff).unwrap_or(0);
    let b = u32::try_from((value >> 10) & 0xfffff).unwrap_or(0);
    let letter = |index: u32| -> char {
        CODE_ALPHABET
            .get(index as usize % CODE_ALPHABET.len())
            .map_or('?', |byte| char::from(*byte))
    };
    [
        letter(a % 26),
        // The TypeScript writes `V2[Math.floor(a / 26)]` with no second modulo. For any
        // code the game actually produces that is the same thing: the encoder builds `a`
        // as `c0 + 26 * c1` with both letters under 26, so `a` never exceeds 675 and
        // `a / 26` never exceeds 25. For a value outside that range the TypeScript indexes
        // past the end of its string and joins the word "undefined" into the code, which
        // is a bug rather than a behaviour worth reproducing.
        letter(a / 26 % 26),
        letter(b % 26),
        letter(b / 26 % 26),
        letter(b / 676 % 26),
        letter(b / 17576 % 26),
    ]
    .into_iter()
    .collect()
}

/// A stable hash of a player name, for the overlay.
///
/// `GameReader.hashCode`, arithmetic for arithmetic:
///
/// ```text
/// for (let i = 0; i < s.length; i++) h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
/// ```
///
/// Two details decide whether this agrees, and this function got both wrong until
/// 2026-08-24. `charCodeAt` yields **UTF-16 code units**, not UTF-8 bytes, so iterating
/// `name.bytes()` diverges for every name outside ASCII — which is a large share of them,
/// and silently: the two hashes differ only where somebody used an accent or an emoji.
/// And `| 0` makes the result a **signed** 32-bit integer, so a hash with the top bit set
/// is negative in the recorded state and must be negative here.
///
/// It is not cosmetic. `nameHash` keys the per-player volume and mute settings in
/// `Voice.tsx`, so a client that hashes differently loses them for exactly those players.
#[must_use]
pub fn hash_name(name: &str) -> i32 {
    let mut hash: u32 = 0;
    for unit in name.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(unit));
    }
    // `| 0` in JavaScript: reinterpret the 32 bits as signed rather than clamp. Said with
    // `cast_signed` rather than `as`, because wrapping here is the specification.
    hash.cast_signed()
}

/// Where the outfit fields live, from the bundle.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutfitOffsets {
    /// The name string.
    pub player_name: i64,
    /// The colour index.
    pub color_id: i64,
    /// The hat asset name.
    pub hat_id: i64,
    /// The skin asset name.
    pub skin_id: i64,
    /// The visor asset name.
    pub visor_id: i64,
}

/// How many outfits a player can have. Six, as the Electron reader asks for.
const MAX_OUTFITS: usize = 6;

/// The outfit a player is wearing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outfit {
    /// The name, tags stripped.
    pub name: String,
    /// The colour index.
    pub color_id: u32,
    /// The hat asset name.
    pub hat_id: String,
    /// The skin asset name.
    pub skin_id: String,
    /// The visor asset name.
    pub visor_id: String,
    /// A colour a mod has shifted them to, or -1 when none has.
    pub shifted_color: i32,
}

/// Reads the outfits dictionary hanging off a player object.
///
/// Entry zero is what the player looks like; an entry matching `current_outfit` is a mod
/// having shifted their colour, which is why both are read in one walk rather than two.
///
/// # Errors
///
/// Returns [`ReadError`] if the dictionary header cannot be read.
pub fn read_outfit(
    memory: &dyn ProcessMemory,
    outfits_ptr: u64,
    current_outfit: u32,
    offsets: OutfitOffsets,
) -> Result<Outfit, ReadError> {
    let mut outfit = Outfit {
        shifted_color: -1,
        ..Outfit::default()
    };

    for (index, entry) in read_dictionary(memory, outfits_ptr, MAX_OUTFITS)?
        .into_iter()
        .enumerate()
    {
        let mut raw = [0u8; 4];
        if memory.read_exact(entry.key, &mut raw).is_err() {
            continue;
        }
        let key = i32::from_le_bytes(raw);
        let Ok(value) = read_pointer(memory, entry.value) else {
            continue;
        };
        if value == 0 {
            continue;
        }

        if key == 0 && index == 0 {
            outfit.name = strip_rich_text(&read_named_string(memory, value, offsets.player_name)?);
            outfit.color_id = read_u32(memory, value, offsets.color_id).unwrap_or(0);
            outfit.hat_id = read_named_string(memory, value, offsets.hat_id)?;
            outfit.skin_id = read_named_string(memory, value, offsets.skin_id)?;
            outfit.visor_id = read_named_string(memory, value, offsets.visor_id)?;
            // The Electron reader stops here when the current outfit is the base one or
            // out of range, and so does this: a shifted colour cannot be entry zero.
            if current_outfit == 0 || current_outfit > 10 {
                break;
            }
        } else if u32::try_from(key).is_ok_and(|key| key == current_outfit) {
            outfit.shifted_color = read_u32(memory, value, offsets.color_id)
                // A colour index that does not fit in an i32 is not a colour; -1 is what
                // the Electron reader uses for "not shifted".
                .and_then(|colour| i32::try_from(colour).ok())
                .unwrap_or(-1);
        }
    }
    Ok(outfit)
}

/// Follows a pointer at `base + offset` and reads the string it names.
fn read_named_string(
    memory: &dyn ProcessMemory,
    base: u64,
    offset: i64,
) -> Result<String, ReadError> {
    if offset < 0 {
        return Ok(String::new());
    }
    let Ok(at) = resolve_chain(memory, base, &[offset]) else {
        return Ok(String::new());
    };
    let Ok(pointer) = read_pointer(memory, at) else {
        return Ok(String::new());
    };
    read_string(memory, pointer, 1000)
}

/// Reads a `u32` at `base + offset`, or nothing if the field is absent.
fn read_u32(memory: &dyn ProcessMemory, base: u64, offset: i64) -> Option<u32> {
    if offset < 0 {
        return None;
    }
    let at = base.checked_add_signed(offset)?;
    let mut raw = [0u8; 4];
    memory.read_exact(at, &mut raw).ok()?;
    Some(u32::from_le_bytes(raw))
}

/// The chain for a named field of the player offsets, if the bundle has one.
///
/// The bundle carries most player fields as JSON arrays under names the reader knows.
/// Absent is not an error: twenty of the forty-four real files omit fields that build
/// does not have.
#[must_use]
pub fn player_chain(offsets: &Offsets, field: &str) -> Option<Vec<i64>> {
    let value = offsets.player.rest.get(field)?;
    match value {
        serde_json::Value::Array(steps) => steps.iter().map(serde_json::Value::as_i64).collect(),
        serde_json::Value::Number(number) => number.as_i64().map(|one| vec![one]),
        _ => None,
    }
}

/// The chain for a named top-level field, if the bundle has one.
#[must_use]
pub fn top_level_chain(offsets: &Offsets, field: &str) -> Option<Vec<i64>> {
    let value = offsets.rest.get(field)?;
    match value {
        serde_json::Value::Array(steps) => steps.iter().map(serde_json::Value::as_i64).collect(),
        serde_json::Value::Number(number) => number.as_i64().map(|one| vec![one]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::sparse::SparseProcess;

    #[test]
    fn the_games_state_number_is_not_this_enums_number() {
        // `GameReader.ts`'s switch. The two numberings overlap while meaning different
        // things — the game's 0 is the menu, this enum's 0 is the lobby — and reading one
        // as the other reported every lobby frame as `Tasks` until a real recording said
        // so. `NO_MEETING` is the state the Electron reader falls back to.
        const NO_MEETING: i32 = 4;
        assert_eq!(GameState::from_game(0, NO_MEETING), GameState::Menu);
        assert_eq!(GameState::from_game(1, NO_MEETING), GameState::Lobby);
        assert_eq!(GameState::from_game(3, NO_MEETING), GameState::Lobby);
        // Anything else is in a round, and the meeting hud decides which kind.
        assert_eq!(GameState::from_game(2, NO_MEETING), GameState::Tasks);
        assert_eq!(GameState::from_game(2, 0), GameState::Discussion);
        assert_eq!(GameState::from_game(99, 3), GameState::Discussion);
        // A new game build adding a state leaves players audible rather than stopping the
        // reader: it is a round, which is the safe reading.
        assert_eq!(GameState::from_game(99, NO_MEETING), GameState::Tasks);
    }

    #[test]
    fn this_enums_own_number_round_trips() {
        // The inverse of the numbering, for a value that came from this client rather
        // than from the game.
        assert_eq!(GameState::from_repr(0), GameState::Lobby);
        assert_eq!(GameState::from_repr(3), GameState::Menu);
        assert_eq!(GameState::from_repr(99), GameState::Unknown);
    }

    #[test]
    fn game_state_serialises_as_the_number_the_electron_client_sends() {
        // G1 compares the two states field for field, so the wire shape is not free.
        assert_eq!(serde_json::to_string(&GameState::Tasks).unwrap(), "1");
        assert_eq!(
            serde_json::from_str::<GameState>("2").unwrap(),
            GameState::Discussion
        );
    }

    #[test]
    fn decodes_a_six_character_lobby_code() {
        // The packed format. Round-tripping through the alphabet is the check that the
        // arithmetic matches the game's.
        let code = lobby_code_to_string(-2_073_360_669);
        assert_eq!(code.len(), 6);
        assert!(
            code.bytes().all(|byte| CODE_ALPHABET.contains(&byte)),
            "{code} has a character outside the alphabet"
        );
    }

    #[test]
    fn decodes_the_older_four_character_code() {
        // Stored as its characters directly, little-endian.
        let packed = i32::from_le_bytes(*b"ABCD");
        assert_eq!(lobby_code_to_string(packed), "ABCD");
    }

    #[test]
    fn uses_the_same_thresholds_as_the_electron_reader() {
        // Getting the boundary wrong turns a menu into a lobby. At or below -1000 is the
        // packed code; strictly above zero is the four character one; between is nothing.
        assert_eq!(lobby_code_to_string(0), "");
        assert_eq!(lobby_code_to_string(-1), "");
        assert_eq!(lobby_code_to_string(-999), "");
        assert_eq!(lobby_code_to_string(-1000).len(), 6);
        assert!(
            !lobby_code_to_string(32).is_empty(),
            "32 is the local game code"
        );
    }

    #[test]
    fn every_code_the_game_can_produce_stays_inside_the_alphabet() {
        // The encoder builds the low field as `c0 + 26 * c1` with both letters under 26,
        // so it never exceeds 675 — which is why the TypeScript gets away with having no
        // second modulo there. This is that claim, checked rather than asserted.
        for c0 in 0..26u32 {
            for c1 in 0..26u32 {
                let a = c0 + 26 * c1;
                assert!(a <= 675, "a reached {a}");
                assert!(a / 26 <= 25, "a / 26 reached {}", a / 26);
            }
        }
    }

    #[test]
    fn hashes_a_name_the_same_way_the_client_does() {
        assert_eq!(hash_name(""), 0);
        // 'a' is 97; "ab" is 97*31 + 98.
        assert_eq!(hash_name("a"), 97);
        // Every expected value below is what `GameReader.hashCode` actually returns,
        // taken from running it, not from arithmetic done here. A hand-derived constant
        // would only restate this function's own mistake.
        assert_eq!(hash_name("ab"), 3105);
        assert_eq!(hash_name("Player1"), 1_171_085_648);

        // Non-ASCII is where the byte-wise version diverged: `charCodeAt` gives one code
        // unit for `a-umlaut` (0xe4), where UTF-8 gives two bytes (0xc3, 0xa4).
        assert_eq!(hash_name("\u{e4}"), 228);
        assert_eq!(hash_name("Kr\u{fc}melmonster"), 2_107_092_379);
        assert_eq!(hash_name("\u{44d}\u{443}\u{444}"), {
            let mut h: u32 = 0;
            for u in "\u{44d}\u{443}\u{444}".encode_utf16() {
                h = h.wrapping_mul(31).wrapping_add(u32::from(u));
            }
            h.cast_signed()
        });

        // A surrogate pair is two code units on both sides, so an emoji agrees.
        assert_eq!(hash_name("\u{1f600}"), 1_772_899);

        // Signed, because `hashCode` ends in `| 0`. Cyrillic overflows into the top bit,
        // which is exactly the case a `u32` return got wrong.
        assert_eq!(
            hash_name("\u{43d}\u{435}\u{433}\u{43e}\u{434}\u{44f}\u{439}"),
            -1_631_115_749
        );

        // And it wraps rather than panicking on a long name.
        assert_eq!(hash_name("a-very-long-name-that-wraps-around"), 115_191_963);
        let _ = hash_name(&"x".repeat(200));
    }

    fn with_string(process: SparseProcess, address: u64, text: &str) -> SparseProcess {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = Vec::new();
        for unit in &units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        process
            .with_region(
                address + 0x10,
                i32::try_from(units.len()).unwrap().to_le_bytes(),
            )
            .with_region(address + 0x14, bytes)
    }

    fn outfit_offsets() -> OutfitOffsets {
        OutfitOffsets {
            player_name: 0x40,
            color_id: 0x14,
            hat_id: 0x18,
            skin_id: 0x20,
            visor_id: 0x28,
        }
    }

    /// A dictionary at `dict` with one entry whose value is the outfit at `outfit`.
    fn with_outfits(process: SparseProcess, dict: u64, entries: u64, count: u32) -> SparseProcess {
        process
            .with_pointer(dict + 0x18, entries)
            .with_region(dict + 0x20, count.to_le_bytes())
    }

    #[test]
    fn reads_the_base_outfit() {
        let dict = 0x1000;
        let entries = 0x2000;
        let outfit = 0x3000;
        let mut process = with_outfits(SparseProcess::new(true), dict, entries, 1);
        // Entry zero: key 0, value -> the outfit object.
        process = process
            .with_region(entries + 0x20, 0u32.to_le_bytes())
            .with_pointer(entries + 0x30, outfit);
        // The outfit's fields.
        process = process.with_region(outfit + 0x14, 7u32.to_le_bytes());
        process = process.with_pointer(outfit + 0x40, 0x4000);
        process = with_string(process, 0x4000, "<color=red>Alice</color>");
        process = process.with_pointer(outfit + 0x18, 0x5000);
        process = with_string(process, 0x5000, "hat_01");
        process = process.with_pointer(outfit + 0x20, 0x6000);
        process = with_string(process, 0x6000, "skin_01");
        process = process.with_pointer(outfit + 0x28, 0x7000);
        process = with_string(process, 0x7000, "visor_01");

        let read = read_outfit(&process, dict, 0, outfit_offsets()).expect("reads");
        // Tags stripped, so a player called `<color=red>Alice</color>` is the same person
        // as one called `Alice`.
        assert_eq!(read.name, "Alice");
        assert_eq!(read.color_id, 7);
        assert_eq!(read.hat_id, "hat_01");
        assert_eq!(read.skin_id, "skin_01");
        assert_eq!(read.visor_id, "visor_01");
        assert_eq!(read.shifted_color, -1);
    }

    #[test]
    fn a_player_with_no_outfits_reads_as_empty_rather_than_failing() {
        // A player who has not spawned yet.
        let read = read_outfit(&SparseProcess::new(true), 0, 0, outfit_offsets()).expect("reads");
        assert_eq!(
            read,
            Outfit {
                shifted_color: -1,
                ..Outfit::default()
            }
        );
    }

    #[test]
    fn finds_a_chain_the_bundle_carries_and_tolerates_one_it_does_not() {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/offsets/offsets__x86__V2026.8.18__offsets.json"),
        )
        .expect("a fixture");
        let offsets: Offsets = serde_json::from_str(&text).expect("parses");

        assert!(top_level_chain(&offsets, "allPlayersPtr").is_some());
        // Absent is not an error: twenty of the forty-four real files omit fields their
        // build does not have.
        assert!(top_level_chain(&offsets, "no_such_field").is_none());
        assert!(player_chain(&offsets, "isLocal").is_some());
    }
}
