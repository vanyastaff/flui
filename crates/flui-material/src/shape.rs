//! [`MaterialShape`] — the minimal shape vocabulary [`crate::material::Material`]
//! clips and paints its surface to.
//!
//! # Flutter parity
//!
//! Flutter's `Material.shape` takes an arbitrary `ShapeBorder` — `material.dart`
//! (oracle tag `3.44.0`) resolves the default (M3's pill-shaped
//! `StadiumBorder`, or a rectangle, per `MaterialType`) into a path at paint
//! time via `ShapeBorder.getOuterPath`. FLUI ships the two concrete shapes
//! PR-1's `Material` actually needs — a plain/rounded rectangle and the M3
//! stadium (pill) shape — rather than the full `ShapeBorder` hierarchy
//! (`RoundedRectangleBorder`, `CircleBorder`, `ContinuousRectangleBorder`,
//! `StarBorder`, a user's own `ShapeBorder` subclass, …). [`MaterialShape`]
//! is `#[non_exhaustive]` so more shapes can be added without a breaking
//! change; a `ShapeBorder`-equivalent open trait is a larger, deliberately
//! deferred design (see the crate-level scope note in `material.rs`).
//!
//! # Named deferral: `OutlinedBorder` sides
//!
//! The oracle's `ShapeBorder`/`OutlinedBorder` hierarchy also carries a
//! `BorderSide` (stroke color/width/style) that `getInnerPath`/`paint`
//! render on top of the fill. [`MaterialShape`] is fill-and-clip-only — no
//! side is drawn. `Material.shape`'s border painting is deferred to when a
//! component actually needs an outlined surface (M3's `OutlinedButton`,
//! PR-2+).

use flui_types::{
    Point, Rect, Size,
    geometry::{RRect, Radius},
    styling::BorderRadius,
};

/// The shape a [`crate::material::Material`] surface clips and paints to.
///
/// Both variants resolve to an [`RRect`] via [`to_rrect`](Self::to_rrect) —
/// [`Stadium`](Self::Stadium)'s corner radius is `shortest_side / 2.0`, which
/// depends on the laid-out [`Size`] and so can only be computed at paint
/// time (Flutter parity: `StadiumBorder.getOuterPath`,
/// `Radius.circular(rect.shortestSide / 2.0)`, `painting/stadium_border.dart`
/// oracle tag `3.44.0`). [`RoundedRect`](Self::RoundedRect)'s radius is a
/// fixed value independent of size, but is resolved through the same
/// size-dependent path so [`crate::material::Material`] can register one
/// owner-lane path clipper regardless of which variant it holds — see
/// `material.rs`'s use of `RenderPhysicalShape`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum MaterialShape {
    /// A rectangle with per-corner radii. Flutter's
    /// `RoundedRectangleBorder`/`Material`'s `borderRadius` path — a zero
    /// [`BorderRadius`] is a plain sharp-cornered rectangle (Flutter's
    /// `MaterialType.canvas` default).
    RoundedRect(BorderRadius),
    /// A pill shape: both ends fully rounded to a semicircle whose radius is
    /// half the shortest side. Flutter's `StadiumBorder` — the M3 default
    /// shape for filled buttons.
    Stadium,
}

impl MaterialShape {
    /// A plain, sharp-cornered rectangle — [`MaterialShape`]'s default
    /// (Flutter's `MaterialType.canvas`, the oracle default when no `shape`,
    /// `borderRadius`, or non-canvas `type` is given).
    #[must_use]
    pub fn rectangle() -> Self {
        Self::RoundedRect(BorderRadius::all(Radius::ZERO))
    }

    /// Resolves this shape to a rounded rectangle covering `size` (placed at
    /// the local origin — the same convention `RenderPhysicalShape`'s path
    /// clipper closures use).
    #[must_use]
    pub fn to_rrect(self, size: Size) -> RRect {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        match self {
            Self::RoundedRect(radius) => RRect::from_rect_and_corners(
                bounds,
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left,
            ),
            Self::Stadium => {
                let shortest_side = size.width.get().min(size.height.get());
                let radius = Radius::circular(flui_types::geometry::px(shortest_side / 2.0));
                RRect::from_rect_and_radius(bounds, radius)
            }
        }
    }

    /// [`to_rrect`](Self::to_rrect), converted to a [`flui_types::painting::Path`]
    /// — what [`crate::material::Material`] registers as its owner-lane path
    /// clipper.
    #[must_use]
    pub fn to_path(self, size: Size) -> flui_types::painting::Path {
        flui_types::painting::Path::from_rrect(self.to_rrect(size))
    }
}

impl Default for MaterialShape {
    /// [`Self::rectangle`] — Flutter's `MaterialType.canvas` default.
    fn default() -> Self {
        Self::rectangle()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;
    use flui_types::styling::BorderRadiusExt;

    use super::*;

    fn size(width: f32, height: f32) -> Size {
        Size::new(px(width), px(height))
    }

    #[test]
    fn rounded_rect_uses_the_configured_per_corner_radii() {
        let radius = BorderRadius::only(
            Radius::circular(px(4.0)),
            Radius::circular(px(8.0)),
            Radius::circular(px(12.0)),
            Radius::circular(px(16.0)),
        );
        let rrect = MaterialShape::RoundedRect(radius).to_rrect(size(100.0, 50.0));

        assert_eq!(rrect.top_left, radius.top_left);
        assert_eq!(rrect.top_right, radius.top_right);
        assert_eq!(rrect.bottom_right, radius.bottom_right);
        assert_eq!(rrect.bottom_left, radius.bottom_left);
        assert_eq!(
            rrect.rect,
            Rect::from_origin_size(Point::ZERO, size(100.0, 50.0))
        );
    }

    #[test]
    fn stadium_radius_is_half_the_shortest_side_when_wider_than_tall() {
        let rrect = MaterialShape::Stadium.to_rrect(size(120.0, 40.0));
        // shortest side is height (40); radius = 20.
        assert_eq!(rrect.top_left, Radius::circular(px(20.0)));
        assert_eq!(rrect.top_right, Radius::circular(px(20.0)));
        assert_eq!(rrect.bottom_right, Radius::circular(px(20.0)));
        assert_eq!(rrect.bottom_left, Radius::circular(px(20.0)));
    }

    #[test]
    fn stadium_radius_is_half_the_shortest_side_when_taller_than_wide() {
        let rrect = MaterialShape::Stadium.to_rrect(size(30.0, 90.0));
        // shortest side is width (30); radius = 15.
        assert_eq!(rrect.top_left, Radius::circular(px(15.0)));
    }

    #[test]
    fn stadium_radius_on_a_square_is_half_that_side() {
        let rrect = MaterialShape::Stadium.to_rrect(size(50.0, 50.0));
        assert_eq!(rrect.top_left, Radius::circular(px(25.0)));
    }

    #[test]
    fn rectangle_constructor_is_a_zero_radius_rounded_rect() {
        let rrect = MaterialShape::rectangle().to_rrect(size(10.0, 10.0));
        assert_eq!(rrect.top_left, Radius::ZERO);
        assert_eq!(rrect.top_right, Radius::ZERO);
    }

    #[test]
    fn default_is_the_plain_rectangle() {
        assert_eq!(MaterialShape::default(), MaterialShape::rectangle());
    }

    #[test]
    fn to_path_traces_the_same_shape_as_to_rrect() {
        let shape = MaterialShape::Stadium;
        let dimensions = size(80.0, 40.0);
        let rrect = shape.to_rrect(dimensions);
        let mut path = shape.to_path(dimensions);
        // A stadium's bounding box matches the rrect's rect exactly.
        assert_eq!(path.bounds(), rrect.rect);
    }
}
