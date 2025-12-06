//! RenderPadding - adds padding around a child
//!
//! This module provides [`RenderPadding`], a render object that adds empty space
//! around its child, following Flutter's RenderPadding protocol exactly.
//!
//! # Flutter Equivalence
//!
//! This implementation matches Flutter's `RenderPadding` class from
//! `package:flutter/src/rendering/shifted_box.dart`.
//!
//! **Flutter API:**
//! ```dart
//! class RenderPadding extends RenderShiftedBox {
//!   RenderPadding({
//!     EdgeInsetsGeometry? padding,
//!     RenderBox? child,
//!   });
//! }
//! ```
//!
//! # Layout Protocol
//!
//! 1. **Constraints**: Deflates parent constraints by padding amount
//! 2. **Child Layout**: Lays out child with deflated constraints
//! 3. **Sizing**: Adds padding to child size
//! 4. **Positioning**: Offsets child by `(left, top)` padding
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constant-time constraint deflation
//! - **Paint**: O(1) - direct child paint with offset
//! - **Memory**: 32 bytes (EdgeInsets = 4 × f32)

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{EdgeInsets, Offset, Size};

/// RenderObject that adds padding around its child.
///
/// Padding increases the size of the render object by the padding amount.
/// The child is laid out with constraints deflated by the padding,
/// then the final size includes the padding.
///
/// # Flutter Compliance
///
/// This implementation follows Flutter's RenderPadding protocol:
///
/// | Flutter Method | FLUI Equivalent | Behavior |
/// |----------------|-----------------|----------|
/// | `performLayout()` | `layout()` | Deflate constraints, layout child, add padding |
/// | `paint()` | `paint()` | Paint child at padded offset |
/// | `hitTestChildren()` | `hit_test()` | Transform position by padding |
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// // Uniform padding
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
///
/// // Asymmetric padding
/// let padding = RenderPadding::new(EdgeInsets::only(
///     left: 10.0,
///     right: 20.0,
///     top: 5.0,
///     bottom: 15.0,
/// ));
///
/// // Horizontal/Vertical shortcuts
/// let h_padding = RenderPadding::new(EdgeInsets::symmetric_h(20.0));
/// let v_padding = RenderPadding::new(EdgeInsets::symmetric_v(10.0));
/// ```
///
/// # Layout Algorithm
///
/// ```text
/// Parent Constraints: min=0×0, max=400×600
/// Padding: EdgeInsets(left: 10, top: 20, right: 10, bottom: 20)
///
/// Step 1: Deflate constraints
///   child_constraints = BoxConstraints(
///     min: 0×0,
///     max: (400 - 20) × (600 - 40) = 380×560
///   )
///
/// Step 2: Layout child
///   child_size = child.layout(child_constraints) = 300×400
///
/// Step 3: Add padding
///   size = (300 + 20) × (400 + 40) = 320×440
/// ```
#[derive(Debug, Clone)]
pub struct RenderPadding {
    /// The padding to apply around the child
    pub padding: EdgeInsets,
}

impl RenderPadding {
    /// Create new RenderPadding
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    /// Set new padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }
}

impl RenderObject for RenderPadding {}

impl RenderBox<Single> for RenderPadding {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let padding = self.padding;

        // Deflate constraints by padding
        let child_constraints = ctx.constraints.deflate(&padding);

        // Layout child with deflated constraints (using convenience method)
        let child_size = ctx.layout_child(ctx.single_child(), child_constraints)?;

        // Add padding to child size
        Ok(Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        ))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Apply padding offset and paint child (using convenience method)
        let child_offset = Offset::new(self.padding.left, self.padding.top);
        ctx.paint_child(ctx.single_child(), ctx.offset + child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_padding_new() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_padding_set() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        padding.set_padding(EdgeInsets::all(20.0));
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }
}
