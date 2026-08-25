//! The unelevated half of the client.
//!
//! `docs/rust-port/04-implementation-plan.md` §4.7 splits the client in two: `acl-helper`
//! runs elevated and holds the memory reader, the keyboard hook and the overlay window;
//! this side holds tokio, signalling, WebRTC, audio and the GUI, and never elevates.
//!
//! What is here now is the part of that split which is a decision rather than a platform
//! call: what still works when there is no helper, and when the client is allowed to ask
//! for one. Both are things a port gets subtly wrong and nobody notices until a player
//! answers a dialog with No and then cannot speak.

pub mod helper;
