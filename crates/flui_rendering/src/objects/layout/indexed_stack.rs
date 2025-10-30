//! RenderIndexedStack - shows only one child by index

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::MultiRender;
use flui_engine::{BoxedLayer, layer::pool};
use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};

/// RenderObject that shows only one child from a list
///
/// This is like a Stack, but only one child is visible at a time,
/// determined by the index. All children are laid out, but only
/// the selected one is painted.
///
/// Useful for tab views, page views, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderIndexedStack;
///
/// let mut indexed_stack = RenderIndexedStack::new(Some(0));
/// ```
#[derive(Debug)]
pub struct RenderIndexedStack {
    /// Index of child to display (None = show nothing)
    pub index: Option<usize>,
    /// How to align the selected child
    pub alignment: Alignment,

    // Cache for paint
    child_sizes: Vec<Size>,
    size: Size,
}

impl RenderIndexedStack {
    /// Create new indexed stack
    pub fn new(index: Option<usize>) -> Self {
        Self {
            index,
            alignment: Alignment::TOP_LEFT,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(index: Option<usize>, alignment: Alignment) -> Self {
        Self {
            index,
            alignment,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set new index
    pub fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

impl Default for RenderIndexedStack {
    fn default() -> Self {
        Self::new(None)
    }
}

impl MultiRender for RenderIndexedStack {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        if child_ids.is_empty() {
            self.child_sizes.clear();
            return constraints.smallest();
        }

        // Layout all children (to maintain their state)
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        self.child_sizes.clear();

        for child in child_ids.iter().copied() {
            let child_size = tree.layout_child(child, constraints);
            self.child_sizes.push(child_size);
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Size is the max of all children
        self.size = Size::new(
            max_width.clamp(constraints.min_width, constraints.max_width),
            max_height.clamp(constraints.min_height, constraints.max_height),
        );
        self.size
    }

    fn paint(&self, tree: &ElementTree, child_ids: &[ElementId], offset: Offset) -> BoxedLayer {
        let mut container = pool::acquire_container();

        // Only paint the selected child
        if let Some(index) = self.index
            && let (Some(&child_id), Some(&child_size)) =
                (child_ids.get(index), self.child_sizes.get(index))
        {
            // Calculate aligned position
            let child_offset = self.alignment.calculate_offset(child_size, self.size);

            // Paint child with combined offset
            let child_layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(child_layer);
        }

        Box::new(container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_indexed_stack_new() {
        let stack = RenderIndexedStack::new(Some(0));
        assert_eq!(stack.index, Some(0));
        assert_eq!(stack.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_indexed_stack_with_alignment() {
        let stack = RenderIndexedStack::with_alignment(Some(1), Alignment::CENTER);
        assert_eq!(stack.index, Some(1));
        assert_eq!(stack.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_indexed_stack_default() {
        let stack = RenderIndexedStack::default();
        assert_eq!(stack.index, None);
    }

    #[test]
    fn test_render_indexed_stack_set_index() {
        let mut stack = RenderIndexedStack::new(Some(0));
        stack.set_index(Some(1));
        assert_eq!(stack.index, Some(1));
    }

    #[test]
    fn test_render_indexed_stack_set_alignment() {
        let mut stack = RenderIndexedStack::new(Some(0));
        stack.set_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment, Alignment::CENTER);
    }
}
