//! RenderCustomMultiChildLayoutBox - Custom multi-child layout with delegate pattern
//!
//! Implements Flutter's custom multi-child layout system using a delegate pattern
//! for full control over child positioning and sizing. Unlike FlowDelegate which
//! uses transformation matrices, this delegate directly positions children with
//! offsets. Ideal for complex layouts that don't fit standard containers.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderCustomMultiChildLayoutBox` | `RenderCustomMultiChildLayoutBox` from `package:flutter/src/rendering/custom_layout.dart` |
//! | `MultiChildLayoutDelegate` | `MultiChildLayoutDelegate` trait |
//! | `MultiChildLayoutContext` | `MultiChildLayoutContext` class |
//! | `perform_layout()` | `performLayout()` method |
//! | `should_relayout()` | `shouldRelayout()` method |
//! | `layout_child()` | `layoutChild()` method (in context) |
//! | `child_count()` | `childCount` getter (in context) |
//! | `SimpleGridDelegate` | Example implementation (not in Flutter) |
//!
//! # Layout Protocol
//!
//! 1. **Create layout context**
//!    - Wrap BoxLayoutCtx in MultiChildLayoutContext
//!    - Provide child IDs array for delegate access
//!    - Expose layout_child() for delegate
//!
//! 2. **Delegate performs layout**
//!    - Call `delegate.perform_layout(context, constraints)`
//!    - Delegate layouts each child with custom constraints
//!    - Delegate calculates child positions (offsets)
//!    - Delegate returns (container_size, child_offsets)
//!
//! 3. **Cache results**
//!    - Store child offsets for paint phase
//!    - Constrain returned size to parent constraints
//!    - Return final container size
//!
//! # Paint Protocol
//!
//! 1. **Paint children with cached offsets**
//!    - Iterate children in order
//!    - Use cached offset from layout phase
//!    - Paint each child at parent_offset + child_offset
//!
//! # Performance
//!
//! - **Layout**: O(n) + delegate complexity - single delegate call, delegate controls layout
//! - **Paint**: O(n) - paint each child once at cached offset
//! - **Memory**: 32 bytes base + O(n) for cached offsets (16 bytes per child)
//!
//! # Use Cases
//!
//! - **Custom grid layouts**: Grids with non-uniform cells or custom spacing
//! - **Masonry layouts**: Pinterest-style variable-height grids
//! - **Complex responsive**: Layouts that change based on available space
//! - **Custom positioning**: Absolute positioning with custom logic
//! - **Dynamic layouts**: Layouts computed at runtime based on data
//! - **Overlay positioning**: Tooltips, popovers with calculated positions
//!
//! # Delegate Pattern Benefits
//!
//! - **Full control**: Complete control over child constraints and positions
//! - **Separation of concerns**: Layout logic separate from render object
//! - **Reusability**: Same delegate can be used with different instances
//! - **Testability**: Delegates can be unit tested independently
//! - **Flexibility**: Easy to create complex layouts without subclassing
//! - **Optimization**: `should_relayout()` optimizes layout updates
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFlow**: Flow uses transforms, this uses direct offsets
//! - **vs RenderGrid**: Grid has fixed track system, this is fully custom
//! - **vs RenderTable**: Table has column/row structure, this is arbitrary
//! - **vs RenderStack**: Stack has simple layering, this has custom positioning
//!
//! # Delegate Implementation Pattern
//!
//! ```rust,ignore
//! use flui_rendering::objects::layout::{
//!     MultiChildLayoutDelegate, MultiChildLayoutContext
//! };
//!
//! #[derive(Debug)]
//! struct MyCustomDelegate {
//!     // Configuration fields
//! }
//!
//! impl MultiChildLayoutDelegate for MyCustomDelegate {
//!     fn perform_layout(
//!         &mut self,
//!         context: &mut MultiChildLayoutContext,
//!         constraints: BoxConstraints,
//!     ) -> (Size, Vec<Offset>) {
//!         let mut offsets = Vec::new();
//!
//!         // Layout each child with custom logic
//!         for i in 0..context.child_count() {
//!             let child_constraints = /* custom constraints */;
//!             let child_size = context.layout_child(i, child_constraints);
//!
//!             // Calculate position based on your logic
//!             let offset = /* calculate offset */;
//!             offsets.push(offset);
//!         }
//!
//!         // Return container size and child offsets
//!         let size = /* calculate container size */;
//!         (size, offsets)
//!     }
//!
//!     fn should_relayout(&self, old: &dyn Any) -> bool {
//!         // Return true if configuration changed
//!         true
//!     }
//!
//!     fn as_any(&self) -> &dyn Any { self }
//! }
//! ```
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::objects::layout::{
//!     RenderCustomMultiChildLayoutBox, SimpleGridDelegate
//! };
//!
//! // Simple grid with 3 columns and 10px spacing
//! let delegate = SimpleGridDelegate::new(3, 10.0);
//! let layout = RenderCustomMultiChildLayoutBox::new(Box::new(delegate));
//!
//! // Custom masonry layout
//! struct MasonryDelegate {
//!     column_count: usize,
//!     column_heights: Vec<f32>,
//! }
//!
//! // The delegate would track column heights and place each item
//! // in the shortest column for Pinterest-style masonry effect
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Variable};
use crate::{RenderObject, RenderResult};
use flui_foundation::ElementId;
use flui_types::{BoxConstraints, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Context provided to delegate during layout
pub struct MultiChildLayoutContext<'a, 'b> {
    /// The layout context
    ctx: &'a mut BoxLayoutCtx<'b, Variable>,
    /// The children IDs
    pub children: &'a [ElementId],
}

impl<'a, 'b> MultiChildLayoutContext<'a, 'b> {
    /// Layout a child with the given constraints
    ///
    /// # Arguments
    /// * `index` - Child index
    /// * `constraints` - Constraints for the child
    ///
    /// # Returns
    /// The size of the laid out child
    pub fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        if index >= self.children.len() {
            return Size::ZERO;
        }
        // Note: This returns Size, unwrapping the Result for the delegate API
        self.ctx
            .layout_child(self.children[index], constraints)
            .unwrap_or(Size::ZERO)
    }

    /// Get the number of children
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
}

/// Delegate trait for custom multi-child layout logic
///
/// This trait allows full custom control over:
/// - How each child is laid out (constraints)
/// - Where each child is positioned (offset)
/// - The final size of the container
///
/// Unlike FlowDelegate which uses transforms, this delegate uses direct positioning.
pub trait MultiChildLayoutDelegate: Debug + Send + Sync {
    /// Perform layout for all children
    ///
    /// The delegate should:
    /// 1. Layout each child using `context.layout_child(index, constraints)`
    /// 2. Store child sizes and positions for paint phase
    /// 3. Return the final size of the container
    ///
    /// # Arguments
    /// * `context` - Layout context with tree and children access
    /// * `constraints` - Box constraints for this container
    ///
    /// # Returns
    /// The final size of the container and child offsets
    fn perform_layout(
        &mut self,
        context: &mut MultiChildLayoutContext,
        constraints: BoxConstraints,
    ) -> (Size, Vec<Offset>);

    /// Check if layout should be recomputed
    ///
    /// Return true if the delegate's state has changed in a way that requires relayout.
    fn should_relayout(&self, old: &dyn Any) -> bool;

    /// For Any trait (downcasting)
    fn as_any(&self) -> &dyn Any;
}

/// A simple grid delegate for demonstration
///
/// Arranges children in a grid with fixed column count.
#[derive(Debug, Clone)]
pub struct SimpleGridDelegate {
    /// Number of columns
    pub column_count: usize,
    /// Spacing between items
    pub spacing: f32,
}

impl SimpleGridDelegate {
    /// Create new grid delegate
    pub fn new(column_count: usize, spacing: f32) -> Self {
        Self {
            column_count,
            spacing,
        }
    }
}

impl MultiChildLayoutDelegate for SimpleGridDelegate {
    fn perform_layout(
        &mut self,
        context: &mut MultiChildLayoutContext,
        constraints: BoxConstraints,
    ) -> (Size, Vec<Offset>) {
        let child_count = context.child_count();
        if child_count == 0 {
            return (constraints.smallest(), Vec::new());
        }

        // Calculate cell size
        let column_count = self.column_count.max(1);
        let total_spacing = self.spacing * (column_count - 1) as f32;
        let available_width = constraints.max_width - total_spacing;
        let cell_width = (available_width / column_count as f32).max(0.0);

        let mut child_offsets = Vec::with_capacity(child_count);
        let mut max_height_in_row = 0.0f32;
        let mut current_row = 0;
        let mut total_height = 0.0f32;

        // Layout children and calculate positions
        for i in 0..child_count {
            let column = i % column_count;
            let row = i / column_count;

            // New row
            if row != current_row {
                total_height += max_height_in_row + self.spacing;
                max_height_in_row = 0.0;
                current_row = row;
            }

            // Layout child with square constraints
            let child_constraints = BoxConstraints::tight(Size::new(cell_width, cell_width));
            let child_size = context.layout_child(i, child_constraints);

            // Calculate position
            let x = column as f32 * (cell_width + self.spacing);
            let y = total_height;

            child_offsets.push(Offset::new(x, y));

            // Track max height in row
            max_height_in_row = max_height_in_row.max(child_size.height);
        }

        // Add last row height
        total_height += max_height_in_row;

        // Final container size
        let size = Size::new(constraints.max_width, total_height);

        (size, child_offsets)
    }

    fn should_relayout(&self, old: &dyn Any) -> bool {
        if let Some(old_delegate) = old.downcast_ref::<Self>() {
            self.column_count != old_delegate.column_count || self.spacing != old_delegate.spacing
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// RenderObject for custom multi-child layout with delegate pattern.
///
/// Delegates all layout logic to a MultiChildLayoutDelegate trait, providing full
/// control over child constraints, sizing, and positioning. The delegate computes
/// child offsets directly (unlike FlowDelegate which uses transforms) and returns
/// both container size and child positions.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+). Delegate controls how
/// children are laid out and positioned.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Delegated Multi-Child Layout** - Delegates layout logic to trait, delegate
/// provides full control over child constraints and positioning, direct offset
/// positioning (not transforms), sizes to delegate-computed size.
///
/// # Use Cases
///
/// - **Custom grids**: Non-uniform grids or grids with custom spacing rules
/// - **Masonry layouts**: Pinterest-style variable-height column layouts
/// - **Complex responsive**: Layouts that reorganize based on available space
/// - **Absolute positioning**: Custom absolute positioning with computed logic
/// - **Dynamic layouts**: Layouts computed at runtime based on data
/// - **Overlay positioning**: Tooltips, popovers with calculated positions
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderCustomMultiChildLayoutBox behavior:
/// - Delegate pattern for custom layout logic
/// - MultiChildLayoutContext provides layout_child() access
/// - Delegate returns (Size, Vec<Offset>) from perform_layout()
/// - should_relayout() optimization for layout updates
/// - Direct offset positioning (not transformation matrices)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::{
///     RenderCustomMultiChildLayoutBox, SimpleGridDelegate
/// };
///
/// // Simple 3-column grid with 10px spacing
/// let delegate = SimpleGridDelegate::new(3, 10.0);
/// let layout = RenderCustomMultiChildLayoutBox::new(Box::new(delegate));
///
/// // Custom delegate for masonry layout
/// struct MasonryDelegate {
///     column_count: usize,
///     spacing: f32,
///     column_heights: Vec<f32>,
/// }
///
/// impl MultiChildLayoutDelegate for MasonryDelegate {
///     fn perform_layout(
///         &mut self,
///         context: &mut MultiChildLayoutContext,
///         constraints: BoxConstraints,
///     ) -> (Size, Vec<Offset>) {
///         // Reset column heights
///         self.column_heights = vec![0.0; self.column_count];
///         let mut offsets = Vec::new();
///
///         // Layout each child in shortest column
///         for i in 0..context.child_count() {
///             // Find shortest column
///             let col = self.shortest_column();
///
///             // Layout child
///             let child_size = context.layout_child(i, /* constraints */);
///
///             // Position in column
///             let offset = Offset::new(
///                 col as f32 * (width + self.spacing),
///                 self.column_heights[col]
///             );
///             offsets.push(offset);
///
///             // Update column height
///             self.column_heights[col] += child_size.height + self.spacing;
///         }
///
///         (container_size, offsets)
///     }
///     // ... should_relayout, as_any
/// }
/// ```
#[derive(Debug)]
pub struct RenderCustomMultiChildLayoutBox {
    /// The layout delegate
    pub delegate: Box<dyn MultiChildLayoutDelegate>,

    // Cache for paint
    child_offsets: Vec<Offset>,
}

impl RenderCustomMultiChildLayoutBox {
    /// Create new custom multi-child layout
    pub fn new(delegate: Box<dyn MultiChildLayoutDelegate>) -> Self {
        Self {
            delegate,
            child_offsets: Vec::new(),
        }
    }

    /// Set new delegate
    pub fn set_delegate(&mut self, delegate: Box<dyn MultiChildLayoutDelegate>) {
        self.delegate = delegate;
    }
}

impl RenderObject for RenderCustomMultiChildLayoutBox {}

impl RenderBox<Variable> for RenderCustomMultiChildLayoutBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Collect children for delegate
        let child_ids: Vec<ElementId> = children.iter().map(|id| *id).collect();

        // Create layout context for delegate
        let mut layout_ctx = MultiChildLayoutContext {
            ctx: &mut ctx,
            children: &child_ids,
        };

        // Let delegate perform layout
        let (size, child_offsets) = self.delegate.perform_layout(&mut layout_ctx, constraints);

        // Cache offsets for paint
        self.child_offsets = child_offsets;

        // Constrain size to bounds
        Ok(constraints.constrain(size))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        // Paint children at their calculated offsets
        for (index, child_id) in child_ids.into_iter().enumerate() {
            if index >= self.child_offsets.len() {
                break;
            }

            let child_offset = self.child_offsets[index];
            ctx.paint_child(*child_id, offset + child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_grid_delegate_new() {
        let delegate = SimpleGridDelegate::new(3, 10.0);

        assert_eq!(delegate.column_count, 3);
        assert_eq!(delegate.spacing, 10.0);
    }

    #[test]
    fn test_render_custom_multi_child_layout_box_new() {
        let delegate = SimpleGridDelegate::new(2, 5.0);
        let layout = RenderCustomMultiChildLayoutBox::new(Box::new(delegate));

        assert_eq!(layout.child_offsets.len(), 0);
    }

    #[test]
    fn test_simple_grid_delegate_should_relayout() {
        let delegate1 = SimpleGridDelegate::new(3, 10.0);
        let delegate2 = SimpleGridDelegate::new(3, 10.0);
        let delegate3 = SimpleGridDelegate::new(4, 10.0);

        // Same configuration - no relayout needed
        assert!(!delegate1.should_relayout(delegate2.as_any()));

        // Different configuration - relayout needed
        assert!(delegate1.should_relayout(delegate3.as_any()));
    }
}
