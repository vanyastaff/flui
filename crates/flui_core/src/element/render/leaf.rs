//! LeafRenderObjectElement for RenderObjects without children

use std::fmt;

use crate::{DynElement, Element, ElementId, LeafRenderObjectWidget};
use super::super::ElementLifecycle;
use crate::DynWidget;
use crate::foundation::Key;

/// Element for RenderObjects with no children (optimized for leaf nodes)
///
/// LeafRenderObjectElement is a specialized element type for widgets that:
/// - Create a RenderObject for layout and painting
/// - Have NO children (e.g., Text, Image, Icon)
/// - Are optimized for minimal memory overhead
///
/// # Examples
///
/// ```rust,ignore
/// // Text widget creates a LeafRenderObjectElement
/// let text = Text::new("Hello");
/// let element = text.into_element(); // LeafRenderObjectElement<Text>
/// ```
///
/// # See Also
///
/// - [`SingleChildRenderObjectElement`] - For widgets with one child
/// - [`MultiChildRenderObjectElement`] - For widgets with multiple children
pub struct LeafRenderObjectElement<W: LeafRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    render_object: Option<Box<dyn crate::DynRenderObject>>,
}

impl<W: LeafRenderObjectWidget> LeafRenderObjectElement<W> {
    /// Creates a new leaf render object element
    #[must_use]
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            render_object: None,
        }
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
            .field("lifecycle", &self.lifecycle)
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

// ========== Implement DynElement for LeafRenderObjectElement ==========

impl<W: LeafRenderObjectWidget> DynElement for LeafRenderObjectElement<W> {
    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        DynWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        // Leaf elements have no children to unmount
        self.render_object = None;
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        if let Ok(new_widget) = new_widget.downcast::<W>() {
            self.widget = *new_widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object
        self.update_render_object();

        // Leaf elements have no children
        Vec::new()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
        // Leaf elements have no children to deactivate
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        // Element is being reinserted into tree (GlobalKey reparenting)
        self.dirty = true; // Mark for rebuild in new location
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(std::iter::empty()) // No children
    }

    fn set_tree_ref(&mut self, _tree: std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        // Leaf elements don't need tree reference
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None // No children
    }

    fn set_child_after_mount(&mut self, _child_id: ElementId) {
        // No children
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn render_object(&self) -> Option<&dyn crate::DynRenderObject> {
        self.render_object.as_ref().map(|ro| ro.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject> {
        self.render_object.as_mut().map(|ro| ro.as_mut())
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // No children
    }

    fn forget_child(&mut self, _child_id: ElementId) {
        // No children
    }
}

// ========== Implement Element for LeafRenderObjectElement (with associated types) ==========

impl<W: LeafRenderObjectWidget> Element for LeafRenderObjectElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        // Zero-cost! No downcast needed!
        self.widget = new_widget;
        self.update_render_object();
        self.dirty = true;
    }

    fn widget(&self) -> &W {
        &self.widget
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

    impl crate::render::RenderObject for MockRenderText {
        type ParentData = ();
        type Child = ();

        fn parent_data(&self) -> Option<&Self::ParentData> {
            None
        }

        fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData> {
            None
        }
    }

    impl crate::DynRenderObject for MockRenderText {
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

        fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn crate::DynRenderObject)) {}

        fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn crate::DynRenderObject)) {}
    }

    // Mock leaf widget (like Text)
    #[derive(Debug, Clone)]
    struct MockTextWidget {
        text: String,
    }

    impl Widget for MockTextWidget {
        type Element = LeafRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            LeafRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockTextWidget {
        fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
            Box::new(MockRenderText::new(self.text.clone()))
        }

        fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
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
        assert_eq!(element.lifecycle, ElementLifecycle::Initial);
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
        assert_eq!(element.lifecycle, ElementLifecycle::Active);
    }

    #[test]
    fn test_leaf_element_render_object_creation() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);
        element.mount(None, 0);

        let render_object = element.render_object().unwrap();
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
        element.update(new_widget);

        let render_object = element.render_object().unwrap();
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
        assert_eq!(element.lifecycle, ElementLifecycle::Defunct);
    }

    #[test]
    fn test_leaf_element_no_children() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let element = LeafRenderObjectElement::new(widget);

        // Leaf elements have no children - verify via children_iter
        assert_eq!(element.children_iter().count(), 0);
    }

    #[test]
    fn test_leaf_element_lifecycle_transitions() {
        let widget = MockTextWidget {
            text: "Hello".to_string(),
        };
        let mut element = LeafRenderObjectElement::new(widget);

        // Initial -> Active
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        element.mount(None, 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Active -> Inactive
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Inactive -> Active (GlobalKey reparenting)
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_dirty()); // Marked dirty on activation

        // Active -> Defunct
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }
}