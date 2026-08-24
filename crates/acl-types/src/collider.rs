//! Whether a wall stands between two players.
//!
//! This is what makes "walls block audio" work, and it is a straight port of
//! `src/common/ColliderMap.ts`. The tables it reads are generated from that file by
//! `scripts/port-collider-data.mjs`, so the two implementations cannot drift apart in the
//! data — only in this function, which is why `ColliderMap.test.ts` is ported alongside
//! it and checks the same six cases against the same numbers.
//!
//! # Coordinate space
//!
//! The paths were traced in SVG coordinates, where y grows downwards and the origin sits
//! at the top left of a 80×80 box. A world point `(x, y)` is therefore at
//! `(x + 40, 40 - y)`. That conversion happens here and nowhere else.

use kurbo::{BezPath, Line, PathSeg, Point};

use crate::collider_data::{colliders_for, door_for};
use crate::map::{MapType, Vector2};

/// World origin in path coordinates.
const ORIGIN: f64 = 40.0;

/// Whether the straight line between two players crosses a wall or a closed door.
///
/// Returns `false` for a map with no collider data, which is the honest answer: an
/// unknown map means unknown walls, and silently reporting "blocked" would mute a lobby
/// the moment the game shipped a map this build had never seen.
#[must_use]
pub fn pose_collide(from: Vector2, to: Vector2, map: MapType, closed_doors: &[u32]) -> bool {
    // dlekS is The Skeld mirrored, so the same tables answer for it once x is flipped.
    let (from, to, map) = if map == MapType::TheSkeldApril {
        (
            Vector2 {
                x: -from.x,
                y: from.y,
            },
            Vector2 { x: -to.x, y: to.y },
            MapType::TheSkeld,
        )
    } else {
        (from, to, map)
    };

    if map == MapType::Unknown {
        return false;
    }

    let sight = Line::new(to_path_space(from), to_path_space(to));

    if let Some(walls) = colliders_for(map) {
        for wall in walls {
            if crosses(wall, sight) {
                return true;
            }
        }
    }

    for door_id in closed_doors {
        if let Some(door) = door_for(map, *door_id)
            && crosses(door, sight)
        {
            return true;
        }
    }

    false
}

/// World coordinates to the space the paths were traced in.
fn to_path_space(point: Vector2) -> Point {
    Point::new(point.x + ORIGIN, ORIGIN - point.y)
}

/// Whether a path crosses a line segment anywhere.
fn crosses(path_data: &str, sight: Line) -> bool {
    let Ok(path) = BezPath::from_svg(path_data) else {
        // A malformed path is a bug in the tables, not in the game. Reporting "no wall"
        // keeps players audible rather than silently muting a room; the parity test is
        // what is meant to catch it.
        return false;
    };

    // A zero-length sight line is a player listening to themselves. kurbo would report the
    // degenerate segment as intersecting anything that touches the point, so answer it
    // here rather than letting the geometry decide.
    if sight.p0 == sight.p1 {
        return false;
    }

    path.segments().any(|segment| match segment {
        // `intersect_line` returns parameters along the *segment*; a hit is only real if
        // it also falls within the sight line, which is what `line_t` bounds.
        PathSeg::Line(_) | PathSeg::Quad(_) | PathSeg::Cubic(_) => segment
            .intersect_line(sight)
            .iter()
            .any(|hit| (0.0..=1.0).contains(&hit.line_t) && (0.0..=1.0).contains(&hit.segment_t)),
    })
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    // Ported from src/common/ColliderMap.test.ts, case for case and number for number.
    // It is the parity check the port gets for free: the same six questions, answered by
    // two implementations that share only the data.

    #[test]
    fn finds_the_wall_between_two_points_on_opposite_sides_of_it() {
        // The Skeld starts with a vertical wall at path x 33.65, spanning path y 35.32 to
        // 37.57, which is world x -6.35 and world y 2.43 to 4.68.
        assert!(pose_collide(
            Vector2 { x: -7.0, y: 3.5 },
            Vector2 { x: -5.5, y: 3.5 },
            MapType::TheSkeld,
            &[]
        ));
    }

    #[test]
    fn reports_nothing_between_a_point_and_itself() {
        assert!(!pose_collide(
            Vector2 { x: -7.0, y: 3.5 },
            Vector2 { x: -7.0, y: 3.5 },
            MapType::TheSkeld,
            &[]
        ));
    }

    #[test]
    fn lets_two_points_on_the_same_side_of_that_wall_hear_each_other() {
        assert!(!pose_collide(
            Vector2 { x: -7.0, y: 3.5 },
            Vector2 { x: -6.9, y: 3.5 },
            MapType::TheSkeld,
            &[]
        ));
    }

    #[test]
    fn mirrors_the_april_fools_map_onto_the_skeld_colliders() {
        // dlekS is the Skeld flipped, so the same wall sits at world x +6.35.
        assert!(pose_collide(
            Vector2 { x: 7.0, y: 3.5 },
            Vector2 { x: 5.5, y: 3.5 },
            MapType::TheSkeldApril,
            &[]
        ));
    }

    #[test]
    fn lets_sound_through_an_open_door_and_stops_it_at_a_closed_one() {
        // Polus door 0, path 'M 51.257 48.531 V 50.205': world x 11.257, y -8.531 to -10.205.
        let a = Vector2 { x: 11.0, y: -9.5 };
        let b = Vector2 { x: 11.6, y: -9.5 };
        assert!(!pose_collide(a, b, MapType::Polus, &[]));
        assert!(pose_collide(a, b, MapType::Polus, &[0]));
    }

    #[test]
    fn never_blocks_on_a_map_it_has_no_data_for() {
        assert!(!pose_collide(
            Vector2 { x: -7.0, y: 3.5 },
            Vector2 { x: -5.5, y: 3.5 },
            MapType::Unknown,
            &[]
        ));
    }

    #[test]
    fn every_table_entry_parses() {
        // The tables are generated, so a parse failure is a generator bug rather than a
        // typo — and `crosses` deliberately answers "no wall" for one, which would make
        // every case above pass while blocking nothing.
        for map in [
            MapType::TheSkeld,
            MapType::MiraHq,
            MapType::Polus,
            MapType::Airship,
            MapType::Fungle,
            MapType::Submerged,
        ] {
            for wall in colliders_for(map).unwrap_or(&[]) {
                assert!(
                    BezPath::from_svg(wall).is_ok(),
                    "unparsable collider on {map:?}: {wall}"
                );
            }
            for door_id in 0..32 {
                if let Some(door) = door_for(map, door_id) {
                    assert!(
                        BezPath::from_svg(door).is_ok(),
                        "unparsable door {door_id} on {map:?}"
                    );
                }
            }
        }
    }
}
