//! `RenderLimitedBox` — caps unbounded incoming constraints with explicit
//! `max_width` / `max_height` values, leaving bounded constraints untouched.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderLimitedBox`](https://api.flutter.dev/flutter/rendering/RenderLimitedBox-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! Flutter stores `maxWidth` / `maxHeight` as `double` with `double.infinity`
//! as the "no limit" sentinel. The Rust port models them as
//! `Option<Pixels>` — `None` means "do not impose a cap" — and the typed
//! `Pixels` boundary prevents the rest of the codebase from accidentally
//! treating an infinite cap as a meaningful upper bound.

use flui_tree::Single;
use flui_types::{Offset, Pixels, Point, Rect, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// A render object that imposes maximum dimensions on its child *only* when
/// the corresponding incoming constraint is unbounded.
///
/// When the parent already constrains a dimension (e.g. inside a fixed
/// `Column` cross-axis), the cap is ignored; the child sees the unmodified
/// incoming constraint. When the parent is unbounded (e.g. a horizontal
/// `Scrollable`), the cap kicks in so the child has a finite extent to lay
/// itself out against.
///
/// # Common use cases
///
/// * Giving a `Text` widget a finite max width inside a horizontal scroll
///   view so it can wrap rather than running off to infinity.
/// * Wrapping a `Container` in a `LimitedBox` to provide a sensible default
///   size when placed in a `ListView`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::RenderLimitedBox;
/// use flui_types::geometry::px;
///
/// // Cap width at 240, leave height alone.
/// let _node = RenderLimitedBox::new(Some(px(240.0)), None);
/// ```
#[derive(Debug, Clone)]
pub struct RenderLimitedBox {
    /// Max width to impose when the parent constraint is unbounded.
    max_width: Option<Pixels>,
    /// Max height to impose when the parent constraint is unbounded.
    max_height: Option<Pixels>,
    /// Final size after layout.
    size: Size,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl RenderLimitedBox {
    /// Default maximum width (matches Flutter's `double.infinity`).
    pub const DEFAULT_MAX_WIDTH: Option<Pixels> = None;
    /// Default maximum height (matches Flutter's `double.infinity`).
    pub const DEFAULT_MAX_HEIGHT: Option<Pixels> = None;

    /// Creates a limited box with optional caps for each dimension.
    ///
    /// Passing `None` for a dimension means "no cap" — the incoming
    /// constraint is used as-is for that axis. Passing `Some(px)` only takes
    /// effect when the incoming constraint is unbounded for that axis.
    pub const fn new(max_width: Option<Pixels>, max_height: Option<Pixels>) -> Self {
        Self {
            max_width,
            max_height,
            size: Size::ZERO,
            has_child: false,
        }
    }

    /// Creates a limited box that caps width only.
    pub const fn width(max_width: Pixels) -> Self {
        Self::new(Some(max_width), None)
    }

    /// Creates a limited box that caps height only.
    pub const fn height(max_height: Pixels) -> Self {
        Self::new(None, Some(max_height))
    }

    /// Creates a limited box that caps both dimensions.
    pub const fn both(max_width: Pixels, max_height: Pixels) -> Self {
        Self::new(Some(max_width), Some(max_height))
    }

    /// Returns the configured maximum width.
    #[inline]
    pub fn max_width(&self) -> Option<Pixels> {
        self.max_width
    }

    /// Returns the configured maximum height.
    #[inline]
    pub fn max_height(&self) -> Option<Pixels> {
        self.max_height
    }

    /// Sets the maximum width; returns true if the value changed.
    pub fn set_max_width(&mut self, max_width: Option<Pixels>) -> bool {
        if self.max_width == max_width {
            return false;
        }
        self.max_width = max_width;
        true
    }

    /// Sets the maximum height; returns true if the value changed.
    pub fn set_max_height(&mut self, max_height: Option<Pixels>) -> bool {
        if self.max_height == max_height {
            return false;
        }
        self.max_height = max_height;
        true
    }

    /// Returns the constraints the child will see after limiting is applied.
    ///
    /// This is the heart of `RenderLimitedBox`: each axis is independently
    /// checked and only patched when the incoming constraint is unbounded
    /// *and* a cap was supplied.
    fn limit_constraints(&self, incoming: BoxConstraints) -> BoxConstraints {
        let max_w = if incoming.has_bounded_width() {
            incoming.max_width
        } else {
            self.max_width.unwrap_or(Pixels::INFINITY)
        };
        let max_h = if incoming.has_bounded_height() {
            incoming.max_height
        } else {
            self.max_height.unwrap_or(Pixels::INFINITY)
        };
        BoxConstraints::new(
            incoming.min_width,
            max_w.max(incoming.min_width),
            incoming.min_height,
            max_h.max(incoming.min_height),
        )
    }
}

impl Default for RenderLimitedBox {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_WIDTH, Self::DEFAULT_MAX_HEIGHT)
    }
}

impl flui_foundation::Diagnosticable for RenderLimitedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "max_width",
            self.max_width
                .map(|v| format!("{}", v.get()))
                .unwrap_or_else(|| "unset".to_string()),
        );
        builder.add(
            "max_height",
            self.max_height
                .map(|v| format!("{}", v.get()))
                .unwrap_or_else(|| "unset".to_string()),
        );
    }
}

impl RenderBox for RenderLimitedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let incoming = *ctx.constraints();
        let limited = self.limit_constraints(incoming);

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, limited);
            ctx.position_child(0, Offset::ZERO);
            self.size = incoming.constrain(child_size);
        } else {
            self.has_child = false;
            // Match Flutter: if no child, take the minimum of (incoming.min,
            // limited.max) for each axis — i.e. become as small as possible
            // without violating the parent's lower bound.
            self.size = incoming.constrain(Size::new(limited.min_width, limited.min_height));
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    crate::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Flutter parity: proxy_box.dart `RenderLimitedBox._computeSize`
        // — with a child, its dry size under the limited constraints,
        // re-constrained by the incoming set; without one, the smallest
        // size satisfying the limited constraints.
        let limited = self.limit_constraints(constraints);
        if ctx.child_count() > 0 {
            constraints.constrain(ctx.child_dry_layout(0, limited))
        } else {
            constraints.constrain(Size::new(limited.min_width, limited.min_height))
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: crate::traits::TextBaseline,
        ctx: &mut crate::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        crate::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderLimitedBox {}
impl SemanticsCapability for RenderLimitedBox {}
impl HotReloadCapability for RenderLimitedBox {}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    // ---------- API surface -----------------------------------------------

    #[test]
    fn defaults_are_unset() {
        let node = RenderLimitedBox::default();
        assert!(node.max_width().is_none());
        assert!(node.max_height().is_none());
    }

    #[test]
    fn const_constructors() {
        let w = RenderLimitedBox::width(px(120.0));
        assert_eq!(w.max_width(), Some(px(120.0)));
        assert_eq!(w.max_height(), None);

        let h = RenderLimitedBox::height(px(80.0));
        assert_eq!(h.max_width(), None);
        assert_eq!(h.max_height(), Some(px(80.0)));

        let b = RenderLimitedBox::both(px(120.0), px(80.0));
        assert_eq!(b.max_width(), Some(px(120.0)));
        assert_eq!(b.max_height(), Some(px(80.0)));
    }

    #[test]
    fn setters_return_change_flag() {
        let mut node = RenderLimitedBox::default();
        assert!(node.set_max_width(Some(px(100.0))));
        assert!(!node.set_max_width(Some(px(100.0)))); // no-op
        assert!(node.set_max_width(None));
        assert!(node.set_max_height(Some(px(50.0))));
    }

    // ---------- limit_constraints semantics -------------------------------

    #[test]
    fn unbounded_width_gets_capped() {
        let node = RenderLimitedBox::width(px(200.0));
        let incoming = bc(0.0, f32::INFINITY, 0.0, 100.0);
        let limited = node.limit_constraints(incoming);
        assert_eq!(limited.max_width, px(200.0));
        assert_eq!(limited.max_height, px(100.0));
    }

    #[test]
    fn bounded_width_is_untouched() {
        let node = RenderLimitedBox::width(px(200.0));
        let incoming = bc(0.0, 80.0, 0.0, f32::INFINITY);
        let limited = node.limit_constraints(incoming);
        // Width is already bounded — cap is ignored.
        assert_eq!(limited.max_width, px(80.0));
    }

    #[test]
    fn unbounded_with_no_cap_stays_infinite() {
        let node = RenderLimitedBox::default();
        let incoming = bc(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        let limited = node.limit_constraints(incoming);
        assert!(limited.max_width.get().is_infinite());
        assert!(limited.max_height.get().is_infinite());
    }

    #[test]
    fn cap_below_min_is_clamped_up_to_min() {
        // Cap of 10 with min of 50 → effective max becomes 50.
        let node = RenderLimitedBox::width(px(10.0));
        let incoming = bc(50.0, f32::INFINITY, 0.0, 100.0);
        let limited = node.limit_constraints(incoming);
        assert_eq!(limited.max_width, px(50.0));
        assert_eq!(limited.min_width, px(50.0));
    }

    // ---------- dry layout ------------------------------------------------

    #[test]
    fn dry_layout_without_child_is_smallest() {
        let node = RenderLimitedBox::both(px(120.0), px(60.0));
        let dry = crate::context::intrinsics_test_support::leaf_dry_layout(|ctx| {
            node.compute_dry_layout(bc(0.0, f32::INFINITY, 0.0, f32::INFINITY), ctx)
        });
        assert_eq!(dry, Size::ZERO);
    }

    #[test]
    fn dry_layout_honours_min_constraints() {
        let node = RenderLimitedBox::both(px(120.0), px(60.0));
        let dry = crate::context::intrinsics_test_support::leaf_dry_layout(|ctx| {
            node.compute_dry_layout(bc(40.0, f32::INFINITY, 30.0, f32::INFINITY), ctx)
        });
        assert_eq!(dry, Size::new(px(40.0), px(30.0)));
    }
}
