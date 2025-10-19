//! LeafRenderObjectElement - for RenderObjects without children
//!
//! A specialized element for RenderObjects that have no children.
//! This is the proper Flutter architecture pattern for leaf widgets
//! like Text, Image, or custom painters.

use std::any::Any;
use std::fmt;

use crate::{Element, ElementId, LeafRenderObjectWidget, RenderObject};

/// LeafRenderObjectElement manages RenderObjects with no children
///
/// This follows Flutter's architecture where each type of RenderObjectWidget
/// has a corresponding specialized Element type. This element handles:
/// - Creating and managing the RenderObject
/// - Optimized for leaf nodes (no child management overhead)
/// - Coordinating updates between widget and render object
///
/// # Flutter Equivalent
///
/// This is the Rust equivalent of Flutter's `LeafRenderObjectElement`.
///
/// # Examples
///
/// ```rust,ignore
/// // Text widget creates a LeafRenderObjectElement
/// impl Widget for Text {
///     fn create_element(&self) -> Box<dyn Element> {
///         Box::new(LeafRenderObjectElement::new(self.clone()))
///     }
/// }
/// ```
pub struct LeafRenderObjectElement<W: LeafRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    render_object: Option<Box<dyn RenderObject>>,
}

impl<W: LeafRenderObjectWidget> LeafRenderObjectElement<W> {
    /// Create new leaf render object element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            render_object: None,
        }
    }

    /// Get reference to the render object
    pub fn render_object_ref(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable reference to the render object
    pub fn render_object_mut_ref(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object.as_mut().map(|r| r.as_mut())
    }

    /// Initialize the render object
    fn initialize_render_object(&mut self) {
        if self.render_object.is_none() {
            self.render_object = Some(self.widget.create_render_object());
        }
    }

    /// Update the render object with new widget configuration
    fn update_render_object(&mut self) {
        if let Some(render_object) = &mut self.render_object {
            self.widget.update_render_object(render_object.as_mut());
        }
    }
}

impl<W: LeafRenderObjectWidget> fmt::Debug for LeafRenderObjectElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeafRenderObjectElement")
            .field("id", &self.id)
            .field("widget_type", &std::any::type_name::<W>())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

impl<W: LeafRenderObjectWidget> Element for LeafRenderObjectElement<W> {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Leaf elements have no children to unmount
        self.render_object = None;
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        if let Ok(new_widget) = new_widget.downcast::<W>() {
            self.widget = *new_widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object
        self.update_render_object();

        // Leaf elements have no children
        Vec::new()
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn flui_foundation::Key> {
        self.widget.key()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|ro| ro.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object.as_mut().map(|ro| ro.as_mut())
    }

    // Leaf elements never have children, so these methods do nothing
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn Element)) {}
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn Element)) {}
    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None
    }
    fn set_child_after_mount(&mut self, _child_id: ElementId) {}
    fn child_ids(&self) -> Vec<ElementId> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxConstraints, RenderObjectWidget, Widget};
    use flui_types::{Offset, Size};

    // Mock RenderObject for testing
    #[derive(Debug)]
    struct MockRenderText {
        size: Size,
        text: String,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
    }

    impl MockRenderText {
        fn new(text: String) -> Self {
            Self {
                size: Size::zero(),
                text,
                needs_layout_flag: true,
                needs_paint_flag: true,
            }
        }

        fn set_text(&mut self, text: String) {
            self.text = text;
            self.needs_layout_flag = true;
        }
    }

    impl RenderObject for MockRenderText {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = constraints.smallest();
            self.needs_layout_flag = false;
            self.size
        }

        fn paint(&self, _painter: &egui::Painter, _offset: Offset) {}

        fn size(&self) -> Size {
            self.size
        }

        fn constraints(&self) -> Option<BoxConstraints> {
            None
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

        fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {}

        fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {}
    }

    // Mock leaf widget (like Text)
    #[derive(Debug, Clone)]
    struct MockTextWidget {
        text: String,
    }

    impl Widget for MockTextWidget {
        fn create_element(&self) -> Box<dyn Element> {
            Box::new(LeafRenderObjectElement::new(self.clone()))
        }
    }

    impl RenderObjectWidget for MockTextWidget {
        fn create_render_object(&self) -> Box<dyn RenderObject> {
            Box::new(MockRenderText::new(self.text.clone()))
        }

        fn update_render_object(&self, render_object: &mut dyn RenderObject) {
            if let Some(text) = render_object.downcast_mut::<MockRenderText>() {
                text.set_text(self.text.clone());
            }
        }
    }

    impl LeafRenderObjectWidget for MockTextWidget {}

    #[test]
    fn test_leaf_element_new() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let element = LeafRenderObjectElement::new(widget);
        assert!(element.parent.is_none());
        assert!(element.dirty);
        assert!(element.render_object.is_none());
    }

    #[test]
    fn test_leaf_element_mount() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);

        assert!(element.dirty);
        assert!(element.render_object.is_some());
    }

    #[test]
    fn test_leaf_element_render_object_creation() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);

        let render_object = element.render_object_ref().unwrap();
        let text = render_object.downcast_ref::<MockRenderText>().unwrap();
        assert_eq!(text.text, "Hello");
    }

    #[test]
    fn test_leaf_element_update() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);

        let new_widget = MockTextWidget {
            text: "World".to_string(),
        };
        element.update(Box::new(new_widget));

        let render_object = element.render_object_ref().unwrap();
        let text = render_object.downcast_ref::<MockRenderText>().unwrap();
        assert_eq!(text.text, "World");
    }

    #[test]
    fn test_leaf_element_rebuild_no_children() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);

        let children = element.rebuild();
        assert_eq!(children.len(), 0); // Leaf elements have no children
    }

    #[test]
    fn test_leaf_element_unmount() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);
        assert!(element.render_object.is_some());

        element.unmount();
        assert!(element.render_object.is_none());
    }

    #[test]
    fn test_leaf_element_no_children() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);

        assert_eq!(element.children(), Vec::<ElementId>::new());
        assert_eq!(element.take_old_child_for_rebuild(), None);
    }

    #[test]
    fn test_leaf_element_visit_children_noop() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);

        let mut visited = false;
        element.walk_children(&mut |_| {
            visited = true;
        });
        assert!(!visited);

        element.walk_children_mut(&mut |_| {
            visited = true;
        });
        assert!(!visited);
    }
}
