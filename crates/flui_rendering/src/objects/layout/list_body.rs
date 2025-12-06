//! RenderListBody - Simple sequential list without alignment control
//!
//! Implements a simplified layout container for arranging children sequentially
//! along a main axis without alignment options. Simpler than Flex - children get
//! unbounded main axis (intrinsic sizing) and parent's cross axis constraints.
//! Ideal for simple scrollable lists and sequential content.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderListBody` | `RenderListBody` from `package:flutter/src/rendering/list_body.dart` |
//! | `main_axis` | `mainAxis` property (Axis enum) |
//! | `spacing` | Custom extension (not in Flutter ListBody) |
//! | `vertical()` | Creates Axis.vertical configuration |
//! | `horizontal()` | Creates Axis.horizontal configuration |
//! | `set_main_axis()` | `mainAxis = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Layout children sequentially**
//!    - Give each child:
//!      - Main axis: 0 to infinity (unbounded, intrinsic size)
//!      - Cross axis: parent's constraints (min to max)
//!    - Children determine their own main-axis size
//!    - Store each child's size
//!
//! 2. **Calculate container size**
//!    - Main axis: sum of all child main sizes + spacing
//!    - Cross axis: max of all child cross sizes
//!    - Clamp to parent constraints
//!
//! # Paint Protocol
//!
//! 1. **Paint children in order**
//!    - Paint sequentially along main axis
//!    - Accumulate offset: position + child_size + spacing
//!    - No alignment (children at start of cross axis)
//!
//! # Performance
//!
//! - **Layout**: O(n) - single pass through children
//! - **Paint**: O(n) - paint each child once in sequence
//! - **Memory**: 32 bytes base + O(n) for cached sizes (8 bytes per child)
//!
//! # Use Cases
//!
//! - **Scrollable lists**: Simple vertical/horizontal scrolling lists
//! - **Sequential content**: Basic sequential arrangement without alignment
//! - **Chat messages**: Message bubbles in sequence
//! - **Timeline items**: Sequential timeline entries
//! - **Form fields**: Simple vertical form field stacking
//! - **Menu items**: Simple menu item lists
//! - **Debug layouts**: Quick sequential layouts for testing
//!
//! # Difference from RenderFlex
//!
//! **ListBody (simpler):**
//! - No alignment control (always start-aligned)
//! - Unbounded main axis (intrinsic sizing)
//! - No MainAxisSize control
//! - No flex factors
//! - Lighter weight for simple lists
//!
//! **Flex (more features):**
//! - Full MainAxisAlignment control
//! - Full CrossAxisAlignment control
//! - MainAxisSize (min vs max)
//! - Flex factors (Expanded/Flexible)
//! - Heavier for complex layouts
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFlex**: Flex has alignment/sizing control, ListBody is simpler
//! - **vs RenderStack**: Stack overlaps children, ListBody arranges sequentially
//! - **vs RenderWrap**: Wrap supports wrapping, ListBody is single-line only
//! - **vs RenderColumn/Row**: Column/Row are Flex aliases, ListBody is simpler
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderListBody;
//!
//! // Vertical list (most common - chat, timeline, etc.)
//! let vertical = RenderListBody::vertical();
//!
//! // Horizontal list
//! let horizontal = RenderListBody::horizontal();
//!
//! // With spacing between items
//! let spaced = RenderListBody::vertical().with_spacing(8.0);
//!
//! // Mutable updates
//! let mut list = RenderListBody::default();
//! list.set_spacing(12.0);
//! list.set_main_axis(Axis::Horizontal);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, ChildrenAccess, RenderBox, Variable};
use crate::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::{Axis, Offset, Size};

/// RenderObject that arranges children in a simple sequential list.
///
/// Simpler than Flex - no alignment control, unbounded main axis for intrinsic
/// sizing, parent's cross axis constraints. Ideal for basic scrollable lists
/// and sequential content where alignment isn't needed.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Simple Sequential Container** - Arranges children in a line with unbounded
/// main axis (intrinsic sizing), no alignment control, optional spacing between
/// items, sizes to sum of children.
///
/// # Use Cases
///
/// - **Scrollable lists**: Vertical/horizontal simple scrolling lists
/// - **Chat messages**: Message bubbles in sequence
/// - **Timeline**: Sequential timeline entries
/// - **Simple forms**: Basic vertical form field stacking
/// - **Menu items**: Simple menu lists
/// - **Sequential content**: Any basic sequential arrangement
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderListBody behavior:
/// - Unbounded main axis (children determine own main size)
/// - Cross axis uses parent's constraints
/// - No alignment control (implicit start alignment)
/// - Size = sum of children + spacing
/// - FLUI extension: spacing property (not in Flutter)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderListBody;
/// use flui_types::Axis;
///
/// // Vertical list with spacing
/// let list = RenderListBody::vertical().with_spacing(8.0);
///
/// // Horizontal list
/// let horizontal = RenderListBody::horizontal();
/// ```
#[derive(Debug)]
pub struct RenderListBody {
    /// Main axis direction (horizontal or vertical)
    pub main_axis: Axis,
    /// Spacing between children
    pub spacing: f32,

    // Cache for paint
    child_sizes: Vec<Size>,
}

impl RenderListBody {
    /// Create new list body
    pub fn new(main_axis: Axis) -> Self {
        Self {
            main_axis,
            spacing: 0.0,
            child_sizes: Vec::new(),
        }
    }

    /// Create vertical list
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Create horizontal list
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Set spacing between children
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set main axis
    pub fn set_main_axis(&mut self, main_axis: Axis) {
        self.main_axis = main_axis;
    }

    /// Set spacing
    pub fn set_spacing(&mut self, spacing: f32) {
        self.spacing = spacing;
    }
}

impl Default for RenderListBody {
    fn default() -> Self {
        Self::vertical()
    }
}

impl RenderObject for RenderListBody {}

impl RenderBox<Variable> for RenderListBody {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let children = ctx.children;

        if children.as_slice().is_empty() {
            self.child_sizes.clear();
            return Ok(constraints.smallest());
        }

        // Layout children based on axis
        self.child_sizes.clear();

        match self.main_axis {
            Axis::Vertical => {
                let mut total_height = 0.0_f32;
                let mut max_width = 0.0_f32;

                for child in children.iter() {
                    // Child gets parent's width constraints, infinite height
                    let child_constraints = BoxConstraints::new(
                        constraints.min_width,
                        constraints.max_width,
                        0.0,
                        f32::INFINITY,
                    );

                    let child_size = ctx.layout_child(*child, child_constraints)?;
                    self.child_sizes.push(child_size);

                    total_height += child_size.height;
                    max_width = max_width.max(child_size.width);
                }

                // Add spacing between children
                if !children.as_slice().is_empty() {
                    total_height += self.spacing * (children.as_slice().len() - 1) as f32;
                }

                Ok(constraints.constrain(Size::new(max_width, total_height)))
            }
            Axis::Horizontal => {
                let mut total_width = 0.0_f32;
                let mut max_height = 0.0_f32;

                for child in children.iter() {
                    // Child gets infinite width, parent's height constraints
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        f32::INFINITY,
                        constraints.min_height,
                        constraints.max_height,
                    );

                    let child_size = ctx.layout_child(*child, child_constraints)?;
                    self.child_sizes.push(child_size);

                    total_width += child_size.width;
                    max_height = max_height.max(child_size.height);
                }

                // Add spacing between children
                if !children.as_slice().is_empty() {
                    total_width += self.spacing * (children.as_slice().len() - 1) as f32;
                }

                Ok(constraints.constrain(Size::new(total_width, max_height)))
            }
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        let mut current_offset = 0.0_f32;

        for (i, child_id) in child_ids.into_iter().enumerate() {
            let child_size = self.child_sizes.get(i).copied().unwrap_or(Size::ZERO);

            let child_offset = match self.main_axis {
                Axis::Vertical => Offset::new(0.0, current_offset),
                Axis::Horizontal => Offset::new(current_offset, 0.0),
            };

            // Paint child with combined offset
            ctx.paint_child(*child_id, offset + child_offset);

            current_offset += match self.main_axis {
                Axis::Vertical => child_size.height + self.spacing,
                Axis::Horizontal => child_size.width + self.spacing,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_body_new() {
        let data = RenderListBody::new(Axis::Vertical);
        assert_eq!(data.main_axis, Axis::Vertical);
        assert_eq!(data.spacing, 0.0);
    }

    #[test]
    fn test_list_body_vertical() {
        let data = RenderListBody::vertical();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_list_body_horizontal() {
        let data = RenderListBody::horizontal();
        assert_eq!(data.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_list_body_with_spacing() {
        let data = RenderListBody::vertical().with_spacing(10.0);
        assert_eq!(data.spacing, 10.0);
    }

    #[test]
    fn test_list_body_default() {
        let data = RenderListBody::default();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_render_list_body_set_main_axis() {
        let mut list = RenderListBody::vertical();
        list.set_main_axis(Axis::Horizontal);
        assert_eq!(list.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_render_list_body_set_spacing() {
        let mut list = RenderListBody::default();
        list.set_spacing(8.0);
        assert_eq!(list.spacing, 8.0);
    }
}
