//! Reading Among Us out of another process.
//!
//! Phase 2 of `docs/rust-port/04-implementation-plan.md`. The layer that touches the
//! operating system is small and lives in [`windows`] and [`linux`]; everything above it
//! takes a `&dyn ProcessMemory` and returns a `Result`, which is what lets gate G1 replay
//! recorded frames and a fuzzer explore the same code without a game running.

pub mod dotnet;
pub mod memory;
pub mod mods;
pub mod offsets;
pub mod reader;
pub mod resolve;
pub mod scan;
pub mod sparse;
pub mod state;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

pub use memory::{MAX_CHAIN_DEPTH, MAX_ELEMENTS, Module, ProcessMemory, ReadError};
pub use sparse::{Region, SparseProcess};
pub use state::{AmongUsState, GameState, Player};
