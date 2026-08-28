//! The eight places a frameless window can be dragged to resize it.
//!
//! A window with `with_decorations(false)` keeps `WS_THICKFRAME` and still has no border to
//! grab: winit takes the non-client area away with `WM_NCCALCSIZE`, so the resize margin
//! that Windows would hit-test is nought pixels wide. `with_resizable(true)` is true and
//! useless — the window resizes under `Win`+arrow and under a program that sets its size,
//! and not under a mouse.
//!
//! So the client hit-tests it. [`regions`] carves the window's rim into eight rectangles and
//! [`interact`] turns a drag on one into [`egui::ViewportCommand::BeginResize`], which hands
//! the drag back to the window manager — snap, the minimum size and the double-click-to-
//! maximise gesture all keep working, because after that point it is an ordinary system
//! resize.
//!
//! # Why fixed rectangles
//!
//! The first version put a single small interactive area *at the pointer* each frame. It
//! did not work, and the reason is worth keeping: this window asks for a repaint every 200
//! milliseconds because the game state arrives five times a second. An area placed where
//! the pointer was up to a fifth of a second ago is not where the pointer is when the
//! button goes down, so the press landed on the panel underneath. Rectangles that do not
//! move cannot be behind.

use egui::{CursorIcon, Rect, ResizeDirection, Sense};

/// How far inside its own edge a window still counts as its edge, in points.
///
/// Wide enough to hit with a mouse without aiming, and narrow enough that it does not
/// swallow a click on anything drawn at the rim. Windows' own `SM_CXSIZEFRAME` plus
/// `SM_CXPADDEDBORDER` comes to eight physical pixels at 100%; this is in points, so it
/// scales with the display instead of shrinking on a dense one.
pub const MARGIN: f32 = 6.0;

/// The eight edge rectangles of `rect`, sides first and corners last.
///
/// The order is the hit-testing order and it matters: egui gives an overlap to whichever
/// widget was registered later, so the corners have to come after the sides they meet. As
/// written they do not overlap at all — the sides stop a margin short of each end — but that
/// is a property of these four expressions rather than something the caller should have to
/// re-derive if they change.
#[must_use]
pub fn regions(rect: Rect, margin: f32) -> [(ResizeDirection, Rect); 8] {
    let (left, right, top, bottom) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let corner = |x: f32, y: f32| Rect::from_min_size(egui::pos2(x, y), egui::Vec2::splat(margin));
    [
        (
            ResizeDirection::North,
            Rect::from_min_max(
                egui::pos2(left + margin, top),
                egui::pos2(right - margin, top + margin),
            ),
        ),
        (
            ResizeDirection::South,
            Rect::from_min_max(
                egui::pos2(left + margin, bottom - margin),
                egui::pos2(right - margin, bottom),
            ),
        ),
        (
            ResizeDirection::West,
            Rect::from_min_max(
                egui::pos2(left, top + margin),
                egui::pos2(left + margin, bottom - margin),
            ),
        ),
        (
            ResizeDirection::East,
            Rect::from_min_max(
                egui::pos2(right - margin, top + margin),
                egui::pos2(right, bottom - margin),
            ),
        ),
        (ResizeDirection::NorthWest, corner(left, top)),
        (ResizeDirection::NorthEast, corner(right - margin, top)),
        (ResizeDirection::SouthWest, corner(left, bottom - margin)),
        (
            ResizeDirection::SouthEast,
            corner(right - margin, bottom - margin),
        ),
    ]
}

/// The cursor that says an edge can be dragged.
#[must_use]
pub const fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::West | ResizeDirection::East => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

/// Makes the window's edges draggable.
///
/// Call once per frame and before the panel, so an edge beats whatever is drawn under it.
/// That is the trade the margin's width is chosen against: six points of every panel at the
/// window's rim belong to the resize.
///
/// Returns the direction a resize was started in, for a caller that wants to say so. Nothing
/// depends on the answer.
#[must_use]
pub fn interact(ctx: &egui::Context) -> Option<ResizeDirection> {
    let rect = ctx.content_rect();
    // A window smaller than four margins has no middle left; every click would be an edge.
    if rect.width() < MARGIN * 4.0 || rect.height() < MARGIN * 4.0 {
        return None;
    }

    let mut started = None;
    egui::Area::new(egui::Id::new("resize-edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            // The area is laid out to its content, which is nothing: without this the edge
            // rectangles are outside its clip and are never hit.
            ui.set_clip_rect(rect);
            for (at, (direction, edge)) in regions(rect, MARGIN).into_iter().enumerate() {
                let response = ui.interact(edge, ui.id().with(at), Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor(direction));
                }
                if response.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                    started = Some(direction);
                }
            }
        });
    started
}

#[cfg(test)]
mod tests {
    use super::{MARGIN, cursor, regions};
    use egui::{Pos2, Rect, ResizeDirection};

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))
    }

    /// Which region a point falls in, the way egui resolves it: later wins.
    fn at(rect: Rect, pointer: Pos2, margin: f32) -> Option<ResizeDirection> {
        regions(rect, margin)
            .into_iter()
            .rfind(|(_, edge)| edge.contains(pointer))
            .map(|(direction, _)| direction)
    }

    #[test]
    fn the_middle_is_not_an_edge() {
        assert_eq!(at(window(), Pos2::new(200.0, 150.0), MARGIN), None);
    }

    #[test]
    fn each_side_is_its_own_direction() {
        let rect = window();
        for (pointer, expected) in [
            (Pos2::new(200.0, 1.0), ResizeDirection::North),
            (Pos2::new(200.0, 299.0), ResizeDirection::South),
            (Pos2::new(1.0, 150.0), ResizeDirection::West),
            (Pos2::new(399.0, 150.0), ResizeDirection::East),
        ] {
            assert_eq!(at(rect, pointer, MARGIN), Some(expected), "at {pointer:?}");
        }
    }

    /// A corner is both margins at once, and must not resolve to one of its sides.
    ///
    /// Dragging the bottom-right of a window and having it move only downward is the
    /// failure this guards.
    #[test]
    fn a_corner_resizes_both_axes() {
        let rect = window();
        for (pointer, expected) in [
            (Pos2::new(1.0, 1.0), ResizeDirection::NorthWest),
            (Pos2::new(399.0, 1.0), ResizeDirection::NorthEast),
            (Pos2::new(1.0, 299.0), ResizeDirection::SouthWest),
            (Pos2::new(399.0, 299.0), ResizeDirection::SouthEast),
        ] {
            assert_eq!(at(rect, pointer, MARGIN), Some(expected), "at {pointer:?}");
        }
    }

    /// Nothing is registered twice.
    ///
    /// Overlapping regions are not wrong in themselves — egui would pick the later one — but
    /// they mean the corner-last ordering is load-bearing where it looks decorative, and the
    /// next person to reorder the array would silently break the corners.
    #[test]
    fn the_regions_do_not_overlap() {
        let all = regions(window(), MARGIN);
        for (first, (_, one)) in all.iter().enumerate() {
            for (_, other) in all.iter().skip(first + 1) {
                // Area, not `intersects`: a side and the corner beyond it share their
                // touching corner point, and a shared point is not a shared pixel.
                let shared = one.intersect(*other);
                assert!(
                    shared.width() <= 0.0 || shared.height() <= 0.0,
                    "{one:?} and {other:?} share {shared:?}"
                );
            }
        }
    }

    /// The rim is covered end to end, with no gap for a click to fall through.
    #[test]
    fn every_point_on_the_rim_belongs_to_something() {
        let rect = window();
        for along in 0..400 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a loop counter below four hundred"
            )]
            let x = along as f32 + 0.5;
            assert!(at(rect, Pos2::new(x, 0.5), MARGIN).is_some(), "top at {x}");
            assert!(
                at(rect, Pos2::new(x, 299.5), MARGIN).is_some(),
                "bottom at {x}"
            );
        }
    }

    /// The two diagonals are opposite cursors; swapping them is the classic slip.
    #[test]
    fn the_diagonal_cursors_are_not_the_same() {
        assert_ne!(
            cursor(ResizeDirection::NorthWest),
            cursor(ResizeDirection::NorthEast)
        );
        assert_eq!(
            cursor(ResizeDirection::NorthWest),
            cursor(ResizeDirection::SouthEast)
        );
    }
}
