//! The two .NET shapes the reader has to walk: a string and a dictionary.
//!
//! Both layouts are ported from `src/main/GameReader.ts` and the constants are the same
//! ones. They are Unity's Mono layout rather than anything documented, so the numbers are
//! empirical and the only justification for them is that they work against the game — a
//! reason to keep them in one place with the derivation written down, not to rediscover
//! them at each call site.

use crate::memory::{ProcessMemory, ReadError, read_pointer};

/// Where a `System.String` keeps its length, and where the characters start.
///
/// `[header][length: i32][chars: u16…]`. The header is two pointers, so both offsets
/// depend on the target's width and not the reader's.
const STRING_LENGTH_64: u64 = 0x10;
const STRING_CHARS_64: u64 = 0x14;
const STRING_LENGTH_32: u64 = 0x8;
const STRING_CHARS_32: u64 = 0xc;

/// The longest string the reader will take.
///
/// The length is read out of the target process, so it decides an allocation and is
/// attacker-influenced the moment a mod is loaded. Player names are capped at ten
/// characters by the game; a thousand is generous for a name carrying rich-text tags and
/// still bounded.
pub const MAX_STRING_LENGTH: usize = 1000;

/// Reads a `System.String`.
///
/// Returns an empty string for a null pointer, which is what the game uses for a field
/// that is not set — an error there would turn "this player has no hat" into a failed
/// frame.
///
/// # Errors
///
/// Returns [`ReadError`] if the length or the characters cannot be read.
pub fn read_string(
    memory: &dyn ProcessMemory,
    address: u64,
    max_length: usize,
) -> Result<String, ReadError> {
    if address == 0 {
        return Ok(String::new());
    }
    let (length_at, chars_at) = if memory.is_64bit() {
        (STRING_LENGTH_64, STRING_CHARS_64)
    } else {
        (STRING_LENGTH_32, STRING_CHARS_32)
    };

    let mut raw = [0u8; 4];
    memory.read_exact(address + length_at, &mut raw)?;
    let declared = i32::from_le_bytes(raw);
    // Clamped rather than refused: the TypeScript does `max(0, min(length, maxLength))`,
    // and a name field that has been reused for something else should read as nonsense
    // rather than stop the frame.
    let length = declared
        .max(0)
        .try_into()
        .unwrap_or(0usize)
        .min(max_length.min(MAX_STRING_LENGTH));
    if length == 0 {
        return Ok(String::new());
    }

    let mut bytes = vec![0u8; length * 2];
    memory.read_exact(address + chars_at, &mut bytes)?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .filter(|unit| *unit != 0)
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

/// Strips the rich-text tags Among Us allows in names.
///
/// The TypeScript does `.split(/<.*?>/).join('')`. A player whose name is
/// `<color=red>bob</color>` has to compare equal to one called `bob`, or the two readers
/// disagree about who is who.
#[must_use]
pub fn strip_rich_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(character),
            _ => {}
        }
    }
    out
}

/// Where a `Dictionary<K, V>` keeps its entries and its count.
const DICT_ENTRIES_64: u64 = 0x18;
const DICT_COUNT_64: u64 = 0x20;
const DICT_FIRST_ENTRY_64: u64 = 0x20;
const DICT_ENTRY_SIZE_64: u64 = 0x18;
const DICT_VALUE_IN_ENTRY_64: u64 = 0x10;

const DICT_ENTRIES_32: u64 = 0xc;
const DICT_COUNT_32: u64 = 0x10;
const DICT_FIRST_ENTRY_32: u64 = 0x10;
const DICT_ENTRY_SIZE_32: u64 = 0x10;
const DICT_VALUE_IN_ENTRY_32: u64 = 0xc;

/// One entry of a dictionary: where its key is, and where its value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictEntry {
    /// Address of the key.
    pub key: u64,
    /// Address of the value.
    pub value: u64,
}

/// Walks a `Dictionary<K, V>`, returning the address of each entry's key and value.
///
/// `max_entries` is the caller's bound and is applied on top of [`crate::MAX_ELEMENTS`].
/// The outfits dictionary the player parser walks is asked for six.
///
/// # Errors
///
/// Returns [`ReadError`] if the header cannot be read.
pub fn read_dictionary(
    memory: &dyn ProcessMemory,
    address: u64,
    max_entries: usize,
) -> Result<Vec<DictEntry>, ReadError> {
    if address == 0 {
        return Ok(Vec::new());
    }
    let (entries_at, count_at, first, stride, value_in_entry) = if memory.is_64bit() {
        (
            DICT_ENTRIES_64,
            DICT_COUNT_64,
            DICT_FIRST_ENTRY_64,
            DICT_ENTRY_SIZE_64,
            DICT_VALUE_IN_ENTRY_64,
        )
    } else {
        (
            DICT_ENTRIES_32,
            DICT_COUNT_32,
            DICT_FIRST_ENTRY_32,
            DICT_ENTRY_SIZE_32,
            DICT_VALUE_IN_ENTRY_32,
        )
    };

    let entries = read_pointer(memory, address + entries_at)?;
    if entries == 0 {
        return Ok(Vec::new());
    }

    let mut raw = [0u8; 4];
    memory.read_exact(address + count_at, &mut raw)?;
    let declared = u32::from_le_bytes(raw) as usize;
    // Two bounds, and both matter. The caller's is what the shape of the data says; the
    // global one is what stops a corrupted count from being believed at all.
    let count = declared.min(max_entries).min(crate::MAX_ELEMENTS);

    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let offset = entries
            .checked_add(first)
            .and_then(|base| base.checked_add((index as u64).checked_mul(stride)?))
            .ok_or(ReadError::Chain {
                step: index,
                reason: "dictionary entry moves past the address space",
            })?;
        out.push(DictEntry {
            key: offset,
            value: offset + value_in_entry,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::sparse::SparseProcess;

    /// Lays a `System.String` out the way the game does.
    fn with_string(process: SparseProcess, address: u64, text: &str) -> SparseProcess {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = Vec::with_capacity(units.len() * 2);
        for unit in &units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let (length_at, chars_at) = if process.is_64bit() {
            (STRING_LENGTH_64, STRING_CHARS_64)
        } else {
            (STRING_LENGTH_32, STRING_CHARS_32)
        };
        process
            .with_region(
                address + length_at,
                i32::try_from(units.len()).unwrap().to_le_bytes(),
            )
            .with_region(address + chars_at, bytes)
    }

    #[test]
    fn reads_a_dotnet_string() {
        let process = with_string(SparseProcess::new(true), 0x1000, "Alice");
        assert_eq!(read_string(&process, 0x1000, 50).unwrap(), "Alice");
    }

    #[test]
    fn reads_a_string_in_a_32_bit_target() {
        // The header is two pointers, so both offsets are narrower.
        let process = with_string(SparseProcess::new(false), 0x1000, "Bob");
        assert_eq!(read_string(&process, 0x1000, 50).unwrap(), "Bob");
    }

    #[test]
    fn a_null_string_is_empty_rather_than_an_error() {
        // The game uses null for a field that is not set. An error would turn "this
        // player has no hat" into a failed frame.
        let process = SparseProcess::new(true);
        assert_eq!(read_string(&process, 0, 50).unwrap(), "");
    }

    #[test]
    fn clamps_a_length_the_target_could_have_written() {
        // The length decides an allocation and comes out of the game.
        let process = SparseProcess::new(true)
            .with_region(0x1000 + STRING_LENGTH_64, 0x7fff_ffffi32.to_le_bytes())
            .with_region(0x1000 + STRING_CHARS_64, vec![0u8; 64]);
        let read = read_string(&process, 0x1000, 8);
        // Clamped to the caller's maximum, so the read is 16 bytes and succeeds.
        assert!(read.is_ok(), "{read:?}");

        let negative =
            SparseProcess::new(true).with_region(0x1000 + STRING_LENGTH_64, (-5i32).to_le_bytes());
        assert_eq!(read_string(&negative, 0x1000, 50).unwrap(), "");
    }

    #[test]
    fn strips_the_rich_text_a_player_can_put_in_a_name() {
        // A player called `<color=red>bob</color>` has to compare equal to one called
        // `bob`, or the two readers disagree about who is who.
        assert_eq!(strip_rich_text("<color=red>bob</color>"), "bob");
        assert_eq!(strip_rich_text("plain"), "plain");
        assert_eq!(strip_rich_text("<b><i>x</i></b>"), "x");
        // An unclosed tag swallows the rest, which is what the TypeScript regex does too.
        assert_eq!(strip_rich_text("a<b"), "a");
    }

    /// Lays out a dictionary with `count` entries starting at `entries`.
    fn with_dictionary(
        process: SparseProcess,
        address: u64,
        entries: u64,
        count: u32,
    ) -> SparseProcess {
        let (entries_at, count_at) = if process.is_64bit() {
            (DICT_ENTRIES_64, DICT_COUNT_64)
        } else {
            (DICT_ENTRIES_32, DICT_COUNT_32)
        };
        process
            .with_pointer(address + entries_at, entries)
            .with_region(address + count_at, count.to_le_bytes())
    }

    #[test]
    fn walks_a_dictionary() {
        let process = with_dictionary(SparseProcess::new(true), 0x1000, 0x2000, 3);
        let entries = read_dictionary(&process, 0x1000, 6).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].key, 0x2020);
        assert_eq!(entries[0].value, 0x2030);
        assert_eq!(entries[1].key, 0x2038);
        assert_eq!(entries[2].key, 0x2050);
    }

    #[test]
    fn honours_the_callers_bound_and_the_global_one() {
        // The outfits dictionary is asked for six however many it claims.
        let process = with_dictionary(SparseProcess::new(true), 0x1000, 0x2000, 500);
        assert_eq!(read_dictionary(&process, 0x1000, 6).unwrap().len(), 6);

        // And a count past the global bound is not believed even if the caller is
        // generous.
        let absurd = with_dictionary(SparseProcess::new(true), 0x1000, 0x2000, u32::MAX);
        assert_eq!(
            read_dictionary(&absurd, 0x1000, usize::MAX).unwrap().len(),
            crate::MAX_ELEMENTS
        );
    }

    #[test]
    fn an_empty_dictionary_is_not_an_error() {
        let process = with_dictionary(SparseProcess::new(true), 0x1000, 0, 0);
        assert!(read_dictionary(&process, 0x1000, 6).unwrap().is_empty());
        // And a null dictionary pointer is a player who has not spawned yet.
        assert!(
            read_dictionary(&SparseProcess::new(true), 0, 6)
                .unwrap()
                .is_empty()
        );
    }
}
