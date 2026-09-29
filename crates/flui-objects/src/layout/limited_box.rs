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
//! `Option<f64>` — `None` means "do not impose a cap" — so no caller can
//! mistake an infinite cap for a meaningful upper bound.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    traits::RenderBox,
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
/// use flui_objects::RenderLimitedBox;
///
/// // Cap width at 240, leave height alone.
/// let _node = RenderLimitedBox::new(Some(240.0), None);
/// ```
#[derive(Debug, Clone)]
pub struct RenderLimitedBox {
    /// Max width to impose when the parent constraint is unbounded.
    max_width: Option<f64>,
    /// Max height to impose when the parent constraint is unbounded.
    max_height: Option<f64>,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl RenderLimitedBox {
    /// Default maximum width (matches Flutter's `double.infinity`).
    pub const DEFAULT_MAX_WIDTH: Option<f64> = None;
    /// Default maximum height (matches Flutter's `double.infinity`).
    pub const DEFAULT_MAX_HEIGHT: Option<f64> = None;

    /// Creates a limited box with optional caps for each dimension.
    ///
    /// Passing `None` for a dimension means "no cap" — the incoming
    /// constraint is used as-is for that axis. Passing `Some(px)` only takes
    /// effect when the incoming constraint is unbounded for that axis.
    pub const fn new(max_width: Option<f64>, max_height: Option<f64>) -> Self {
        Self {
            max_width,
            max_height,
            has_child: false,
        }
    }

    /// Creates a limited box that caps width only.
    pub const fn width(max_width: f64) -> Self {
        Self::new(Some(max_width), None)
    }

    /// Creates a limited box that caps height only.
    pub const fn height(max_height: f64) -> Self {
        Self::new(None, Some(max_height))
    }

    /// Creates a limited box that caps both dimensions.
    pub const fn both(max_width: f64, max_height: f64) -> Self {
        Self::new(Some(max_width), Some(max_height))
    }

    /// Returns the configured maximum width.
    #[inline]
    pub fn max_width(&self) -> Option<f64> {
        self.max_width
    }

    /// Returns the configured maximum height.
    #[inline]
    pub fn max_height(&self) -> Option<f64> {
        self.max_height
    }

    /// Sets the maximum width and reports layout when changed.
    pub fn set_max_width(&mut self, max_width: Option<f64>) -> flui_rendering::RenderUpdateImpact {
        if self.max_width == max_width {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.max_width = max_width;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the maximum height and reports layout when changed.
    pub fn set_max_height(
        &mut self,
        max_height: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.max_height == max_height {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.max_height = max_height;
        flui_rendering::RenderUpdateImpact::LAYOUT
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
            self.max_width.unwrap_or(f64::INFINITY)
        };
        let max_h = if incoming.has_bounded_height() {
            incoming.max_height
        } else {
            self.max_height.unwrap_or(f64::INFINITY)
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
                .map_or_else(|| "unset".to_string(), |v| format!("{v}")),
        );
        builder.add(
            "max_height",
            self.max_height
                .map_or_else(|| "unset".to_string(), |v| format!("{v}")),
        );
    }
}

impl RenderBox for RenderLimitedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let incoming = *ctx.constraints();
        let limited = self.limit_constraints(incoming);

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, limited);
            ctx.position_child(0, Offset::ZERO);
            incoming.constrain(child_size)
        } else {
            self.has_child = false;
            // Match Flutter: if no child, take the minimum of (incoming.min,
            // limited.max) for each axis — i.e. become as small as possible
            // without violating the parent's lower bound.
            incoming.constrain(Size::new(limited.min_width, limited.min_height))
        }
    }

    flui_rendering::forward_single_child_box_hit_test!();

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
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
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        flui_rendering::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {

    use super::*;

    fn bc(min_w: f64, max_w: f64, min_h: f64, max_h: f64) -> BoxConstraints {
        BoxConstraints::new(min_w, max_w, min_h, max_h)
    }

    // ---------- API surface -----------------------------------------------

    // ---------- limit_constraints semantics -------------------------------

    #[test]
    fn cap_below_min_is_clamped_up_to_min() {
        // Cap of 10 with min of 50 → effective max becomes 50.
        let node = RenderLimitedBox::width(10.0);
        let incoming = bc(50.0, f64::INFINITY, 0.0, 100.0);
        let limited = node.limit_constraints(incoming);
        assert_eq!(limited.max_width, 50.0);
        assert_eq!(limited.min_width, 50.0);
    }

    // ---------- dry layout ------------------------------------------------
}
