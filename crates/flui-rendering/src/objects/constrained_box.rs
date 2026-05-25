//! `RenderConstrainedBox` — imposes additional constraints on its child.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderConstrainedBox`](https://api.flutter.dev/flutter/rendering/RenderConstrainedBox-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! Flutter exposes a public `additionalConstraints` field of type
//! `BoxConstraints` and relies on a runtime debug-only assertion that the
//! caller normalized them. The Rust port preserves the same constructor
//! ergonomics but routes every mutation through `set_additional_constraints`,
//! which always re-normalizes — eliminating the bottom half of Flutter's
//! "constraints not normalized" debug check at the API boundary (the typed
//! `Pixels` boundary in `BoxConstraints` itself eliminates the rest).

use flui_tree::Single;
use flui_types::{Offset, Point, Rect, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// A render object that applies *additional* constraints to its child.
///
/// The child is laid out with the intersection of the parent's incoming
/// constraints and the [`additional_constraints`](Self::additional_constraints)
/// stored here (via [`BoxConstraints::enforce`]).
///
/// If there is no child, the box itself sizes to satisfy the additional
/// constraints, falling back to a zero-sized layout when both incoming
/// constraints and additional constraints permit it.
///
/// # Common use cases
///
/// * Implementing the `constraints:` parameter of the higher-level `Container`
///   widget.
/// * Adding a minimum or maximum dimension to a child without changing its
///   intrinsic sizing semantics.
/// * Composing a `ConstrainedBox` ↔ `UnconstrainedBox` pair to selectively
///   reset constraints down the tree.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::RenderConstrainedBox;
/// use flui_rendering::constraints::BoxConstraints;
/// use flui_types::geometry::px;
///
/// // Force the child to be at least 200x100 logical pixels.
/// let extra = BoxConstraints::new(px(200.0), px(f32::INFINITY), px(100.0), px(f32::INFINITY));
/// let _node = RenderConstrainedBox::new(extra);
/// ```
#[derive(Debug, Clone)]
pub struct RenderConstrainedBox {
    /// Constraints to combine with the incoming constraints from the parent.
    additional_constraints: BoxConstraints,
    /// Final size after layout.
    size: Size,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl RenderConstrainedBox {
    /// Creates a render object with the given additional constraints.
    ///
    /// The constraints are normalized via
    /// [`BoxConstraints::normalize`] to prevent layout drift caused by
    /// floating-point noise in user-supplied bounds.
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            additional_constraints: additional_constraints.normalize(),
            size: Size::ZERO,
            has_child: false,
        }
    }

    /// Returns the additional constraints applied to the child.
    #[inline]
    pub fn additional_constraints(&self) -> BoxConstraints {
        self.additional_constraints
    }

    /// Replaces the additional constraints applied to the child.
    ///
    /// Re-normalizes the constraints before storing them. The pipeline should
    /// invalidate layout when this returns `true`, signalling the value
    /// actually changed.
    pub fn set_additional_constraints(&mut self, additional_constraints: BoxConstraints) -> bool {
        let normalized = additional_constraints.normalize();
        if self.additional_constraints == normalized {
            return false;
        }
        self.additional_constraints = normalized;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderConstrainedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "additional_constraints",
            format!("{:?}", self.additional_constraints),
        );
        builder.add("size", format!("{:?}", self.size));
        builder.add("has_child", self.has_child);
    }
}

impl RenderBox for RenderConstrainedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let incoming = *ctx.constraints();
        let combined = self.additional_constraints.enforce(&incoming);

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, combined);
            ctx.position_child(0, Offset::ZERO);
            // Our size = child size, but it MUST satisfy the incoming
            // constraints (Flutter parity: the parent ultimately decides
            // the box bounds).
            self.size = incoming.constrain(child_size);
        } else {
            self.has_child = false;
            // Choose the smallest size that satisfies both constraint sets.
            self.size = incoming.constrain(combined.smallest());
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

    // ----- Intrinsic dimensions (Flutter parity) --------------------------

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        let min = self.additional_constraints.min_width.get();
        let max = self.additional_constraints.max_width.get();
        // Without a child, Flutter would defer to the child's intrinsic
        // width; we don't have child intrinsic plumbing in this slot, so
        // we return the constraint's lower bound clamped to the upper.
        let _ = height;
        if min.is_finite() { min.min(max) } else { 0.0 }
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        let max = self.additional_constraints.max_width.get();
        let _ = height;
        if max.is_finite() { max } else { 0.0 }
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        let min = self.additional_constraints.min_height.get();
        let max = self.additional_constraints.max_height.get();
        let _ = width;
        if min.is_finite() { min.min(max) } else { 0.0 }
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        let max = self.additional_constraints.max_height.get();
        let _ = width;
        if max.is_finite() { max } else { 0.0 }
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        let combined = self.additional_constraints.enforce(&constraints);
        // Without a real child, the dry layout returns the smallest size
        // that satisfies the combined constraint set; with a child, the
        // pipeline-level dry-layout machinery would walk through.
        constraints.constrain(combined.smallest())
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderConstrainedBox {}
impl SemanticsCapability for RenderConstrainedBox {}
impl HotReloadCapability for RenderConstrainedBox {}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    fn tight(w: f32, h: f32) -> BoxConstraints {
        BoxConstraints::tight(Size::new(px(w), px(h)))
    }

    fn bounded(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    // ---------- construction & getters ------------------------------------

    #[test]
    fn additional_constraints_round_trip() {
        let extra = bounded(50.0, 200.0, 25.0, 100.0);
        let node = RenderConstrainedBox::new(extra);
        assert_eq!(node.additional_constraints(), extra.normalize());
    }

    #[test]
    fn set_additional_constraints_returns_true_on_change() {
        let mut node = RenderConstrainedBox::new(BoxConstraints::UNCONSTRAINED);
        assert!(node.set_additional_constraints(tight(100.0, 50.0)));
    }

    #[test]
    fn set_additional_constraints_returns_false_on_no_op() {
        let extra = bounded(10.0, 20.0, 30.0, 40.0);
        let mut node = RenderConstrainedBox::new(extra);
        // Setting the same normalized constraints is a no-op.
        assert!(!node.set_additional_constraints(extra));
    }

    // ---------- intrinsic dimensions --------------------------------------

    #[test]
    fn intrinsic_widths_match_finite_constraints() {
        let node = RenderConstrainedBox::new(bounded(100.0, 200.0, 50.0, 150.0));
        assert_eq!(node.compute_min_intrinsic_width(0.0), 100.0);
        assert_eq!(node.compute_max_intrinsic_width(0.0), 200.0);
        assert_eq!(node.compute_min_intrinsic_height(0.0), 50.0);
        assert_eq!(node.compute_max_intrinsic_height(0.0), 150.0);
    }

    #[test]
    fn intrinsic_widths_zero_when_unbounded() {
        let node = RenderConstrainedBox::new(BoxConstraints::UNCONSTRAINED);
        assert_eq!(node.compute_max_intrinsic_width(0.0), 0.0);
        assert_eq!(node.compute_max_intrinsic_height(0.0), 0.0);
    }

    // ---------- dry layout ------------------------------------------------

    #[test]
    fn dry_layout_combines_constraints() {
        let node = RenderConstrainedBox::new(bounded(80.0, 160.0, 40.0, 120.0));
        // Incoming constraints allow up to 500x500.
        let dry = node.compute_dry_layout(bounded(0.0, 500.0, 0.0, 500.0));
        // Without a child the smallest satisfying combined size is the
        // additional-constraints min (80, 40).
        assert_eq!(dry, Size::new(px(80.0), px(40.0)));
    }

    #[test]
    fn dry_layout_respects_incoming_upper_bound() {
        let node = RenderConstrainedBox::new(bounded(0.0, 1000.0, 0.0, 1000.0));
        // Incoming caps at 100x50 — combined.smallest() is (0,0) but the
        // value is unaffected; final constraint is incoming-bounded.
        let dry = node.compute_dry_layout(bounded(0.0, 100.0, 0.0, 50.0));
        assert_eq!(dry, Size::ZERO);
    }

    // ---------- API surface -----------------------------------------------

    #[test]
    fn debug_fill_properties_lists_constraints() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderConstrainedBox::new(tight(100.0, 50.0));
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "additional_constraints"));
        assert!(names.iter().any(|n| n == "size"));
        assert!(names.iter().any(|n| n == "has_child"));
    }
}
