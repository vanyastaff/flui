//! RenderElement for RenderWidget
//!
//! This element type is created by RenderWidget and owns a Render.
//! It bridges the Widget tree to the Render tree.

use parking_lot::RwLock;
use std::cell::RefCell;

use super::dyn_element::ElementLifecycle;
use crate::element::ElementId;
use crate::render::{RenderNode, RenderState};
use crate::widget::{BoxedWidget, DynWidget};

/// Element for RenderWidget (type-erased)
///
/// RenderElement owns a Render and manages its lifecycle.
/// Both the widget and render object are type-erased to enable storage
/// in the `enum Element` without generic parameters.
///
/// # Architecture
///
/// ```text
/// RenderElement
///   ├─ widget: Box<dyn DynWidget> (type-erased RenderWidget)
///   ├─ render_object: RenderNode (type-erased Render)
///   ├─ render_state: RwLock<RenderState> (size, constraints, dirty flags)
///   ├─ parent_data: Option<Box<dyn ParentData>> (metadata from parent)
///   ├─ children: Vec<ElementId> (managed by Render arity)
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Unlike the old generic `RenderElement<W, A>`, this version uses type erasure
/// for both widget and render object:
///
/// - **Widget**: `Box<dyn DynWidget>` (user-extensible, unbounded types)
/// - **Render**: `RenderNode` (user-extensible, unbounded types)
/// - **Arity**: Runtime information via RenderNode trait
///
/// # Performance
///
/// Render is Box<dyn>, but this is acceptable because:
/// - Layout/paint operations use interior mutability (RwLock)
/// - Hot path (layout) uses trait methods, not enum dispatch
/// - Element enum provides fast dispatch for element operations
///
/// # Lifecycle
///
/// 1. **create_render_object()** - Widget creates Render
/// 2. **mount()** - Element mounted to tree
/// 3. **update_render_object()** - Widget config changes
/// 4. **layout()** - Render layout pass
/// 5. **paint()** - Render paint pass
/// 6. **unmount()** - Render cleanup
pub struct RenderElement {
    /// The widget this element represents (type-erased)
    widget: BoxedWidget,

    /// The render object created by the widget (type-erased)
    ///
    /// Wrapped in RefCell to allow interior mutability during layout.
    /// This is safe because:
    /// - Layout is single-threaded (no concurrent access)
    /// - Borrow checking at runtime prevents aliasing
    /// - More sound than raw pointer casting
    render_object: RefCell<RenderNode>,

    /// Render state (size, constraints, dirty flags)
    ///
    /// Uses RwLock for interior mutability during layout/paint.
    /// Atomic flags inside RenderState provide lock-free checks.
    render_state: RwLock<RenderState>,

    /// Parent data attached by parent Render
    ///
    /// This metadata is set by the parent's layout algorithm (e.g., FlexParentData for Flex,
    /// StackParentData for Stack). Parent accesses this during layout to determine how to
    /// position and size this child.
    parent_data: Option<Box<dyn crate::render::ParentData>>,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child elements (count enforced by Render arity at runtime)
    children: Vec<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag (needs rebuild)
    dirty: bool,
}

// Manual Debug implementation because RefCell<Box<dyn Trait>> doesn't auto-derive
impl std::fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("widget", &"<BoxedWidget>")
            .field("render_object", &"<RefCell<RenderNode>>")
            .field("render_state", &self.render_state)
            .field("parent_data", &self.parent_data.is_some())
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("slot", &self.slot)
            .field("lifecycle", &self.lifecycle)
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl RenderElement {
    /// Create a new RenderElement from a widget and render object
    ///
    /// # Parameters
    ///
    /// - `widget` - Type-erased RenderWidget
    /// - `render_object` - Type-erased Render created by widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Box::new(Container::new());
    /// let render = widget.create_render_object();
    /// let element = RenderElement::new(widget, Box::new(render));
    /// ```
    pub fn new(widget: BoxedWidget, render_object: RenderNode) -> Self {
        Self {
            widget,
            render_object: RefCell::new(render_object),
            render_state: RwLock::new(RenderState::new()),
            parent_data: None,
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
    ///
    /// # Panics
    ///
    /// Panics if the render object is currently borrowed mutably.
    /// This should never happen in correct usage since layout is single-threaded.
    #[inline]
    #[must_use]
    pub fn render_object(&self) -> std::cell::Ref<'_, RenderNode> {
        self.render_object.borrow()
    }

    /// Get mutable reference to the render object
    ///
    /// # Panics
    ///
    /// Panics if the render object is currently borrowed.
    /// This should never happen in correct usage since layout is single-threaded.
    #[inline]
    #[must_use]
    pub fn render_object_mut(&self) -> std::cell::RefMut<'_, RenderNode> {
        self.render_object.borrow_mut()
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

    /// Get parent data attached to this element
    ///
    /// Returns the ParentData trait object if parent has attached metadata,
    /// None otherwise.
    #[inline]
    #[must_use]
    pub fn parent_data(&self) -> Option<&dyn crate::render::ParentData> {
        self.parent_data
            .as_ref()
            .map(|pd| &**pd as &dyn crate::render::ParentData)
    }

    /// Get mutable parent data attached to this element
    #[inline]
    #[must_use]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn crate::render::ParentData> {
        self.parent_data
            .as_mut()
            .map(|pd| &mut **pd as &mut dyn crate::render::ParentData)
    }

    /// Set parent data for this element
    ///
    /// Called by parent Render during setup or when parent changes.
    pub fn set_parent_data(&mut self, parent_data: Box<dyn crate::render::ParentData>) {
        self.parent_data = Some(parent_data);
    }

    /// Clear parent data
    pub fn clear_parent_data(&mut self) {
        self.parent_data = None;
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
        // TODO: Enforce arity constraints via RenderNode::arity() method
        self.children = children;
    }

    /// Add a child (for MultiArity)
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        // TODO: Add arity check via RenderNode::arity() method
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
    /// but may have children in the Render tree (managed by layout).
    ///
    /// # Returns
    ///
    /// Always returns empty vec as RenderWidget doesn't have widget children.
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
    use crate::{BoxedLayer, LayoutCx, LeafArity, PaintCx, Render};
    use flui_types::Size;

    // Test Render with LeafArity
    #[derive(Debug)]
    struct TestLeafRender {
        size: Size,
    }

    impl Render for TestLeafRender {
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
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let element = RenderElement::new(widget, render);

        assert_eq!(element.children().len(), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_mount() {
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let mut element = RenderElement::new(widget, render);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_render_element_update() {
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let mut element = RenderElement::new(widget, render);
        element.mount(Some(0), 0);

        // Update with new widget
        let new_widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(200.0, 100.0),
        });
        element.update(new_widget);

        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_unmount() {
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let mut element = RenderElement::new(widget, render);
        element.mount(Some(0), 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert_eq!(element.children().len(), 0);
    }

    #[test]
    fn test_render_element_lifecycle() {
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
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
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
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
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
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
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let element = RenderElement::new(widget, render);

        // Test render object access
        let render_obj = element.render_object();
        assert!(render_obj.debug_name().contains("TestLeafRender"));
    }

    #[test]
    fn test_render_element_render_state_access() {
        let widget: BoxedWidget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let element = RenderElement::new(widget, render);

        // Test render state access
        let render_state = element.render_state();
        let state = render_state.read();

        // RenderState should be initialized with default values
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
    }
}
