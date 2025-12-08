//! RenderPadding - Adds empty space around child widget
//!
//! Implements Flutter's RenderPadding that adds padding (empty space) around its
//! child. Deflates constraints before laying out the child, then adds padding to
//! the final size. Extends RenderShiftedBox in Flutter's class hierarchy.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderPadding` | `RenderPadding` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `padding` | `padding` property (EdgeInsetsGeometry) |
//! | `set_padding()` | `padding = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Deflate constraints**
//!    - Reduce max width by padding.horizontal_total()
//!    - Reduce max height by padding.vertical_total()
//!    - Ensure constraints remain valid (min ≤ max)
//!
//! 2. **Layout child**
//!    - Pass deflated constraints to child
//!    - Child determines its size within available space
//!
//! 3. **Calculate final size**
//!    - Add horizontal padding to child width
//!    - Add vertical padding to child height
//!    - Final size = child size + padding
//!
//! # Paint Protocol
//!
//! 1. **Calculate child offset**
//!    - Offset = (padding.left, padding.top)
//!    - Child painted inside padded area
//!
//! 2. **Paint child**
//!    - Child painted at parent offset + padding offset
//!    - Padding area remains empty (no fill)
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constant-time constraint deflation
//! - **Paint**: O(1) - direct child paint with offset
//! - **Memory**: 32 bytes (EdgeInsets = 4 × f32)
//!
//! # Use Cases
//!
//! - **Spacing**: Add space around widgets (buttons, cards, containers)
//! - **Layout margins**: Create outer margins for widgets
//! - **Safe areas**: Respect device safe areas (notches, rounded corners)
//! - **Breathing room**: Improve visual hierarchy with whitespace
//! - **Touch targets**: Increase tap target size without changing visual size
//! - **Grid spacing**: Add padding to grid cells
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderPadding;
//! use flui_types::EdgeInsets;
//!
//! // Uniform padding (all sides equal)
//! let padding = RenderPadding::new(EdgeInsets::all(10.0));
//!
//! // Symmetric padding (horizontal and vertical)
//! let padding = RenderPadding::new(EdgeInsets::symmetric(20.0, 10.0));
//!
//! // Asymmetric padding (each side different)
//! let padding = RenderPadding::new(EdgeInsets::ltrb(10.0, 20.0, 10.0, 20.0));
//!
//! // Common patterns
//! let horizontal = RenderPadding::new(EdgeInsets::symmetric_h(20.0));
//! let vertical = RenderPadding::new(EdgeInsets::symmetric_v(10.0));
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{EdgeInsets, Offset, Size};

/// RenderObject that adds padding (empty space) around its child.
///
/// Padding increases the final size by the padding amount. Constraints are
/// deflated before passing to the child, ensuring the child fits within the
/// available space minus padding. Child is then painted at an offset
/// corresponding to the padding's left and top values.
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
/// **Constraint Deflator with Offset** - Deflates constraints by padding amount,
/// paints child at padding offset, inflates child size by padding for final size.
///
/// # Use Cases
///
/// - **Widget spacing**: Add space around buttons, cards, images
/// - **Layout margins**: Create outer margins for containers
/// - **Safe areas**: Respect device safe areas (notches, rounded corners)
/// - **Visual hierarchy**: Improve readability with whitespace
/// - **Touch targets**: Increase tap area without changing visual size
/// - **Grid cells**: Add uniform spacing in grid layouts
/// - **Content insets**: Inset content from edges of containers
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderPadding behavior:
/// - Deflates constraints by padding amount before child layout
/// - Adds padding to child size for final size
/// - Child painted at offset (padding.left, padding.top)
/// - Hit testing adjusted for padding offset
/// - Extends RenderShiftedBox base class
///
/// # Comparison with Related Objects
///
/// - **vs RenderMargin**: Margin adds space outside, Padding adds space inside (same implementation)
/// - **vs RenderSizedBox**: SizedBox forces size, Padding adjusts constraints
/// - **vs RenderAlign**: Align positions child, Padding offsets child
/// - **vs RenderConstrainedBox**: ConstrainedBox adds constraints, Padding deflates them
///
/// # Layout Algorithm Example
///
/// ```text
/// Input:
///   Parent constraints: min=0×0, max=400×600
///   Padding: left=10, top=20, right=10, bottom=20
///
/// Step 1 - Deflate constraints:
///   horizontal = 10 + 10 = 20
///   vertical = 20 + 20 = 40
///   child_constraints = max=(400-20)×(600-40) = 380×560
///
/// Step 2 - Layout child:
///   child_size = 300×400 (child determines size)
///
/// Step 3 - Add padding:
///   final_size = (300+20)×(400+40) = 320×440
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// // Uniform padding (all sides 10px)
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
///
/// // Asymmetric padding (different on each side)
/// let padding = RenderPadding::new(EdgeInsets::ltrb(10.0, 20.0, 10.0, 20.0));
///
/// // Symmetric padding (horizontal and vertical)
/// let padding = RenderPadding::new(EdgeInsets::symmetric(20.0, 10.0));
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
