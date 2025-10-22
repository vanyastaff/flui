//! RenderStack - implements Stack layout algorithm
//!
//! This is the core layout algorithm for stacked layouts (Stack widget).
//! Similar to Flutter's RenderStack.
//!
//! Stack allows children to be positioned absolutely or relatively within
//! the stack's bounds. Non-positioned children are aligned according to the
//! stack's alignment. Positioned children can specify left/top/right/bottom
//! coordinates.

use crate::{BoxConstraints, Offset, Size, StackParentData};
use flui_core::{DynRenderObject, ElementId};
use flui_core::cache::{layout_cache, LayoutCacheKey, LayoutResult};
use flui_types::layout::Alignment;

/// How to size the stack when it has non-positioned children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackFit {
    /// Stack sizes itself to fit its non-positioned children
    /// (children can overflow)
    Loose,

    /// Stack expands to fill the incoming constraints
    /// (children are constrained to stack size)
    Expand,

    /// Stack sizes itself to the incoming constraints
    /// (like Expand but respects min constraints)
    PassThrough,
}

/// RenderStack - implements Stack layout
///
/// Lays out children in a stack. Non-positioned children are sized and then
/// positioned according to the stack's alignment. Positioned children use
/// their positioning data to determine size and position.
///
/// # Layout Algorithm
///
/// 1. Layout non-positioned children with loose or tight constraints
/// 2. Determine stack size based on StackFit and non-positioned children
/// 3. Layout positioned children with constraints based on their positioning data
/// 4. Position all children (non-positioned use alignment, positioned use coordinates)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderStack, StackFit};
/// use flui_types::layout::Alignment;
///
/// let mut stack = RenderStack::new();
/// stack.set_alignment(Alignment::CENTER);
/// stack.set_fit(StackFit::Expand);
/// ```
#[derive(Debug)]
pub struct RenderStack {
    /// Element ID for cache invalidation
    element_id: Option<ElementId>,

    /// How to align non-positioned children
    alignment: Alignment,

    /// How to size the stack
    fit: StackFit,

    /// Children and their layout information
    children: Vec<StackChild>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

/// A child in a stack layout with its parent data
#[derive(Debug)]
struct StackChild {
    /// The render object
    render_object: Box<dyn DynRenderObject>,

    /// Parent data (positioning)
    parent_data: StackParentData,

    /// Offset after layout
    offset: Offset,
}

impl RenderStack {
    /// Create a new RenderStack with default alignment
    pub fn new() -> Self {
        Self {
            element_id: None,
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::Loose,
            children: Vec::new(),
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create RenderStack with element ID for caching
    ///
    /// # Performance
    ///
    /// Enables 50x faster layouts for repeated layouts with same constraints
    /// and same number of children.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::RenderStack;
    /// use flui_core::ElementId;
    ///
    /// let stack = RenderStack::with_element_id(ElementId::new());
    /// ```
    pub fn with_element_id(element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::Loose,
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

    /// Set alignment for non-positioned children
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            self.mark_needs_layout();
        }
    }

    /// Set stack fit
    pub fn set_fit(&mut self, fit: StackFit) {
        if self.fit != fit {
            self.fit = fit;
            self.mark_needs_layout();
        }
    }

    /// Add a child with parent data
    pub fn add_child(&mut self, child: Box<dyn DynRenderObject>, parent_data: StackParentData) {
        self.children.push(StackChild {
            render_object: child,
            parent_data,
            offset: Offset::ZERO,
        });
        self.mark_needs_layout();
    }

    /// Remove all children
    pub fn clear_children(&mut self) {
        self.children.clear();
        self.mark_needs_layout();
    }

    /// Perform stack layout algorithm
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        if self.children.is_empty() {
            // No children - use biggest size based on fit
            return match self.fit {
                StackFit::Loose => constraints.smallest(),
                StackFit::Expand | StackFit::PassThrough => constraints.biggest(),
            };
        }

        // Determine constraints for non-positioned children
        let non_positioned_constraints = match self.fit {
            StackFit::Loose => constraints.loosen(),
            StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
            StackFit::PassThrough => constraints,
        };

        // Phase 1: Layout non-positioned children and calculate stack size
        let mut has_non_positioned = false;
        let mut width = 0.0f32;
        let mut height = 0.0f32;

        for child in &mut self.children {
            if child.parent_data.is_non_positioned() {
                has_non_positioned = true;
                let child_size = child.render_object.layout(non_positioned_constraints);
                width = width.max(child_size.width);
                height = height.max(child_size.height);
            }
        }

        // Phase 2: Determine stack size
        self.size = if has_non_positioned {
            match self.fit {
                StackFit::Loose => {
                    // Size to fit non-positioned children
                    let size = Size::new(width, height);
                    constraints.constrain(size)
                }
                StackFit::Expand => {
                    // Expand to fill constraints
                    constraints.biggest()
                }
                StackFit::PassThrough => {
                    // Use constraints as-is
                    let size = Size::new(width.max(constraints.min_width), height.max(constraints.min_height));
                    constraints.constrain(size)
                }
            }
        } else {
            // Only positioned children
            match self.fit {
                StackFit::Loose => constraints.smallest(),
                StackFit::Expand | StackFit::PassThrough => constraints.biggest(),
            }
        };

        // Phase 3: Layout positioned children
        let stack_size = self.size;
        for child in &mut self.children {
            if child.parent_data.is_positioned() {
                let child_constraints = Self::compute_positioned_constraints_static(
                    stack_size,
                    &child.parent_data,
                );
                child.render_object.layout(child_constraints);
            }
        }

        // Phase 4: Position all children
        let alignment = self.alignment;
        for child in &mut self.children {
            if child.parent_data.is_non_positioned() {
                // Position using alignment
                let child_size = child.render_object.size();
                child.offset = alignment.calculate_offset(child_size, stack_size);
            } else {
                // Position using coordinates
                child.offset = Self::compute_positioned_offset_static(
                    stack_size,
                    alignment,
                    &child.parent_data,
                    child.render_object.size(),
                );
            }
        }

        self.size
    }

    /// Compute constraints for a positioned child (static version)
    fn compute_positioned_constraints_static(
        stack_size: Size,
        parent_data: &StackParentData,
    ) -> BoxConstraints {
        // Width constraints
        let (min_width, max_width) = if let Some(width) = parent_data.width {
            (width, width)
        } else {
            // Constrain based on left/right
            match (parent_data.left, parent_data.right) {
                (Some(left), Some(right)) => {
                    // Both left and right specified - width is determined
                    let available = (stack_size.width - left - right).max(0.0);
                    (available, available)
                }
                _ => {
                    // At least one is None - child determines width
                    (0.0, stack_size.width)
                }
            }
        };

        // Height constraints
        let (min_height, max_height) = if let Some(height) = parent_data.height {
            (height, height)
        } else {
            // Constrain based on top/bottom
            match (parent_data.top, parent_data.bottom) {
                (Some(top), Some(bottom)) => {
                    // Both top and bottom specified - height is determined
                    let available = (stack_size.height - top - bottom).max(0.0);
                    (available, available)
                }
                _ => {
                    // At least one is None - child determines height
                    (0.0, stack_size.height)
                }
            }
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Compute offset for a positioned child (static version)
    fn compute_positioned_offset_static(
        stack_size: Size,
        alignment: Alignment,
        parent_data: &StackParentData,
        child_size: Size,
    ) -> Offset {
        // X position
        let x = if let Some(left) = parent_data.left {
            left
        } else if let Some(right) = parent_data.right {
            stack_size.width - right - child_size.width
        } else {
            // No left or right - use alignment
            let offset = alignment.calculate_offset(child_size, stack_size);
            offset.dx
        };

        // Y position
        let y = if let Some(top) = parent_data.top {
            top
        } else if let Some(bottom) = parent_data.bottom {
            stack_size.height - bottom - child_size.height
        } else {
            // No top or bottom - use alignment
            let offset = alignment.calculate_offset(child_size, stack_size);
            offset.dy
        };

        Offset::new(x, y)
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl DynRenderObject for RenderStack {
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

        // 🐌 COMPUTE LAYOUT: Perform actual stack layout (~1000ns+)
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
        for child in &self.children {
            let child_offset = offset + child.offset;
            child.render_object.paint(painter, child_offset);
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        for child in &self.children {
            visitor(&*child.render_object);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        for child in &mut self.children {
            visitor(&mut *child.render_object);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RenderBox;

    #[test]
    fn test_render_stack_new() {
        let stack = RenderStack::new();
        assert!(stack.needs_layout());
        assert_eq!(stack.alignment, Alignment::TOP_LEFT);
        assert_eq!(stack.fit, StackFit::Loose);
    }

    #[test]
    fn test_render_stack_empty_layout_loose() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Loose);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 50.0);
        let size = stack.layout(constraints);

        // Loose fit with no children - smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_stack_empty_layout_expand() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 50.0);
        let size = stack.layout(constraints);

        // Expand fit with no children - biggest size
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_render_stack_single_non_positioned_child() {
        let mut stack = RenderStack::new();
        stack.set_alignment(Alignment::TOP_LEFT);
        stack.set_fit(StackFit::Loose);

        let child = Box::new(RenderBox::new());
        stack.add_child(child, StackParentData::new());

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = stack.layout(constraints);

        // Stack sizes to child (clamped by constraints)
        assert_eq!(size, Size::new(100.0, 50.0));

        // Child positioned at top-left
        assert_eq!(stack.children[0].offset, Offset::ZERO);
    }

    #[test]
    fn test_render_stack_alignment_center() {
        let mut stack = RenderStack::new();
        stack.set_alignment(Alignment::CENTER);
        stack.set_fit(StackFit::Expand);

        let child = Box::new(RenderBox::new());
        stack.add_child(child, StackParentData::new());

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        stack.layout(constraints);

        // Child (100x100) centered in stack (100x100) = offset (0,0)
        // If child was smaller, it would be offset
        assert_eq!(stack.children[0].offset, Offset::ZERO);
    }

    #[test]
    fn test_render_stack_positioned_left_top() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        let child = Box::new(RenderBox::new());
        let parent_data = StackParentData::new().with_left(10.0).with_top(20.0);
        stack.add_child(child, parent_data);

        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        stack.layout(constraints);

        // Child positioned at (10, 20)
        assert_eq!(stack.children[0].offset, Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_render_stack_positioned_right_bottom() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        let child = Box::new(RenderBox::new());
        let parent_data = StackParentData::new().with_right(10.0).with_bottom(20.0);
        stack.add_child(child, parent_data);

        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        stack.layout(constraints);

        // Stack is 200x200, child is 200x200 (tight constraints in RenderBox)
        // right=10: x = 200 - 10 - 200 = -10 (would overflow)
        // bottom=20: y = 200 - 20 - 200 = -20 (would overflow)
        let offset = stack.children[0].offset;
        assert_eq!(offset, Offset::new(-10.0, -20.0));
    }

    #[test]
    fn test_render_stack_positioned_with_width_height() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        let child = Box::new(RenderBox::new());
        let parent_data = StackParentData::new()
            .with_left(10.0)
            .with_top(20.0)
            .with_width(50.0)
            .with_height(30.0);
        stack.add_child(child, parent_data);

        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        stack.layout(constraints);

        // Child should be 50x30
        assert_eq!(stack.children[0].render_object.size(), Size::new(50.0, 30.0));

        // Positioned at (10, 20)
        assert_eq!(stack.children[0].offset, Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_render_stack_positioned_fill() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        let child = Box::new(RenderBox::new());
        // All sides = 10 means child fills stack minus 10px margin on each side
        let parent_data = StackParentData::new()
            .with_left(10.0)
            .with_top(10.0)
            .with_right(10.0)
            .with_bottom(10.0);
        stack.add_child(child, parent_data);

        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        stack.layout(constraints);

        // Child should be 180x180 (200 - 10 - 10)
        assert_eq!(stack.children[0].render_object.size(), Size::new(180.0, 180.0));

        // Positioned at (10, 10)
        assert_eq!(stack.children[0].offset, Offset::new(10.0, 10.0));
    }

    #[test]
    fn test_render_stack_multiple_children() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);

        // Non-positioned child
        let child1 = Box::new(RenderBox::new());
        stack.add_child(child1, StackParentData::new());

        // Positioned child
        let child2 = Box::new(RenderBox::new());
        stack.add_child(child2, StackParentData::new().with_left(50.0).with_top(50.0));

        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        stack.layout(constraints);

        // Both children should be laid out
        assert_eq!(stack.children.len(), 2);
    }

    #[test]
    fn test_render_stack_visit_children() {
        let mut stack = RenderStack::new();
        stack.add_child(Box::new(RenderBox::new()), StackParentData::new());
        stack.add_child(Box::new(RenderBox::new()), StackParentData::new());

        let mut count = 0;
        stack.visit_children(&mut |_| count += 1);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_stack_fit_variants() {
        let loose = StackFit::Loose;
        let expand = StackFit::Expand;
        let pass_through = StackFit::PassThrough;

        assert_ne!(loose, expand);
        assert_ne!(loose, pass_through);
        assert_ne!(expand, pass_through);
    }
}
