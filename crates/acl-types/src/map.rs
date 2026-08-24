//! Maps, and the coordinate space the collider tables live in.

use serde::{Deserialize, Serialize};

/// A point in the game's world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vector2 {
    /// Horizontal, growing to the right.
    pub x: f64,
    /// Vertical, growing upwards — the opposite of the collider paths, see [`crate::collider`].
    pub y: f64,
}

/// Which map a lobby is playing.
///
/// The discriminants are the game's own, not ours: `SUBMERGED` is 105 because that is
/// what the mod reports. `UNKNOWN` exists so a map that has not been read yet is a value
/// rather than an absence — the Electron client learned that the hard way, where an
/// undefined map silently reported that no wall was ever in the way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum MapType {
    /// The Skeld.
    TheSkeld = 0,
    /// Mira HQ.
    MiraHq = 1,
    /// Polus.
    Polus = 2,
    /// dlekS, the April Fools mirror of The Skeld.
    TheSkeldApril = 3,
    /// The Airship.
    Airship = 4,
    /// The Fungle.
    Fungle = 5,
    /// Not read yet, or a map this build does not know.
    Unknown = 6,
    /// Submerged, a mod.
    Submerged = 105,
}

impl MapType {
    /// The map for a value read out of the game, or [`MapType::Unknown`].
    #[must_use]
    pub fn from_game(value: u32) -> Self {
        match value {
            0 => Self::TheSkeld,
            1 => Self::MiraHq,
            2 => Self::Polus,
            3 => Self::TheSkeldApril,
            4 => Self::Airship,
            5 => Self::Fungle,
            105 => Self::Submerged,
            _ => Self::Unknown,
        }
    }
}
