//! RenderConstrainedBox - Applies additional constraints on top of parent constraints
//!
//! Implements Flutter's foundational constraint modification container that enforces
//! additional min/max size constraints by intersecting them with incoming parent
//! constraints. This is the base building block for all constraint modification in
//! Flutter's layout system.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderConstrainedBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `additional_constraints` | `additionalConstraints` property (BoxConstraints) |
//! | `set_additional_constraints()` | `additionalConstraints = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Enforce additional constraints**
//!    - Intersect parent constraints with `additional_constraints`
//!    - Uses `BoxConstraints::enforce()` for proper constraint composition
//!    - Result: constraints that satisfy both parent AND additional rules
//!
//! 2. **Layout child (if present)**
//!    - Child laid out with enforced (intersected) constraints
//!    - Child size automatically satisfies both constraint sets
//!
//! 3. **No child fallback**
//!    - Return minimum size from enforced constraints
//!    - Reserves minimum required space even without child
//!
//! 4. **Return size**
//!    - Child size (when child present)
//!    - Minimum enforced size (when childless)
//!
//! # Paint Protocol
//!
//! 1. **Paint child if present**
//!    - Child painted at parent offset
//!    - No transformation or clipping applied
//!
//! 2. **No child case**
//!    - Nothing painted (space reserved by layout)
//!
//! # Performance
//!
//! - **Layout**: O(1) - constraint intersection + single child layout
//! - **Paint**: O(1) - direct child paint at offset (no transformation)
//! - **Memory**: 16 bytes (BoxConstraints = 4 × f32)
//!
//! # Use Cases
//!
//! - **Minimum size enforcement**: Ensure child is at least certain dimensions
//! - **Maximum size capping**: Prevent child from exceeding size limits
//! - **Exact size forcing**: Use tight constraints for fixed dimensions
//! - **Space reservation**: Reserve minimum space even without child
//! - **Constraint tightening**: Make loose constraints more restrictive
//! - **Foundation for SizedBox**: SizedBox uses ConstrainedBox internally
//!
//! # Constraint Intersection
//!
//! ```text
//! Parent constraints: 0-400 × 0-600
//! Additional constraints: 100-200 × 50-150
//! Enforced (intersected): 100-200 × 50-150 (tightest of both)
//!
//! Parent constraints: 50-300 × 50-300
//! Additional constraints: 0-400 × 0-200
//! Enforced (intersected): 50-300 × 50-200 (respects parent min, additional max)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderSizedBox**: SizedBox is convenience wrapper, ConstrainedBox is foundation
//! - **vs RenderOverflowBox**: OverflowBox loosens constraints, ConstrainedBox enforces them
//! - **vs RenderConstrainedOverflowBox**: OverflowBox allows overflow, ConstrainedBox respects parent
//! - **vs RenderLimitedBox**: LimitedBox only applies when infinite, ConstrainedBox always applies
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderConstrainedBox;
//! use flui_types::constraints::BoxConstraints;
//!
//! // Enforce minimum size (child can be larger)
//! let min_size = BoxConstraints::new(100.0, f32::INFINITY, 50.0, f32::INFINITY);
//! let min_constrained = RenderConstrainedBox::new(min_size);
//!
//! // Enforce maximum size (child can be smaller)
//! let max_size = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
//! let max_constrained = RenderConstrainedBox::new(max_size);
//!
//! // Enforce exact size (tight constraints)
//! let exact = BoxConstraints::tight(Size::new(100.0, 100.0));
//! let fixed = RenderConstrainedBox::new(exact);
//!
//! // Enforce range (min and max)
//! let range = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);
//! let ranged = RenderConstrainedBox::new(range);
//! ```

use crate::{RenderObject, RenderResult};

use crate::core::{BoxLayoutCtx, BoxPaintCtx};
use crate::core::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that applies additional constraints on top of parent constraints.
///
/// Enforces additional min/max size constraints by intersecting them with
/// incoming parent constraints. This is the foundational constraint modifier
/// in Flutter's layout system - all other constraint widgets build on this.
///
/// # Arity
///
/// `Optional` - Can have 0 or 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Constraint Modifier** - Enforces additional constraints by intersection,
/// always respects parent constraints.
///
/// # Use Cases
///
/// - **Minimum size enforcement**: Ensure child is at least certain dimensions
/// - **Maximum size capping**: Prevent child from exceeding size limits
/// - **Exact size forcing**: Use tight constraints for fixed dimensions
/// - **Space reservation**: Reserve minimum space even without child
/// - **Constraint tightening**: Make loose constraints more restrictive
/// - **Foundation for other widgets**: Base class for SizedBox, etc.
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderConstrainedBox behavior:
/// - Enforces constraints using `BoxConstraints.enforce()` (proper intersection)
/// - Always respects incoming parent constraints (never violates parent rules)
/// - Returns minimum enforced size when childless
/// - Extends RenderProxyBox base class
///
/// # Constraint Enforcement
///
/// The `enforce()` method computes the intersection of parent and additional
/// constraints, ensuring the result satisfies both:
/// - min = max(parent.min, additional.min)
/// - max = min(parent.max, additional.max)
///
/// This guarantees parent constraints are never violated.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderConstrainedBox;
/// use flui_types::constraints::BoxConstraints;
///
/// // Minimum size (child can grow larger)
/// let min = BoxConstraints::new(100.0, f32::INFINITY, 50.0, f32::INFINITY);
/// let min_box = RenderConstrainedBox::new(min);
///
/// // Maximum size (child can shrink smaller)
/// let max = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
/// let max_box = RenderConstrainedBox::new(max);
///
/// // Exact size (tight constraints)
/// let exact = BoxConstraints::tight(Size::new(100.0, 100.0));
/// let fixed_box = RenderConstrainedBox::new(exact);
/// ```
#[derive(Debug)]
pub struct RenderConstrainedBox {
    /// Additional constraints to apply
    pub additional_constraints: BoxConstraints,
}

impl RenderConstrainedBox {
    /// Create new RenderConstrainedBox with additional constraints
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            additional_constraints,
        }
    }

    /// Set new additional constraints
    pub fn set_additional_constraints(&mut self, constraints: BoxConstraints) {
        self.additional_constraints = constraints;
    }
}

impl Default for RenderConstrainedBox {
    fn default() -> Self {
        Self::new(BoxConstraints::UNCONSTRAINED)
    }
}

impl RenderObject for RenderConstrainedBox {}

impl RenderBox<Optional> for RenderConstrainedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Enforce additional constraints by intersecting with incoming constraints
        let child_constraints = constraints.enforce(self.additional_constraints);

        tracing::trace!(
            incoming = ?constraints,
            additional = ?self.additional_constraints,
            child_constraints = ?child_constraints,
            "RenderConstrainedBox::layout"
        );

        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        let size = if let Some(&child_id) = ctx.children.get() {
            // Layout child with enforced (intersected) constraints
            ctx.layout_child(child_id, child_constraints)?
        } else {
            // No child - return minimum size from enforced constraints
            // This reserves the minimum required space
            Size::new(child_constraints.min_width, child_constraints.min_height)
        };

        tracing::trace!(size = ?size, "RenderConstrainedBox::layout complete");

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Paint child at parent offset (no transformation)
            ctx.paint_child(child_id, ctx.offset);
        }
        // If no child, nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_constrained_box_new() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let constrained = RenderConstrainedBox::new(constraints);
        assert_eq!(constrained.additional_constraints, constraints);
    }

    #[test]
    fn test_render_constrained_box_default() {
        let constrained = RenderConstrainedBox::default();
        assert_eq!(
            constrained.additional_constraints,
            BoxConstraints::UNCONSTRAINED
        );
    }

    #[test]
    fn test_render_constrained_box_set_constraints() {
        let constraints1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut constrained = RenderConstrainedBox::new(constraints1);

        let constraints2 = BoxConstraints::tight(Size::new(200.0, 200.0));
        constrained.set_additional_constraints(constraints2);
        assert_eq!(constrained.additional_constraints, constraints2);
    }
}
