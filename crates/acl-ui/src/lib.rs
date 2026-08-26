//! Phase 6's views, and the decisions behind them.
//!
//! `docs/rust-port/03-target-architecture.md` puts the egui views here: main, settings,
//! lobby browser, overlay. None of them exists yet.
//!
//! What is here is the part of a view that is not a view — the ordering, the filtering,
//! the formatting a screen needs before it can draw anything. It lives in its own modules
//! because a decision inside a paint function is a decision nobody can test, and this
//! project has already paid for that once: `sortLobbies` was four lines inside
//! `LobbyBrowser.tsx` and was not a consistent ordering for the whole life of the
//! feature, because nothing could reach it to check.

pub mod config;
pub mod cosmetics;
pub mod lobby_list;
pub mod renderer;
pub mod roster;
pub mod server_url;
pub mod settings;
pub mod settings_screen;
pub mod sprite;
pub mod views;
pub mod window_state;
