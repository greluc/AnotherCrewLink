//! A process that exists only as bytes.
//!
//! One implementation serves two purposes the plan names separately. As `ReplayProcess`
//! it answers from regions recorded off a real game, which is what gate G1's parity run
//! reads. As `FuzzProcess` it answers from arbitrary bytes, which is what makes
//! `AmongUsState::read_from` fuzzable for almost nothing on top of the trait that already
//! exists.
//!
//! They are the same thing — a sparse map from address to bytes — and writing it twice
//! would mean the fuzzer exercised a different reader than the parity run.

use std::collections::BTreeMap;

use crate::memory::{Module, ProcessMemory, ReadError};

/// One contiguous run of bytes at a known address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Where it starts in the target's address space.
    pub address: u64,
    /// What is there.
    pub bytes: Vec<u8>,
}

/// A process made of recorded or invented bytes.
#[derive(Debug, Clone, Default)]
pub struct SparseProcess {
    /// Keyed by start address, so a read can find the region before it in one lookup.
    regions: BTreeMap<u64, Vec<u8>>,
    modules: Vec<Module>,
    is_64bit: bool,
}

impl SparseProcess {
    /// A 64-bit process with no regions.
    #[must_use]
    pub fn new(is_64bit: bool) -> Self {
        Self {
            regions: BTreeMap::new(),
            modules: Vec::new(),
            is_64bit,
        }
    }

    /// Adds a run of bytes.
    ///
    /// Overlapping regions are merged by later writes winning, which is what a recording
    /// of successive frames does naturally.
    #[must_use]
    pub fn with_region(mut self, address: u64, bytes: impl Into<Vec<u8>>) -> Self {
        self.regions.insert(address, bytes.into());
        self
    }

    /// Adds a module.
    #[must_use]
    pub fn with_module(mut self, name: &str, base: u64, size: u64) -> Self {
        self.modules.push(Module {
            name: name.to_owned(),
            base,
            size,
        });
        self
    }

    /// Writes a pointer at an address, in the target's width. For building fixtures.
    #[must_use]
    pub fn with_pointer(self, address: u64, points_to: u64) -> Self {
        if self.is_64bit {
            self.with_region(address, points_to.to_le_bytes().to_vec())
        } else {
            let narrow = u32::try_from(points_to).unwrap_or(u32::MAX);
            self.with_region(address, narrow.to_le_bytes().to_vec())
        }
    }

    /// Builds a process from arbitrary bytes, for fuzzing.
    ///
    /// The bytes are laid down as one region at a plausible module base, and the module is
    /// declared to span them. A fuzzer then explores the parsing layer rather than the
    /// address arithmetic that would reject everything before reaching it.
    #[must_use]
    pub fn from_arbitrary(bytes: &[u8], is_64bit: bool) -> Self {
        const BASE: u64 = 0x1000_0000;
        Self::new(is_64bit)
            .with_region(BASE, bytes.to_vec())
            .with_module("GameAssembly.dll", BASE, bytes.len() as u64)
    }

    /// How many regions this process holds.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
}

impl ProcessMemory for SparseProcess {
    fn read_exact(&self, address: u64, into: &mut [u8]) -> Result<(), ReadError> {
        if into.is_empty() {
            return Ok(());
        }
        // The region that starts at or before this address is the only one that can hold
        // it, because regions are keyed by start and a read never spans two.
        let Some((start, bytes)) = self.regions.range(..=address).next_back() else {
            return Err(ReadError::Unreadable {
                address,
                length: into.len(),
            });
        };
        let offset = usize::try_from(address - start).map_err(|_| ReadError::Unreadable {
            address,
            length: into.len(),
        })?;
        let Some(available) = bytes.get(offset..) else {
            return Err(ReadError::Unreadable {
                address,
                length: into.len(),
            });
        };
        if available.len() < into.len() {
            return Err(ReadError::Short {
                address,
                wanted: into.len(),
                got: available.len(),
            });
        }
        let Some(exact) = available.get(..into.len()) else {
            return Err(ReadError::Short {
                address,
                wanted: into.len(),
                got: available.len(),
            });
        };
        into.copy_from_slice(exact);
        Ok(())
    }

    fn module(&self, name: &str) -> Option<Module> {
        self.modules
            .iter()
            .find(|module| module.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn is_64bit(&self) -> bool {
        self.is_64bit
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::memory::{MAX_CHAIN_DEPTH, MAX_ELEMENTS, read_count, read_pointer, resolve_chain};

    #[test]
    fn reads_what_was_put_there() {
        let process = SparseProcess::new(true).with_region(0x1000, vec![1, 2, 3, 4]);
        let mut buffer = [0u8; 4];
        process.read_exact(0x1000, &mut buffer).unwrap();
        assert_eq!(buffer, [1, 2, 3, 4]);

        // And from the middle of a region.
        let mut two = [0u8; 2];
        process.read_exact(0x1002, &mut two).unwrap();
        assert_eq!(two, [3, 4]);
    }

    #[test]
    fn a_short_read_is_an_error_rather_than_zeroes() {
        // The C this replaces zero-filled the remainder and returned success, so a
        // partially mapped region produced a struct full of plausible zeros: a player at
        // the origin, alive, in no vent.
        let process = SparseProcess::new(true).with_region(0x1000, vec![1, 2]);
        let mut buffer = [0u8; 8];
        let error = process.read_exact(0x1000, &mut buffer).unwrap_err();
        assert!(matches!(
            error,
            ReadError::Short {
                wanted: 8,
                got: 2,
                ..
            }
        ));
    }

    #[test]
    fn an_unmapped_address_is_an_error() {
        let process = SparseProcess::new(true).with_region(0x2000, vec![0; 4]);
        let mut buffer = [0u8; 4];
        assert!(matches!(
            process.read_exact(0x1000, &mut buffer).unwrap_err(),
            ReadError::Unreadable { .. }
        ));
    }

    #[test]
    fn pointer_width_follows_the_target_and_not_the_reader() {
        let wide = SparseProcess::new(true).with_region(0x1000, 0xdead_beef_u64.to_le_bytes());
        assert_eq!(read_pointer(&wide, 0x1000).unwrap(), 0xdead_beef);

        let narrow = SparseProcess::new(false).with_region(0x1000, 0xdead_beef_u32.to_le_bytes());
        assert_eq!(read_pointer(&narrow, 0x1000).unwrap(), 0xdead_beef);
        assert_eq!(narrow.pointer_size(), 4);
    }

    #[test]
    fn walks_a_chain_the_way_the_offsets_tables_mean_it() {
        // base + 0x10 -> 0x2000, then 0x2000 + 0x8 is the field. The last offset is not
        // dereferenced.
        let process = SparseProcess::new(true)
            .with_pointer(0x1010, 0x2000)
            .with_region(0x2008, 42u32.to_le_bytes());
        let address = resolve_chain(&process, 0x1000, &[0x10, 0x8]).unwrap();
        assert_eq!(address, 0x2008);

        let mut value = [0u8; 4];
        process.read_exact(address, &mut value).unwrap();
        assert_eq!(u32::from_le_bytes(value), 42);
    }

    #[test]
    fn a_single_offset_chain_is_just_an_offset() {
        let process = SparseProcess::new(true);
        assert_eq!(resolve_chain(&process, 0x1000, &[0x20]).unwrap(), 0x1020);
    }

    #[test]
    fn stops_at_an_offset_of_minus_one() {
        // Twenty of the forty-four real offsets files use -1 for "not on this build".
        // Reading at base-1 would be a wild address.
        let process = SparseProcess::new(true);
        let error = resolve_chain(&process, 0x1000, &[-1]).unwrap_err();
        assert!(matches!(error, ReadError::Chain { step: 0, .. }));
    }

    #[test]
    fn refuses_a_null_pointer_rather_than_reading_at_the_offset() {
        let process = SparseProcess::new(true).with_pointer(0x1010, 0);
        let error = resolve_chain(&process, 0x1000, &[0x10, 0x8]).unwrap_err();
        assert!(matches!(error, ReadError::Chain { reason, .. } if reason.contains("null")));
    }

    #[test]
    fn refuses_a_self_referential_chain() {
        // Reachable from a modded or corrupted process, and the reason a depth bound
        // alone is not enough: this loop never gets longer.
        let process = SparseProcess::new(true).with_pointer(0x1010, 0x1010);
        let error = resolve_chain(&process, 0x1000, &[0x10, 0x8]).unwrap_err();
        assert!(matches!(error, ReadError::Chain { reason, .. } if reason.contains("itself")));
    }

    #[test]
    fn refuses_a_chain_longer_than_any_real_one() {
        let process = SparseProcess::new(true);
        let long = vec![0x10i64; MAX_CHAIN_DEPTH + 1];
        assert!(matches!(
            resolve_chain(&process, 0x1000, &long).unwrap_err(),
            ReadError::Chain { .. }
        ));
    }

    #[test]
    fn bounds_a_count_that_will_size_an_allocation() {
        let sane = SparseProcess::new(true).with_region(0x1000, 15u32.to_le_bytes());
        assert_eq!(read_count(&sane, 0x1000).unwrap(), 15);

        // The value comes out of the target process and is attacker-influenced the moment
        // a mod is loaded.
        let absurd = SparseProcess::new(true).with_region(
            0x1000,
            u32::try_from(MAX_ELEMENTS + 1).unwrap().to_le_bytes(),
        );
        assert!(read_count(&absurd, 0x1000).is_err());

        let negative = SparseProcess::new(true).with_region(0x1000, (-1i32).to_le_bytes());
        assert!(read_count(&negative, 0x1000).is_err());
    }

    #[test]
    fn arbitrary_bytes_make_a_process_a_fuzzer_can_use() {
        let process = SparseProcess::from_arbitrary(&[7u8; 64], true);
        assert_eq!(process.region_count(), 1);
        let module = process.module("gameassembly.dll").expect("declared");
        assert_eq!(module.size, 64);
        // Case-insensitively, as the operating system reports names inconsistently.
        assert!(process.module("GameAssembly.DLL").is_some());
    }

    #[test]
    fn a_module_excludes_its_own_base() {
        let module = Module {
            name: "GameAssembly.dll".to_owned(),
            base: 0x1000,
            size: 0x100,
        };
        // The base is the PE header. An address resolving there means a pattern matched
        // nothing, not that it matched the first byte.
        assert!(!module.contains(0x1000));
        assert!(module.contains(0x1001));
        assert!(module.contains(0x10ff));
        assert!(!module.contains(0x1100));
    }
}
