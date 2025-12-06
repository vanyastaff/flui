//! RenderConstrainedBox - applies additional constraints to a child
//!
//! Implements Flutter's constraint modification container that enforces
//! additional min/max size constraints on top of parent constraints.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderConstrainedBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `additional_constraints` | `additionalConstraints` property |
//!
//! # Layout Protocol
//!
//! 1. **Enforce constraints**
//!    - Intersect incoming constraints with additional_constraints
//!    - Uses `BoxConstraints::enforce()` for proper constraint composition
//!
//! 2. **Layout child**
//!    - If child exists: layout with enforced constraints
//!    - If no child: return min size from enforced constraints
//!
//! 3. **Return size**
//!    - Child size (already satisfies enforced constraints)
//!    - Or minimum size when childless
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constant-time constraint enforcement
//! - **Paint**: O(1) - direct child paint at offset (no transformation)
//! - **Memory**: 16 bytes (BoxConstraints = 4 × f32)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderConstrainedBox;
//! use flui_types::constraints::BoxConstraints;
//!
//! // Enforce minimum size
//! let min_size = BoxConstraints::new(100.0, f32::INFINITY, 50.0, f32::INFINITY);
//! let constrained = RenderConstrainedBox::new(min_size);
//!
//! // Enforce exact size (tight constraints)
//! let exact = BoxConstraints::tight_for(100.0, 100.0);
//! let fixed = RenderConstrainedBox::new(exact);
//! ```

use crate::{RenderObject, RenderResult};

use crate::core::{BoxLayoutCtx, BoxPaintCtx};
use crate::core::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that applies additional constraints to its child.
///
/// Enforces minimum or maximum sizes by intersecting incoming constraints
/// with additional constraints.
///
/// # Arity
///
/// `Optional` - Can have 0 or 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Minimum size**: Ensure child is at least a certain size
/// - **Maximum size**: Cap child's size
/// - **Exact size**: Force child to specific dimensions (tight constraints)
/// - **Space reservation**: Reserve minimum space even without child
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderConstrainedBox behavior:
/// - Enforces constraints using proper intersection
/// - Respects incoming constraints (doesn't violate parent rules)
/// - Returns minimum size when childless
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

        let size = if let Some(child_id) = ctx.children.get() {
            // Layout child with combined constraints
            ctx.layout_child(*child_id, child_constraints)?
        } else {
            // No child - return minimum size from additional constraints
            Size::new(child_constraints.min_width, child_constraints.min_height)
        };

        tracing::trace!(size = ?size, "RenderConstrainedBox::layout complete");

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // If we have a child, paint it at our offset
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(*child_id, ctx.offset);
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
