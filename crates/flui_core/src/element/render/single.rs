//! SingleChildRenderObjectElement - for RenderObjects with one child
//!
//! A specialized element for RenderObjects that have exactly one child.
//! This is the proper Flutter architecture pattern for single-child widgets
//! like Padding, Opacity, Transform, etc.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{Element, ElementId, ElementTree, RenderObject, SingleChildRenderObjectWidget, Widget};

/// SingleChildRenderObjectElement manages RenderObjects with one child
///
/// This follows Flutter's architecture where each type of RenderObjectWidget
/// has a corresponding specialized Element type. This element handles:
/// - Creating and managing the RenderObject
/// - Managing a single child element
/// - Coordinating updates between widget, element, and render object
///
/// # Flutter Equivalent
///
/// This is the Rust equivalent of Flutter's `SingleChildRenderObjectElement`.
///
/// # Examples
///
/// ```rust,ignore
/// // Padding widget creates a SingleChildRenderObjectElement
/// impl Widget for Padding {
///     fn create_element(&self) -> Box<dyn Element> {
///         Box::new(SingleChildRenderObjectElement::new(self.clone()))
///     }
/// }
/// ```
pub struct SingleChildRenderObjectElement<W: SingleChildRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    render_object: Option<Box<dyn RenderObject>>,
    /// Child element ID (managed by ElementTree)
    child: Option<ElementId>,
    /// Reference to ElementTree for child management
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: SingleChildRenderObjectWidget> SingleChildRenderObjectElement<W> {
    /// Create new single child render object element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            render_object: None,
            child: None,
            tree: None,
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

    /// Get child element ID
    pub fn child_id(&self) -> Option<ElementId> {
        self.child
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

    /// Set child element ID (called by ElementTree after mounting)
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Take old child for rebuild (called by ElementTree during rebuild)
    pub(crate) fn take_old_child(&mut self) -> Option<ElementId> {
        self.child.take()
    }
}

impl<W: SingleChildRenderObjectWidget> fmt::Debug for SingleChildRenderObjectElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SingleChildRenderObjectElement")
            .field("id", &self.id)
            .field("widget_type", &std::any::type_name::<W>())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .field("child", &self.child)
            .finish()
    }
}

impl<W: SingleChildRenderObjectWidget> Element for SingleChildRenderObjectElement<W> {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }
        // Then clear render object
        self.render_object = None;
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        if let Ok(new_widget) = new_widget.downcast::<W>() {
            self.widget = *new_widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn Widget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object
        self.update_render_object();

        // Return child widget to be mounted/updated by ElementTree
        // Clone the child widget using dyn_clone::clone_box
        let child_widget = self.widget.child();
        vec![(self.id, dyn_clone::clone_box(child_widget), 0)]
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

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element)) {
        if let Some(child_id) = self.child {
            if let Some(tree) = &self.tree {
                let tree_guard = tree.read();
                if let Some(child_element) = tree_guard.get(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        if let Some(child_id) = self.child {
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                if let Some(child_element) = tree_guard.get_mut(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
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

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.take_old_child()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.set_child(child_id)
    }

    fn child_ids(&self) -> Vec<ElementId> {
        if let Some(child_id) = self.child {
            vec![child_id]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxConstraints, BuildContext, RenderObjectWidget, StatelessWidget};
    use flui_types::{EdgeInsets, Offset, Size};

    // Mock RenderObject for testing
    #[derive(Debug)]
    struct MockRenderPadding {
        size: Size,
        padding: EdgeInsets,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
    }

    impl MockRenderPadding {
        fn new(padding: EdgeInsets) -> Self {
            Self {
                size: Size::zero(),
                padding,
                needs_layout_flag: true,
                needs_paint_flag: true,
            }
        }

        fn set_padding(&mut self, padding: EdgeInsets) {
            self.padding = padding;
            self.needs_layout_flag = true;
        }
    }

    impl RenderObject for MockRenderPadding {
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

    // Mock child widget
    #[derive(Debug, Clone)]
    struct MockChildWidget;

    impl StatelessWidget for MockChildWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
            Box::new(MockChildWidget)
        }
    }

    // Mock parent widget (like Padding)
    #[derive(Debug, Clone)]
    struct MockPaddingWidget {
        padding: EdgeInsets,
        child: Box<dyn Widget>,
    }

    impl Widget for MockPaddingWidget {
        fn create_element(&self) -> Box<dyn Element> {
            Box::new(SingleChildRenderObjectElement::new(self.clone()))
        }
    }

    impl RenderObjectWidget for MockPaddingWidget {
        fn create_render_object(&self) -> Box<dyn RenderObject> {
            Box::new(MockRenderPadding::new(self.padding))
        }

        fn update_render_object(&self, render_object: &mut dyn RenderObject) {
            if let Some(padding) = render_object.downcast_mut::<MockRenderPadding>() {
                padding.set_padding(self.padding);
            }
        }
    }

    impl SingleChildRenderObjectWidget for MockPaddingWidget {
        fn child(&self) -> &dyn Widget {
            &*self.child
        }
    }

    #[test]
    fn test_single_child_element_new() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let element = SingleChildRenderObjectElement::new(widget);
        assert!(element.parent.is_none());
        assert!(element.dirty);
        assert!(element.render_object.is_none());
        assert!(element.child.is_none());
    }

    #[test]
    fn test_single_child_element_mount() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        assert!(element.dirty);
        assert!(element.render_object.is_some());
    }

    #[test]
    fn test_single_child_element_render_object_creation() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let render_object = element.render_object_ref().unwrap();
        let padding = render_object.downcast_ref::<MockRenderPadding>().unwrap();
        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_single_child_element_update() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let new_widget = MockPaddingWidget {
            padding: EdgeInsets::all(20.0),
            child: Box::new(MockChildWidget),
        };
        element.update(Box::new(new_widget));

        let render_object = element.render_object_ref().unwrap();
        let padding = render_object.downcast_ref::<MockRenderPadding>().unwrap();
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_single_child_element_rebuild_returns_child() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let children = element.rebuild();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, element.id());
        assert_eq!(children[0].2, 0); // slot
    }

    #[test]
    fn test_single_child_element_child_management() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);

        let child_id = ElementId::new();
        element.set_child(child_id);
        assert_eq!(element.child_id(), Some(child_id));

        let taken = element.take_old_child();
        assert_eq!(taken, Some(child_id));
        assert_eq!(element.child_id(), None);
    }

    #[test]
    fn test_single_child_element_child_ids() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);

        assert_eq!(element.children(), Vec::<ElementId>::new());

        let child_id = ElementId::new();
        element.set_child(child_id);
        assert_eq!(element.children(), vec![child_id]);
    }
}
