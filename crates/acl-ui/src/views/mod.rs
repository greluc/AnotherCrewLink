//! The drawing.
//!
//! Everything else in this crate is a decision with no window in it, and that is on purpose
//! — §4.8's views are what remained. This module is where the two meet: it takes what
//! [`crate::roster`] selected, what [`crate::cosmetics`] measured and what
//! [`acl_types::player_colors`] names, and puts pixels on a screen.
//!
//! It is the only part of this crate that depends on `egui`, and the split is kept so that
//! the answers stay testable without one. A decision that has migrated into a paint
//! function is a decision nobody can check, which is the mistake `sortLobbies` was.

pub mod colour;
pub mod lobby_browser;
pub mod main;
pub mod settings;
