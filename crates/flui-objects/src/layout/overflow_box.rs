//! RenderOverflowBox - Imposes custom constraints allowing child overflow
//!
//! Implements Flutter's OverflowBox that applies different constraints to its child
//! than received from parent, allowing the child to overflow parent boundaries.
//! Parent sizes itself using loosened constraints while child receives custom
//! (potentially tighter or looser) constraints.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderOverflowBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` (with overflow) |
//! | `min_width` | `minWidth` property (overrides parent if set) |
//! | `max_width` | `maxWidth` property (overrides parent if set) |
//! | `min_height` | `minHeight` property (overrides parent if set) |
//! | `max_height` | `maxHeight` property (overrides parent if set) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `set_min_width()` | `minWidth = value` setter |
//! | `set_max_width()` | `maxWidth = value` setter |
//! | `set_alignment()` | `alignment = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Calculate child constraints**
//!    - Override parent constraints with custom min/max values
//!    - If custom value None: use parent constraint value
//!    - If custom value Some(v): use v instead
//!
//! 2. **Layout child**
//!    - Child laid out with custom constraints
//!    - Child may exceed parent size (overflow scenario)
//!    - Child size cached for alignment calculation
//!
//! 3. **Calculate parent size**
//!    - Parent size = parent constraints.biggest() OR constrained size
//!    - Parent ignores child's actual size (allows overflow)
//!    - Uses max available space from parent
//!
//! 4. **Return parent size**
//!    - Size based on parent constraints only
//!    - Child size ignored for parent sizing (key difference)
//!
//! # Paint Protocol
//!
//! 1. **Calculate alignment offset**
//!    - Compute offset to align child within parent bounds
//!    - Uses available space: parent_size - child_size
//!    - Offset = (available * (alignment + 1.0)) / 2.0
//!
//! 2. **Paint child at aligned offset**
//!    - Child painted at parent offset + alignment offset
//!    - May paint outside parent bounds (overflow)
//!    - No clipping applied
//!
//! # Performance
//!
//! - **Layout**: O(1) - constraint override + single child layout
//! - **Paint**: O(1) - alignment calculation + child paint
//! - **Memory**: 48 bytes (4 × Option<f32> + Alignment + 2 × Size cache)
//!
//! # Use Cases
//!
//! - **Overflow scenarios**: Allow child to exceed parent boundaries
//! - **Fixed-size rendering**: Render at specific size regardless of parent
//! - **Modal overlays**: Content larger than container
//! - **Constraint override**: Apply custom constraints different from parent
//! - **Debug layouts**: Force specific sizes for testing
//! - **Scrolling with overflow**: Child larger than viewport
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderConstrainedOverflowBox**: Both allow overflow, but OverflowBox loosens parent size calculation
//! - **vs RenderConstrainedBox**: ConstrainedBox enforces, OverflowBox allows overflow
//! - **vs RenderSizedBox**: SizedBox forces exact size, OverflowBox is more flexible
//! - **vs RenderConstraintsTransformBox**: TransformBox uses callback, OverflowBox uses fixed overrides
//!
//! # Important Note
//!
//! Consider wrapping in `RenderClipRect` to avoid confusing hit testing behavior
//! when child overflows parent bounds. Without clipping, hit testing may register
//! events outside visible parent area.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderOverflowBox;
//! use flui_types::Alignment;
//!
//! // Allow child to overflow in all directions
//! let overflow = RenderOverflowBox::new();
//!
//! // Child can be up to 200×200 (may overflow parent)
//! let sized = RenderOverflowBox::with_constraints(
//!     Some(0.0), Some(200.0),
//!     Some(0.0), Some(200.0),
//! );
//!
//! // Top-left aligned overflow
//! let aligned = RenderOverflowBox::with_alignment(Alignment::TOP_LEFT);
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_types::constraints::BoxConstraints;
use flui_types::{Alignment, Offset, Size};

/// RenderObject that imposes custom constraints on child, allowing overflow.
///
/// Applies different constraints to child than received from parent,
/// potentially allowing child to be larger than parent. Parent sizes
/// itself using available space while child uses custom constraints.
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
/// **Constraint Modifier with Overflow** - Overrides child constraints,
/// parent sizes independently allowing child overflow.
///
/// # Use Cases
///
/// - **Overflow scenarios**: Allow child to exceed parent boundaries
/// - **Fixed-size rendering**: Render at specific size regardless of parent
/// - **Modal overlays**: Content larger than container
/// - **Constraint override**: Custom constraints different from parent
/// - **Debug layouts**: Force specific sizes for testing
/// - **Scrolling**: Child larger than viewport
///
/// # Flutter Compliance
///
/// Matches Flutter's OverflowBox behavior:
/// - Parent sizes to incoming constraints (loosened)
/// - Custom constraints override parent constraints for child
/// - Child size ignored for parent sizing (allows overflow)
/// - Alignment applied when sizes differ
/// - Extends RenderAligningShiftedBox base class
///
/// # Overflow Behavior
///
/// Child can overflow parent because:
/// 1. Parent size determined from parent constraints
/// 2. Child constraints are custom, independent of parent size
/// 3. No clipping applied during paint
///
/// Wrap in RenderClipRect if clipping is desired.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOverflowBox;
/// use flui_types::Alignment;
///
/// // Allow free overflow
/// let overflow = RenderOverflowBox::new();
///
/// // Child can be up to 200×200
/// let sized = RenderOverflowBox::with_constraints(
///     Some(0.0), Some(200.0),
///     Some(0.0), Some(200.0),
/// );
///
/// // Top-left aligned
/// let aligned = RenderOverflowBox::with_alignment(Alignment::TOP_LEFT);
/// ```
#[derive(Debug)]
pub struct RenderOverflowBox {
    /// Minimum width for child_id (overrides parent constraints)
    pub min_width: Option<f32>,
    /// Maximum width for child_id (overrides parent constraints)
    pub max_width: Option<f32>,
    /// Minimum height for child_id (overrides parent constraints)
    pub min_height: Option<f32>,
    /// Maximum height for child_id (overrides parent constraints)
    pub max_height: Option<f32>,
    /// How to align the overflowing child_id
    pub alignment: Alignment,

    // Cache for paint
    child_size: Size,
    container_size: Size,
}

impl RenderOverflowBox {
    /// Create new RenderOverflowBox
    pub fn new() -> Self {
        Self {
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
            child_size: Size::ZERO,
            container_size: Size::ZERO,
        }
    }

    /// Create with specific constraints
    pub fn with_constraints(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            alignment: Alignment::CENTER,
            child_size: Size::ZERO,
            container_size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            ..Self::new()
        }
    }

    /// Set minimum width
    pub fn set_min_width(&mut self, min_width: Option<f32>) {
        self.min_width = min_width;
    }

    /// Set maximum width
    pub fn set_max_width(&mut self, max_width: Option<f32>) {
        self.max_width = max_width;
    }

    /// Set alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

impl Default for RenderOverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderOverflowBox {}

impl RenderBox<Single> for RenderOverflowBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let constraints = ctx.constraints;

        // Calculate child constraints by overriding parent constraints
        // If custom value is None: use parent's value
        // If custom value is Some(v): use v instead
        let child_min_width = self.min_width.unwrap_or(constraints.min_width);
        let child_max_width = self.max_width.unwrap_or(constraints.max_width);
        let child_min_height = self.min_height.unwrap_or(constraints.min_height);
        let child_max_height = self.max_height.unwrap_or(constraints.max_height);

        let child_constraints = BoxConstraints::new(
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
        );

        // Layout child with custom (overridden) constraints
        // Child may be larger than parent will be (overflow scenario)
        let child_size = ctx.layout_child(child_id, child_constraints)?;

        // Parent size determined by parent constraints (ignores child size)
        // This allows child to overflow parent bounds
        let size = constraints.constrain(Size::new(constraints.max_width, constraints.max_height));

        // Cache sizes for alignment calculation in paint
        self.child_size = child_size;
        self.container_size = size;

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let offset = ctx.offset;

        // Calculate alignment offset
        // Available space may be negative if child overflows
        let available_width = self.container_size.width - self.child_size.width;
        let available_height = self.container_size.height - self.child_size.height;

        // Alignment formula: offset = (available * (alignment + 1.0)) / 2.0
        let aligned_x = (available_width * (self.alignment.x + 1.0)) / 2.0;
        let aligned_y = (available_height * (self.alignment.y + 1.0)) / 2.0;

        let child_offset = offset + Offset::new(aligned_x, aligned_y);

        // Paint child at aligned offset
        // May paint outside parent bounds (no clipping)
        ctx.paint_child(child_id, child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_overflow_box_new() {
        let overflow = RenderOverflowBox::new();
        assert_eq!(overflow.min_width, None);
        assert_eq!(overflow.max_width, None);
        assert_eq!(overflow.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_overflow_box_with_constraints() {
        let overflow =
            RenderOverflowBox::with_constraints(Some(10.0), Some(100.0), Some(20.0), Some(200.0));
        assert_eq!(overflow.min_width, Some(10.0));
        assert_eq!(overflow.max_width, Some(100.0));
        assert_eq!(overflow.min_height, Some(20.0));
        assert_eq!(overflow.max_height, Some(200.0));
    }

    #[test]
    fn test_render_overflow_box_with_alignment() {
        let overflow = RenderOverflowBox::with_alignment(Alignment::TOP_LEFT);
        assert_eq!(overflow.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_overflow_box_set_min_width() {
        let mut overflow = RenderOverflowBox::new();
        overflow.set_min_width(Some(50.0));
        assert_eq!(overflow.min_width, Some(50.0));
    }

    #[test]
    fn test_render_overflow_box_set_alignment() {
        let mut overflow = RenderOverflowBox::new();
        overflow.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(overflow.alignment, Alignment::BOTTOM_RIGHT);
    }
}
