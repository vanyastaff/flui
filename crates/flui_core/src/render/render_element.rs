//! Unified storage for render objects.
//!
//! `RenderElement` is the core type that holds a type-erased render object
//! along with its protocol, arity, state, and children.
//!
//! # Architecture
//!
//! ```text
//! View → Element::Render(RenderElement) → RenderObject (type-erased)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! // Create a leaf render element
//! let element = RenderElement::r#box::<Leaf, _>(RenderText::new("Hello"));
//!
//! // Create with children
//! let mut element = RenderElement::r#box::<Single, _>(RenderPadding::new(padding));
//! element.set_unmounted_children(vec![child_element]);
//! ```

use parking_lot::RwLock;
use std::fmt::Debug;

use crate::element::element_base::ElementBase;
use crate::element::{Element, ElementId, ElementLifecycle};
use crate::render::arity::Arity;
use crate::render::protocol::{BoxProtocol, LayoutProtocol, Protocol, SliverProtocol};
use crate::render::render_box::RenderBox;
use crate::render::render_object::{
    Constraints as DynConstraints, Geometry as DynGeometry, RenderObject,
};
use crate::render::render_silver::SliverRender;
use crate::render::render_state::RenderState;
use crate::render::wrappers::{BoxRenderWrapper, SliverRenderWrapper};
use crate::render::RuntimeArity;

// ============================================================================
// RENDER ELEMENT
// ============================================================================

/// Unified render element for Box and Sliver protocols
///
/// Single source of truth for protocol and arity with type-erased dispatch.
///
/// # Lock Ordering
///
/// Always acquire locks in this order:
/// 1. `render_object` (first)
/// 2. `render_state` (second)
///
/// # Transactional Updates
///
/// Use `begin_children_update()` / `commit_children_update()` for batch operations.
pub struct RenderElement {
    base: ElementBase,
    protocol: LayoutProtocol,
    arity: RuntimeArity,
    render_object: RwLock<Box<dyn RenderObject>>,
    render_state: RwLock<RenderState<BoxProtocol>>,
    offset: flui_types::Offset,
    children: Vec<ElementId>,
    updating_children: bool,
    unmounted_children: Option<Vec<Element>>,
}

impl Debug for RenderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("render_object", &self.render_object.read().debug_name())
            .field("children_count", &self.children.len())
            .finish()
    }
}

impl RenderElement {
    // ========================================================================
    // CONSTRUCTORS
    // ========================================================================

    /// Box protocol element
    ///
    /// ```rust,ignore
    /// RenderElement::r#box::<Leaf, _>(my_render)
    /// RenderElement::r#box::<Single, _>(my_render)
    /// ```
    pub fn r#box<A, R>(render: R) -> Self
    where
        A: Arity,
        R: RenderBox<A> + 'static,
    {
        Self::new_internal(
            LayoutProtocol::Box,
            A::runtime_arity(),
            Box::new(BoxRenderWrapper::<A, R>::new(render)),
            None,
        )
    }

    /// Sliver protocol element
    ///
    /// ```rust,ignore
    /// RenderElement::sliver::<Leaf, _>(my_render)
    /// RenderElement::sliver::<Variable, _>(my_render)
    /// ```
    pub fn sliver<A, R>(render: R) -> Self
    where
        A: Arity,
        R: SliverRender<A> + 'static,
    {
        Self::new_internal(
            LayoutProtocol::Sliver,
            A::runtime_arity(),
            Box::new(SliverRenderWrapper::<A, R>::new(render)),
            None,
        )
    }

    /// Element with children (any protocol)
    ///
    /// ```rust,ignore
    /// RenderElement::with::<BoxProtocol, Variable, _>(my_render, children)
    /// RenderElement::with::<SliverProtocol, Single, _>(my_sliver, vec![child])
    /// ```
    pub fn with<P, A, R>(render: R, children: Vec<Element>) -> Self
    where
        P: Protocol,
        A: Arity,
        R: IntoRenderObject<A, P>,
    {
        Self::new_internal(
            P::ID,
            A::runtime_arity(),
            render.into_render_object(),
            Some(children),
        )
    }

    fn new_internal(
        protocol: LayoutProtocol,
        arity: RuntimeArity,
        render_object: Box<dyn RenderObject>,
        unmounted_children: Option<Vec<Element>>,
    ) -> Self {
        Self {
            base: ElementBase::new(),
            protocol,
            arity,
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            children: Vec::new(),
            updating_children: false,
            unmounted_children,
        }
    }

    // ========================================================================
    // ACCESSORS
    // ========================================================================

    /// Returns the layout protocol (Box or Sliver).
    #[inline]
    pub fn protocol(&self) -> LayoutProtocol {
        self.protocol
    }

    /// Returns the runtime arity specification.
    #[inline]
    pub fn arity(&self) -> &RuntimeArity {
        &self.arity
    }

    /// Returns the mounted children IDs.
    #[inline]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Returns mutable access to children vector.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Returns reference to the render state lock.
    #[inline]
    pub fn render_state(&self) -> &RwLock<RenderState<BoxProtocol>> {
        &self.render_state
    }

    /// Acquires read lock on the render object.
    #[inline]
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, Box<dyn RenderObject>> {
        self.render_object.read()
    }

    /// Acquires write lock on the render object.
    #[inline]
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Box<dyn RenderObject>> {
        self.render_object.write()
    }

    /// Returns the current paint offset.
    #[inline]
    pub fn offset(&self) -> flui_types::Offset {
        self.offset
    }

    /// Sets the paint offset.
    #[inline]
    pub fn set_offset(&mut self, offset: flui_types::Offset) {
        self.offset = offset;
    }

    /// Takes unmounted children for mounting during tree construction.
    pub fn take_unmounted_children(&mut self) -> Option<Vec<Element>> {
        self.unmounted_children.take()
    }

    /// Sets unmounted children to be mounted later.
    pub fn set_unmounted_children(&mut self, children: Vec<Element>) {
        self.unmounted_children = Some(children);
    }

    // ========================================================================
    // CHILDREN MANAGEMENT
    // ========================================================================

    /// Begins a transactional children update.
    ///
    /// Call this before multiple `push_child` calls to defer arity validation
    /// until `commit_children_update`.
    pub fn begin_children_update(&mut self) {
        self.updating_children = true;
    }

    /// Commits a transactional children update.
    ///
    /// Validates arity and marks layout as dirty.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if arity is violated or called without `begin_children_update`.
    pub fn commit_children_update(&mut self) {
        debug_assert!(
            self.updating_children,
            "commit_children_update called without begin_children_update"
        );
        self.updating_children = false;

        debug_assert!(
            self.arity.validate(self.children.len()),
            "Arity violation: expected {:?}, got {}",
            self.arity,
            self.children.len()
        );

        self.render_state.write().mark_needs_layout();
    }

    /// Adds a child element.
    ///
    /// Validates arity immediately unless inside a transactional update.
    pub fn push_child(&mut self, child_id: ElementId) {
        if !self.updating_children {
            debug_assert!(
                self.arity.validate(self.children.len() + 1),
                "Cannot add child: arity {:?}",
                self.arity
            );
        }

        self.children.push(child_id);

        if !self.updating_children {
            self.render_state.write().mark_needs_layout();
        }
    }

    /// Removes a child element.
    ///
    /// Returns `true` if the child was found and removed.
    pub fn remove_child(&mut self, child_id: ElementId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child_id) {
            if !self.updating_children {
                debug_assert!(
                    self.arity.validate(self.children.len() - 1),
                    "Cannot remove child: arity {:?}",
                    self.arity
                );
            }

            self.children.remove(pos);

            if !self.updating_children {
                self.render_state.write().mark_needs_layout();
            }

            true
        } else {
            false
        }
    }

    /// Replaces all children at once.
    ///
    /// Validates arity and marks layout as dirty.
    pub fn replace_children(&mut self, new_children: Vec<ElementId>) {
        debug_assert!(
            self.arity.validate(new_children.len()),
            "Arity violation: expected {:?}, got {}",
            self.arity,
            new_children.len()
        );

        self.children = new_children;
        self.render_state.write().mark_needs_layout();
    }

    /// Removes child without arity validation (used during unmount).
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    // ========================================================================
    // LIFECYCLE
    // ========================================================================

    /// Returns the parent element ID.
    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Returns the current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Mounts the element into the tree.
    ///
    /// Called when the element is first inserted into the element tree.
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<crate::foundation::Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmounts the element from the tree.
    ///
    /// Called when the element is removed from the tree permanently.
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Deactivates the element.
    ///
    /// Called when the element is removed but may be reinserted later.
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activates the element.
    ///
    /// Called when a previously deactivated element is reinserted.
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Returns whether this element needs rebuild.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Marks this element as needing rebuild.
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Returns an iterator over mounted children IDs.
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    /// Handles an event, returning whether it was consumed.
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false
    }

    /// Returns parent data for this element.
    ///
    /// Parent data is used by parent layouts to store per-child metadata.
    pub fn parent_data(&self) -> Option<&dyn crate::render::ParentData> {
        None
    }

    /// Updates the slot for a child element.
    ///
    /// Called when a child's position in the parent changes.
    pub fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // No-op
    }

    // ========================================================================
    // LAYOUT & PAINT
    // ========================================================================

    /// Performs layout computation for Box protocol elements.
    ///
    /// Delegates to the type-erased render object's layout method.
    ///
    /// # Panics
    ///
    /// Panics if called on a Sliver protocol element.
    pub(crate) fn layout_render(
        &self,
        tree: &crate::element::ElementTree,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> flui_types::Size {
        debug_assert!(
            matches!(self.protocol, LayoutProtocol::Box),
            "layout_render called on non-Box protocol"
        );

        let mut render = self.render_object.write();

        let geometry = render.layout(tree, &self.children, &DynConstraints::Box(constraints));

        match geometry {
            DynGeometry::Box(size) => size,
            _ => panic!("Expected Box geometry"),
        }
    }

    /// Performs paint computation for Box protocol elements.
    ///
    /// Delegates to the type-erased render object's paint method.
    ///
    /// # Panics
    ///
    /// Panics if called on a Sliver protocol element.
    pub(crate) fn paint_render(
        &self,
        tree: &crate::element::ElementTree,
        offset: flui_types::Offset,
    ) -> flui_painting::Canvas {
        debug_assert!(
            matches!(self.protocol, LayoutProtocol::Box),
            "paint_render called on non-Box protocol"
        );

        let render = self.render_object.read();
        render.paint(tree, &self.children, offset)
    }
}

// ============================================================================
// INTO RENDER OBJECT TRAIT
// ============================================================================

/// Converts a typed render implementation into a type-erased [`RenderObject`].
///
/// This trait enables the `RenderElement::with` constructor to accept both
/// `RenderBox` and `SliverRender` implementations and automatically wrap them
/// in the appropriate wrapper type.
///
/// # Implementation
///
/// The trait is automatically implemented for:
/// - `RenderBox<A>` implementations (wrapped in `BoxRenderWrapper`)
/// - `SliverRender<A>` implementations (wrapped in `SliverRenderWrapper`)
pub trait IntoRenderObject<A: Arity, P: Protocol>: Sized + Send + Sync + Debug + 'static {
    /// Converts self into a boxed type-erased render object.
    fn into_render_object(self) -> Box<dyn RenderObject>;
}

impl<A: Arity, R: RenderBox<A>> IntoRenderObject<A, BoxProtocol> for R {
    fn into_render_object(self) -> Box<dyn RenderObject> {
        Box::new(BoxRenderWrapper::<A, R>::new(self))
    }
}

impl<A: Arity, R: SliverRender<A>> IntoRenderObject<A, SliverProtocol> for R {
    fn into_render_object(self) -> Box<dyn RenderObject> {
        Box::new(SliverRenderWrapper::<A, R>::new(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::contexts::{LayoutContext, PaintContext};
    use crate::render::{Leaf, Variable};
    use flui_types::Size;

    #[derive(Debug)]
    struct MockLeaf;

    impl RenderBox<Leaf> for MockLeaf {
        fn layout(&mut self, _ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {}
    }

    #[derive(Debug)]
    struct MockVariable;

    impl RenderBox<Variable> for MockVariable {
        fn layout(&mut self, _ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _ctx: &mut PaintContext<'_, Variable>) {}
    }

    #[test]
    fn test_box_leaf() {
        let element = RenderElement::r#box::<Leaf, _>(MockLeaf);
        assert_eq!(element.protocol(), LayoutProtocol::Box);
        assert_eq!(element.children().len(), 0);
    }

    #[test]
    fn test_transactional_update() {
        let mut element = RenderElement::r#box::<Variable, _>(MockVariable);

        element.begin_children_update();
        element.push_child(ElementId::new(1));
        element.push_child(ElementId::new(2));
        element.commit_children_update();

        assert_eq!(element.children().len(), 2);
    }

    #[test]
    #[should_panic(expected = "Cannot add child")]
    fn test_arity_violation() {
        let mut element = RenderElement::r#box::<Leaf, _>(MockLeaf);
        element.push_child(ElementId::new(1));
    }
}
