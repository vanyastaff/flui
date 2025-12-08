//! RenderSizedOverflowBox - Fixed container size with custom child constraints
//!
//! Implements Flutter's SizedOverflowBox that combines fixed container sizing
//! with custom child constraints. Container has explicit size while child receives
//! potentially different constraints, allowing overflow scenarios.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSizedOverflowBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` (with fixed size + overflow) |
//! | `width` | `width` property (container's explicit width) |
//! | `height` | `height` property (container's explicit height) |
//! | `child_min_width` | `minWidth` property (child constraint override) |
//! | `child_max_width` | `maxWidth` property (child constraint override) |
//! | `child_min_height` | `minHeight` property (child constraint override) |
//! | `child_max_height` | `maxHeight` property (child constraint override) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `set_width()` | `width = value` setter |
//! | `set_height()` | `height = value` setter |
//! | `set_alignment()` | `alignment = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Calculate child constraints**
//!    - Override parent constraints with custom min/max values
//!    - If child override None: use parent constraint value
//!    - If child override Some(v): use v instead
//!
//! 2. **Layout child**
//!    - Child laid out with custom constraints
//!    - Child may be larger or smaller than container
//!    - Child size cached for alignment
//!
//! 3. **Calculate container size**
//!    - Width = specified width OR parent constraints.max_width
//!    - Height = specified height OR parent constraints.max_height
//!    - Size constrained to parent bounds
//!
//! 4. **Return container size**
//!    - Size based on explicit dimensions (child size ignored)
//!    - Allows child to overflow container bounds
//!
//! # Paint Protocol
//!
//! 1. **Calculate alignment offset**
//!    - Offset = alignment.calculate_offset(child_size, container_size)
//!    - Centers or positions child within container
//!
//! 2. **Paint child at aligned offset**
//!    - Child painted at parent offset + alignment offset
//!    - May overflow container bounds
//!    - No clipping applied
//!
//! # Performance
//!
//! - **Layout**: O(1) - constraint override + single child layout
//! - **Paint**: O(1) - alignment calculation + child paint
//! - **Memory**: 56 bytes (2 × Option<f32> size + 4 × Option<f32> child + Alignment + 2 × Size cache)
//!
//! # Use Cases
//!
//! - **Fixed-size overflow**: Container with explicit size, child can overflow
//! - **Modal overlays**: Fixed-size container with larger content
//! - **Constraint override with sizing**: Both container sizing AND child constraint control
//! - **Debug layouts**: Force specific container and child sizes
//! - **Aspect ratio with overflow**: Fixed aspect ratio container, flexible child
//! - **Image overlays**: Fixed-size frame with potentially larger image
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderSizedBox**: SizedBox forces child size, SizedOverflowBox allows different child constraints
//! - **vs RenderOverflowBox**: OverflowBox sizes to parent, SizedOverflowBox has explicit size
//! - **vs RenderConstrainedOverflowBox**: Both allow overflow, SizedOverflowBox has explicit dimensions
//! - **vs RenderAlign**: Align only positions, SizedOverflowBox controls both sizing and constraints
//!
//! # Important Note
//!
//! This is essentially SizedBox + OverflowBox combined:
//! - SizedBox behavior for container dimensions
//! - OverflowBox behavior for child constraints
//!
//! Consider wrapping in RenderClipRect if clipping is needed.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSizedOverflowBox;
//! use flui_types::Alignment;
//!
//! // 100×100 container, child can be up to 200×200
//! let sized = RenderSizedOverflowBox::with_child_constraints(
//!     Some(100.0), Some(100.0),
//!     None, Some(200.0),
//!     None, Some(200.0),
//! );
//!
//! // Fixed size with centered child
//! let centered = RenderSizedOverflowBox::with_alignment(
//!     Some(150.0), Some(150.0),
//!     Alignment::CENTER,
//! );
//!
//! // Width fixed, height flexible, child can overflow
//! let partial = RenderSizedOverflowBox::new(Some(100.0), None);
//! ```

use flui_rendering::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::{Alignment, Size};

/// RenderObject with fixed container size that allows child to have custom constraints.
///
/// Combines fixed container sizing with custom child constraints, allowing
/// the child to potentially overflow the container. Container has explicit
/// dimensions while child receives potentially different constraints.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Fixed Sizing with Constraint Override** - Container has explicit size,
/// child receives custom constraints allowing overflow.
///
/// # Use Cases
///
/// - **Fixed-size overflow**: Explicit container size, child can overflow
/// - **Modal overlays**: Fixed-size frame with larger content
/// - **Constraint override with sizing**: Control both container AND child
/// - **Debug layouts**: Force specific sizes for testing
/// - **Image overlays**: Fixed frame with potentially larger image
/// - **Aspect ratio with overflow**: Fixed aspect, flexible child
///
/// # Flutter Compliance
///
/// Combines SizedBox and OverflowBox behaviors:
/// - Container sized explicitly (SizedBox pattern)
/// - Child receives custom constraints (OverflowBox pattern)
/// - Child size ignored for container sizing
/// - Alignment applied when sizes differ
///
/// # Overflow Behavior
///
/// Child can overflow container because:
/// 1. Container size determined by explicit width/height
/// 2. Child constraints are custom, independent of container
/// 3. No clipping applied during paint
///
/// Wrap in RenderClipRect if clipping is desired.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedOverflowBox;
/// use flui_types::Alignment;
///
/// // 100×100 container, child up to 200×200
/// let sized = RenderSizedOverflowBox::with_child_constraints(
///     Some(100.0), Some(100.0),
///     None, Some(200.0),
///     None, Some(200.0),
/// );
///
/// // Fixed size with alignment
/// let aligned = RenderSizedOverflowBox::with_alignment(
///     Some(150.0), Some(150.0),
///     Alignment::CENTER,
/// );
/// ```
#[derive(Debug)]
pub struct RenderSizedOverflowBox {
    /// Explicit width for this widget
    pub width: Option<f32>,
    /// Explicit height for this widget
    pub height: Option<f32>,
    /// Minimum width for child_id (overrides parent constraints)
    pub child_min_width: Option<f32>,
    /// Maximum width for child_id (overrides parent constraints)
    pub child_max_width: Option<f32>,
    /// Minimum height for child_id (overrides parent constraints)
    pub child_min_height: Option<f32>,
    /// Maximum height for child_id (overrides parent constraints)
    pub child_max_height: Option<f32>,
    /// How to align the child_id
    pub alignment: Alignment,

    // Cache for paint
    size: Size,
    child_size: Size,
}

// ===== Public API =====

impl RenderSizedOverflowBox {
    /// Create new sized overflow box
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            width,
            height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Create with explicit size and child_id constraints
    pub fn with_child_constraints(
        width: Option<f32>,
        height: Option<f32>,
        child_min_width: Option<f32>,
        child_max_width: Option<f32>,
        child_min_height: Option<f32>,
        child_max_height: Option<f32>,
    ) -> Self {
        Self {
            width,
            height,
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
            alignment: Alignment::CENTER,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(width: Option<f32>, height: Option<f32>, alignment: Alignment) -> Self {
        Self {
            width,
            height,
            alignment,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            size: Size::ZERO,
            child_size: Size::ZERO,
        }
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderSizedOverflowBox {}

impl RenderBox<Single> for RenderSizedOverflowBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Build child constraints from override values
        // If child override is None: use parent's constraint value
        // If child override is Some(v): use v instead
        let child_min_width = self.child_min_width.unwrap_or(ctx.constraints.min_width);
        let child_max_width = self.child_max_width.unwrap_or(ctx.constraints.max_width);
        let child_min_height = self.child_min_height.unwrap_or(ctx.constraints.min_height);
        let child_max_height = self.child_max_height.unwrap_or(ctx.constraints.max_height);

        let child_constraints = BoxConstraints::new(
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
        );

        // Layout child with custom (overridden) constraints
        // Child may be larger or smaller than container
        self.child_size = ctx.layout_child(child_id, child_constraints)?;

        // Container size is explicit (or defaults to parent's max)
        // Child size is IGNORED for container sizing
        let width = self.width.unwrap_or(ctx.constraints.max_width);
        let height = self.height.unwrap_or(ctx.constraints.max_height);

        // Constrain container size to parent bounds
        self.size = ctx.constraints.constrain(Size::new(width, height));
        Ok(self.size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate alignment offset to position child within container
        let align_offset = self.alignment.calculate_offset(self.child_size, self.size);
        let child_offset = ctx.offset + align_offset;

        // Paint child at aligned offset
        // May overflow container bounds (no clipping)
        ctx.paint_child(child_id, child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sized_overflow_box_new() {
        let sized_overflow = RenderSizedOverflowBox::new(Some(100.0), Some(200.0));
        assert_eq!(sized_overflow.width, Some(100.0));
        assert_eq!(sized_overflow.height, Some(200.0));
        assert_eq!(sized_overflow.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_sized_overflow_box_set_width() {
        let mut sized_overflow = RenderSizedOverflowBox::new(None, None);

        sized_overflow.set_width(Some(150.0));
        assert_eq!(sized_overflow.width, Some(150.0));
    }

    #[test]
    fn test_render_sized_overflow_box_set_alignment() {
        let mut sized_overflow = RenderSizedOverflowBox::new(None, None);

        sized_overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(sized_overflow.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_render_sized_overflow_box_with_child_constraints() {
        let sized_overflow = RenderSizedOverflowBox::with_child_constraints(
            Some(100.0),
            Some(100.0),
            None,
            Some(200.0),
            None,
            Some(200.0),
        );
        assert_eq!(sized_overflow.width, Some(100.0));
        assert_eq!(sized_overflow.height, Some(100.0));
        assert_eq!(sized_overflow.child_max_width, Some(200.0));
        assert_eq!(sized_overflow.child_max_height, Some(200.0));
    }

    #[test]
    fn test_render_sized_overflow_box_with_alignment() {
        let sized_overflow =
            RenderSizedOverflowBox::with_alignment(Some(50.0), Some(75.0), Alignment::TOP_LEFT);
        assert_eq!(sized_overflow.alignment, Alignment::TOP_LEFT);
    }
}
