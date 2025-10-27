//! RenderElement for RenderObjectWidget
//!
//! This element type is created by RenderObjectWidget and owns a RenderObject.
//! It bridges the Widget tree to the RenderObject tree.


use parking_lot::RwLock;

use crate::element::ElementId;
use crate::widget::{DynWidget, BoxedWidget};
use crate::render::{DynRenderObject, RenderState};
use super::dyn_element::ElementLifecycle;

/// Element for RenderObjectWidget (type-erased)
///
/// RenderElement owns a RenderObject and manages its lifecycle.
/// Both the widget and render object are type-erased to enable storage
/// in the `enum Element` without generic parameters.
///
/// # Architecture
///
/// ```text
/// RenderElement
///   ├─ widget: Box<dyn DynWidget> (type-erased RenderObjectWidget)
///   ├─ render_object: Box<dyn DynRenderObject> (type-erased RenderObject)
///   ├─ render_state: RwLock<RenderState> (size, constraints, dirty flags)
///   ├─ children: Vec<ElementId> (managed by RenderObject arity)
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Unlike the old generic `RenderObjectElement<W, A>`, this version uses type erasure
/// for both widget and render object:
///
/// - **Widget**: `Box<dyn DynWidget>` (user-extensible, unbounded types)
/// - **RenderObject**: `Box<dyn DynRenderObject>` (user-extensible, unbounded types)
/// - **Arity**: Runtime information via DynRenderObject trait
///
/// # Performance
///
/// RenderObject is Box<dyn>, but this is acceptable because:
/// - Layout/paint operations use interior mutability (RwLock)
/// - Hot path (layout) uses trait methods, not enum dispatch
/// - Element enum provides fast dispatch for element operations
///
/// # Lifecycle
///
/// 1. **create_render_object()** - Widget creates RenderObject
/// 2. **mount()** - Element mounted to tree
/// 3. **update_render_object()** - Widget config changes
/// 4. **layout()** - RenderObject layout pass
/// 5. **paint()** - RenderObject paint pass
/// 6. **unmount()** - RenderObject cleanup
#[derive(Debug)]
pub struct RenderElement {
    /// The widget this element represents (type-erased)
    widget: BoxedWidget,

    /// The render object created by the widget (type-erased)
    render_object: Box<dyn DynRenderObject>,

    /// Render state (size, constraints, dirty flags)
    ///
    /// Uses RwLock for interior mutability during layout/paint.
    /// Atomic flags inside RenderState provide lock-free checks.
    render_state: RwLock<RenderState>,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child elements (count enforced by RenderObject arity at runtime)
    children: Vec<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag (needs rebuild)
    dirty: bool,
}

impl RenderElement {
    /// Create a new RenderElement from a widget and render object
    ///
    /// # Parameters
    ///
    /// - `widget` - Type-erased RenderObjectWidget
    /// - `render_object` - Type-erased RenderObject created by widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Box::new(Container::new());
    /// let render = widget.create_render_object();
    /// let element = RenderElement::new(widget, Box::new(render));
    /// ```
    pub fn new(widget: BoxedWidget, render_object: Box<dyn DynRenderObject>) -> Self {
        Self {
            widget,
            render_object,
            render_state: RwLock::new(RenderState::new()),
            parent: None,
            children: Vec::new(),
            slot: 0,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
        }
    }

    /// Get reference to the widget (as DynWidget trait object)
    ///
    /// Following Rust API Guidelines - no `get_` prefix for getters.
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &dyn DynWidget {
        &*self.widget
    }

    /// Get reference to the render object
    #[inline]
    #[must_use]
    pub fn render_object(&self) -> &dyn DynRenderObject {
        &*self.render_object
    }

    /// Get mutable reference to the render object
    #[inline]
    #[must_use]
    pub fn render_object_mut(&mut self) -> &mut dyn DynRenderObject {
        &mut *self.render_object
    }

    /// Get children element IDs
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get reference to render state
    ///
    /// RenderState contains size, constraints, and dirty flags.
    /// Uses RwLock for interior mutability during layout/paint.
    #[inline]
    #[must_use]
    pub fn render_state(&self) -> &RwLock<RenderState> {
        &self.render_state
    }

    /// Update with a new widget
    ///
    /// The new widget must be compatible (same type and key) with the current widget.
    pub fn update(&mut self, new_widget: BoxedWidget) {
        self.widget = new_widget;

        // TODO: Call update_render_object to sync config to render object
        // This requires calling a method on the type-erased widget
        // For now, mark dirty to trigger rebuild

        self.dirty = true;
    }

    /// Set children (enforces arity constraints at runtime)
    pub(crate) fn set_children(&mut self, children: Vec<ElementId>) {
        // TODO: Enforce arity constraints via DynRenderObject::arity() method
        self.children = children;
    }

    /// Add a child (for MultiArity)
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        // TODO: Add arity check via DynRenderObject::arity() method
        self.children.push(child_id);
    }

    /// Remove a child by ID
    pub(crate) fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    // ========== DynElement-like Interface ==========
    //
    // These methods match the DynElement trait and are called by Element enum.
    // Following API Guidelines: is_* for predicates, no get_* prefix.

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get iterator over child element IDs
    #[inline]
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    /// Mount element to tree
    ///
    /// Sets parent, slot, and transitions to Active lifecycle state.
    /// Marks element as dirty to trigger initial build.
    pub fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Will rebuild on first frame
    }

    /// Unmount element from tree
    ///
    /// Transitions to Defunct lifecycle state and clears children.
    /// The children will be unmounted by ElementTree separately.
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        // Children will be unmounted by ElementTree
        self.children.clear();
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    /// Activate element
    ///
    /// Called when element is reactivated. Marks dirty to trigger rebuild.
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Rebuild when reactivated
    }

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: is_* prefix for boolean predicates.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Perform rebuild
    ///
    /// RenderElement doesn't create child widgets - it's a leaf in the Widget tree
    /// but may have children in the RenderObject tree (managed by layout).
    ///
    /// # Returns
    ///
    /// Always returns empty vec as RenderObjectWidget doesn't have widget children.
    pub fn rebuild(
        &mut self,
        _element_id: ElementId,
        _tree: std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> Vec<(ElementId, BoxedWidget, usize)> {
        self.dirty = false;
        Vec::new()
    }

    /// Forget child element
    ///
    /// Called by ElementTree when child is being removed.
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Update slot for child
    ///
    /// Slot is managed by parent, children don't need to track it.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Slot is managed by parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderObject, LayoutCx, PaintCx, LeafArity, BoxedLayer};
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

    impl crate::DynWidget for TestLeafWidget {
        // Minimal implementation for testing
    }

    #[test]
    fn test_render_element_creation() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let element = RenderElement::new(widget, render);

        assert_eq!(element.children().len(), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_mount() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_render_element_update() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);
        element.mount(Some(0), 0);

        // Update with new widget
        let new_widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(200.0, 100.0) });
        element.update(new_widget);

        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_unmount() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);
        element.mount(Some(0), 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert_eq!(element.children().len(), 0);
    }

    #[test]
    fn test_render_element_lifecycle() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);

        // Initial
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        // Mount → Active
        element.mount(Some(0), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Deactivate → Inactive
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Activate → Active
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_dirty()); // Should mark dirty on activate

        // Unmount → Defunct
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_render_element_dirty_flag() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);

        // Initially dirty
        assert!(element.is_dirty());

        // Rebuild clears dirty
        element.rebuild(1);
        assert!(!element.is_dirty());

        // Mark dirty
        element.mark_dirty();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_children_management() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let mut element = RenderElement::new(widget, render);

        // No children initially
        assert_eq!(element.children().len(), 0);

        // Set children
        element.set_children(vec![1, 2, 3]);
        assert_eq!(element.children().len(), 3);

        // Add child
        element.add_child(4);
        assert_eq!(element.children().len(), 4);

        // Remove child
        element.remove_child(2);
        assert_eq!(element.children().len(), 3);
        assert_eq!(element.children(), &[1, 3, 4]);

        // Forget child
        element.forget_child(3);
        assert_eq!(element.children().len(), 2);
        assert_eq!(element.children(), &[1, 4]);
    }

    #[test]
    fn test_render_element_render_object_access() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let element = RenderElement::new(widget, render);

        // Test render object access
        let render_obj = element.render_object();
        assert!(render_obj.debug_name().contains("TestLeafRender"));
    }

    #[test]
    fn test_render_element_render_state_access() {
        let widget: BoxedWidget = Box::new(TestLeafWidget { size: Size::new(100.0, 50.0) });
        let render: Box<dyn DynRenderObject> = Box::new(TestLeafRender { size: Size::new(100.0, 50.0) });
        let element = RenderElement::new(widget, render);

        // Test render state access
        let render_state = element.render_state();
        let state = render_state.read();

        // RenderState should be initialized with default values
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
    }
}
