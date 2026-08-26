//! Where an avatar's layers go, and in what order they are painted.
//!
//! §3.3 of the architecture assigns this to `acl-ui`: "Avatars are composited in `acl-ui`:
//! the recoloured base sprite ..., then hat-back, skin, hat-front, visor and pet as
//! textures. The hat collection is still fetched at runtime, but the per-hat geometry —
//! currently CSS strings like `"32%"` — is parsed once into `f32` fractions at load."
//!
//! This is that parse and that arithmetic. It is also what the overlay's sprite channel
//! needs: `acl_ipc::CoreMessage::DrawSprite` carries a rasterised picture and a position,
//! and something has to decide the position.
//!
//! # What the collection actually contains
//!
//! Measured on 2026-08-26 against the pinned collection —
//! `AnotherCrewLink-Hats@14bb0cb5`, 983 hats:
//!
//! * **one mod entry**, `NONE`, whose defaults are `130%`, `-78%`, `-14%`;
//! * **not one hat carries its own geometry**. Every value in the field today is the mod
//!   default.
//!
//! Both halves matter. The per-hat override is kept because the format has it and a future
//! collection may use it — but nothing exercises it today, so a mistake there would ship
//! unnoticed, which is why it has tests of its own. And percent is the only unit that
//! appears, which is what makes a fraction the right representation at all.
//!
//! # Where it differs from the client it replaces
//!
//! The shipped client passes these strings straight into CSS, so anything CSS understands
//! works there: `px`, `em`, `calc()`. Here they become fractions of the avatar's box, and a
//! value in any other unit cannot be one — the avatar's size is not known when the
//! collection is parsed. Such a value is refused rather than guessed at, and refused means
//! the layer falls back to zero, exactly as an absent one does.

/// The layers of an avatar, in the order they are painted.
///
/// Back to front. Taken from the z-indices in `Avatar.tsx` — hat-back 1, base 2, skin and
/// visor 3, hat-front 4 — with the tie between skin and visor broken the way the DOM breaks
/// it: the visor element comes after the skin, so it is on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The part of a hat that sits behind the head.
    HatBack,
    /// The body, recoloured.
    Base,
    /// The skin.
    Skin,
    /// The visor.
    Visor,
    /// The hat itself.
    HatFront,
}

/// Every layer, back to front.
pub const PAINT_ORDER: [Layer; 5] = [
    Layer::HatBack,
    Layer::Base,
    Layer::Skin,
    Layer::Visor,
    Layer::HatFront,
];

/// Where the base sprite's top edge sits, as a fraction of the avatar's box.
///
/// `top: '22%'` in `Avatar.tsx`, and the cosmetics are offset from it rather than from the
/// box — `calc(22% + <top>)`. A hat's own `top` is therefore relative to the head, not to
/// the frame, which is why the collection's default is negative.
pub const BASE_TOP: f32 = 0.22;

/// How wide the base sprite is drawn, as a fraction of the avatar's box.
///
/// `width: '105%'`, so it overflows its container slightly. Kept because changing it moves
/// every hat relative to every head.
pub const BASE_WIDTH: f32 = 1.05;

/// One layer's placement, as fractions of the avatar's box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    /// Offset from the base's top edge, downward.
    pub top: f32,
    /// Offset from the box's left edge, rightward.
    pub left: f32,
    /// Width.
    pub width: f32,
}

/// Reads one of the collection's values.
///
/// Percentages and a bare zero, and nothing else. A bare zero because `'0'` is what the
/// shipped code substitutes for an absent value and is valid CSS unitless; anything with
/// another unit is refused, for the reason in the module documentation.
///
/// # Examples
///
/// ```
/// use acl_ui::cosmetics::fraction;
/// assert_eq!(fraction("130%"), Some(1.3));
/// assert_eq!(fraction("-78%"), Some(-0.78));
/// assert_eq!(fraction("0"), Some(0.0));
/// assert_eq!(fraction("12px"), None);
/// ```
#[must_use]
pub fn fraction(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return percent
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value / 100.0);
    }
    // Unitless is only meaningful at zero. `'1'` in a CSS length is invalid, and treating
    // it as one whole avatar would put a hat somewhere nobody asked for.
    //
    // Compared against a literal rather than a tolerance, and deliberately: a tolerance
    // would let a near-zero unitless value through as a length, which it is not.
    match trimmed {
        "0" | "-0" | "0.0" | "-0.0" | "0." => Some(0.0),
        _ => None,
    }
}

/// Resolves one layer's geometry from the hat's own values and the collection's defaults.
///
/// The order is the shipped one: the hat's value, then the mod's default, then zero. Each
/// axis independently — a hat that overrides only its width keeps the default top and left,
/// which is what `getHat`'s three `??` do.
#[must_use]
pub fn resolve(own: [Option<&str>; 3], defaults: [Option<&str>; 3]) -> Geometry {
    let axis = |index: usize| -> f32 {
        own.get(index)
            .copied()
            .flatten()
            .and_then(fraction)
            .or_else(|| defaults.get(index).copied().flatten().and_then(fraction))
            .unwrap_or(0.0)
    };
    Geometry {
        top: axis(0),
        left: axis(1),
        width: axis(2),
    }
}

/// A rectangle in pixels, relative to the avatar's own top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
}

/// How thick the ring around an avatar is drawn.
///
/// `Math.max(2, size / 40)` in `Avatar.tsx`. It is here because half of it is part of every
/// cosmetic's horizontal offset, so a change to one is a change to where every hat sits.
#[must_use]
pub fn border(size: f32) -> f32 {
    (size / 40.0).max(2.0)
}

/// Where the base sprite goes.
///
/// `padding_left` is the caller's own inset, in pixels — `paddingLeft` in `Avatar.tsx`,
/// which the compact overlay uses to slide an avatar sideways within its slot.
#[must_use]
pub fn base_rect(size: f32, padding_left: f32) -> Rect {
    Rect {
        x: padding_left,
        y: BASE_TOP * size,
        width: BASE_WIDTH * size,
    }
}

/// Where one cosmetic goes.
///
/// `top: calc(22% + <top>)` and `left: calc(<left> + <border/2 + padding>)`, both from
/// `Avatar.tsx`. The vertical offset is relative to the base's top and the horizontal one is
/// not, which reads like an inconsistency and is the shipped behaviour: the collection's
/// values were tuned against it, so "correcting" it moves 983 hats at once.
#[must_use]
pub fn cosmetic_rect(geometry: Geometry, size: f32, padding_left: f32) -> Rect {
    Rect {
        x: geometry
            .left
            .mul_add(size, border(size) / 2.0 + padding_left),
        y: (BASE_TOP + geometry.top) * size,
        width: geometry.width * size,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{
        BASE_TOP, BASE_WIDTH, Geometry, Layer, PAINT_ORDER, base_rect, border, cosmetic_rect,
        fraction, resolve,
    };

    /// The three values the whole collection actually uses today.
    const DEFAULTS: [Option<&str>; 3] = [Some("-78%"), Some("-14%"), Some("130%")];

    /// A thousandth of a pixel. These are screen coordinates arrived at by multiplying a
    /// fraction by a size, so they carry `f32`'s rounding; a tolerance tighter than the
    /// smallest thing anybody can see is a test about arithmetic rather than about layout.
    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1e-3
    }

    #[test]
    fn a_percentage_is_a_fraction() {
        assert_eq!(fraction("130%"), Some(1.3));
        assert_eq!(fraction("-78%"), Some(-0.78));
        assert_eq!(fraction(" 50 % "), Some(0.5));
    }

    /// `'0'` is what the shipped code substitutes for an absent value, and unitless zero is
    /// valid CSS. Any other bare number is not a length at all, and reading `1` as a whole
    /// avatar would put a hat somewhere nobody asked for.
    #[test]
    fn a_bare_zero_is_zero_and_a_bare_one_is_nothing() {
        assert_eq!(fraction("0"), Some(0.0));
        assert_eq!(fraction("0.0"), Some(0.0));
        assert_eq!(fraction("1"), None);
        assert_eq!(fraction("-3"), None);
    }

    /// Units this cannot turn into a fraction are refused rather than guessed at: the
    /// avatar's size is not known when the collection is parsed. The shipped client passes
    /// them to CSS, which does know, so this is a real difference and it is deliberate.
    #[test]
    fn any_other_unit_is_refused() {
        for value in ["12px", "1em", "calc(10% + 2px)", "", "%", "auto"] {
            assert_eq!(fraction(value), None, "{value:?} should not parse");
        }
    }

    /// Each axis falls back on its own, which is what `getHat`'s three separate `??` do.
    #[test]
    fn a_hat_can_override_one_axis_and_keep_the_others() {
        let geometry = resolve([None, None, Some("90%")], DEFAULTS);
        assert!(close(geometry.top, -0.78));
        assert!(close(geometry.left, -0.14));
        assert!(close(geometry.width, 0.9));
    }

    /// Nothing anywhere is zero, which is `getHatDementions`'s `?? '0'`.
    #[test]
    fn nothing_at_all_is_zero() {
        assert_eq!(resolve([None; 3], [None; 3]), Geometry::default());
    }

    /// A value that cannot be read falls through to the default, and then to zero. It must
    /// not be taken as zero *before* the default is tried, or a collection with one bad
    /// entry would drop that hat to the corner instead of using the mod's placement.
    #[test]
    fn an_unreadable_value_falls_through_to_the_default() {
        let geometry = resolve([Some("12px"), None, None], DEFAULTS);
        assert!(
            close(geometry.top, -0.78),
            "a value in an unknown unit should have fallen through, got {}",
            geometry.top
        );

        assert!(
            close(resolve([Some("12px"), None, None], [None; 3]).top, 0.0),
            "and with no default either, zero"
        );
    }

    /// The border is half of every cosmetic's horizontal offset, so it is part of this
    /// arithmetic rather than a drawing detail.
    #[test]
    fn the_border_never_goes_below_two_pixels() {
        assert!(close(border(40.0), 2.0));
        assert!(
            close(border(20.0), 2.0),
            "small avatars keep a visible ring"
        );
        assert!(close(border(400.0), 10.0));
    }

    /// The base overflows its box slightly and starts a fifth of the way down.
    #[test]
    fn the_base_sits_where_the_shipped_client_puts_it() {
        let rect = base_rect(100.0, 0.0);
        assert!(close(rect.y, BASE_TOP * 100.0));
        assert!(close(rect.width, BASE_WIDTH * 100.0));
        assert!(close(rect.x, 0.0));
        assert!(close(base_rect(100.0, 7.0).x, 7.0), "the inset shifts it");
    }

    /// The vertical offset is relative to the base's top and the horizontal one is not.
    /// That reads like an inconsistency and is the shipped behaviour; the collection's 983
    /// hats were tuned against it.
    #[test]
    fn a_cosmetic_is_offset_from_the_head_vertically_and_from_the_frame_horizontally() {
        let geometry = resolve([None; 3], DEFAULTS);
        let rect = cosmetic_rect(geometry, 200.0, 0.0);
        // 22% - 78% = -56% of 200.
        assert!(close(rect.y, -112.0), "got {}", rect.y);
        // -14% of 200, plus half a 5px border.
        assert!(close(rect.x, -28.0 + 2.5), "got {}", rect.x);
        assert!(close(rect.width, 260.0));
    }

    /// Back to front, and the tie between skin and visor broken the way the DOM breaks it.
    #[test]
    fn the_paint_order_is_back_to_front() {
        assert_eq!(
            PAINT_ORDER,
            [
                Layer::HatBack,
                Layer::Base,
                Layer::Skin,
                Layer::Visor,
                Layer::HatFront
            ]
        );
    }
}
