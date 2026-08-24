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

/// Where a security camera sits, when a player is looking through one.
///
/// The numbering is the Electron client's `CameraLocation`, and it is a wire value: it
/// reaches `voice_params` from the game reader and decides where a distant player is
/// heard from. `None` is 7 and is a value rather than an absence, which is why the reader
/// defaults to it rather than to zero — zero is the engine room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CameraLocation {
    /// Engine room.
    East = 0,
    /// Vault.
    Central = 1,
    /// Records.
    Northeast = 2,
    /// Security.
    South = 3,
    /// Cargo bay.
    SouthWest = 4,
    /// Meeting room.
    NorthWest = 5,
    /// The Skeld has one console covering four rooms, so it has no index of its own.
    Skeld = 6,
    /// Not looking through a camera.
    None = 7,
}

impl CameraLocation {
    /// The camera for a value out of the game state, or [`CameraLocation::None`].
    #[must_use]
    pub fn from_state(value: u32) -> Self {
        match value {
            0 => Self::East,
            1 => Self::Central,
            2 => Self::Northeast,
            3 => Self::South,
            4 => Self::SouthWest,
            5 => Self::NorthWest,
            6 => Self::Skeld,
            _ => Self::None,
        }
    }
}

/// The camera positions for one map, in the game's coordinates.
///
/// Transcribed from `src/common/AmongusMap.ts`. Three maps have none: Mira HQ and The
/// Fungle have no camera console, and Submerged's are not mapped. The Skeld's four are
/// keyed 0 to 3 because its console shows all four at once and the client picks the one
/// nearest the speaker rather than the one being watched — the reason [`nearest`] exists.
#[must_use]
pub fn cameras(map: MapType) -> &'static [(CameraLocation, Vector2)] {
    const SKELD: [(CameraLocation, Vector2); 4] = [
        (
            CameraLocation::East,
            Vector2 {
                x: 13.2417,
                y: -4.348,
            },
        ),
        (
            CameraLocation::Central,
            Vector2 {
                x: 0.6216,
                y: -6.5642,
            },
        ),
        (
            CameraLocation::Northeast,
            Vector2 {
                x: -7.1503,
                y: 1.6709,
            },
        ),
        (
            CameraLocation::South,
            Vector2 {
                x: -17.8098,
                y: -4.8983,
            },
        ),
    ];
    const POLUS: [(CameraLocation, Vector2); 6] = [
        (CameraLocation::East, Vector2 { x: 29.0, y: -15.7 }),
        (CameraLocation::Central, Vector2 { x: 15.4, y: -15.4 }),
        (CameraLocation::Northeast, Vector2 { x: 24.4, y: -8.5 }),
        (CameraLocation::South, Vector2 { x: 17.0, y: -20.6 }),
        (CameraLocation::SouthWest, Vector2 { x: 4.7, y: -22.73 }),
        (CameraLocation::NorthWest, Vector2 { x: 11.6, y: -8.2 }),
    ];
    const AIRSHIP: [(CameraLocation, Vector2); 6] = [
        (
            CameraLocation::East,
            Vector2 {
                x: -8.2872,
                y: 0.0527,
            },
        ),
        (
            CameraLocation::Central,
            Vector2 {
                x: -4.0477,
                y: 9.1447,
            },
        ),
        (
            CameraLocation::Northeast,
            Vector2 {
                x: 23.5616,
                y: 9.8882,
            },
        ),
        (
            CameraLocation::South,
            Vector2 {
                x: 4.881,
                y: -11.1688,
            },
        ),
        (
            CameraLocation::SouthWest,
            Vector2 {
                x: 30.3702,
                y: -0.874,
            },
        ),
        (
            CameraLocation::NorthWest,
            Vector2 {
                x: 3.3018,
                y: 16.2631,
            },
        ),
    ];

    match map {
        MapType::TheSkeld => &SKELD,
        MapType::Polus => &POLUS,
        MapType::Airship => &AIRSHIP,
        // The April Fools mirror shares The Skeld's geometry for collision, but the
        // Electron client gives it no cameras, and this table is that client's.
        _ => &[],
    }
}

/// The camera position for one location on one map, if the map has that camera.
#[must_use]
pub fn camera(map: MapType, at: CameraLocation) -> Option<Vector2> {
    cameras(map)
        .iter()
        .find(|(location, _)| *location == at)
        .map(|(_, position)| *position)
}

/// The camera nearest a point, for The Skeld.
///
/// Its console shows four rooms at once, so the client picks the camera closest to the
/// player being heard rather than one the watcher selected. Returns `None` for a map with
/// no cameras, which is what stops a map without camera data from silently behaving as
/// though the listener were standing at the origin.
#[must_use]
pub fn nearest(map: MapType, to: Vector2) -> Option<Vector2> {
    cameras(map)
        .iter()
        .map(|(_, position)| *position)
        .min_by(|a, b| {
            let da = (to.x - a.x).powi(2) + (to.y - a.y).powi(2);
            let db = (to.x - b.x).powi(2) + (to.y - b.y).powi(2);
            da.total_cmp(&db)
        })
}

#[cfg(test)]
mod camera_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn the_maps_with_cameras_are_the_ones_the_client_lists() {
        assert_eq!(cameras(MapType::TheSkeld).len(), 4);
        assert_eq!(cameras(MapType::Polus).len(), 6);
        assert_eq!(cameras(MapType::Airship).len(), 6);
        // Mira HQ and The Fungle have no camera console; Submerged's are not mapped.
        assert!(cameras(MapType::MiraHq).is_empty());
        assert!(cameras(MapType::Fungle).is_empty());
        assert!(cameras(MapType::Submerged).is_empty());
        assert!(cameras(MapType::Unknown).is_empty());
    }

    #[test]
    fn looks_up_one_camera_by_location() {
        let central = camera(MapType::Polus, CameraLocation::Central).unwrap();
        assert!((central.x - 15.4).abs() < 1e-9);
        assert!((central.y - -15.4).abs() < 1e-9);
        // Polus has no Skeld camera: that location belongs to the other console.
        assert!(camera(MapType::Polus, CameraLocation::Skeld).is_none());
    }

    #[test]
    fn picks_the_nearest_camera_on_the_skeld() {
        // Standing next to the first one.
        let near = nearest(MapType::TheSkeld, Vector2 { x: 13.0, y: -4.3 }).unwrap();
        assert!((near.x - 13.2417).abs() < 1e-9);
    }

    #[test]
    fn a_map_without_cameras_has_no_nearest_one() {
        // Not the origin, which is what a `(0, 0)` fallback would make it and would put
        // every listener in the middle of the map.
        assert!(nearest(MapType::MiraHq, Vector2 { x: 1.0, y: 1.0 }).is_none());
    }

    #[test]
    fn a_camera_number_the_client_does_not_know_is_none() {
        assert_eq!(CameraLocation::from_state(0), CameraLocation::East);
        assert_eq!(CameraLocation::from_state(6), CameraLocation::Skeld);
        assert_eq!(CameraLocation::from_state(7), CameraLocation::None);
        assert_eq!(CameraLocation::from_state(99), CameraLocation::None);
    }
}
