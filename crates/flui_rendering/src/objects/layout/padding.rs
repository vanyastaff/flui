//! RenderPadding - adds padding around a child
//!
//! Inflates child's constraints by the padding amount, then deflates the child's size.

use crate::{BoxConstraints, Offset, Size};
use flui_core::{DynRenderObject, ElementId};
use flui_core::cache::{layout_cache, LayoutCacheKey, LayoutResult};
use flui_types::layout::EdgeInsets;

/// RenderPadding - adds padding around a child
///
/// Similar to Flutter's RenderPadding. Takes a child and adds padding around it.
/// The padding is specified using EdgeInsets.
///
/// # Layout Algorithm
///
/// 1. Deflate incoming constraints by padding amount
/// 2. Layout child with deflated constraints
/// 3. Add padding back to child's size to get final size
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPadding;
/// use flui_types::layout::EdgeInsets;
///
/// let padding = EdgeInsets::all(10.0);
/// let mut render_padding = RenderPadding::new(padding);
/// ```
#[derive(Debug)]
pub struct RenderPadding {
    /// Element ID for cache invalidation
    element_id: Option<ElementId>,

    /// The padding to add around the child
    padding: EdgeInsets,

    /// The child render object
    child: Option<Box<dyn DynRenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderPadding {
    /// Create a new RenderPadding with the given padding
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            element_id: None,
            padding,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create RenderPadding with element ID for caching
    pub fn with_element_id(element_id: ElementId, padding: EdgeInsets) -> Self {
        Self {
            element_id: Some(element_id),
            padding,
            child: None,
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

    /// Set the padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        if self.padding != padding {
            self.padding = padding;
            self.mark_needs_layout();
        }
    }

    /// Get the padding
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Set the child
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Get a reference to the child
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }

    /// Perform layout with padding
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);

        if let Some(child) = &mut self.child {
            // Deflate constraints by padding (shrink available space)
            let inner_constraints = BoxConstraints::new(
                (constraints.min_width - self.padding.horizontal_total()).max(0.0),
                (constraints.max_width - self.padding.horizontal_total()).max(0.0),
                (constraints.min_height - self.padding.vertical_total()).max(0.0),
                (constraints.max_height - self.padding.vertical_total()).max(0.0),
            );

            // Layout child with deflated constraints
            let child_size = child.layout(inner_constraints);

            // Add padding back to child size to get final size
            self.size = self.padding.expand_size(child_size);

            // Constrain to original constraints
            self.size = constraints.constrain(self.size);
        } else {
            // No child - use minimum size (just padding)
            let min_size = self.padding.total_size();
            self.size = constraints.constrain(min_size);
        }

        self.size
    }
}

impl DynRenderObject for RenderPadding {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // ⚡ FAST PATH: Early return if layout not needed
        if !self.needs_layout_flag && self.constraints == Some(constraints) {
            return self.size;
        }

        // 🔍 GLOBAL CACHE: Check layout cache
        if let Some(element_id) = self.element_id {
            if !self.needs_layout_flag {
                let cache_key = LayoutCacheKey::new(element_id, constraints);

                if let Some(cached) = layout_cache().get(&cache_key) {
                    if !cached.needs_layout {
                        self.constraints = Some(constraints);
                        self.size = cached.size;
                        return cached.size;
                    }
                }
            }
        }

        // 🐌 COMPUTE LAYOUT: Perform actual padding layout
        self.needs_layout_flag = false;
        let size = self.perform_layout(constraints);

        // 💾 CACHE RESULT: Store for next time (if element_id set)
        if let Some(element_id) = self.element_id {
            let cache_key = LayoutCacheKey::new(element_id, constraints);
            layout_cache().insert(cache_key, LayoutResult::new(size));
        }

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // Paint child at offset + padding top-left
            let child_offset = offset + self.padding.top_left();
            child.paint(painter, child_offset);
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
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RenderBox;

    #[test]
    fn test_render_padding_new() {
        let padding = EdgeInsets::all(10.0);
        let render_padding = RenderPadding::new(padding);
        assert_eq!(render_padding.padding(), padding);
        assert!(render_padding.needs_layout());
    }

    #[test]
    fn test_render_padding_no_child() {
        let mut render_padding = RenderPadding::new(EdgeInsets::all(10.0));
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = render_padding.layout(constraints);

        // With no child and tight constraints, must satisfy constraints
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_padding_with_child() {
        let mut render_padding = RenderPadding::new(EdgeInsets::all(10.0));

        // Child that will take 50x50
        let child = Box::new(RenderBox::new());
        render_padding.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(100.0, 100.0));
        let size = render_padding.layout(constraints);

        // Child gets loose constraints deflated by padding: 0..80 x 0..80
        // RenderBox takes biggest: 80x80
        // Add padding back: 80 + 20 = 100x100
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_padding_symmetric() {
        let mut render_padding = RenderPadding::new(EdgeInsets::symmetric(20.0, 10.0));

        let child = Box::new(RenderBox::new());
        render_padding.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render_padding.layout(constraints);

        // Child should get constraints deflated by padding
        // Horizontal padding: 20 * 2 = 40
        // Vertical padding: 10 * 2 = 20
        // So child gets tight(60x80)
        if let Some(child) = render_padding.child() {
            assert_eq!(child.size(), Size::new(60.0, 80.0));
        }
    }

    #[test]
    fn test_render_padding_set_padding() {
        let mut render_padding = RenderPadding::new(EdgeInsets::all(10.0));

        render_padding.set_padding(EdgeInsets::all(20.0));
        assert_eq!(render_padding.padding(), EdgeInsets::all(20.0));
        assert!(render_padding.needs_layout());
    }

    #[test]
    fn test_render_padding_child_offset() {
        let mut render_padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 0.0, 0.0));

        let child = Box::new(RenderBox::new());
        render_padding.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render_padding.layout(constraints);

        // Child should be offset by padding.top_left() = (10, 20)
        // We can't directly test paint offset, but we can verify padding is applied
        assert_eq!(render_padding.padding().left, 10.0);
        assert_eq!(render_padding.padding().top, 20.0);
    }

    #[test]
    fn test_render_padding_visit_children() {
        let mut render_padding = RenderPadding::new(EdgeInsets::all(10.0));
        render_padding.set_child(Some(Box::new(RenderBox::new())));

        let mut count = 0;
        render_padding.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_padding_visit_children_no_child() {
        let render_padding = RenderPadding::new(EdgeInsets::all(10.0));

        let mut count = 0;
        render_padding.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }
}
