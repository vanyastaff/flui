//! Fully typed RenderElement - Element that owns and manages a RenderObject.
//!
//! This module provides a generic `RenderElement<R, P>` where:
//! - `R: RenderObject` - The concrete render object type
//! - `P: Protocol` - The layout protocol (BoxProtocol or SliverProtocol)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Typed Layer (Compile-time)                    │
//! │  RenderElement<RenderPadding, BoxProtocol>                       │
//! │  RenderElement<RenderFlex, BoxProtocol>                          │
//! │  RenderElement<RenderSliver, SliverProtocol>                     │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼ impl RenderElementNode
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                  Type-Erased Layer (Runtime)                     │
//! │  Box<dyn RenderElementNode>                                      │
//! │  ElementNodeStorage                                              │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Benefits
//!
//! 1. **Compile-time type safety**: Protocol mismatch caught at compile time
//! 2. **Direct state access**: No runtime downcasting for constraints/geometry
//! 3. **Zero overhead**: No Box<dyn> for RenderState
//! 4. **Heterogeneous storage**: Via RenderElementNode trait

use std::any::Any;
use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_tree::RuntimeArity;
use flui_types::{Offset, Size, SliverGeometry};

use crate::element_node::RenderElementNode;
use crate::flags::AtomicRenderFlags;
use crate::lifecycle::RenderLifecycle;
use crate::object::RenderObject;
use crate::parent_data::ParentData;
use crate::protocol::{BoxProtocol, Protocol, ProtocolId, SliverProtocol};
use crate::state::{BoxRenderState, RenderState, SliverRenderState};
use crate::tree::RenderId;
use crate::{BoxConstraints, SliverConstraints};

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// RenderElement for Box protocol.
pub type BoxRenderElement<R> = RenderElement<R, BoxProtocol>;

/// RenderElement for Sliver protocol.
pub type SliverRenderElement<R> = RenderElement<R, SliverProtocol>;

// ============================================================================
// RENDER ELEMENT (Fully Generic)
// ============================================================================

/// Fully typed RenderElement with compile-time protocol safety.
///
/// This is the bridge between the element tree and render tree, corresponding
/// to Flutter's `RenderObjectElement`.
///
/// # Type Parameters
///
/// - `R: RenderObject` - The concrete render object type (e.g., RenderPadding)
/// - `P: Protocol` - The layout protocol (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// // Create a typed element for RenderPadding
/// let element: BoxRenderElement<RenderPadding> = RenderElement::new(
///     Some(render_id),
///     RuntimeArity::Exact(1),
/// );
///
/// // Direct access to typed state - no downcasting!
/// let size: Size = element.state().size();
/// let constraints: &BoxConstraints = element.state().constraints().unwrap();
/// ```
pub struct RenderElement<R: RenderObject, P: Protocol> {
    // ========== Identity ==========
    /// This element's ID (set during mount).
    id: Option<ElementId>,

    /// Parent element ID (None for root).
    parent: Option<ElementId>,

    /// Child element IDs.
    children: Vec<ElementId>,

    /// Depth in tree (0 for root).
    depth: usize,

    // ========== Render Object ==========
    /// Reference into RenderTree (four-tree architecture).
    render_id: Option<RenderId>,

    /// Runtime arity (child count validation).
    arity: RuntimeArity,

    // ========== Render State (Typed!) ==========
    /// Protocol-specific state - DIRECT, not Box<dyn>!
    state: RenderState<P>,

    // ========== Lifecycle ==========
    /// Current lifecycle state.
    lifecycle: RenderLifecycle,

    // ========== ParentData ==========
    /// Parent data set by parent (for positioning, flex, etc).
    parent_data: Option<Box<dyn ParentData>>,

    // ========== Debug ==========
    /// Debug name for diagnostics.
    debug_name: Option<&'static str>,

    // ========== PhantomData ==========
    /// Marker for RenderObject type.
    _phantom: PhantomData<R>,
}

impl<R: RenderObject, P: Protocol> fmt::Debug for RenderElement<R, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("type", &std::any::type_name::<R>())
            .field("protocol", &std::any::type_name::<P>())
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("children_count", &self.children.len())
            .field("depth", &self.depth)
            .field("arity", &self.arity)
            .field("offset", &self.state.offset())
            .field("lifecycle", &self.lifecycle)
            .field("flags", &self.state.flags().load())
            .field("debug_name", &self.debug_name())
            .finish()
    }
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    /// Creates new RenderElement with a RenderId reference.
    pub fn new(render_id: Option<RenderId>, arity: RuntimeArity) -> Self {
        Self {
            id: None,
            parent: None,
            children: Vec::new(),
            depth: 0,
            render_id,
            arity,
            state: RenderState::new(),
            lifecycle: RenderLifecycle::Detached,
            parent_data: None,
            debug_name: None,
            _phantom: PhantomData,
        }
    }

    /// Builder: set debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Builder: set initial render id.
    pub fn with_render_id(mut self, render_id: RenderId) -> Self {
        self.render_id = Some(render_id);
        self
    }
}

// ============================================================================
// TYPED STATE ACCESS (Zero-cost!)
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    /// Returns reference to typed render state.
    ///
    /// This is the key benefit of generic RenderElement - direct access
    /// to typed state without any downcasting or runtime checks!
    #[inline]
    pub fn state(&self) -> &RenderState<P> {
        &self.state
    }

    /// Returns mutable reference to typed render state.
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderState<P> {
        &mut self.state
    }

    /// Returns protocol ID at runtime.
    #[inline]
    pub fn protocol_id(&self) -> ProtocolId {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            ProtocolId::Box
        } else {
            ProtocolId::Sliver
        }
    }
}

// ============================================================================
// BOX PROTOCOL CONVENIENCE METHODS
// ============================================================================

impl<R: RenderObject> RenderElement<R, BoxProtocol> {
    /// Returns size (Box protocol only).
    #[inline]
    pub fn size(&self) -> Size {
        self.state.size()
    }

    /// Sets size (Box protocol only).
    #[inline]
    pub fn set_size(&self, size: Size) {
        self.state.set_size(size);
    }

    /// Returns constraints (Box protocol only).
    #[inline]
    pub fn constraints(&self) -> Option<&BoxConstraints> {
        self.state.constraints()
    }

    /// Sets constraints (Box protocol only).
    #[inline]
    pub fn set_constraints(&self, constraints: BoxConstraints) {
        self.state.set_constraints(constraints);
    }
}

// ============================================================================
// SLIVER PROTOCOL CONVENIENCE METHODS
// ============================================================================

impl<R: RenderObject> RenderElement<R, SliverProtocol> {
    /// Returns sliver geometry.
    #[inline]
    pub fn geometry(&self) -> Option<SliverGeometry> {
        self.state.geometry()
    }

    /// Sets sliver geometry.
    #[inline]
    pub fn set_geometry(&self, geometry: SliverGeometry) {
        self.state.set_sliver_geometry(geometry);
    }

    /// Returns sliver constraints.
    #[inline]
    pub fn sliver_constraints(&self) -> Option<&SliverConstraints> {
        self.state.constraints()
    }

    /// Sets sliver constraints.
    #[inline]
    pub fn set_sliver_constraints(&self, constraints: SliverConstraints) {
        self.state.set_constraints(constraints);
    }
}

// ============================================================================
// LIFECYCLE
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    /// Mounts element to tree.
    pub fn mount(&mut self, id: ElementId, parent: Option<ElementId>) {
        debug_assert!(
            self.lifecycle.is_detached(),
            "Cannot mount: already mounted (state: {:?})",
            self.lifecycle
        );

        self.id = Some(id);
        self.parent = parent;
        self.depth = if parent.is_some() { 0 } else { 0 };

        self.lifecycle.attach();
        self.state.flags().mark_needs_layout();
        self.state.flags().mark_needs_paint();
    }

    /// Unmounts element from tree.
    pub fn unmount(&mut self) {
        debug_assert!(
            self.lifecycle.is_attached(),
            "Cannot unmount: not attached (state: {:?})",
            self.lifecycle
        );

        self.id = None;
        self.parent = None;
        self.children.clear();
        self.depth = 0;

        self.lifecycle.detach();
    }

    /// Updates element when properties change.
    pub fn update(&mut self) {
        self.mark_needs_paint();
    }

    /// Activates element (for reparenting).
    pub fn activate(&mut self) {
        self.lifecycle.attach();
    }

    /// Deactivates element (for reparenting).
    pub fn deactivate(&mut self) {
        self.lifecycle.detach();
    }
}

// ============================================================================
// PARENT DATA MANAGEMENT
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    /// Sets up parent data for this child.
    pub fn setup_parent_data(&mut self, parent_data: Box<dyn ParentData>) {
        self.parent_data = Some(parent_data);
    }

    /// Returns parent data (if set).
    pub fn get_parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_deref()
    }

    /// Returns mutable parent data.
    pub fn get_parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_deref_mut()
    }

    /// Downcasts parent data to specific type.
    pub fn parent_data_as<T: ParentData>(&self) -> Option<&T> {
        self.parent_data
            .as_ref()
            .and_then(|pd| pd.as_any().downcast_ref::<T>())
    }

    /// Downcasts parent data to specific type (mutable).
    pub fn parent_data_as_mut<T: ParentData>(&mut self) -> Option<&mut T> {
        self.parent_data
            .as_mut()
            .and_then(|pd| pd.as_any_mut().downcast_mut::<T>())
    }
}

// ============================================================================
// IDENTITY & TREE NAVIGATION
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn id(&self) -> Option<ElementId> {
        self.id
    }

    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    #[inline]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    #[inline]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    #[inline]
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    #[inline]
    pub fn add_child(&mut self, child: ElementId) {
        self.children.push(child);
    }

    #[inline]
    pub fn remove_child(&mut self, child: ElementId) {
        self.children.retain(|&id| id != child);
    }

    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

// ============================================================================
// RENDER ID ACCESS
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    #[inline]
    pub fn set_render_id(&mut self, render_id: Option<RenderId>) {
        self.render_id = render_id;
    }

    #[inline]
    pub fn has_render_id(&self) -> bool {
        self.render_id.is_some()
    }
}

// ============================================================================
// ARITY
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn arity(&self) -> RuntimeArity {
        self.arity
    }

    #[inline]
    pub fn is_box(&self) -> bool {
        std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>()
    }

    #[inline]
    pub fn is_sliver(&self) -> bool {
        std::any::TypeId::of::<P>() == std::any::TypeId::of::<SliverProtocol>()
    }
}

// ============================================================================
// OFFSET (Common to all protocols)
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn offset(&self) -> Offset {
        self.state.offset()
    }

    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.state.set_offset(offset);
    }
}

// ============================================================================
// DIRTY FLAGS
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn flags(&self) -> &AtomicRenderFlags {
        self.state.flags()
    }

    pub fn mark_needs_layout(&mut self) {
        if self.flags().needs_layout() {
            return;
        }

        self.state.flags().mark_needs_layout();
        self.state.flags().mark_needs_paint();
        self.lifecycle.mark_needs_layout();
    }

    pub fn mark_needs_paint(&mut self) {
        if self.flags().needs_paint() {
            return;
        }

        self.state.flags().mark_needs_paint();

        if self.lifecycle.is_laid_out() {
            self.lifecycle.mark_needs_paint();
        }
    }

    pub fn mark_needs_compositing(&mut self) {
        self.state.flags().mark_needs_compositing();
    }

    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags().needs_layout()
    }

    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags().needs_paint()
    }

    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags().needs_compositing()
    }

    pub fn clear_needs_layout(&mut self) {
        self.state.flags().clear_needs_layout();

        if self.lifecycle.is_attached() && !self.lifecycle.is_laid_out() {
            self.lifecycle.mark_laid_out();
        }
    }

    pub fn clear_needs_paint(&mut self) {
        self.state.flags().clear_needs_paint();

        if self.lifecycle == RenderLifecycle::LaidOut {
            self.lifecycle.mark_painted();
        }
    }
}

// ============================================================================
// LIFECYCLE STATE
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    #[inline]
    pub fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    #[inline]
    pub fn is_detached(&self) -> bool {
        self.lifecycle.is_detached()
    }

    #[inline]
    pub fn is_laid_out(&self) -> bool {
        self.lifecycle.is_laid_out()
    }

    #[inline]
    pub fn is_painted(&self) -> bool {
        self.lifecycle.is_painted()
    }

    #[inline]
    pub fn is_clean(&self) -> bool {
        self.lifecycle.is_clean() && self.flags().is_clean()
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        !self.is_clean()
    }
}

// ============================================================================
// DEBUG
// ============================================================================

impl<R: RenderObject, P: Protocol> RenderElement<R, P> {
    pub fn debug_name(&self) -> &'static str {
        self.debug_name
            .unwrap_or_else(|| std::any::type_name::<R>())
    }

    pub fn set_debug_name(&mut self, name: &'static str) {
        self.debug_name = Some(name);
    }

    pub fn debug_description(&self) -> String {
        format!("{}#{:?} ({})", self.debug_name(), self.id, self.lifecycle)
    }
}

// ============================================================================
// RENDER ELEMENT NODE IMPLEMENTATION
// ============================================================================

impl<R: RenderObject + 'static, P: Protocol + 'static> RenderElementNode for RenderElement<R, P> {
    fn id(&self) -> Option<ElementId> {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children(&self) -> &[ElementId] {
        &self.children
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn protocol_id(&self) -> ProtocolId {
        RenderElement::protocol_id(self)
    }

    fn arity(&self) -> RuntimeArity {
        self.arity
    }

    fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    fn set_render_id(&mut self, render_id: Option<RenderId>) {
        self.render_id = render_id;
    }

    fn offset(&self) -> Offset {
        self.state.offset()
    }

    fn set_offset(&mut self, offset: Offset) {
        self.state.set_offset(offset);
    }

    fn flags(&self) -> &AtomicRenderFlags {
        self.state.flags()
    }

    fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_deref()
    }

    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_deref_mut()
    }

    fn set_parent_data(&mut self, parent_data: Box<dyn ParentData>) {
        self.parent_data = Some(parent_data);
    }

    fn as_box_state(&self) -> Option<&BoxRenderState> {
        // Runtime type check for protocol conversion
        //
        // SAFETY INVARIANTS:
        // 1. RenderState<P> has identical memory layout for all P (verified by #[repr(C)] or layout tests)
        // 2. TypeId check guarantees P == BoxProtocol before cast
        // 3. Both BoxProtocol and SliverProtocol are zero-sized marker types
        // 4. The only difference is the PhantomData<P> which is zero-sized
        //
        // This is a sound transmute because:
        // - Same size and alignment (verified at compile time by Protocol trait bounds)
        // - Same field layout (AtomicRenderFlags, OnceCell<Geometry>, OnceCell<Constraints>, AtomicOffset)
        // - PhantomData doesn't affect layout
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            // SAFETY: TypeId check guarantees P == BoxProtocol
            // RenderState<BoxProtocol> and RenderState<P> have identical layout
            Some(unsafe {
                &*(&self.state as *const RenderState<P> as *const RenderState<BoxProtocol>)
            })
        } else {
            None
        }
    }

    fn as_box_state_mut(&mut self) -> Option<&mut BoxRenderState> {
        // See safety documentation in as_box_state()
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            // SAFETY: TypeId check guarantees P == BoxProtocol
            Some(unsafe {
                &mut *(&mut self.state as *mut RenderState<P> as *mut RenderState<BoxProtocol>)
            })
        } else {
            None
        }
    }

    fn as_sliver_state(&self) -> Option<&SliverRenderState> {
        // See safety documentation in as_box_state()
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<SliverProtocol>() {
            // SAFETY: TypeId check guarantees P == SliverProtocol
            Some(unsafe {
                &*(&self.state as *const RenderState<P> as *const RenderState<SliverProtocol>)
            })
        } else {
            None
        }
    }

    fn as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState> {
        // See safety documentation in as_box_state()
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<SliverProtocol>() {
            // SAFETY: TypeId check guarantees P == SliverProtocol
            Some(unsafe {
                &mut *(&mut self.state as *mut RenderState<P> as *mut RenderState<SliverProtocol>)
            })
        } else {
            None
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        RenderElement::debug_name(self)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::RenderObject;
    use crate::BoxConstraints;
    use crate::RenderResult;
    use flui_painting::Canvas;

    // Minimal test RenderObject
    #[derive(Debug)]
    struct TestRenderObject;

    impl RenderObject for TestRenderObject {
        fn perform_layout(
            &mut self,
            _element_id: ElementId,
            constraints: BoxConstraints,
            _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> RenderResult<Size>,
        ) -> RenderResult<Size> {
            Ok(constraints.smallest())
        }

        fn paint(
            &self,
            _element_id: ElementId,
            _offset: Offset,
            _size: Size,
            _canvas: &mut Canvas,
            _paint_child: &mut dyn FnMut(ElementId, Offset, &mut Canvas),
        ) {
        }
    }

    #[test]
    fn test_new_element() {
        let render_id = RenderId::new(1);
        let element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(Some(render_id), RuntimeArity::Exact(0));

        assert!(element.has_render_id());
        assert_eq!(element.render_id(), Some(render_id));
        assert!(element.is_detached());
        assert_eq!(element.child_count(), 0);
        assert!(element.is_box());
        assert!(!element.is_sliver());
    }

    #[test]
    fn test_typed_state_access() {
        let element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(None, RuntimeArity::Exact(0));

        // Direct typed access - no downcasting!
        let state: &RenderState<BoxProtocol> = element.state();
        assert_eq!(state.size(), Size::ZERO);

        // Protocol-specific convenience methods
        assert_eq!(element.size(), Size::ZERO);
    }

    #[test]
    fn test_mount_unmount() {
        let render_id = RenderId::new(1);
        let mut element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(Some(render_id), RuntimeArity::Exact(0));

        let id = ElementId::new(1);
        element.mount(id, None);

        assert!(element.is_attached());
        assert_eq!(element.id(), Some(id));
        assert_eq!(element.depth(), 0);

        element.unmount();

        assert!(element.is_detached());
        assert_eq!(element.id(), None);
    }

    #[test]
    fn test_children() {
        let render_id = RenderId::new(1);
        let mut element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(Some(render_id), RuntimeArity::Exact(0));

        let child1 = ElementId::new(10);
        let child2 = ElementId::new(20);

        element.add_child(child1);
        element.add_child(child2);

        assert_eq!(element.child_count(), 2);
        assert!(element.has_children());

        element.remove_child(child1);
        assert_eq!(element.child_count(), 1);
    }

    #[test]
    fn test_dirty_flags() {
        let render_id = RenderId::new(1);
        let mut element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(Some(render_id), RuntimeArity::Exact(0));

        element.mount(ElementId::new(1), None);

        assert!(element.needs_layout());
        assert!(element.needs_paint());

        element.clear_needs_layout();
        assert!(!element.needs_layout());
        assert!(element.needs_paint());

        element.clear_needs_paint();
        assert!(!element.needs_paint());
        assert!(element.is_clean());

        element.mark_needs_paint();
        assert!(!element.needs_layout());
        assert!(element.needs_paint());
    }

    #[test]
    fn test_render_element_node_trait() {
        use crate::element_node::ElementNodeStorage;

        let element: BoxRenderElement<TestRenderObject> =
            RenderElement::new(None, RuntimeArity::Exact(0));

        // Store as type-erased
        let storage = ElementNodeStorage::new(element);

        // Access via trait
        assert_eq!(storage.protocol_id(), ProtocolId::Box);
        assert!(storage.is_box());

        // Downcast back
        let typed = storage.downcast_ref::<BoxRenderElement<TestRenderObject>>();
        assert!(typed.is_some());
    }
}
