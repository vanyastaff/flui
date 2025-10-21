//! SingleChildRenderObjectElement for RenderObjects with one child

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{DynElement, Element, ElementId, ElementTree, SingleChildRenderObjectWidget};
use super::super::ElementLifecycle;
use crate::widget::DynWidget;
use crate::foundation::Key;

/// Element for RenderObjects with a single child (Padding, Opacity, Transform, etc.)
///
/// SingleChildRenderObjectElement is specialized for widgets that:
/// - Create a RenderObject for layout and painting
/// - Have exactly ONE child widget
/// - Apply transformations/constraints to their child (e.g., Padding, Opacity)
///
/// # Examples
///
/// ```rust,ignore
/// // Padding widget creates a SingleChildRenderObjectElement
/// let padding = Padding::all(10.0, child: Text::new("Hello"));
/// let element = padding.into_element(); // SingleChildRenderObjectElement<Padding>
/// ```
///
/// # See Also
///
/// - [`LeafRenderObjectElement`] - For widgets with no children
/// - [`MultiChildRenderObjectElement`] - For widgets with multiple children
pub struct SingleChildRenderObjectElement<W: SingleChildRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    render_object: Option<Box<dyn crate::DynRenderObject>>,
    /// Child element ID (managed by ElementTree)
    child: Option<ElementId>,
    /// Reference to ElementTree for child management
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: SingleChildRenderObjectWidget> SingleChildRenderObjectElement<W> {
    /// Creates a new single child render object element
    #[must_use]
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            render_object: None,
            child: None,
            tree: None,
        }
    }

    /// Returns the child element ID
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
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
            .field("lifecycle", &self.lifecycle)
            .field("has_render_object", &self.render_object.is_some())
            .field("child", &self.child)
            .finish()
    }
}

// ========== Implement DynElement for SingleChildRenderObjectElement ==========

impl<W: SingleChildRenderObjectWidget> DynElement for SingleChildRenderObjectElement<W> {
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
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }
        // Then clear render object
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

        // Return child widget to be mounted/updated by ElementTree
        // Clone the child widget using dyn_clone::clone_box
        let child_widget = self.widget.child();
        vec![(self.id, dyn_clone::clone_box(child_widget), 0)]
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
        // Note: child stays attached but inactive
        // Will be unmounted if not reactivated before frame end
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        // Element is being reinserted into tree (GlobalKey reparenting)
        self.dirty = true; // Mark for rebuild in new location
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.take_old_child()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.set_child(child_id)
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
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
        // Single child element doesn't need slot updates
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }
}

// ========== Implement Element for SingleChildRenderObjectElement (with associated types) ==========

impl<W: SingleChildRenderObjectWidget> Element for SingleChildRenderObjectElement<W> {
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
    use crate::{BoxConstraints, Context, RenderObjectWidget, StatelessWidget, Widget};
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

    impl crate::render::RenderObject for MockRenderPadding {
        type ParentData = ();
        type Child = Box<dyn crate::DynRenderObject>;

        fn parent_data(&self) -> Option<&Self::ParentData> {
            None
        }

        fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData> {
            None
        }
    }

    impl crate::DynRenderObject for MockRenderPadding {
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

    // Mock child widget
    #[derive(Debug, Clone)]
    struct MockChildWidget;

    impl StatelessWidget for MockChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(MockChildWidget)
        }
    }

    // Mock parent widget (like Padding)
    #[derive(Debug, Clone)]
    struct MockPaddingWidget {
        padding: EdgeInsets,
        child: Box<dyn DynWidget>,
    }

    impl Widget for MockPaddingWidget {
        type Element = SingleChildRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            SingleChildRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockPaddingWidget {
        fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
            Box::new(MockRenderPadding::new(self.padding))
        }

        fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
            if let Some(padding) = render_object.downcast_mut::<MockRenderPadding>() {
                padding.set_padding(self.padding);
            }
        }
    }

    impl SingleChildRenderObjectWidget for MockPaddingWidget {
        fn child(&self) -> &dyn DynWidget {
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
        assert_eq!(element.lifecycle, ElementLifecycle::Initial);
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
        assert_eq!(element.lifecycle, ElementLifecycle::Active);
    }

    #[test]
    fn test_single_child_element_render_object_creation() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let render_object = element.render_object().unwrap();
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
        Element::update(&mut element, new_widget);

        let render_object = element.render_object().unwrap();
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
        assert_eq!(element.child(), Some(child_id));

        let taken = element.take_old_child();
        assert_eq!(taken, Some(child_id));
        assert_eq!(element.child(), None);
    }

    #[test]
    fn test_single_child_element_children_iter() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);

        assert_eq!(element.children_iter().collect::<Vec<_>>(), Vec::<ElementId>::new());

        let child_id = ElementId::new();
        element.set_child(child_id);
        assert_eq!(element.children_iter().collect::<Vec<_>>(), vec![child_id]);
    }

    #[test]
    fn test_single_child_element_lifecycle_transitions() {
        let widget = MockPaddingWidget {
            padding: EdgeInsets::all(10.0),
            child: Box::new(MockChildWidget),
        };
        let mut element = SingleChildRenderObjectElement::new(widget);

        // Initial -> Active
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        element.mount(None, 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Active -> Inactive
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Inactive -> Active
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_dirty());

        // Active -> Defunct
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }
}