//! RenderElement - Unified render object storage with protocol and arity
//!
//! This is the Phase 6 implementation of the unified-renderobject-system.
//! It stores protocol and arity as the single source of truth and uses DynRenderObject
//! for type-erased dispatch.
//!
//! # Architecture
//!
//! ```text
//! RenderElement
//!   ├─ protocol: LayoutProtocol (Box or Sliver)
//!   ├─ arity: RuntimeArity (Exact(n), Variable, etc.)
//!   ├─ render_object: Box<dyn DynRenderObject> (type-erased wrapper)
//!   ├─ render_state: RwLock<RenderState> (size, constraints, flags)
//!   ├─ children: Vec<ElementId> (validated against arity)
//!   └─ updating_children: bool (for transactional updates)
//! ```
//!
//! # Key Design
//!
//! - **Single Source of Truth**: Protocol and arity stored only in RenderElement
//! - **Type Erasure**: Wrappers (BoxRenderObjectWrapper) implement DynRenderObject
//! - **Thread Safety**: Explicit lock ordering (render_object → render_state)
//! - **Zero Cost**: Debug assertions validate arity (removed in release builds)

use parking_lot::RwLock;
use std::fmt::Debug;

use super::ElementBase;
use crate::element::ElementId;
use crate::render::traits::{Render, SliverRender};
use crate::render::{
    Arity, BoxRenderObjectWrapper, DynRenderObject, LayoutProtocol, Leaf, Optional, Pair,
    RenderState, RuntimeArity, Single, SliverRenderObjectWrapper, Triple, Variable,
};

/// RenderElement - Unified render object with protocol and arity
///
/// Stores protocol and arity as single source of truth.
///
/// # Lock Ordering (CRITICAL)
///
/// Always acquire locks in this order to prevent deadlocks:
/// 1. `render_object` lock (first)
/// 2. `render_state` lock (second)
///
/// Never acquire in reverse order!
///
/// # Transactional Children Updates
///
/// Use `begin_children_update()` / `commit_children_update()` for batch operations
/// that temporarily violate arity constraints:
///
/// ```rust,ignore
/// element.begin_children_update();
/// element.remove_child(old_child);  // May violate arity temporarily
/// element.push_child(new_child);    // Restore valid arity
/// element.commit_children_update(); // Validates final state
/// ```
pub struct RenderElement {
    /// Common element data (parent, slot, lifecycle)
    base: ElementBase,

    /// Layout protocol (Box or Sliver) - source of truth
    protocol: LayoutProtocol,

    /// Child count arity - source of truth
    arity: RuntimeArity,

    /// Type-erased render object (wrapped in BoxRenderObjectWrapper or SliverRenderObjectWrapper)
    ///
    /// **Lock Ordering**: Always acquire this lock BEFORE render_state lock
    render_object: RwLock<Box<dyn DynRenderObject>>,

    /// Render state (size/geometry, constraints, dirty flags)
    ///
    /// **Lock Ordering**: Always acquire this lock AFTER render_object lock
    render_state: RwLock<RenderState>,

    /// Position of this element relative to parent
    ///
    /// Set during layout phase by parent. Used for hit testing and painting.
    offset: flui_types::Offset,

    /// Child element IDs (validated against arity)
    children: Vec<ElementId>,

    /// Flag indicating transactional update in progress
    ///
    /// When true, arity validation is skipped to allow temporary violations.
    /// Must call commit_children_update() to validate final state.
    updating_children: bool,

    /// Unmounted children waiting to be inserted into the tree
    ///
    /// Used by IntoElement implementations to store child Elements that haven't
    /// been mounted yet. When this RenderElement is inserted into the tree,
    /// these children are automatically mounted and added as children.
    unmounted_children: Option<Vec<crate::element::Element>>,
}

impl Debug for RenderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Acquire render_object lock for debug_name (respects lock ordering)
        let render_obj = self.render_object.read();
        let debug_name = render_obj.debug_name();

        f.debug_struct("RenderElement")
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("render_object", &debug_name)
            .field("offset", &self.offset)
            .field("children_count", &self.children.len())
            .field("updating_children", &self.updating_children)
            .finish()
    }
}

impl RenderElement {
    // ============================================================================
    // LEGACY COMPATIBILITY (Deprecated)
    // ============================================================================

    /// Legacy constructor for backward compatibility
    ///
    /// **DEPRECATED**: This method is only for compatibility with old code using

    /// Create a Box protocol element with Leaf arity (0 children)
    pub fn box_leaf<R>(render: R) -> Self
    where
        R: Render<Leaf> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Leaf, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Leaf::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Optional arity (0-1 children)
    pub fn box_optional<R>(render: R) -> Self
    where
        R: Render<Optional> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Optional, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Optional::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Single arity (exactly 1 child)
    pub fn box_single<R>(render: R) -> Self
    where
        R: Render<Single> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Single, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Single::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Single arity with unmounted child
    ///
    /// The child will be automatically mounted when this element is inserted into the tree.
    pub fn box_single_with_child<R>(render: R, child: crate::element::Element) -> Self
    where
        R: Render<Single> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Single, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Single::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: Some(vec![child]),
        }
    }

    /// Create a Box protocol element with Pair arity (exactly 2 children)
    pub fn box_pair<R>(render: R) -> Self
    where
        R: Render<Pair> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Pair, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Pair::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Triple arity (exactly 3 children)
    pub fn box_triple<R>(render: R) -> Self
    where
        R: Render<Triple> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Triple, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Triple::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Variable arity (any number of children)
    pub fn box_variable<R>(render: R) -> Self
    where
        R: Render<Variable> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Variable, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Variable::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Box protocol element with Variable arity with unmounted children
    ///
    /// The children will be automatically mounted when this element is inserted into the tree.
    pub fn box_variable_with_children<R>(render: R, children: Vec<crate::element::Element>) -> Self
    where
        R: Render<Variable> + 'static,
    {
        let wrapper = BoxRenderObjectWrapper::<Variable, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Box,
            arity: Variable::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: Some(children),
        }
    }

    // ============================================================================
    // CONSTRUCTORS (Sliver Protocol)
    // ============================================================================

    /// Create a Sliver protocol element with Single arity (exactly 1 child)
    pub fn sliver_single<R>(render: R) -> Self
    where
        R: SliverRender<Single> + 'static,
    {
        let wrapper = SliverRenderObjectWrapper::<Single, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Sliver,
            arity: Single::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    /// Create a Sliver protocol element with Variable arity (any number of children)
    pub fn sliver_variable<R>(render: R) -> Self
    where
        R: SliverRender<Variable> + 'static,
    {
        let wrapper = SliverRenderObjectWrapper::<Variable, R>::new(render);
        Self {
            base: ElementBase::new(),
            protocol: LayoutProtocol::Sliver,
            arity: Variable::runtime_arity(),
            render_object: RwLock::new(Box::new(wrapper)),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children: None,
        }
    }

    // ============================================================================
    // ACCESSORS
    // ============================================================================

    /// Get the layout protocol (Box or Sliver)
    #[inline(always)]
    pub fn protocol(&self) -> LayoutProtocol {
        self.protocol
    }

    /// Get the runtime arity
    #[inline(always)]
    pub fn runtime_arity(&self) -> &RuntimeArity {
        &self.arity
    }

    /// Get children slice
    #[inline(always)]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable children slice (for internal use only)
    #[allow(dead_code)] // Reserved for future dynamic child management
    #[inline(always)]
    pub(crate) fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Get render state lock
    #[inline(always)]
    pub fn render_state(&self) -> &RwLock<RenderState> {
        &self.render_state
    }

    /// Get render object read guard
    #[inline(always)]
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, Box<dyn DynRenderObject>> {
        self.render_object.read()
    }

    /// Get render object write guard
    #[inline(always)]
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Box<dyn DynRenderObject>> {
        self.render_object.write()
    }

    // ============================================================================
    // ELEMENT BASE DELEGATION (Lifecycle, Parent, etc.)
    // ============================================================================

    /// Get parent element ID
    #[inline(always)]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get lifecycle state
    #[inline(always)]
    pub fn lifecycle(&self) -> super::ElementLifecycle {
        self.base.lifecycle()
    }

    /// Mount this element into the tree
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<crate::foundation::Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount this element from the tree
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Get children iterator
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    /// Handle event (placeholder - render elements don't handle events directly)
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false
    }

    /// Deactivate this element
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate this element
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element is dirty (needs rebuild)
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as dirty (needs rebuild)
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Get parent data (not yet implemented in unified system)
    ///
    /// ParentData is a legacy concept from the old render system.
    /// In the unified system, metadata is stored via the metadata() method
    /// on render objects. This method returns None for now to maintain
    /// compatibility with Element enum.
    ///
    /// TODO: Implement proper ParentData integration if needed
    pub fn parent_data(&self) -> Option<&dyn crate::render::ParentData> {
        None
    }

    /// Forget a child (remove from internal list without unmounting)
    ///
    /// Used by Element enum's forget_child method. Removes the child from
    /// the children vector without performing any lifecycle operations.
    ///
    /// # Thread Safety
    ///
    /// Does NOT mark needs layout to avoid lock ordering violations.
    /// The caller is responsible for calling `mark_needs_layout()` after
    /// this operation completes.
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
        // NOTE: Layout marking is caller's responsibility to avoid lock ordering issues
    }

    /// Update slot for a child
    ///
    /// In the unified system, slots are managed by the parent element.
    /// This is a compatibility stub for the Element enum.
    pub fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // TODO: Implement slot tracking if needed
        // For now, this is a no-op as slots are managed differently
    }

    /// Get the offset (position relative to parent)
    #[inline]
    #[must_use]
    pub fn offset(&self) -> flui_types::Offset {
        self.offset
    }

    /// Set the offset (called by parent during layout)
    #[inline]
    pub fn set_offset(&mut self, offset: flui_types::Offset) {
        self.offset = offset;
    }

    /// Take unmounted children
    ///
    /// Returns and clears any unmounted children waiting to be inserted into the tree.
    /// Called by ElementTree::insert() to automatically mount children when this
    /// element is inserted.
    pub fn take_unmounted_children(&mut self) -> Option<Vec<crate::element::Element>> {
        self.unmounted_children.take()
    }

    /// Set unmounted children
    ///
    /// Used by IntoElement implementations to store child Elements that haven't
    /// been mounted yet. When this RenderElement is inserted into the tree,
    /// these children are automatically mounted.
    pub fn set_unmounted_children(&mut self, children: Vec<crate::element::Element>) {
        self.unmounted_children = Some(children);
    }

    /// Set children (replaces all children)
    ///
    /// Compatibility method for Element enum. Use replace_children() instead
    /// for better arity validation.
    ///
    /// # Arity Validation
    ///
    /// This method delegates to `replace_children()` which performs full arity
    /// validation. If the new children count violates the arity constraint,
    /// this will panic with a descriptive error message.
    ///
    /// # Panics
    ///
    /// Panics if `new_children.len()` doesn't match this element's arity constraint.
    pub fn set_children(&mut self, new_children: Vec<ElementId>) {
        self.replace_children(new_children);
    }

    // ============================================================================
    // CHILDREN MANAGEMENT (Transactional API)
    // ============================================================================

    /// Begin transactional children update
    ///
    /// Disables arity validation for intermediate operations.
    /// Must call `commit_children_update()` to validate final state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// element.begin_children_update();
    /// element.remove_child(old_child);  // May temporarily violate arity
    /// element.push_child(new_child);
    /// element.commit_children_update(); // Validates final state
    /// ```
    pub fn begin_children_update(&mut self) {
        self.updating_children = true;
    }

    /// Commit transactional children update
    ///
    /// Validates final children count against arity and panics if invalid.
    /// Clears the `updating_children` flag.
    ///
    /// # Panics
    ///
    /// Panics if final children count violates arity constraints.
    /// Panics in debug builds if commit_children_update is called without begin_children_update.
    pub fn commit_children_update(&mut self) {
        debug_assert!(
            self.updating_children,
            "commit_children_update called without begin_children_update"
        );

        self.updating_children = false;

        // Validate final state
        if !self.arity.validate(self.children.len()) {
            panic!(
                "Arity violation after commit: expected {:?}, got {} children",
                self.arity,
                self.children.len()
            );
        }

        // Mark needs layout since children changed
        self.render_state.write().mark_needs_layout();
    }

    /// Replace all children atomically
    ///
    /// Validates arity before replacement. Recommended for rebuilds and reconciliation.
    ///
    /// # Panics
    ///
    /// Panics if new children count violates arity constraints.
    pub fn replace_children(&mut self, new_children: Vec<ElementId>) {
        // Validate before replacement
        if !self.arity.validate(new_children.len()) {
            panic!(
                "Arity violation in replace_children: expected {:?}, got {} children",
                self.arity,
                new_children.len()
            );
        }

        self.children = new_children;
        self.render_state.write().mark_needs_layout();
    }

    /// Add a child element
    ///
    /// During transactions (between begin_children_update and commit_children_update),
    /// arity validation is skipped. Outside transactions, validates immediately.
    ///
    /// # Panics
    ///
    /// Panics if adding child would violate arity (outside transactions).
    pub fn push_child(&mut self, child_id: ElementId) {
        // Skip validation during transaction
        if !self.updating_children {
            // Validate before adding
            if !self.arity.validate(self.children.len() + 1) {
                panic!(
                    "Arity violation: cannot add child to {:?}, expected {:?}",
                    self.render_object.read().debug_name(),
                    self.arity
                );
            }
        }

        self.children.push(child_id);

        // Mark needs layout if not in transaction
        if !self.updating_children {
            self.render_state.write().mark_needs_layout();
        }
    }

    /// Remove a child element (pub(crate) to prevent misuse outside transactions)
    ///
    /// During transactions, arity validation is skipped.
    /// Outside transactions, validates that removal won't violate arity.
    ///
    /// # Panics
    ///
    #[allow(dead_code)] // Reserved for future dynamic child management
    /// Panics if removing child would violate arity (outside transactions).
    pub(crate) fn remove_child(&mut self, child_id: ElementId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child_id) {
            // Skip validation during transaction
            if !self.updating_children {
                // Validate before removal
                if !self.arity.validate(self.children.len() - 1) {
                    panic!(
                        "Arity violation: cannot remove child from {:?}, expected {:?}",
                        self.render_object.read().debug_name(),
                        self.arity
                    );
                }
            }

            self.children.remove(pos);

            // Mark needs layout if not in transaction
            if !self.updating_children {
                self.render_state.write().mark_needs_layout();
            }

            true
        } else {
            false
        }
    }

    // ============================================================================
    // LAYOUT AND PAINT (For ElementTree integration)
    // ============================================================================

    /// Perform layout on this render object
    ///
    /// Calls the type-erased dyn_layout method through DynRenderObject.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `constraints`: Layout constraints from parent
    ///
    /// # Returns
    ///
    /// The computed size
    pub(crate) fn layout_render(
        &self,
        tree: &crate::element::ElementTree,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> flui_types::Size {
        use crate::render::protocol::BoxConstraints as ProtocolBoxConstraints;
        use crate::render::{DynConstraints, DynGeometry};

        // Hot path assertion: verify protocol matches
        debug_assert!(
            matches!(self.protocol, crate::render::protocol::LayoutProtocol::Box),
            "layout_render called on non-Box protocol element"
        );

        let mut render = self.render_object.write();

        // Convert flui_types::BoxConstraints to protocol::BoxConstraints
        let protocol_constraints = ProtocolBoxConstraints {
            min_width: constraints.min_width,
            max_width: constraints.max_width,
            min_height: constraints.min_height,
            max_height: constraints.max_height,
        };

        let dyn_constraints = DynConstraints::Box(protocol_constraints);
        let geometry = render.dyn_layout(tree, &self.children, &dyn_constraints);

        // Extract size from geometry (assumes Box protocol)
        match geometry {
            DynGeometry::Box(size) => size,
            _ => panic!("Expected Box geometry from Box protocol render object"),
        }
    }

    /// Perform paint on this render object
    ///
    /// Calls the type-erased dyn_paint method through DynRenderObject.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `offset`: Paint offset in parent's coordinate space
    ///
    /// # Returns
    ///
    /// A canvas containing the painted content
    pub(crate) fn paint_render(
        &self,
        tree: &crate::element::ElementTree,
        offset: flui_types::Offset,
    ) -> flui_painting::Canvas {
        // Hot path assertion: verify protocol matches
        debug_assert!(
            matches!(self.protocol, crate::render::protocol::LayoutProtocol::Box),
            "paint_render called on non-Box protocol element"
        );

        let render = self.render_object.read();
        render.dyn_paint(tree, &self.children, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::protocol::{BoxGeometry, BoxLayoutContext, BoxPaintContext};
    use crate::render::Leaf;
    use flui_types::Size;

    // Mock leaf render for testing
    #[derive(Debug)]
    struct MockLeaf;

    impl Render<Leaf> for MockLeaf {
        fn layout(&mut self, _ctx: &BoxLayoutContext<Leaf>) -> BoxGeometry {
            BoxGeometry {
                size: Size::new(100.0, 100.0),
            }
        }

        fn paint(&self, _ctx: &BoxPaintContext<Leaf>) {
            // No-op
        }
    }

    // Mock variable render for testing
    #[derive(Debug)]
    struct MockVariable;

    impl Render<Variable> for MockVariable {
        fn layout(&mut self, _ctx: &BoxLayoutContext<Variable>) -> BoxGeometry {
            BoxGeometry {
                size: Size::new(100.0, 100.0),
            }
        }

        fn paint(&self, _ctx: &BoxPaintContext<Variable>) {
            // No-op
        }
    }

    #[test]
    fn test_box_leaf_constructor() {
        let element = RenderElement::box_leaf(MockLeaf);
        assert_eq!(element.protocol(), LayoutProtocol::Box);
        assert_eq!(element.runtime_arity(), &RuntimeArity::Exact(0));
        assert_eq!(element.children().len(), 0);
    }

    #[test]
    fn test_transactional_update() {
        // Use box_variable which accepts any number of children
        let mut element = RenderElement::box_variable(MockVariable);

        // Begin transaction
        element.begin_children_update();

        // Add children during transaction
        let child_id1 = ElementId::new(1);
        let child_id2 = ElementId::new(2);
        element.push_child(child_id1);
        element.push_child(child_id2);

        // Commit validates final state
        element.commit_children_update();

        assert_eq!(element.children().len(), 2);
    }

    #[test]
    #[should_panic(expected = "Arity violation")]
    fn test_push_child_violates_leaf_arity() {
        let mut element = RenderElement::box_leaf(MockLeaf);

        // Attempting to add child to Leaf should panic
        let child_id = ElementId::new(1);
        element.push_child(child_id);
    }

    #[test]
    fn test_replace_children_atomic() {
        let mut element = RenderElement::box_variable(MockVariable);

        let children = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        element.replace_children(children.clone());

        assert_eq!(element.children(), &children[..]);
    }
}
