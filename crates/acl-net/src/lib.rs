//! Signalling: the Socket.IO wire protocol, the session it drives, and when to rebuild a
//! peer connection.
//!
//! There is no `rust_socketio` here, and not for one release either: it pulls `backoff`
//! (RUSTSEC-2025-0012) and `instant` (RUSTSEC-2024-0384), both unmaintained with no fixed
//! version, so CI would be red from the first commit that added it. Writing the client by
//! hand is a few hundred lines against a protocol this project already has two
//! implementations of, and the five ways such a client usually fails are named as tests
//! rather than discovered in the field.

pub mod client;
pub mod engineio;
pub mod reconnect;
pub mod socketio;
pub mod transport;
