//! RenderElement - Performs layout and paint
//!
//! RenderElement owns a RenderObject and manages layout/paint lifecycle.
//! Per FINAL_ARCHITECTURE_V2.md, it does NOT store widget or view.

use parking_lot::RwLock;

use super::{ElementBase, ElementLifecycle};
use crate::element::ElementId;
use crate::foundation::Slot;
use crate::render::{RenderNode, RenderState};

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
///   ├─ widget: Widget (type-erased RenderWidget)
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
/// - **Widget**: `Widget` (user-extensible, unbounded types)
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
    /// Common element data (widget, parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// The render object created by the widget (type-erased)
    ///
    /// Wrapped in RwLock to allow interior mutability during layout.
    /// RwLock allows multiple concurrent reads OR one exclusive write:
    /// - Multiple immutable borrows during read operations (safe)
    /// - Single mutable borrow during write operations
    /// - More flexible than RefCell which panics on overlapping borrows
    ///
    /// **Note**: When you have `&mut self`, prefer using `render_object_mut_direct()`
    /// to avoid lock contention during the build/mount phase.
    render_object: RwLock<RenderNode>,

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

    /// Child elements (count enforced by Render arity at runtime)
    children: Vec<ElementId>,
}

// Manual Debug implementation because RwLock<Box<dyn Trait>> doesn't auto-derive
impl std::fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("base", &self.base)
            .field("render_object", &"<RwLock<RenderNode>>")
            .field("render_state", &self.render_state)
            .field("parent_data", &self.parent_data.is_some())
            .field("children", &self.children)
            .finish()
    }
}

impl RenderElement {
    /// Create a new RenderElement from a render object
    ///
    /// # Parameters
    ///
    /// - `render_object` - The RenderObject for this element
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render = RenderBox::new();
    /// let element = RenderElement::new(render);
    /// ```
    pub fn new(render_object: RenderNode) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderState::new()),
            parent_data: None,
            children: Vec::new(),
        }
    }

    // Note: widget() method removed - RenderElement no longer stores widget
    // Per FINAL_ARCHITECTURE_V2.md, elements don't store widgets/views

    /// Get reference to the render object
    ///
    /// Returns a read guard that allows multiple concurrent immutable borrows.
    /// This uses parking_lot::RwLock which is more flexible than RefCell.
    #[inline]
    #[must_use]
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, RenderNode> {
        self.render_object.read()
    }

    /// Get mutable reference to the render object (through RwLock)
    ///
    /// Returns a write guard that allows exclusive mutable access.
    /// This blocks if there are any active read or write guards.
    #[inline]
    #[must_use]
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, RenderNode> {
        self.render_object.write()
    }

    /// Get direct mutable reference to the render object
    ///
    /// This method bypasses the RwLock when we have `&mut self` access.
    /// Use this instead of `render_object_mut()` when you have mutable access
    /// to avoid lock contention.
    ///
    /// # Safety
    ///
    /// This is safe because having `&mut self` guarantees exclusive access.
    #[inline]
    #[must_use]
    fn render_object_mut_direct(&mut self) -> &mut RenderNode {
        self.render_object.get_mut()
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

    // Note: update() method removed - will be replaced with View-based update
    // TODO(Phase 5): Implement View::rebuild() integration

    /// Set children (enforces arity constraints at runtime)
    #[allow(dead_code)]
    pub(crate) fn set_children(&mut self, children: Vec<ElementId>) {
        // Enforce arity constraints
        let render = self.render_object_mut_direct();
        let arity = render.arity();

        match arity {
            Some(0) => {
                // Leaf render - no children allowed
                if !children.is_empty() {
                    panic!(
                        "Leaf render object (arity=0) cannot have children, got {} children",
                        children.len()
                    );
                }
            }
            Some(1) => {
                // Single render - exactly one child required
                if children.len() != 1 {
                    panic!(
                        "Single render object (arity=1) must have exactly 1 child, got {}",
                        children.len()
                    );
                }
            }
            Some(n) => {
                // Fixed arity > 1 - exactly n children required
                if children.len() != n {
                    panic!(
                        "Fixed arity render object (arity={}) must have exactly {} children, got {}",
                        n, n, children.len()
                    );
                }
            }
            None => {
                // Multi render - any number of children allowed
            }
        }

        self.children = children;
    }

    /// Add a child (for MultiArity or SingleArity)
    ///
    /// NOTE: This should only be called with RenderElement children!
    /// ComponentElements should not be added as children to RenderElements.
    /// The element tree building should ensure that RenderElements only have
    /// RenderElement children (by walking down through ComponentElements).
    #[allow(dead_code)]
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        // Add to children list
        self.children.push(child_id);

        // Also update RenderNode's child/children field
        // We MUST do this so the render object knows which children to layout/paint!

        // Access render_object directly to allow simultaneous borrow of children
        // (using the method would borrow all of &mut self)
        let render = self.render_object.get_mut();

        if render.arity() == Some(1) {
            render.set_child(child_id);
        } else {
            // Pass slice reference - no clone needed!
            // This works because we borrow self.render_object mutably
            // and self.children immutably (different fields)
            render.set_children(&self.children);
        }
    }

    /// Remove a child by ID
    #[allow(dead_code)]
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
        self.base.parent()
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
        self.base.lifecycle()
    }

    /// Mount element to tree
    ///
    /// Sets parent, slot, and transitions to Active lifecycle state.
    /// Marks element as dirty to trigger initial build.
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    ///
    /// Transitions to Defunct lifecycle state and clears children.
    /// The children will be unmounted by ElementTree separately.
    pub fn unmount(&mut self) {
        self.base.unmount();
        // Children will be unmounted by ElementTree
        self.children.clear();
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element
    ///
    /// Called when element is reactivated. Marks dirty to trigger rebuild.
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: is_* prefix for boolean predicates.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Perform rebuild
    ///
    /// RenderElement doesn't create child widgets - it's a leaf in the Widget tree
    /// but may have children in the Render tree (managed by layout).
    ///
    /// # Returns
    ///
    /// Always returns empty vec as RenderWidget doesn't have widget children.

    // NOTE: Commented out during Widget → View migration
    // TODO(Phase 5): Reimplement using View system
    /*
    pub fn rebuild(
        &mut self,
        element_id: ElementId,
        _tree: std::sync::Arc<parking_lot::RwLock<crate::pipeline::ElementTree>>,
    ) -> Vec<(ElementId, Widget, usize)> {
        self.base.clear_dirty();

        // RenderWidget doesn't "build" like StatelessWidget, but it may have children
        // that need to be mounted in the tree
        let widget = self.base.widget();

        // Check if widget has children (multi-child widget like Row/Column/Stack)
        if let Some(children) = widget.render_widget_children() {
            // Return all children to be mounted
            children
                .iter()
                .enumerate()
                .map(|(slot, child)| (element_id, child.clone(), slot))
                .collect()
        } else if let Some(child) = widget.render_widget_child() {
            // Single child widget (like Center, Padding, etc.)
            vec![(element_id, child.clone(), 0)]
        } else {
            // Leaf widget (like Text) - no children
            Vec::new()
        }
    }
    */

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

    impl crate::Widget for TestLeafWidget {
        // Minimal implementation for testing
    }

    #[test]
    fn test_render_element_creation() {
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let mut element = RenderElement::new(widget, render);
        element.mount(Some(0), 0);

        // Update with new widget
        let new_widget: Widget = Box::new(TestLeafWidget {
            size: Size::new(200.0, 100.0),
        });
        element.update(new_widget);

        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_unmount() {
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
            size: Size::new(100.0, 50.0),
        });
        let render: RenderNode = Box::new(TestLeafRender {
            size: Size::new(100.0, 50.0),
        });
        let mut element = RenderElement::new(widget, render);

        // Initially dirty
        assert!(element.is_dirty());

        // Rebuild clears dirty
        let tree = std::sync::Arc::new(parking_lot::RwLock::new(crate::pipeline::ElementTree::new()));
        element.rebuild(1, tree);
        assert!(!element.is_dirty());

        // Mark dirty
        element.mark_dirty();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_element_children_management() {
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
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
        let widget: Widget = Box::new(TestLeafWidget {
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
