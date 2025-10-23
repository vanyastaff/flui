//! RenderIndexedStack - shows only one child by index

use flui_types::{Offset, Size, constraints::BoxConstraints, Alignment};
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// Data for RenderIndexedStack
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexedStackData {
    /// Index of child to display (None = show nothing)
    pub index: Option<usize>,
    /// How to align the selected child
    pub alignment: Alignment,
}

impl IndexedStackData {
    /// Create new indexed stack data
    pub fn new(index: Option<usize>) -> Self {
        Self {
            index,
            alignment: Alignment::TOP_LEFT,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(index: Option<usize>, alignment: Alignment) -> Self {
        Self { index, alignment }
    }
}

impl Default for IndexedStackData {
    fn default() -> Self {
        Self::new(None)
    }
}

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
/// use flui_rendering::{ContainerRenderBox, objects::layout::IndexedStackData};
///
/// let mut indexed_stack = ContainerRenderBox::new(IndexedStackData::new(Some(0)));
/// ```
pub type RenderIndexedStack = ContainerRenderBox<IndexedStackData>;

// ===== Public API =====

impl RenderIndexedStack {
    /// Get reference to type-specific data
    pub fn data(&self) -> &IndexedStackData {
        &self.data
    }

    /// Get mutable reference to type-specific data
    pub fn data_mut(&mut self) -> &mut IndexedStackData {
        &mut self.data
    }

    /// Get the current index
    pub fn index(&self) -> Option<usize> {
        self.data().index
    }

    /// Get the alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set new index
    pub fn set_index(&mut self, index: Option<usize>) {
        if self.data().index != index {
            self.data_mut().index = index;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderIndexedStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        if self.children.is_empty() {
            let size = constraints.smallest();
            self.state_mut().size = Some(size);
            self.clear_needs_layout();
            return size;
        }

        // Layout all children (to maintain their state)
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for child in &mut self.children {
            let child_size = child.layout(constraints);
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Size is the max of all children
        let size = Size::new(
            max_width.clamp(constraints.min_width, constraints.max_width),
            max_height.clamp(constraints.min_height, constraints.max_height),
        );

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Only paint the selected child
        if let Some(index) = self.data().index {
            if let Some(child) = self.children.get(index) {
                let size = self.state().size.unwrap_or(Size::ZERO);
                let alignment = self.data().alignment;
                let child_size = child.size();

                // Calculate aligned position
                let child_offset = alignment.calculate_offset(child_size, size);

                let paint_offset = Offset::new(
                    offset.dx + child_offset.dx,
                    offset.dy + child_offset.dy,
                );

                child.paint(painter, paint_offset);
            }
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexed_stack_data_new() {
        let data = IndexedStackData::new(Some(0));
        assert_eq!(data.index, Some(0));
        assert_eq!(data.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_indexed_stack_data_with_alignment() {
        let data = IndexedStackData::with_alignment(Some(1), Alignment::CENTER);
        assert_eq!(data.index, Some(1));
        assert_eq!(data.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_indexed_stack_data_default() {
        let data = IndexedStackData::default();
        assert_eq!(data.index, None);
    }

    #[test]
    fn test_render_indexed_stack_new() {
        let stack = ContainerRenderBox::new(IndexedStackData::new(Some(0)));
        assert_eq!(stack.index(), Some(0));
        assert_eq!(stack.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_indexed_stack_set_index() {
        let mut stack = ContainerRenderBox::new(IndexedStackData::new(Some(0)));

        // Clear initial needs_layout flag
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let _ = stack.layout(constraints);

        stack.set_index(Some(1));
        assert_eq!(stack.index(), Some(1));
        assert!(RenderBoxMixin::needs_paint(&stack));
        assert!(!RenderBoxMixin::needs_layout(&stack));
    }

    #[test]
    fn test_render_indexed_stack_set_alignment() {
        let mut stack = ContainerRenderBox::new(IndexedStackData::new(Some(0)));

        stack.set_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment(), Alignment::CENTER);
        assert!(RenderBoxMixin::needs_layout(&stack));
    }

    #[test]
    fn test_render_indexed_stack_layout_no_children() {
        let mut stack = ContainerRenderBox::new(IndexedStackData::new(Some(0)));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = stack.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
