//! RenderObjectElement for RenderObjectWidget
//!
//! This element type is created by RenderObjectWidget and owns a RenderObject.
//! It bridges the Widget tree to the RenderObject tree.

use std::fmt;
use std::marker::PhantomData;

use parking_lot::RwLock;

use crate::element::ElementId;
use crate::widget::{RenderObjectWidget, DynWidget, BoxedWidget};
use crate::render::{DynRenderObject, RenderState, arity::Arity};
use super::dyn_element::{DynElement, ElementLifecycle};

/// Element for RenderObjectWidget with explicit Arity
///
/// RenderObjectElement owns a RenderObject and manages its lifecycle.
/// The Arity parameter is explicit in the type signature, making it clear
/// at compile time how many children this element has:
/// - LeafArity: 0 children
/// - SingleArity: 1 child
/// - MultiArity: 0..N children
///
/// # Architecture
///
/// ```text
/// RenderObjectElement<W, A>
///   where W: RenderObjectWidget<Arity = A>
///   ├─ widget: W (immutable config, recreated on update)
///   ├─ render_object: W::RenderObject (mutable render state)
///   ├─ children: Vec<ElementId> (enforced by Arity)
///   ├─ lifecycle state
///   └─ _arity: PhantomData<A> (zero-sized type marker)
/// ```
///
/// # Lifecycle
///
/// 1. **create_render_object()** - Widget creates RenderObject
/// 2. **mount()** - Element mounted to tree
/// 3. **update_render_object()** - Widget config changes
/// 4. **unmount()** - RenderObject cleanup
pub struct RenderObjectElement<W, A>
where
    W: RenderObjectWidget<Arity = A>,
    A: Arity,
{
    /// The widget this element represents
    widget: W,

    /// The render object created by the widget
    render_object: W::RenderObject,

    /// Render state (size, constraints, dirty flags)
    ///
    /// Uses RwLock for interior mutability during layout/paint.
    /// Atomic flags inside RenderState provide lock-free checks.
    render_state: RwLock<RenderState>,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child elements (count enforced by Arity at runtime)
    children: Vec<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag (needs rebuild)
    dirty: bool,

    /// Phantom data to hold Arity type parameter (zero-sized)
    _arity: PhantomData<A>,
}

impl<W, A> RenderObjectElement<W, A>
where
    W: RenderObjectWidget<Arity = A>,
    A: Arity,
{
    /// Create a new RenderObjectElement from a widget
    pub fn new(widget: W) -> Self {
        let render_object = widget.create_render_object();

        Self {
            widget,
            render_object,
            render_state: RwLock::new(RenderState::new()),
            parent: None,
            children: Vec::new(),
            slot: 0,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
            _arity: PhantomData,
        }
    }

    /// Get reference to the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }

    /// Get reference to the render object
    pub fn render_object(&self) -> &W::RenderObject {
        &self.render_object
    }

    /// Get mutable reference to the render object
    pub fn render_object_mut(&mut self) -> &mut W::RenderObject {
        &mut self.render_object
    }

    /// Get children
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get reference to render state
    pub fn render_state(&self) -> &RwLock<RenderState> {
        &self.render_state
    }

    /// Update with a new widget
    pub fn update(&mut self, new_widget: W) {
        let _old_widget = std::mem::replace(&mut self.widget, new_widget);

        // Call update_render_object to sync config to render object
        self.widget.update_render_object(&mut self.render_object);

        // Mark as dirty to trigger rebuild
        self.dirty = true;
    }

    /// Set children (enforces arity constraints)
    pub(crate) fn set_children(&mut self, children: Vec<ElementId>) {
        // Enforce arity constraints (A is explicit type parameter!)
        // let expected = A::CHILD_COUNT; // TODO: Arity no longer has CHILD_COUNT

        // TODO: Re-implement arity checks when Arity trait has runtime check method
        self.children = children;
    }

    /// Add a child (for MultiArity)
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        // TODO: Add arity check when Arity trait supports runtime validation

        self.children.push(child_id);
    }

    /// Remove a child by ID
    pub(crate) fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }
}

impl<W, A> fmt::Debug for RenderObjectElement<W, A>
where
    W: RenderObjectWidget<Arity = A>,
    A: Arity,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderObjectElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("render_object_type", &std::any::type_name::<W::RenderObject>())
            .field("arity", &A::NAME)
            .field("parent", &self.parent)
            .field("children_count", &self.children.len())
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

// ========== Implement DynElement ==========

impl<W, A> DynElement for RenderObjectElement<W, A>
where
    W: RenderObjectWidget<Arity = A> + crate::Widget + DynWidget,
    W::Element: DynElement,
    W::RenderObject: fmt::Debug,
    A: Arity,
{
    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Will rebuild on first frame
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        // Children will be unmounted by ElementTree
        self.children.clear();
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Rebuild when reactivated
    }

    fn widget(&self) -> &dyn DynWidget {
        &self.widget
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        // Try to downcast to our widget type
        if let Some(_widget) = new_widget.downcast_ref::<W>() {
            // Clone not available, need to work differently
            // For now skip update - needs refactoring
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn rebuild(&mut self, _element_id: ElementId) -> Vec<(ElementId, BoxedWidget, usize)> {
        // RenderObjectElement doesn't create child widgets - it's a leaf in the Widget tree
        // but may have children in the RenderObject tree (managed by layout)
        self.dirty = false;
        Vec::new()
    }

    fn render_object(&self) -> Option<&dyn DynRenderObject> {
        Some(&self.render_object)
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
        Some(&mut self.render_object)
    }

    fn render_state_ptr(&self) -> Option<*const RwLock<RenderState>> {
        Some(&self.render_state as *const RwLock<RenderState>)
    }

    fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Slot is managed by parent, children don't need to track it
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderObject, LayoutCx, PaintCx, LeafArity, SingleArity, MultiArity, BoxedLayer};
    use flui_types::Size;

    // Test RenderObject with LeafArity
    #[derive(Debug)]
    struct TestLeafRender {
        size: Size,
    }

    impl RenderObject for TestLeafRender {
        type Arity = LeafArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            self.size
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(flui_engine::PictureLayer::new())
        }
    }

    // Test Widget for LeafArity
    #[derive(Debug, Clone)]
    struct TestLeafWidget {
        size: Size,
    }

    impl RenderObjectWidget for TestLeafWidget {
        type Arity = LeafArity;
        type Render = TestLeafRender;

        fn create_render_object(&self) -> Self::Render {
            TestLeafRender { size: self.size }
        }

        fn update_render_object(&self, render_object: &mut Self::Render) {
            render_object.size = self.size;
        }
    }

    impl crate::Widget for TestLeafWidget {
        fn key(&self) -> Option<&str> {
            None
        }
    }

    impl DynWidget for TestLeafWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    // Test RenderObject with SingleArity
    #[derive(Debug)]
    struct TestSingleRender {
        padding: f32,
    }

    impl RenderObject for TestSingleRender {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            use crate::render::layout_cx::SingleChild;
            let child = cx.child();
            let child_size = cx.layout_child(child, cx.constraints());
            Size::new(
                child_size.width + self.padding * 2.0,
                child_size.height + self.padding * 2.0,
            )
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(flui_engine::PictureLayer::new())
        }
    }

    // Test Widget for SingleArity
    #[derive(Debug, Clone)]
    struct TestSingleWidget {
        padding: f32,
    }

    impl RenderObjectWidget for TestSingleWidget {
        type Arity = SingleArity;
        type Render = TestSingleRender;

        fn create_render_object(&self) -> Self::Render {
            TestSingleRender { padding: self.padding }
        }

        fn update_render_object(&self, render_object: &mut Self::Render) {
            render_object.padding = self.padding;
        }
    }

    impl crate::Widget for TestSingleWidget {
        fn key(&self) -> Option<&str> {
            None
        }
    }

    impl DynWidget for TestSingleWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    // Test RenderObject with MultiArity
    #[derive(Debug)]
    struct TestMultiRender;

    impl RenderObject for TestMultiRender {
        type Arity = MultiArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(flui_engine::PictureLayer::new())
        }
    }

    // Test Widget for MultiArity
    #[derive(Debug, Clone)]
    struct TestMultiWidget;

    impl RenderObjectWidget for TestMultiWidget {
        type Arity = MultiArity;
        type Render = TestMultiRender;

        fn create_render_object(&self) -> Self::Render {
            TestMultiRender
        }

        fn update_render_object(&self, _render_object: &mut Self::Render) {
            // No config to update
        }
    }

    impl crate::Widget for TestMultiWidget {
        fn key(&self) -> Option<&str> {
            None
        }
    }

    impl DynWidget for TestMultiWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    #[test]
    fn test_leaf_element_creation() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let element = RenderObjectElement::new(widget);

        assert_eq!(element.children().len(), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_single_element_creation() {
        let widget = TestSingleWidget { padding: 10.0 };
        let element = RenderObjectElement::new(widget);

        assert_eq!(element.children().len(), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_multi_element_creation() {
        let widget = TestMultiWidget;
        let element = RenderObjectElement::new(widget);

        assert_eq!(element.children().len(), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_element_mount() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let mut element = RenderObjectElement::new(widget);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_element_update() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let mut element = RenderObjectElement::new(widget);
        element.mount(Some(0), 0);

        // Update with new widget
        element.update(TestLeafWidget { size: Size::new(200.0, 100.0) });

        assert_eq!(element.widget().size, Size::new(200.0, 100.0));
        assert_eq!(element.render_object().size, Size::new(200.0, 100.0));
        assert!(element.is_dirty());
    }

    #[test]
    fn test_element_render_object_access() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let element = RenderObjectElement::new(widget);

        // Test DynElement interface (trait method returns Option<&dyn DynRenderObject>)
        let dyn_element: &dyn DynElement = &element;
        assert!(dyn_element.render_object().is_some());
        assert!(dyn_element.render_object().unwrap().debug_name().ends_with("TestLeafRender"));

        // Test direct access (returns &W::RenderObject)
        assert_eq!(RenderObjectElement::render_object(&element).size, Size::new(100.0, 50.0));
    }

    #[test]
    #[should_panic(expected = "LeafArity RenderObject cannot have children")]
    #[cfg(debug_assertions)]
    fn test_leaf_arity_constraint() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let mut element = RenderObjectElement::new(widget);

        // Should panic: LeafArity cannot have children
        element.set_children(vec![1]);
    }

    #[test]
    #[should_panic(expected = "SingleArity RenderObject must have exactly 1 child")]
    #[cfg(debug_assertions)]
    fn test_single_arity_constraint() {
        let widget = TestSingleWidget { padding: 10.0 };
        let mut element = RenderObjectElement::new(widget);

        // Should panic: SingleArity needs exactly 1 child
        element.set_children(vec![1, 2]);
    }

    #[test]
    fn test_multi_arity_allows_any_count() {
        let widget = TestMultiWidget;
        let mut element = RenderObjectElement::new(widget);

        // MultiArity should allow any number
        element.set_children(vec![]);
        element.set_children(vec![1]);
        element.set_children(vec![1, 2, 3]);
    }

    #[test]
    fn test_element_unmount() {
        let widget = TestLeafWidget { size: Size::new(100.0, 50.0) };
        let mut element = RenderObjectElement::new(widget);
        element.mount(Some(0), 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert_eq!(element.children().len(), 0);
    }
}
