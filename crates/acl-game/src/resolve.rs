//! Filling the scanned addresses into the bundle before a frame is read.
//!
//! The offsets file is not self-contained, and this is the thing about it that is easiest
//! to get wrong. Seven of its pointer chains start with `-1`, and that is not "this build
//! does not have the field" — it is a hole the pattern scanner fills. `GameReader.ts` does
//! it by assignment:
//!
//! ```text
//! this.offsets.innerNetClient.base[0] = innerNetClient;
//! this.offsets.allPlayersPtr[0]       = gameData;
//! ```
//!
//! A reader that takes the file at face value walks a chain beginning at module base minus
//! one and finds nothing, which is exactly how this port first failed its own test. The
//! same shape as the function RVAs, which are placeholders for the same reason — and the
//! reason the bundle's *signatures* are what the validator bounds tightly.
//!
//! Seven fills, then, and not twelve. The other five entries in the Electron reader's list
//! were addresses to write to; see the note beside them below.

use std::collections::BTreeMap;

use crate::memory::{Module, ProcessMemory, ReadError};
use crate::offsets::{Offsets, SignatureEntry};
use crate::scan::{Pattern, Signature, find_pattern, resolve_signature};

/// Which bundle field each signature fills in.
///
/// The order is the Electron reader's, and the pairs are its assignments. `gameoptionsData`
/// appears twice on purpose: a build with a game options manager uses that, and one
/// without falls back to the player control singleton — the same field, filled from a
/// different scan.
const FILLS: [(&str, &str); 7] = [
    ("innerNetClient", "innerNetClient.base"),
    ("meetingHud", "meetingHud"),
    ("gameData", "allPlayersPtr"),
    ("shipStatus", "shipStatus"),
    ("miniGame", "miniGame"),
    ("palette", "palette"),
    ("gameOptionsManager", "gameoptionsData"),
];

/// The fallback when a build has no game options manager.
const OPTIONS_FALLBACK: (&str, &str) = ("playerControl", "gameoptionsData");

/// A bundle with the scanned addresses filled in.
#[derive(Debug, Clone)]
pub struct ResolvedOffsets {
    /// The bundle, with the holes filled.
    pub offsets: Offsets,
    /// Which signatures were found, for the log line that explains a partial frame.
    pub found: BTreeMap<String, u64>,
    /// Which were looked for and not found.
    pub missing: Vec<String>,
}

/// Runs every signature scan and fills the results into the bundle.
///
/// A signature that matches nothing leaves its field as it was, and the name goes into
/// [`ResolvedOffsets::missing`]. That is deliberate: many Among Us builds ship without
/// moving anything, so a build where one scan fails is usually still readable, and a
/// reader that gave up on the first miss would refuse to work on more builds than it had
/// to.
///
/// # Errors
///
/// Returns [`ReadError`] only if the module itself cannot be read at all.
pub fn resolve_offsets(
    memory: &dyn ProcessMemory,
    module: &Module,
    offsets: &Offsets,
) -> Result<ResolvedOffsets, ReadError> {
    let mut resolved = offsets.clone();
    let mut found = BTreeMap::new();
    let mut missing = Vec::new();

    let mut options_filled = false;
    for (signature_name, field) in FILLS {
        match scan(memory, module, offsets.signatures.get(signature_name))? {
            Some(address) => {
                if fill_first_step(&mut resolved, field, address, module) {
                    found.insert(signature_name.to_owned(), address);
                    if field == "gameoptionsData" {
                        options_filled = true;
                    }
                }
            }
            None => missing.push(signature_name.to_owned()),
        }
    }

    if !options_filled {
        // A build without a game options manager keeps the same value on the player
        // control singleton.
        let (signature_name, field) = OPTIONS_FALLBACK;
        if let Some(address) = scan(memory, module, offsets.signatures.get(signature_name))?
            && fill_first_step(&mut resolved, field, address, module)
        {
            found.insert(signature_name.to_owned(), address);
        }
    }

    // The bundle also carries five whole-value offsets -- `connectFunc`,
    // `fixedUpdateFunc`, `showModStampFunc`, `modLateUpdateFunc` and
    // `pingMessageString`. Every one of them existed to be written to or jumped into, and
    // nothing writes any more, so this reader does not scan for them. They are still parsed
    // and still bounds-checked, because they are in the file and the validator's job is the
    // file; they are simply never resolved against a live process. Ninety entries in the
    // corpus are `{}` here in any case: the write path was 32-bit only.

    Ok(ResolvedOffsets {
        offsets: resolved,
        found,
        missing,
    })
}

/// Runs one signature, if the bundle has one for this build.
fn scan(
    memory: &dyn ProcessMemory,
    module: &Module,
    entry: Option<&SignatureEntry>,
) -> Result<Option<u64>, ReadError> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let Some(text) = entry.sig.as_deref() else {
        // `{}`. Not an error: it is how a 64-bit file says the write path does not apply.
        return Ok(None);
    };
    let Ok(pattern) = Pattern::parse(text) else {
        // The validator has already refused a malformed pattern, so reaching here means
        // the bundle was not validated. Treating it as a miss keeps the reader working
        // rather than crashing on data it should never have been given.
        return Ok(None);
    };

    let Some(matched) = find_pattern(memory, module, &pattern, 0)? else {
        return Ok(None);
    };
    let signature = Signature {
        pattern_offset: usize::try_from(entry.pattern_offset.unwrap_or(0)).unwrap_or(0),
        address_offset: entry.address_offset.unwrap_or(0),
        // 64-bit builds address globals as `[rip + disp32]`; 32-bit ones store the
        // absolute address. That is a property of the target, not of the signature.
        relative: memory.is_64bit(),
    };
    Ok(resolve_signature(memory, module, matched, signature).ok())
}

/// Replaces the first step of a chain, creating nothing that was not there.
///
/// Returns whether the field existed and was a chain. `innerNetClient.base` is written
/// with a dot because it lives one level down.
///
/// # Module-relative, not absolute
///
/// `resolve_signature` returns an absolute address -- it checks `module.contains` on it --
/// and every chain here is walked by `resolve_chain`, which *starts at the module base and
/// adds each step*. Storing the absolute address therefore adds the base a second time, and
/// the sum is an address outside the module.
///
/// Measured on a live 32-bit Among Us on 2026-08-27: `innerNetClient` resolved to
/// `0x5d071351` in a module based at `0x5cb80000`; reading at the signature worked and
/// reading at `0x5cb80000 + 0x5d071351 = 0xb9bf1351` did not. Every signature-rooted chain
/// failed, so `innerNetClient` was zero, the game state fell back through "no lobby code"
/// to `Menu`, and the client showed an empty lobby while the game was plainly running.
///
/// Subtracting the base is right whatever the caller does with it: `base + (address - base)`
/// is `address` for any base, including the zero a replayed process reports.
fn fill_first_step(offsets: &mut Offsets, field: &str, address: u64, module: &Module) -> bool {
    let Ok(value) = i64::try_from(address.saturating_sub(module.base)) else {
        return false;
    };
    let target = match field.split_once('.') {
        Some((block, inner)) => offsets
            .rest
            .get_mut(block)
            .and_then(|block| block.get_mut(inner)),
        None => offsets.rest.get_mut(field),
    };
    let Some(serde_json::Value::Array(steps)) = target else {
        return false;
    };
    let Some(first) = steps.first_mut() else {
        return false;
    };
    *first = serde_json::Value::from(value);
    true
}

/// [`fill_first_step`], for a sibling module's tests.
#[cfg(test)]
pub(crate) fn fill_first_step_for_test(offsets: &mut Offsets, field: &str, address: u64) -> bool {
    // A zero base, so the address is stored as given: this shim exists for tests that are
    // about the chain's shape rather than about where the module sits.
    let module = Module {
        name: "GameAssembly.dll".to_owned(),
        base: 0,
        size: u64::MAX,
    };
    fill_first_step(offsets, field, address, &module)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::sparse::SparseProcess;
    use crate::state::top_level_chain;

    fn offsets() -> Offsets {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../test/fixtures/offsets/offsets__x86__V2026.8.18__offsets.json"),
        )
        .expect("a fixture");
        serde_json::from_str(&text).expect("parses")
    }

    /// A module at a stated base, for the arithmetic below.
    fn based(base: u64) -> Module {
        Module {
            name: "GameAssembly.dll".to_owned(),
            base,
            size: 0x0100_0000,
        }
    }

    fn module() -> Module {
        Module {
            name: "GameAssembly.dll".to_owned(),
            base: 0x1000_0000,
            size: 0x4000,
        }
    }

    #[test]
    fn the_bundle_really_does_ship_holes_for_the_scanner() {
        // The claim this whole module rests on. If a future bundle stops using -1 here,
        // this test says so rather than the reader silently doing twice the work.
        let offsets = offsets();
        let base = offsets
            .rest
            .get("innerNetClient")
            .and_then(|block| block.get("base"))
            .and_then(serde_json::Value::as_array)
            .expect("innerNetClient.base is a chain");
        assert_eq!(
            base.first().and_then(serde_json::Value::as_i64),
            Some(-1),
            "the first step is meant to be a hole for the pattern scan"
        );
    }

    #[test]
    fn fills_a_scanned_address_into_the_first_chain_step() {
        let mut offsets = offsets();
        assert!(fill_first_step(
            &mut offsets,
            "innerNetClient.base",
            0x1234,
            &based(0)
        ));
        let chain = offsets
            .rest
            .get("innerNetClient")
            .and_then(|block| block.get("base"))
            .and_then(serde_json::Value::as_array)
            .expect("still a chain");
        assert_eq!(chain[0].as_i64(), Some(0x1234));
        // And the rest of the chain is untouched.
        assert_eq!(chain.len(), 3);
    }

    /// What goes into the chain is module-*relative*, because that is what walks it.
    ///
    /// `resolve_chain` starts at the module base and adds each step, so storing an absolute
    /// address adds the base twice. Live on a 32-bit build that put every signature-rooted
    /// chain outside the module, and the reader saw an empty menu while a game was running.
    #[test]
    fn the_stored_step_is_relative_to_the_module() {
        let mut offsets = offsets();
        assert!(fill_first_step(
            &mut offsets,
            "innerNetClient.base",
            0x5cb8_1234,
            &based(0x5cb8_0000)
        ));
        let chain = offsets
            .rest
            .get("innerNetClient")
            .and_then(|block| block.get("base"))
            .and_then(serde_json::Value::as_array)
            .expect("still a chain");
        assert_eq!(
            chain[0].as_i64(),
            Some(0x1234),
            "the base must not survive into the step that gets added to the base"
        );
    }

    #[test]
    fn a_field_that_is_not_a_chain_is_left_alone() {
        let mut offsets = offsets();
        assert!(!fill_first_step(
            &mut offsets,
            "no_such_field",
            0x1234,
            &based(0)
        ));
        assert!(!fill_first_step(
            &mut offsets,
            "disableWriting",
            0x1234,
            &based(0)
        ));
    }

    #[test]
    fn a_signature_that_matches_nothing_is_recorded_rather_than_fatal() {
        // Many Among Us builds ship without moving anything, so a build where one scan
        // fails is usually still readable. Giving up on the first miss would refuse to
        // work on more builds than necessary.
        let offsets = offsets();
        let empty = SparseProcess::new(false)
            .with_region(0x1000_0000, vec![0u8; 0x4000])
            .with_module("GameAssembly.dll", 0x1000_0000, 0x4000);

        let resolved = resolve_offsets(&empty, &module(), &offsets).expect("no hard failure");
        assert!(!resolved.missing.is_empty(), "nothing should have matched");
        // The chain still holds its placeholder, which is what makes the frame fail
        // loudly rather than read from a wild address.
        let chain = top_level_chain(&resolved.offsets, "allPlayersPtr").expect("still there");
        assert_eq!(chain.first(), Some(&-1));
    }

    /// A 32-bit signature, end to end: match the pattern, read what it stores, store that
    /// relative to the module.
    ///
    /// Both halves were wrong until 2026-08-27 and this test agreed with both, because it
    /// let the pattern be its own answer: `pattern_offset: 0` with the resolved address
    /// compared against the address of the match. Written that way it cannot tell the
    /// address of the immediate from the immediate, which is the entire distinction.
    ///
    /// So the pattern is four bytes and the address it names is *after* them.
    #[test]
    fn finds_a_signature_that_is_in_the_module() {
        let mut offsets = offsets();
        offsets.signatures.insert(
            "gameData".to_owned(),
            SignatureEntry {
                sig: Some("DE AD BE EF".to_owned()),
                // Past the pattern, where a 32-bit instruction keeps its immediate.
                pattern_offset: Some(4),
                address_offset: Some(0),
            },
        );
        let mut bytes = vec![0u8; 0x4000];
        bytes[0x800..0x804].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        // The global this signature names, stored where the instruction keeps it.
        bytes[0x804..0x808].copy_from_slice(&0x1000_0c00_u32.to_le_bytes());
        let process = SparseProcess::new(false)
            .with_region(0x1000_0000, bytes)
            .with_module("GameAssembly.dll", 0x1000_0000, 0x4000);

        let resolved = resolve_offsets(&process, &module(), &offsets).expect("resolves");
        assert_eq!(
            resolved.found.get("gameData"),
            Some(&0x1000_0c00),
            "the answer is the address stored at the match, not the match"
        );

        // And what lands in the chain is module-relative, because `resolve_chain` adds the
        // module base to it. Storing the absolute address adds the base twice.
        let chain = top_level_chain(&resolved.offsets, "allPlayersPtr").expect("a chain");
        assert_eq!(chain.first(), Some(&0x0c00));
    }
}
