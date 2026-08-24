//! Writing a detour into the game, and everything that has to be true first.
//!
//! 32-bit Windows only, and behind the `injection` feature, which is also what adds
//! `PROCESS_VM_WRITE | PROCESS_VM_OPERATION` to the rights the reader asks for. With the
//! feature off, this module still compiles and is still tested: the decision is pure, and
//! only the write is not.
//!
//! # Why the whole prologue and not five bytes
//!
//! The patch overwrites five bytes with `E9 rel32`. Checking only those five is not
//! enough, because an instruction can start inside them and end outside: the plan names
//! the one at +4 specifically. If that instruction has changed — a different game build,
//! another tool's hook, a hot patch — then the five bytes can still look untouched while
//! the byte at +5 is now the middle of something else, and the detour lands in the middle
//! of an instruction that no longer exists.
//!
//! So the prologue is captured once, wider than the patch, and *replayed*: before writing,
//! the bytes are compared against what was seen at attach time.
//!
//! # Why three states and not two
//!
//! The initialisation path can run again against a process this client already patched —
//! the app restarts, the game does not. A check that only knows "matches" and "does not
//! match" then refuses, and the mod stamp stays broken until the player restarts the game.
//! [`Verdict::AlreadyOurs`] is that case, and it is a success.

use crate::memory::{Module, ProcessMemory, ReadError};

/// The length of the `E9 rel32` detour, plus the `0x90` that pads it to the next
/// instruction boundary.
pub const DETOUR_LENGTH: usize = 5;

/// How much of the prologue is captured and replayed.
///
/// Sixteen bytes covers the patch and every instruction that can straddle its end — x86
/// instructions are at most fifteen bytes, so an instruction starting at +4 cannot reach
/// past +18, and in real prologues nothing comes close.
pub const PROLOGUE_LENGTH: usize = 16;

/// How far past an allocation a detour may point and still be one of ours.
///
/// A page, not the size actually requested: `VirtualAllocEx` rounds a commit up to page
/// granularity, and the Electron client relies on that — its second stub lives at
/// `base + 0x300`, past a requested `0x60`. One recorded base covers both.
pub const SHELLCODE_PAGE_SIZE: u64 = 0x1000;

/// What was found at a detour site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The prologue is what it was at attach time. Safe to patch.
    Ready,
    /// A detour into a page this process allocated. Already done; leave it alone.
    AlreadyOurs {
        /// Where it jumps to.
        destination: u64,
    },
    /// Anything else, with the reason.
    Refuse(RefusalReason),
}

/// Why a site was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefusalReason {
    /// The address is not inside the module the offsets describe.
    #[error("{address:#x} is outside {module}")]
    OutsideModule {
        /// The address.
        address: u64,
        /// The module it should have been in.
        module: String,
    },
    /// The prologue could not be read.
    #[error("the prologue at {address:#x} is not readable: {source}")]
    Unreadable {
        /// The address.
        address: u64,
        /// Why.
        #[source]
        source: ReadError,
    },
    /// All zeros, which is not code.
    #[error("the prologue at {address:#x} is all zeros, so it is not code")]
    NotCode {
        /// The address.
        address: u64,
    },
    /// Something else already jumps out of here.
    #[error("already detoured to {destination:#x}, which is not ours")]
    ForeignDetour {
        /// Where it goes.
        destination: u64,
    },
    /// The prologue is not what it was when it was captured.
    #[error("the prologue at {address:#x} has changed since it was captured")]
    Changed {
        /// The address.
        address: u64,
    },
    /// Nothing was captured to compare against.
    #[error("no prologue was captured for {address:#x}")]
    NotCaptured {
        /// The address.
        address: u64,
    },
}

/// The prologues seen at attach time, and the pages this process has allocated.
#[derive(Debug, Default, Clone)]
pub struct InjectionState {
    captured: Vec<(u64, [u8; PROLOGUE_LENGTH])>,
    pages: Vec<u64>,
}

impl InjectionState {
    /// Nothing captured, nothing allocated.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the prologue at a site, before anything is written anywhere.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] if the prologue cannot be read.
    pub fn capture(&mut self, memory: &dyn ProcessMemory, address: u64) -> Result<(), ReadError> {
        let mut prologue = [0u8; PROLOGUE_LENGTH];
        memory.read_exact(address, &mut prologue)?;
        self.captured.retain(|(at, _)| *at != address);
        self.captured.push((address, prologue));
        Ok(())
    }

    /// Records a page allocated in the target, so a detour into it is recognisable.
    pub fn note_page(&mut self, base: u64) {
        if !self.pages.contains(&base) {
            self.pages.push(base);
        }
    }

    /// Whether an address falls inside a page this process allocated.
    #[must_use]
    pub fn owns(&self, destination: u64) -> bool {
        self.pages
            .iter()
            .any(|page| destination >= *page && destination < page + SHELLCODE_PAGE_SIZE)
    }

    /// What was captured at a site, if anything.
    #[must_use]
    pub fn captured_at(&self, address: u64) -> Option<&[u8; PROLOGUE_LENGTH]> {
        self.captured
            .iter()
            .find(|(at, _)| *at == address)
            .map(|(_, prologue)| prologue)
    }

    /// Forgets everything. Called when the game exits, so a stale page from a previous
    /// process cannot make a foreign detour look like ours.
    pub fn clear(&mut self) {
        self.captured.clear();
        self.pages.clear();
    }
}

/// Decides whether a site may be patched.
///
/// Pure: bytes in, verdict out. That is what makes every branch testable without a game,
/// and the reason this is separate from the write.
#[must_use]
pub fn inspect(
    state: &InjectionState,
    module: &Module,
    address: u64,
    current: &[u8; PROLOGUE_LENGTH],
) -> Verdict {
    if !module.contains(address) {
        // A signature that matched nothing resolves to zero, and a hostile one can resolve
        // anywhere. Either way this is not an address inside the module.
        return Verdict::Refuse(RefusalReason::OutsideModule {
            address,
            module: module.name.clone(),
        });
    }

    if current[0] == 0xe9 {
        let relative = i32::from_le_bytes([current[1], current[2], current[3], current[4]]);
        let destination = address
            .wrapping_add(DETOUR_LENGTH as u64)
            .wrapping_add_signed(i64::from(relative));
        return if state.owns(destination) {
            Verdict::AlreadyOurs { destination }
        } else {
            Verdict::Refuse(RefusalReason::ForeignDetour { destination })
        };
    }

    if current.iter().all(|byte| *byte == 0) {
        // Unmapped or freshly zeroed memory reads as zeros. It means the address is
        // wrong, not that the function is unusual.
        return Verdict::Refuse(RefusalReason::NotCode { address });
    }

    let Some(captured) = state.captured_at(address) else {
        return Verdict::Refuse(RefusalReason::NotCaptured { address });
    };
    if captured != current {
        // The whole prologue, not the five bytes the patch overwrites. An instruction
        // starting at +4 ends outside them, so those five can match while the code they
        // are part of has changed underneath.
        return Verdict::Refuse(RefusalReason::Changed { address });
    }
    Verdict::Ready
}

/// Reads a site and decides on it in one step.
///
/// # Errors
///
/// Never: an unreadable prologue is a [`Verdict::Refuse`] rather than an error, because
/// the caller's next move is the same either way and a `Result<Verdict>` invites a `?`
/// that skips the refusal.
#[must_use]
pub fn inspect_site(
    state: &InjectionState,
    memory: &dyn ProcessMemory,
    module: &Module,
    address: u64,
) -> Verdict {
    let mut current = [0u8; PROLOGUE_LENGTH];
    if let Err(source) = memory.read_exact(address, &mut current) {
        return Verdict::Refuse(RefusalReason::Unreadable { address, source });
    }
    inspect(state, module, address, &current)
}

/// Builds the five bytes that jump from `from` to `to`.
///
/// # Errors
///
/// Returns [`RefusalReason::OutsideModule`] shaped as `None` if the distance does not fit
/// in a signed 32-bit displacement, which is what an allocation far from the module
/// produces on a 64-bit target — and the reason this path is 32-bit only.
#[must_use]
pub fn detour_bytes(from: u64, to: u64) -> Option<[u8; DETOUR_LENGTH]> {
    // Signed arithmetic on addresses, done once and checked. A jump's displacement is
    // the distance from the end of the instruction to the target, and it has to fit in
    // thirty-two signed bits — which is the whole reason this path is 32-bit only.
    let from = i64::try_from(from).ok()?;
    let to = i64::try_from(to).ok()?;
    let length = i64::try_from(DETOUR_LENGTH).ok()?;
    let relative = i32::try_from(to.checked_sub(from)?.checked_sub(length)?).ok()?;
    let displacement = relative.to_le_bytes();
    Some([
        0xe9,
        displacement[0],
        displacement[1],
        displacement[2],
        displacement[3],
    ])
}

/// Whether every site is safe to write, given their verdicts.
///
/// Both detours are decided before either is written, so a refusal leaves the process
/// exactly as it was found rather than half patched.
#[must_use]
pub fn all_ready(verdicts: &[Verdict]) -> bool {
    !verdicts.is_empty() && verdicts.iter().all(|verdict| *verdict == Verdict::Ready)
}

/// Whether every site already carries our detour, so there is nothing to do.
#[must_use]
pub fn all_already_ours(verdicts: &[Verdict]) -> bool {
    !verdicts.is_empty()
        && verdicts
            .iter()
            .all(|verdict| matches!(verdict, Verdict::AlreadyOurs { .. }))
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
            size: 0x0100_0000,
        }
    }

    /// A plausible function prologue: push ebp; mov ebp, esp; sub esp, 8; …
    fn prologue() -> [u8; PROLOGUE_LENGTH] {
        let mut bytes = [0x90u8; PROLOGUE_LENGTH];
        bytes[..6].copy_from_slice(&[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08]);
        bytes
    }

    #[test]
    fn a_captured_prologue_that_has_not_moved_may_be_patched() {
        let address = 0x1000_1000;
        let process = SparseProcess::new(false).with_region(address, prologue().to_vec());
        let mut state = InjectionState::new();
        state.capture(&process, address).expect("captures");

        assert_eq!(
            inspect_site(&state, &process, &module(), address),
            Verdict::Ready
        );
    }

    #[test]
    fn refuses_a_prologue_that_changed_beyond_the_five_bytes_being_patched() {
        // The point of the whole module. The five bytes the patch overwrites are
        // untouched; the instruction that starts inside them and ends outside is not.
        let address = 0x1000_1000;
        let mut state = InjectionState::new();
        let original = SparseProcess::new(false).with_region(address, prologue().to_vec());
        state.capture(&original, address).expect("captures");

        let mut changed = prologue();
        changed[5] = 0x10;
        assert_eq!(changed[..DETOUR_LENGTH], prologue()[..DETOUR_LENGTH]);

        let verdict = inspect(&state, &module(), address, &changed);
        assert!(
            matches!(verdict, Verdict::Refuse(RefusalReason::Changed { .. })),
            "a five-byte check would have accepted this: {verdict:?}"
        );
    }

    #[test]
    fn recognises_its_own_detour_and_leaves_it_alone() {
        // The app restarts, the game does not. A check that only knows "matches" and
        // "does not" refuses here, and the mod stamp stays broken until the player
        // restarts the game.
        let address = 0x1000_1000;
        let page = 0x2000_0000;
        let mut state = InjectionState::new();
        state.note_page(page);

        let mut patched = [0x90u8; PROLOGUE_LENGTH];
        patched[..DETOUR_LENGTH].copy_from_slice(&detour_bytes(address, page + 0x10).unwrap());

        assert_eq!(
            inspect(&state, &module(), address, &patched),
            Verdict::AlreadyOurs {
                destination: page + 0x10
            }
        );
    }

    #[test]
    fn refuses_a_detour_that_belongs_to_something_else() {
        let address = 0x1000_1000;
        let mut state = InjectionState::new();
        state.note_page(0x2000_0000);

        let mut patched = [0x90u8; PROLOGUE_LENGTH];
        // Somewhere we did not allocate: another tool owns this function, and
        // overwriting it breaks whatever it is doing.
        patched[..DETOUR_LENGTH].copy_from_slice(&detour_bytes(address, 0x3000_0000).unwrap());

        assert!(matches!(
            inspect(&state, &module(), address, &patched),
            Verdict::Refuse(RefusalReason::ForeignDetour { .. })
        ));
    }

    #[test]
    fn refuses_an_address_outside_the_module() {
        let state = InjectionState::new();
        assert!(matches!(
            inspect(&state, &module(), 0, &prologue()),
            Verdict::Refuse(RefusalReason::OutsideModule { .. })
        ));
        assert!(matches!(
            inspect(&state, &module(), 0x9000_0000, &prologue()),
            Verdict::Refuse(RefusalReason::OutsideModule { .. })
        ));
    }

    #[test]
    fn refuses_zeros_because_they_are_not_code() {
        let state = InjectionState::new();
        assert!(matches!(
            inspect(&state, &module(), 0x1000_1000, &[0u8; PROLOGUE_LENGTH]),
            Verdict::Refuse(RefusalReason::NotCode { .. })
        ));
    }

    #[test]
    fn refuses_a_site_that_was_never_captured() {
        // Patching without having looked first is exactly what the replay exists to stop.
        let state = InjectionState::new();
        assert!(matches!(
            inspect(&state, &module(), 0x1000_1000, &prologue()),
            Verdict::Refuse(RefusalReason::NotCaptured { .. })
        ));
    }

    #[test]
    fn an_unreadable_prologue_is_a_refusal_rather_than_an_error() {
        let state = InjectionState::new();
        let empty = SparseProcess::new(false);
        assert!(matches!(
            inspect_site(&state, &empty, &module(), 0x1000_1000),
            Verdict::Refuse(RefusalReason::Unreadable { .. })
        ));
    }

    #[test]
    fn builds_a_jump_that_lands_where_it_says() {
        let from = 0x1000_1000u64;
        let to = 0x1000_2000u64;
        let bytes = detour_bytes(from, to).expect("in range");
        assert_eq!(bytes[0], 0xe9);

        let relative = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        let landed = from
            .wrapping_add(DETOUR_LENGTH as u64)
            .wrapping_add_signed(i64::from(relative));
        assert_eq!(landed, to);
    }

    #[test]
    fn refuses_a_jump_that_does_not_fit_in_a_displacement() {
        // Which is what an allocation far from the module produces on a 64-bit target,
        // and the reason this path is 32-bit only.
        assert!(detour_bytes(0x1000_0000, 0x9000_0000_0000).is_none());
    }

    #[test]
    fn both_sites_are_decided_before_either_is_written() {
        // A refusal has to leave the process as it was found rather than half patched.
        let ready = vec![Verdict::Ready, Verdict::Ready];
        assert!(all_ready(&ready));

        let mixed = vec![
            Verdict::Ready,
            Verdict::Refuse(RefusalReason::NotCode { address: 1 }),
        ];
        assert!(!all_ready(&mixed));
        assert!(!all_already_ours(&mixed));

        let done = vec![
            Verdict::AlreadyOurs { destination: 1 },
            Verdict::AlreadyOurs { destination: 2 },
        ];
        assert!(all_already_ours(&done));
        assert!(!all_ready(&done));

        // And an empty list is neither, so a caller that found no sites does not read as
        // "everything is fine".
        assert!(!all_ready(&[]));
        assert!(!all_already_ours(&[]));
    }

    #[test]
    fn forgetting_state_stops_a_stale_page_vouching_for_a_foreign_detour() {
        let address = 0x1000_1000;
        let mut state = InjectionState::new();
        state.note_page(0x2000_0000);
        let mut patched = [0x90u8; PROLOGUE_LENGTH];
        patched[..DETOUR_LENGTH].copy_from_slice(&detour_bytes(address, 0x2000_0010).unwrap());
        assert!(matches!(
            inspect(&state, &module(), address, &patched),
            Verdict::AlreadyOurs { .. }
        ));

        // The game exited and a new one started at the same addresses. Nothing here is
        // ours any more.
        state.clear();
        assert!(matches!(
            inspect(&state, &module(), address, &patched),
            Verdict::Refuse(RefusalReason::ForeignDetour { .. })
        ));
    }
}
