//! Finding things in a module, and reading the shapes .NET puts there.
//!
//! # On alignment
//!
//! `docs/rust-port/04-implementation-plan.md` §4.4 item 2 bans zerocopy's reference APIs
//! on any struct with a 64-bit field, because on `i686-pc-windows-msvc` native code may
//! align an 8-byte type to 4 and a reference into misaligned bytes is instant
//! undefined behaviour. Nothing here takes a reference into target memory at all: every
//! read copies into a local buffer and every field is assembled with `from_le_bytes`.
//! Tens of bytes at 30 Hz — the copy costs nothing and the hazard disappears rather than
//! being linted around.

use crate::memory::{Module, ProcessMemory, ReadError, read_count, read_pointer};

/// A byte pattern with wildcards, as the offsets bundle writes them.
///
/// `"48 8B 05 ? ? ? ? 48 85 C0"` — two hex digits for a byte that must match, `?` or `??`
/// for one that may be anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// `None` is a wildcard.
    bytes: Vec<Option<u8>>,
}

/// Why a pattern could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// A token that is neither two hex digits nor a wildcard.
    #[error("token {index} is {token:?}, which is not a byte or a wildcard")]
    BadToken {
        /// Which token.
        index: usize,
        /// What it was.
        token: String,
    },
    /// A pattern with nothing in it matches everywhere, which is never what was meant.
    #[error("pattern is empty")]
    Empty,
}

impl Pattern {
    /// Reads a pattern in the bundle's format.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] for an empty pattern or a token that is not a byte or a
    /// wildcard.
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let mut bytes = Vec::new();
        for (index, token) in text.split_whitespace().enumerate() {
            if token == "?" || token == "??" {
                bytes.push(None);
            } else if token.len() == 2
                && let Ok(byte) = u8::from_str_radix(token, 16)
            {
                bytes.push(Some(byte));
            } else {
                return Err(PatternError::BadToken {
                    index,
                    token: token.to_owned(),
                });
            }
        }
        if bytes.is_empty() {
            return Err(PatternError::Empty);
        }
        Ok(Self { bytes })
    }

    /// How many bytes the pattern covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the pattern covers no bytes. Never true for a parsed one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether the pattern matches at the start of `haystack`.
    #[must_use]
    pub fn matches_at(&self, haystack: &[u8]) -> bool {
        if haystack.len() < self.bytes.len() {
            return false;
        }
        self.bytes
            .iter()
            .zip(haystack)
            .all(|(wanted, actual)| wanted.is_none_or(|byte| byte == *actual))
    }

    /// The offset of the first match within `haystack`.
    #[must_use]
    pub fn find(&self, haystack: &[u8]) -> Option<usize> {
        if haystack.len() < self.bytes.len() {
            return None;
        }
        (0..=haystack.len() - self.bytes.len()).find(|start| {
            haystack
                .get(*start..)
                .is_some_and(|rest| self.matches_at(rest))
        })
    }
}

/// How much of a module is read at a time while scanning.
///
/// `GameAssembly.dll` is around a hundred megabytes, and reading it in one call would
/// mean a hundred-megabyte allocation for a scan that usually hits in the first few. The
/// chunks overlap by the pattern length so a match that straddles a boundary is still
/// found — the bug that shape of loop is famous for.
const CHUNK: usize = 1 << 20;

/// Finds a pattern in a module and returns the absolute address of its first byte.
///
/// `skip` passes over that many matches first, for the rare signature that is not unique.
///
/// # Errors
///
/// Returns [`ReadError`] if the module cannot be read. A pattern that simply is not there
/// is `Ok(None)`, because a game build without a given function is data rather than
/// failure.
pub fn find_pattern(
    memory: &dyn ProcessMemory,
    module: &Module,
    pattern: &Pattern,
    skip: usize,
) -> Result<Option<u64>, ReadError> {
    let overlap = pattern.len().saturating_sub(1);
    let mut buffer = vec![0u8; CHUNK + overlap];
    let mut offset = 0u64;
    let mut skipped = 0usize;

    while offset < module.size {
        let remaining = usize::try_from(module.size - offset).unwrap_or(usize::MAX);
        let want = remaining.min(CHUNK + overlap);
        let Some(window) = buffer.get_mut(..want) else {
            break;
        };

        // A region of a module can be unreadable — a guard page, or memory the game has
        // decommitted. Skipping it is right; failing the whole scan is not.
        if memory.read_exact(module.base + offset, window).is_err() {
            offset += CHUNK as u64;
            continue;
        }

        let mut search_from = 0usize;
        while let Some(found) = window
            .get(search_from..)
            .and_then(|rest| pattern.find(rest))
        {
            let at = search_from + found;
            if skipped == skip {
                return Ok(Some(module.base + offset + at as u64));
            }
            skipped += 1;
            search_from = at + 1;
        }

        offset += CHUNK as u64;
    }
    Ok(None)
}

/// Where a signature says the address it wants actually is.
///
/// The bundle gives two small numbers beside each pattern. `pattern_offset` indexes into
/// the match; `address_offset` steps from there. Both are bounded by the validator in
/// `acl-types`, because they are the one part of a bundle that turns a match into an
/// arbitrary address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    /// How far into the match the interesting bytes start.
    pub pattern_offset: usize,
    /// How far to step from there, which may be negative.
    pub address_offset: i64,
    /// Whether the four bytes at that point are a relative displacement to follow.
    ///
    /// x86-64 addresses most globals as `[rip + disp32]`, so the bundle's patterns point
    /// at the displacement and the reader has to add it to the address of the *next*
    /// instruction. 32-bit builds store the absolute address instead.
    pub relative: bool,
}

/// Resolves a signature match into the address it names.
///
/// # Errors
///
/// Returns [`ReadError`] if the displacement cannot be read, or the result falls outside
/// the module — which is what a hostile or stale signature produces, and is the check the
/// injection path depends on.
pub fn resolve_signature(
    memory: &dyn ProcessMemory,
    module: &Module,
    matched_at: u64,
    signature: Signature,
) -> Result<u64, ReadError> {
    let at = matched_at
        .checked_add(signature.pattern_offset as u64)
        .ok_or(ReadError::Chain {
            step: 0,
            reason: "pattern offset moves past the address space",
        })?;

    let resolved = if signature.relative {
        let mut displacement = [0u8; 4];
        memory.read_exact(at, &mut displacement)?;
        let disp = i64::from(i32::from_le_bytes(displacement));
        // The displacement is relative to the end of the instruction, which is where
        // `address_offset` points.
        let next = at
            .checked_add_signed(signature.address_offset)
            .ok_or(ReadError::Chain {
                step: 0,
                reason: "address offset moves past the address space",
            })?;
        next.checked_add_signed(disp).ok_or(ReadError::Chain {
            step: 0,
            reason: "displacement moves past the address space",
        })?
    } else {
        // A 32-bit build stores the global's absolute address *in* the instruction, so the
        // four bytes at this point are the answer -- not the place the answer is.
        //
        // This returned `at + address_offset` until 2026-08-27: the address of the
        // immediate rather than the immediate. Every signature then resolved to a location
        // inside the code section, `module.contains` was satisfied by it, and the chain
        // walked from a pointer that was really an instruction. `GameReader.ts:863-867`
        // reads it, and the difference is a reader that cannot see a running game at all.
        let mut immediate = [0u8; 4];
        memory.read_exact(at, &mut immediate)?;
        u64::from(u32::from_le_bytes(immediate))
            .checked_add_signed(signature.address_offset)
            .ok_or(ReadError::Chain {
                step: 0,
                reason: "address offset moves past the address space",
            })?
    };

    if !module.contains(resolved) {
        return Err(ReadError::Chain {
            step: 0,
            reason: "signature resolved outside the module",
        });
    }
    Ok(resolved)
}

/// A .NET `List<T>` or array, as Unity lays it out.
///
/// Both are a header, a pointer to a buffer, and a count. The buffer's elements start
/// after its own header, which is why `element_base` is not the buffer pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayLayout {
    /// Offset from the object to the pointer to its backing buffer.
    pub buffer: i64,
    /// Offset from the object to its element count.
    pub count: i64,
    /// Offset from the buffer to its first element.
    pub first_element: i64,
    /// How many bytes each element takes.
    pub element_size: usize,
}

/// Reads the addresses of every element of a .NET array or list.
///
/// # Errors
///
/// Returns [`ReadError`] if the header cannot be read, or if the count is absurd — see
/// [`read_count`], which is where that bound lives.
pub fn read_elements(
    memory: &dyn ProcessMemory,
    object: u64,
    layout: ArrayLayout,
) -> Result<Vec<u64>, ReadError> {
    let count_at = object
        .checked_add_signed(layout.count)
        .ok_or(ReadError::Chain {
            step: 0,
            reason: "count offset moves past the address space",
        })?;
    let count = read_count(memory, count_at)?;

    let buffer_at = object
        .checked_add_signed(layout.buffer)
        .ok_or(ReadError::Chain {
            step: 0,
            reason: "buffer offset moves past the address space",
        })?;
    let buffer = read_pointer(memory, buffer_at)?;
    if buffer == 0 {
        // An empty list is a null buffer, not an error: a lobby before anyone joins.
        return Ok(Vec::new());
    }

    let first = buffer
        .checked_add_signed(layout.first_element)
        .ok_or(ReadError::Chain {
            step: 0,
            reason: "element offset moves past the address space",
        })?;

    // The count is already bounded, so this allocation is too.
    let mut addresses = Vec::with_capacity(count);
    for index in 0..count {
        let stride = (index as u64)
            .checked_mul(layout.element_size as u64)
            .ok_or(ReadError::Chain {
                step: index,
                reason: "element stride overflows",
            })?;
        addresses.push(first.checked_add(stride).ok_or(ReadError::Chain {
            step: index,
            reason: "element address moves past the address space",
        })?);
    }
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::sparse::SparseProcess;

    fn module() -> Module {
        Module {
            name: "GameAssembly.dll".to_owned(),
            base: 0x1000_0000,
            size: 0x1000,
        }
    }

    #[test]
    fn reads_the_pattern_format_the_bundle_uses() {
        let pattern = Pattern::parse("48 8B 05 ? ? ?? ? 48").unwrap();
        assert_eq!(pattern.len(), 8);
        assert!(pattern.matches_at(&[0x48, 0x8b, 0x05, 9, 9, 9, 9, 0x48]));
        assert!(!pattern.matches_at(&[0x48, 0x8b, 0x06, 9, 9, 9, 9, 0x48]));
    }

    #[test]
    fn refuses_a_pattern_it_cannot_read() {
        assert!(matches!(
            Pattern::parse("ZZ").unwrap_err(),
            PatternError::BadToken { .. }
        ));
        assert!(matches!(
            Pattern::parse("   ").unwrap_err(),
            PatternError::Empty
        ));
        // A single hex digit is a typo, not a byte.
        assert!(Pattern::parse("4").is_err());
    }

    #[test]
    fn finds_a_match_in_a_module() {
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x800..0x804].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);
        let pattern = Pattern::parse("DE AD ? EF").unwrap();

        let found = find_pattern(&process, &module(), &pattern, 0).unwrap();
        assert_eq!(found, Some(0x1000_0800));
    }

    #[test]
    fn a_pattern_that_is_not_there_is_data_rather_than_failure() {
        let process = SparseProcess::new(true).with_region(0x1000_0000, vec![0u8; 0x1000]);
        let pattern = Pattern::parse("DE AD BE EF").unwrap();
        assert_eq!(
            find_pattern(&process, &module(), &pattern, 0).unwrap(),
            None
        );
    }

    #[test]
    fn skips_to_a_later_match_when_asked() {
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x100..0x102].copy_from_slice(&[0xaa, 0xbb]);
        bytes[0x200..0x202].copy_from_slice(&[0xaa, 0xbb]);
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);
        let pattern = Pattern::parse("AA BB").unwrap();

        assert_eq!(
            find_pattern(&process, &module(), &pattern, 0).unwrap(),
            Some(0x1000_0100)
        );
        assert_eq!(
            find_pattern(&process, &module(), &pattern, 1).unwrap(),
            Some(0x1000_0200)
        );
        assert_eq!(
            find_pattern(&process, &module(), &pattern, 2).unwrap(),
            None
        );
    }

    #[test]
    fn finds_a_match_that_straddles_a_chunk_boundary() {
        // The bug this shape of loop is famous for. The module is two chunks and the
        // pattern sits across the seam.
        let size = CHUNK + 0x1000;
        let mut bytes = vec![0u8; size];
        let at = CHUNK - 2;
        bytes[at..at + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);
        let wide = Module {
            name: "GameAssembly.dll".to_owned(),
            base: 0x1000_0000,
            size: size as u64,
        };
        let pattern = Pattern::parse("11 22 33 44").unwrap();
        assert_eq!(
            find_pattern(&process, &wide, &pattern, 0).unwrap(),
            Some(0x1000_0000 + at as u64)
        );
    }

    /// A 32-bit signature names an address that is *stored* at the match, not the match.
    ///
    /// This asserted the opposite until 2026-08-27 — that the answer was the address of the
    /// four bytes — which is the shape the code had, so the test agreed with it and neither
    /// was checked against a game. `GameReader.ts:863-867` reads the immediate, and on a
    /// live 32-bit build the difference was a reader that resolved every signature into the
    /// code section and saw an empty menu while a game was running.
    #[test]
    fn an_absolute_signature_is_the_address_stored_at_the_match() {
        let mut bytes = vec![0u8; 0x1000];
        // The instruction's immediate: the global lives at 0x1000_0800.
        bytes[0x103..0x107].copy_from_slice(&0x1000_0800_u32.to_le_bytes());
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);

        let resolved = resolve_signature(
            &process,
            &module(),
            0x1000_0100,
            Signature {
                pattern_offset: 3,
                address_offset: 0,
                relative: false,
            },
        )
        .unwrap();
        assert_eq!(
            resolved, 0x1000_0800,
            "the immediate was not read; this is the address it was read *from*"
        );
    }

    /// And a signature whose immediate points outside the module is refused.
    ///
    /// The check was already there and was being satisfied by the wrong value: the address
    /// of the match is inside the module by construction, so it could never fire. Now it
    /// judges the thing it was written to judge.
    #[test]
    fn an_absolute_signature_pointing_out_of_the_module_is_refused() {
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x103..0x107].copy_from_slice(&0xdead_0000_u32.to_le_bytes());
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);

        assert!(
            resolve_signature(
                &process,
                &module(),
                0x1000_0100,
                Signature {
                    pattern_offset: 3,
                    address_offset: 0,
                    relative: false,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn follows_a_rip_relative_displacement() {
        // `48 8B 05 <disp32>` at 0x…200: the displacement is relative to the end of the
        // instruction, seven bytes in.
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x200..0x203].copy_from_slice(&[0x48, 0x8b, 0x05]);
        bytes[0x203..0x207].copy_from_slice(&0x100i32.to_le_bytes());
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);

        let resolved = resolve_signature(
            &process,
            &module(),
            0x1000_0200,
            Signature {
                pattern_offset: 3,
                address_offset: 4,
                relative: true,
            },
        )
        .unwrap();
        // 0x…203 + 4 = 0x…207, plus 0x100.
        assert_eq!(resolved, 0x1000_0307);
    }

    #[test]
    fn refuses_a_signature_that_resolves_outside_the_module() {
        // What a hostile or stale signature produces, and the check the injection path
        // depends on.
        let mut bytes = vec![0u8; 0x1000];
        bytes[0x203..0x207].copy_from_slice(&0x7000_0000i32.to_le_bytes());
        let process = SparseProcess::new(true).with_region(0x1000_0000, bytes);

        let error = resolve_signature(
            &process,
            &module(),
            0x1000_0200,
            Signature {
                pattern_offset: 3,
                address_offset: 4,
                relative: true,
            },
        )
        .unwrap_err();
        assert!(matches!(error, ReadError::Chain { reason, .. } if reason.contains("outside")));
    }

    fn layout() -> ArrayLayout {
        ArrayLayout {
            buffer: 0x10,
            count: 0x18,
            first_element: 0x20,
            element_size: 8,
        }
    }

    #[test]
    fn walks_a_dotnet_list() {
        let process = SparseProcess::new(true)
            .with_pointer(0x1010, 0x2000)
            .with_region(0x1018, 3u32.to_le_bytes());
        let elements = read_elements(&process, 0x1000, layout()).unwrap();
        assert_eq!(elements, vec![0x2020, 0x2028, 0x2030]);
    }

    #[test]
    fn an_empty_list_is_a_null_buffer_rather_than_an_error() {
        // A lobby before anyone joins.
        let process = SparseProcess::new(true)
            .with_pointer(0x1010, 0)
            .with_region(0x1018, 0u32.to_le_bytes());
        assert!(
            read_elements(&process, 0x1000, layout())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn refuses_a_count_a_mod_could_have_written() {
        // The count is read out of the target and used to size a Vec.
        let process = SparseProcess::new(true)
            .with_pointer(0x1010, 0x2000)
            .with_region(0x1018, 0x7fff_ffffu32.to_le_bytes());
        assert!(read_elements(&process, 0x1000, layout()).is_err());
    }
}
