//! Shared types, map data and collider tables.
//!
//! This crate has no GUI, no audio and no platform dependency, on purpose: it is what
//! makes the go/no-go gates in `docs/rust-port/04-implementation-plan.md` possible, since
//! every one of them wants to test a piece of this port without standing up the whole
//! application.

pub mod collider;
mod collider_data;
pub mod map;
pub mod mods;
pub mod player_colors;

pub use collider::pose_collide;
pub use map::{MapType, Vector2};
