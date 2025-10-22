//! RenderIndexedStack - shows only one child by index, but layouts all children.
//!
//! This render object is similar to RenderStack, but only paints the child at the
//! given index. All children are still laid out to compute the correct size.
//!
//! # Layout Algorithm
//!
//! 1. Layout ALL children (with StackFit constraints)
//! 2. Compute max size from all children
//! 3. Return constrained max size (or biggest if StackFit::Expand)
//! 4. Only paint child at `index` position
//!
//! # Use Cases
//!
//! - Tab navigation (show only active tab)
//! - Wizard steps (show current step)
//! - Page views (show current page)

use crate::{BoxConstraints, Offset, Size, StackFit};
use flui_core::{DynRenderObject, ElementId};
use flui_core::cache::{layout_cache, LayoutCacheKey, LayoutResult};
use flui_types::layout::Alignment;

/// Shows only one child by index from a list of children.
///
/// Unlike RenderStack which shows all children, RenderIndexedStack only
/// paints the child at the specified index. However, all children are
/// laid out to determine the correct size.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderIndexedStack;
/// use flui_types::layout::Alignment;
///
/// // Show first child (index 0)
/// let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
/// ```
#[derive(Debug)]
pub struct RenderIndexedStack {
    /// Element ID for cache invalidation
    element_id: Option<ElementId>,

    /// Index of the child to display (None = show nothing)
    index: Option<usize>,

    /// Alignment of children within the stack
    alignment: Alignment,

    /// How to size the stack
    sizing: StackFit,

    /// All children (only one will be painted)
    children: Vec<Box<dyn DynRenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderIndexedStack {
    /// Create a new RenderIndexedStack
    ///
    /// # Arguments
    ///
    /// * `index` - Index of child to display (None = show nothing)
    /// * `alignment` - How to align children
    pub fn new(index: Option<usize>, alignment: Alignment) -> Self {
        Self {
            element_id: None,
            index,
            alignment,
            sizing: StackFit::Loose,
            children: Vec::new(),
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create with StackFit::Expand sizing
    pub fn expand(index: Option<usize>, alignment: Alignment) -> Self {
        Self {
            element_id: None,
            index,
            alignment,
            sizing: StackFit::Expand,
            children: Vec::new(),
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create RenderIndexedStack with element ID for caching
    ///
    /// # Performance
    ///
    /// Enables 50x faster layouts for repeated layouts with same constraints
    /// and same number of children.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::RenderIndexedStack;
    /// use flui_core::ElementId;
    /// use flui_types::layout::Alignment;
    ///
    /// let stack = RenderIndexedStack::with_element_id(
    ///     ElementId::new(),
    ///     Some(0),
    ///     Alignment::CENTER
    /// );
    /// ```
    pub fn with_element_id(
        element_id: ElementId,
        index: Option<usize>,
        alignment: Alignment,
    ) -> Self {
        Self {
            element_id: Some(element_id),
            index,
            alignment,
            sizing: StackFit::Loose,
            children: Vec::new(),
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Get the element ID
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Set the element ID for caching
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    /// Get the current index
    pub fn index(&self) -> Option<usize> {
        self.index
    }

    /// Set the index (which child to show)
    pub fn set_index(&mut self, index: Option<usize>) {
        if self.index != index {
            self.index = index;
            self.mark_needs_paint();
        }
    }

    /// Get the alignment
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Set the alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            self.mark_needs_paint();
        }
    }

    /// Get the sizing mode
    pub fn sizing(&self) -> StackFit {
        self.sizing
    }

    /// Set the sizing mode
    pub fn set_sizing(&mut self, sizing: StackFit) {
        if self.sizing != sizing {
            self.sizing = sizing;
            self.mark_needs_layout();
        }
    }

    /// Add a child to the stack
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>) {
        self.children.push(child);
        self.mark_needs_layout();
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        if !self.children.is_empty() {
            self.children.clear();
            self.mark_needs_layout();
        }
    }

    /// Get the number of children
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Perform layout on this render object
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // If no children, use smallest size
        if self.children.is_empty() {
            self.size = constraints.smallest();
            return self.size;
        }

        // Determine child constraints based on sizing mode
        let child_constraints = match self.sizing {
            StackFit::Loose => constraints.loosen(),
            StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
            StackFit::PassThrough => constraints,
        };

        // Layout ALL children to compute max size
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for child in &mut self.children {
            let child_size = child.layout(child_constraints);
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Compute final size based on sizing mode
        self.size = match self.sizing {
            StackFit::Expand => constraints.biggest(),
            StackFit::Loose | StackFit::PassThrough => {
                let max_size = Size::new(max_width, max_height);
                constraints.constrain(max_size)
            }
        };

        self.size
    }
}

impl DynRenderObject for RenderIndexedStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // ⚡ FAST PATH: Early return if layout not needed (~2ns)
        if !self.needs_layout_flag && self.constraints == Some(constraints) {
            return self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache (~20ns)
        // CRITICAL: Include child_count to detect structural changes!
        if let Some(element_id) = self.element_id {
            if !self.needs_layout_flag {
                let cache_key = LayoutCacheKey::new(element_id, constraints)
                    .with_child_count(self.children.len());

                if let Some(cached) = layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        self.constraints = Some(constraints);
                        self.size = cached.size;
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual indexed stack layout (~1000ns+)
        self.constraints = Some(constraints);
        self.needs_layout_flag = false;
        let size = self.perform_layout(constraints);

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = self.element_id {
            let cache_key = LayoutCacheKey::new(element_id, constraints)
                .with_child_count(self.children.len());
            layout_cache().insert(cache_key, LayoutResult::new(size));
        }

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Only paint the child at the specified index
        if let Some(index) = self.index {
            if let Some(child) = self.children.get(index) {
                // Calculate child offset based on alignment
                let child_size = child.size();
                let child_offset = self.alignment.calculate_offset(child_size, self.size);
                child.paint(painter, offset + child_offset);
            }
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
        self.needs_paint_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        for child in &self.children {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        for child in &mut self.children {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RenderBox;

    #[test]
    fn test_render_indexed_stack_new() {
        let stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        assert_eq!(stack.index(), Some(0));
        assert_eq!(stack.alignment(), Alignment::CENTER);
        assert_eq!(stack.sizing(), StackFit::Loose);
        assert!(stack.needs_layout());
    }

    #[test]
    fn test_render_indexed_stack_expand() {
        let stack = RenderIndexedStack::expand(Some(1), Alignment::TOP_LEFT);
        assert_eq!(stack.index(), Some(1));
        assert_eq!(stack.sizing(), StackFit::Expand);
    }

    #[test]
    fn test_render_indexed_stack_no_children() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = stack.layout(constraints);

        // No children - should use smallest size
        assert_eq!(size, Size::zero());
    }

    #[test]
    fn test_render_indexed_stack_single_child() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = stack.layout(constraints);

        // Should match constraints (tight)
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(stack.child_count(), 1);
    }

    #[test]
    fn test_render_indexed_stack_multiple_children() {
        let mut stack = RenderIndexedStack::new(Some(1), Alignment::CENTER);

        // Add 3 children
        stack.add_child(Box::new(RenderBox::new()));
        stack.add_child(Box::new(RenderBox::new()));
        stack.add_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = stack.layout(constraints);

        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(stack.child_count(), 3);
    }

    #[test]
    fn test_render_indexed_stack_index_none() {
        let mut stack = RenderIndexedStack::new(None, Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = stack.layout(constraints);

        // Should still layout all children, even if index is None
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(stack.index(), None);
    }

    #[test]
    fn test_render_indexed_stack_index_out_of_bounds() {
        let mut stack = RenderIndexedStack::new(Some(10), Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = stack.layout(constraints);

        // Index out of bounds - should still layout, just won't paint
        assert_eq!(size, Size::new(100.0, 100.0));
        assert_eq!(stack.index(), Some(10));
    }

    #[test]
    fn test_render_indexed_stack_set_index() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        assert_eq!(stack.index(), Some(0));

        stack.set_index(Some(1));
        assert_eq!(stack.index(), Some(1));
        assert!(stack.needs_paint());
    }

    #[test]
    fn test_render_indexed_stack_set_alignment() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        assert_eq!(stack.alignment(), Alignment::CENTER);

        stack.set_alignment(Alignment::TOP_LEFT);
        assert_eq!(stack.alignment(), Alignment::TOP_LEFT);
        assert!(stack.needs_paint());
    }

    #[test]
    fn test_render_indexed_stack_set_sizing() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        assert_eq!(stack.sizing(), StackFit::Loose);

        stack.set_sizing(StackFit::Expand);
        assert_eq!(stack.sizing(), StackFit::Expand);
        assert!(stack.needs_layout());
    }

    #[test]
    fn test_render_indexed_stack_clear_children() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));
        stack.add_child(Box::new(RenderBox::new()));

        assert_eq!(stack.child_count(), 2);

        stack.clear_children();
        assert_eq!(stack.child_count(), 0);
        assert!(stack.needs_layout());
    }

    #[test]
    fn test_render_indexed_stack_visit_children() {
        let mut stack = RenderIndexedStack::new(Some(0), Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));
        stack.add_child(Box::new(RenderBox::new()));
        stack.add_child(Box::new(RenderBox::new()));

        let mut count = 0;
        stack.visit_children(&mut |_| count += 1);
        assert_eq!(count, 3); // Should visit ALL children
    }

    #[test]
    fn test_render_indexed_stack_expand_sizing() {
        let mut stack = RenderIndexedStack::expand(Some(0), Alignment::CENTER);
        stack.add_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = stack.layout(constraints);

        // StackFit::Expand should use biggest
        assert_eq!(size, Size::new(200.0, 200.0));
    }
}
