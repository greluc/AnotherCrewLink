//! Reading Among Us out of another process.
//!
//! Phase 2 of `docs/rust-port/04-implementation-plan.md`. The layer that touches the
//! operating system is small and lives in [`windows`]; everything above it takes a
//! `&dyn ProcessMemory` and returns a `Result`, which is what lets gate G1 replay recorded
//! frames and a fuzzer explore the same code without a game running.
//!
//! There was a `linux` module beside it until 2026-08-25, reading through
//! `process_vm_readv`. It went with the client's Linux support: it was the only thing in
//! the workspace that pulled in `nix`, and nobody here could run it.

pub mod dotnet;
pub mod memory;
pub mod mods;
pub mod offsets;
pub mod reader;
pub mod resolve;
pub mod scan;
pub mod sparse;
pub mod state;
pub mod store;
pub mod systems;

#[cfg(windows)]
pub mod windows;

pub use memory::{MAX_CHAIN_DEPTH, MAX_ELEMENTS, Module, ProcessMemory, ReadError};
pub use sparse::{Region, SparseProcess};
pub use state::{AmongUsState, GameState, Player};
