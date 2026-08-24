//! Reading another process's memory, and the shapes built on top of it.
//!
//! Everything above this file is pure: a `&dyn ProcessMemory` goes in and a `Result` comes
//! out. That is what makes the parsing layer fuzzable and replayable without a game
//! running, and it is a requirement rather than a style preference — `docs/rust-port/
//! 04-implementation-plan.md` §4.4 item 7 asks for exactly it, because a fuzzer that has
//! to open a process finds nothing.

use std::fmt;

/// Why a read did not produce the bytes asked for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The process is gone, or the handle is no longer valid.
    #[error("the process is not readable: {0}")]
    ProcessGone(String),
    /// The address is not mapped, or is not readable at this length.
    #[error("cannot read {length} bytes at {address:#x}")]
    Unreadable {
        /// Where the read started.
        address: u64,
        /// How many bytes were asked for.
        length: usize,
    },
    /// The read returned fewer bytes than asked for.
    ///
    /// A distinct error rather than a short buffer. The C this replaces zero-filled the
    /// remainder and returned success, so a partially mapped region produced a struct
    /// full of plausible zeros — a player at the origin, alive, in no vent.
    #[error("short read at {address:#x}: wanted {wanted}, got {got}")]
    Short {
        /// Where the read started.
        address: u64,
        /// How many bytes were asked for.
        wanted: usize,
        /// How many arrived.
        got: usize,
    },
    /// A pointer chain ran off the end of what is plausible.
    #[error("pointer chain gave up at step {step}: {reason}")]
    Chain {
        /// Which step of the chain.
        step: usize,
        /// Why it stopped.
        reason: &'static str,
    },
    /// A module the reader needs is not loaded.
    #[error("module {0} is not loaded")]
    NoModule(String),
}

/// A loaded module, as the reader needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The file name, as the operating system reports it.
    pub name: String,
    /// Where it starts.
    pub base: u64,
    /// How many bytes it spans.
    pub size: u64,
}

impl Module {
    /// Whether an absolute address falls inside this module.
    ///
    /// The base itself is excluded: that is the PE header, and an address resolving there
    /// means a pattern matched nothing rather than that it matched the first byte.
    #[must_use]
    pub fn contains(&self, address: u64) -> bool {
        address > self.base && address < self.base.saturating_add(self.size)
    }
}

/// How deep a pointer chain may go before it is treated as a loop.
///
/// The longest chain in the real offsets corpus is five. A self-referential chain is
/// reachable from a modded or corrupted game today and would otherwise spin until the
/// address space ran out.
pub const MAX_CHAIN_DEPTH: usize = 16;

/// The most elements the reader will believe an array or dictionary claims to hold.
///
/// A game reports at most fifteen players. The length is read out of the target process,
/// so it is attacker-influenced the moment a mod is loaded, and it is used to size a
/// `Vec` — which is the whole reason for a bound rather than a `debug_assert`.
pub const MAX_ELEMENTS: usize = 4096;

/// Somewhere bytes can be read from.
///
/// Implemented by the Windows and Linux readers, by `ReplayProcess` over a recording, and
/// by `FuzzProcess` over arbitrary bytes.
pub trait ProcessMemory {
    /// Fills `into` completely, or fails.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] if the process is gone, the address is unmapped, or fewer
    /// bytes than asked for were available.
    fn read_exact(&self, address: u64, into: &mut [u8]) -> Result<(), ReadError>;

    /// A loaded module by file name, case-insensitively.
    fn module(&self, name: &str) -> Option<Module>;

    /// Whether the target is a 64-bit process.
    ///
    /// This decides pointer width for every chain, so it is a property of the target and
    /// never of the reader.
    fn is_64bit(&self) -> bool;

    /// How wide a pointer is in the target.
    fn pointer_size(&self) -> usize {
        if self.is_64bit() { 8 } else { 4 }
    }
}

/// Reads one pointer, in the target's width.
///
/// # Errors
///
/// Returns [`ReadError`] if the read fails.
pub fn read_pointer(memory: &dyn ProcessMemory, address: u64) -> Result<u64, ReadError> {
    if memory.is_64bit() {
        let mut bytes = [0u8; 8];
        memory.read_exact(address, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    } else {
        let mut bytes = [0u8; 4];
        memory.read_exact(address, &mut bytes)?;
        Ok(u64::from(u32::from_le_bytes(bytes)))
    }
}

/// Walks a pointer chain: read at `base + chain[0]`, dereference, add `chain[1]`, and so
/// on. The final offset is added without a dereference, which is what the offsets tables
/// mean.
///
/// A `-1` step means "not present on this build" and stops the walk with
/// [`ReadError::Chain`] rather than reading at a wild address; twenty of the forty-four
/// real offsets files contain one.
///
/// # Errors
///
/// Returns [`ReadError`] if a step cannot be read, the chain is longer than
/// [`MAX_CHAIN_DEPTH`], or a dereference produces a null or self-referential pointer.
pub fn resolve_chain(
    memory: &dyn ProcessMemory,
    base: u64,
    chain: &[i64],
) -> Result<u64, ReadError> {
    if chain.len() > MAX_CHAIN_DEPTH {
        return Err(ReadError::Chain {
            step: chain.len(),
            reason: "chain is longer than any real offsets file",
        });
    }

    let mut address = base;
    for (step, offset) in chain.iter().enumerate() {
        if *offset < 0 {
            return Err(ReadError::Chain {
                step,
                reason: "offset is -1, meaning this build does not have the field",
            });
        }
        let Some(next) = address.checked_add_signed(*offset) else {
            return Err(ReadError::Chain {
                step,
                reason: "offset moves the address outside the address space",
            });
        };
        address = next;

        // The last offset is not dereferenced: it names a field, not a pointer to one.
        if step + 1 == chain.len() {
            break;
        }

        let pointed = read_pointer(memory, address)?;
        if pointed == 0 {
            return Err(ReadError::Chain {
                step,
                reason: "pointer is null",
            });
        }
        if pointed == address {
            // Reachable from a modded or corrupted process, and the reason the depth
            // bound alone is not enough: a chain can loop without getting longer.
            return Err(ReadError::Chain {
                step,
                reason: "pointer points at itself",
            });
        }
        address = pointed;
    }
    Ok(address)
}

/// Reads a length that will be used to size an allocation.
///
/// Separate from an ordinary read so the bound is impossible to forget. The value comes
/// out of the target process and is therefore attacker-influenced whenever a mod is
/// loaded.
///
/// # Errors
///
/// Returns [`ReadError`] if the read fails, the value is negative, or it is over
/// [`MAX_ELEMENTS`].
pub fn read_count(memory: &dyn ProcessMemory, address: u64) -> Result<usize, ReadError> {
    let mut bytes = [0u8; 4];
    memory.read_exact(address, &mut bytes)?;
    let raw = i32::from_le_bytes(bytes);
    if raw < 0 {
        return Err(ReadError::Chain {
            step: 0,
            reason: "element count is negative",
        });
    }
    let count = usize::try_from(raw).unwrap_or(usize::MAX);
    if count > MAX_ELEMENTS {
        return Err(ReadError::Chain {
            step: 0,
            reason: "element count is larger than any real game reports",
        });
    }
    Ok(count)
}

impl fmt::Display for Module {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {:#x} ({:#x} bytes)",
            self.name, self.base, self.size
        )
    }
}
