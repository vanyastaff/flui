//! RenderCustomMultiChildLayoutBox - Custom multi-child layout with delegate

use flui_core::element::ElementId;
// TODO: Migrate to Render<A>
// use flui_core::render::{RuntimeArity, LayoutContext, PaintContext, LegacyRender};
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, Size};
use std::any::Any;
use std::fmt::Debug;

/// Context provided to delegate during layout
pub struct MultiChildLayoutContext<'a> {
    /// The element tree for laying out children
    pub tree: &'a flui_core::element::ElementTree,
    /// The children IDs
    pub children: &'a [ElementId],
}

impl<'a> MultiChildLayoutContext<'a> {
    /// Layout a child with the given constraints
    ///
    /// # Arguments
    /// * `index` - Child index
    /// * `constraints` - Constraints for the child
    ///
    /// # Returns
    /// The size of the laid out child
    pub fn layout_child(&self, index: usize, constraints: BoxConstraints) -> Size {
        if index >= self.children.len() {
            return Size::ZERO;
        }
        self.tree.layout_child(self.children[index], constraints)
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
        context: &MultiChildLayoutContext,
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
        context: &MultiChildLayoutContext,
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

/// RenderObject for custom multi-child layout with delegate
///
/// Provides full control over child layout and positioning through a delegate.
/// Unlike RenderFlow which uses transforms, this uses direct positioning.
///
/// # Use Cases
///
/// - Custom grid layouts
/// - Masonry/Pinterest-style layouts
/// - Complex responsive layouts
/// - Any layout that doesn't fit standard containers
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::{RenderCustomMultiChildLayoutBox, SimpleGridDelegate};
///
/// let delegate = SimpleGridDelegate::new(3, 10.0); // 3 columns, 10px spacing
/// let layout = RenderCustomMultiChildLayoutBox::new(Box::new(delegate));
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

impl LegacyRender for RenderCustomMultiChildLayoutBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_ids = ctx.children.as_slice();
        let constraints = ctx.constraints;

        // Create layout context for delegate
        let layout_ctx = MultiChildLayoutContext {
            tree,
            children: child_ids,
        };

        // Let delegate perform layout
        let (size, child_offsets) = self.delegate.perform_layout(&layout_ctx, constraints);

        // Cache offsets for paint
        self.child_offsets = child_offsets;

        // Constrain size to bounds
        constraints.constrain(size)
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_ids = ctx.children.as_slice();
        let offset = ctx.offset;

        let mut canvas = Canvas::new();

        // Paint children at their calculated offsets
        for (index, &child_id) in child_ids.iter().enumerate() {
            if index >= self.child_offsets.len() {
                break;
            }

            let child_offset = self.child_offsets[index];
            let child_canvas = tree.paint_child(child_id, offset + child_offset);
            canvas.append_canvas(child_canvas);
        }

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable // Variable number of children
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

    #[test]
    fn test_arity_is_variable() {
        let delegate = SimpleGridDelegate::new(2, 5.0);
        let layout = RenderCustomMultiChildLayoutBox::new(Box::new(delegate));

        assert_eq!(layout.arity(), RuntimeArity::Variable);
    }
}
