//! RenderPositionedBox - Positions child with explicit coordinates
//!
//! Implements Flutter's positioned box pattern for absolute positioning of children
//! within a Stack layout. Supports positioning using left/top/right/bottom edges
//! and optional explicit width/height constraints.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderPositionedBox` | Similar to `Positioned` widget behavior in `Stack` |
//! | `left` | `left` property (distance from left edge) |
//! | `top` | `top` property (distance from top edge) |
//! | `right` | `right` property (distance from right edge) |
//! | `bottom` | `bottom` property (distance from bottom edge) |
//! | `width` | `width` property (explicit width override) |
//! | `height` | `height` property (explicit height override) |
//! | `set_left()` | `left = value` setter |
//! | `set_top()` | `top = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Calculate child constraints based on positioning**
//!    - If left AND right specified: width = parent_width - left - right
//!    - If only width specified: use explicit width
//!    - Otherwise: use parent's width constraints
//!    - Same logic for height (top/bottom vs explicit height)
//!
//! 2. **Layout child**
//!    - Child laid out with calculated constraints
//!    - Child determines final size within constraints
//!
//! 3. **Return child size**
//!    - Size is child's size (positioning handled in paint)
//!
//! # Paint Protocol
//!
//! 1. **Calculate position offset**
//!    - Offset = (left OR 0, top OR 0)
//!    - Left and top take priority if specified
//!
//! 2. **Paint child at offset**
//!    - Child painted at parent offset + position offset
//!    - Right/bottom positioning handled by parent Stack
//!
//! # Performance
//!
//! - **Layout**: O(1) - constraint calculation + single child layout
//! - **Paint**: O(1) - offset calculation + child paint
//! - **Memory**: 24 bytes (6 × Option<f32>)
//!
//! # Use Cases
//!
//! - **Absolute positioning**: Position widgets at specific coordinates in Stack
//! - **Overlays**: Place overlays at specific positions
//! - **Modal dialogs**: Position dialogs with specific offsets
//! - **Tooltips**: Position tooltips relative to edges
//! - **Floating action buttons**: Position FABs at screen corners
//! - **Badge indicators**: Position badges on corners of widgets
//!
//! # Positioning Examples
//!
//! ```text
//! left=10, top=20:
//!   → Position at (10, 20) from top-left
//!
//! left=10, right=10:
//!   → Fill width with 10px margins, top based on child
//!
//! left=0, top=0, right=0, bottom=0:
//!   → Fill entire parent stack
//!
//! width=100, height=100:
//!   → Fixed 100×100 size, position at (0, 0)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderPositioned**: PositionedBox is simpler, Positioned uses metadata for Stack
//! - **vs RenderAlign**: Align uses alignment factors, PositionedBox uses explicit coordinates
//! - **vs RenderPadding**: Padding adds space, PositionedBox positions absolutely
//! - **vs RenderTransform**: Transform translates visually, PositionedBox positions in layout
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderPositionedBox;
//!
//! // Position at top-left (10, 20)
//! let positioned = RenderPositionedBox::at(10.0, 20.0);
//!
//! // Fill entire stack
//! let fill = RenderPositionedBox::fill(0.0, 0.0, 0.0, 0.0);
//!
//! // Position with explicit size
//! let mut sized = RenderPositionedBox::new();
//! sized.width = Some(100.0);
//! sized.height = Some(100.0);
//! sized.left = Some(50.0);
//! sized.top = Some(50.0);
//! ```

use flui_rendering::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{Offset, Size};

/// RenderObject that positions child with explicit coordinates.
///
/// Positions a child within a Stack using absolute coordinates (left, top,
/// right, bottom) and optional explicit dimensions (width, height). Calculates
/// child constraints based on positioning parameters.
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
/// **Absolute Positioner** - Positions child using explicit coordinates,
/// calculates constraints based on edge distances.
///
/// # Use Cases
///
/// - **Absolute positioning**: Position widgets at specific coordinates
/// - **Overlays**: Place UI elements at specific screen positions
/// - **Modal dialogs**: Position dialogs with edge offsets
/// - **Tooltips**: Position relative to edges
/// - **Floating buttons**: FABs at screen corners
/// - **Badge indicators**: Badges on widget corners
///
/// # Flutter Compliance
///
/// Similar to Flutter's Positioned widget pattern in Stack:
/// - Positions using left/top/right/bottom edge distances
/// - Supports explicit width/height overrides
/// - left + right → determines width
/// - top + bottom → determines height
/// - Child painted at calculated offset
///
/// # Positioning Logic
///
/// Width calculation:
/// - If left AND right: width = parent_width - left - right
/// - If width specified: use explicit width
/// - Otherwise: use parent's max width
///
/// Height calculation (same pattern for vertical).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPositionedBox;
///
/// // Position at (10, 20)
/// let positioned = RenderPositionedBox::at(10.0, 20.0);
///
/// // Fill with margins
/// let fill = RenderPositionedBox::fill(10.0, 10.0, 10.0, 10.0);
/// ```
#[derive(Debug)]
pub struct RenderPositionedBox {
    /// Distance from left edge
    pub left: Option<f32>,
    /// Distance from top edge
    pub top: Option<f32>,
    /// Distance from right edge
    pub right: Option<f32>,
    /// Distance from bottom edge
    pub bottom: Option<f32>,
    /// Explicit width
    pub width: Option<f32>,
    /// Explicit height
    pub height: Option<f32>,
}

impl RenderPositionedBox {
    /// Create new RenderPositionedBox
    pub fn new() -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Create with left and top
    pub fn at(left: f32, top: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            ..Self::new()
        }
    }

    /// Create with all edges
    pub fn fill(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: Some(right),
            bottom: Some(bottom),
            width: None,
            height: None,
        }
    }

    /// Set left position
    pub fn set_left(&mut self, left: Option<f32>) {
        self.left = left;
    }

    /// Set top position
    pub fn set_top(&mut self, top: Option<f32>) {
        self.top = top;
    }
}

impl Default for RenderPositionedBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderPositionedBox {}

impl RenderBox<Single> for RenderPositionedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let constraints = ctx.constraints;

        // Calculate child constraints based on positioning parameters
        // Width calculation: left + right → determines width, else explicit width
        let child_constraints = if let (Some(left), Some(right)) = (self.left, self.right) {
            // Width determined by left and right edge distances
            let width = (constraints.max_width - left - right).max(0.0);
            constraints.tighten(Some(width), None)
        } else if let Some(width) = self.width {
            // Explicit width override
            constraints.tighten(Some(width), None)
        } else {
            // Width unconstrained - use parent's constraints
            constraints
        };

        // Height calculation: top + bottom → determines height, else explicit height
        let child_constraints = if let (Some(top), Some(bottom)) = (self.top, self.bottom) {
            // Height determined by top and bottom edge distances
            let height = (constraints.max_height - top - bottom).max(0.0);
            child_constraints.tighten(None, Some(height))
        } else if let Some(height) = self.height {
            // Explicit height override
            child_constraints.tighten(None, Some(height))
        } else {
            // Height unconstrained - use parent's constraints
            child_constraints
        };

        // Layout child with calculated constraints
        Ok(ctx.layout_child(child_id, child_constraints, true)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate position offset
        // Use left/top if specified, otherwise default to (0, 0)
        let position_offset = Offset::new(self.left.unwrap_or(0.0), self.top.unwrap_or(0.0));
        let child_offset = ctx.offset + position_offset;

        // Paint child at positioned offset
        ctx.paint_child(child_id, child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_positioned_box_new() {
        let positioned = RenderPositionedBox::new();
        assert_eq!(positioned.left, None);
        assert_eq!(positioned.top, None);
        assert_eq!(positioned.right, None);
        assert_eq!(positioned.bottom, None);
    }

    #[test]
    fn test_render_positioned_box_at() {
        let positioned = RenderPositionedBox::at(10.0, 20.0);
        assert_eq!(positioned.left, Some(10.0));
        assert_eq!(positioned.top, Some(20.0));
    }

    #[test]
    fn test_render_positioned_box_fill() {
        let positioned = RenderPositionedBox::fill(10.0, 20.0, 30.0, 40.0);
        assert_eq!(positioned.left, Some(10.0));
        assert_eq!(positioned.top, Some(20.0));
        assert_eq!(positioned.right, Some(30.0));
        assert_eq!(positioned.bottom, Some(40.0));
    }

    #[test]
    fn test_render_positioned_box_set_left() {
        let mut positioned = RenderPositionedBox::new();
        positioned.set_left(Some(15.0));
        assert_eq!(positioned.left, Some(15.0));
    }

    #[test]
    fn test_render_positioned_box_set_top() {
        let mut positioned = RenderPositionedBox::new();
        positioned.set_top(Some(25.0));
        assert_eq!(positioned.top, Some(25.0));
    }
}
