//! `RenderConstrainedBox` — imposes additional constraints on its child.
//!
//! # Design
//!
//! Every mutation of the additional constraints goes through
//! `set_additional_constraints`, which always re-normalizes them, so a
//! "constraints not normalized" state is unrepresentable at the API boundary
//! rather than caught by a debug-only assertion.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    traits::RenderBox,
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
/// use flui_objects::RenderConstrainedBox;
/// use flui_rendering::constraints::BoxConstraints;
///
/// // Force the child to be at least 200x100 logical pixels.
/// let extra = BoxConstraints::new(200.0, (f64::INFINITY), 100.0, (f64::INFINITY));
/// let _node = RenderConstrainedBox::new(extra);
/// ```
#[derive(Debug, Clone)]
pub struct RenderConstrainedBox {
    /// Constraints to combine with the incoming constraints from the parent.
    additional_constraints: BoxConstraints,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl RenderConstrainedBox {
    /// Creates a render object with the given additional constraints.
    ///
    /// The constraints are rounded for caching via
    /// [`BoxConstraints::round_for_cache`] to prevent layout drift caused by
    /// floating-point noise in user-supplied bounds.
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            additional_constraints: additional_constraints.round_for_cache(),
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
    /// Re-rounds the constraints before storing them. The pipeline should
    /// Returns layout impact when the rounded value actually changes.
    pub fn set_additional_constraints(
        &mut self,
        additional_constraints: BoxConstraints,
    ) -> flui_rendering::RenderUpdateImpact {
        let rounded = additional_constraints.round_for_cache();
        if self.additional_constraints == rounded {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.additional_constraints = rounded;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl flui_foundation::Diagnosticable for RenderConstrainedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("additional_constraints", self.additional_constraints);
    }
}

impl RenderBox for RenderConstrainedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let incoming = *ctx.constraints();
        let combined = self.additional_constraints.enforce(&incoming);

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, combined);
            ctx.position_child(0, Offset::ZERO);
            // Our size = child size, but it MUST satisfy the incoming
            // constraints (the parent ultimately decides the box bounds).
            incoming.constrain(child_size)
        } else {
            self.has_child = false;
            // Choose the smallest size that satisfies both constraint sets.
            incoming.constrain(combined.smallest())
        }
    }

    flui_rendering::forward_single_child_box_hit_test!();

    // ----- Intrinsic dimensions -------------------------------------------

    // A tight additional constraint answers directly; otherwise the child's
    // intrinsic is constrained by the additional bounds (unless those
    // bounds are infinite, which would poison the fold).

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        let ac = &self.additional_constraints;
        if ac.has_bounded_width() && ac.has_tight_width() {
            return ac.min_width;
        }
        let width = if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height)
        } else {
            0.0
        };
        debug_assert!(
            width.is_finite(),
            "child min intrinsic width must be finite"
        );
        if ac.has_infinite_width() {
            width
        } else {
            ac.constrain_width(width)
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        let ac = &self.additional_constraints;
        if ac.has_bounded_width() && ac.has_tight_width() {
            return ac.min_width;
        }
        let width = if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height)
        } else {
            0.0
        };
        debug_assert!(
            width.is_finite(),
            "child max intrinsic width must be finite"
        );
        if ac.has_infinite_width() {
            width
        } else {
            ac.constrain_width(width)
        }
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        let ac = &self.additional_constraints;
        if ac.has_bounded_height() && ac.has_tight_height() {
            return ac.min_height;
        }
        let height = if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width)
        } else {
            0.0
        };
        debug_assert!(
            height.is_finite(),
            "child min intrinsic height must be finite"
        );
        if ac.has_infinite_height() {
            height
        } else {
            ac.constrain_height(height)
        }
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        let ac = &self.additional_constraints;
        if ac.has_bounded_height() && ac.has_tight_height() {
            return ac.min_height;
        }
        let height = if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_height(0, width)
        } else {
            0.0
        };
        debug_assert!(
            height.is_finite(),
            "child max intrinsic height must be finite"
        );
        if ac.has_infinite_height() {
            height
        } else {
            ac.constrain_height(height)
        }
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        let combined = self.additional_constraints.enforce(&constraints);
        if ctx.child_count() > 0 {
            ctx.child_dry_layout(0, combined)
        } else {
            combined.constrain(Size::ZERO)
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        let combined = self.additional_constraints.enforce(&constraints);
        if ctx.child_count() > 0 {
            ctx.child_dry_baseline(0, combined, baseline)
        } else {
            None
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================
