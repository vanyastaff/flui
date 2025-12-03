//! RenderElement - Element that owns and manages a RenderObject.
//!
//! This is the bridge between the element tree and render tree, corresponding
//! to Flutter's `RenderObjectElement`.
//!
//! # Flutter RenderObjectElement
//!
//! Flutter has a three-layer architecture:
//! 1. **Widget** - immutable configuration (React-like)
//! 2. **Element** - mutable instance, manages lifecycle
//! 3. **RenderObject** - mutable, does actual layout/paint
//!
//! ```dart
//! abstract class RenderObjectElement extends Element {
//!   RenderObject? _renderObject;
//!
//!   @override
//!   void mount(Element? parent, Object? newSlot) {
//!     super.mount(parent, newSlot);
//!     _renderObject = widget.createRenderObject(this);
//!     attachRenderObject(newSlot);
//!     super.performRebuild();
//!   }
//!
//!   @override
//!   void update(RenderObjectWidget newWidget) {
//!     super.update(newWidget);
//!     widget.updateRenderObject(this, renderObject);
//!     _dirty = false;
//!   }
//!
//!   @override
//!   void unmount() {
//!     super.unmount();
//!     widget.didUnmountRenderObject(renderObject);
//!   }
//! }
//! ```
//!
//! # FLUI Architecture
//!
//! FLUI doesn't use separate Widget layer (too much overhead for Rust).
//! Instead, RenderElement directly owns RenderObject:
//!
//! ```text
//! ┌─────────────────────────────────┐
//! │     Element Tree (flui-element) │  ← Manages lifecycle, rebuild
//! │  - Component, Render, Provider  │
//! └─────────────────────────────────┘
//!              ↓ owns
//! ┌─────────────────────────────────┐
//! │   RenderElement (this file)     │  ← Bridge to rendering
//! │  - Lifecycle management         │
//! │  - ParentData setup             │
//! │  - Dirty tracking               │
//! └─────────────────────────────────┘
//!              ↓ owns
//! ┌─────────────────────────────────┐
//! │   RenderObject (render_object.rs)│  ← Does layout/paint
//! │  - layout(), paint(), hit_test() │
//! └─────────────────────────────────┘
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::ElementId;
use flui_tree::RuntimeArity;
use flui_types::{Offset, Size};

use super::parent_data::ParentData;
use super::protocol::{BoxProtocol, Protocol, ProtocolId, SliverProtocol};
use super::render_flags::{AtomicRenderFlags, RenderFlags};
use super::render_lifecycle::RenderLifecycle;
use super::render_object::RenderObject;
use super::render_state::RenderState;
use super::{BoxConstraints, SliverConstraints, SliverGeometry};

// ============================================================================
// PROTOCOL STATE (Type-Erased RenderState)
// ============================================================================

/// Type-erased protocol state for RenderElement.
///
/// This allows RenderElement to work with both Box and Sliver protocols
/// without being generic over Protocol (which would infect entire tree).
///
/// Implemented by RenderState<P> via blanket impl.
trait ProtocolState: fmt::Debug + Send + Sync {
    /// Returns atomic render flags.
    fn flags(&self) -> &AtomicRenderFlags;

    /// Returns current offset.
    fn offset(&self) -> Offset;

    /// Sets offset.
    fn set_offset(&self, offset: Offset);

    /// Clones the state.
    fn clone_state(&self) -> Box<dyn ProtocolState>;

    /// Returns protocol ID.
    fn protocol_id(&self) -> ProtocolId;

    /// Downcasts to BoxRenderState (if Box protocol).
    fn as_box_state(&self) -> Option<&RenderState<BoxProtocol>>;

    /// Downcasts to BoxRenderState (mutable).
    fn as_box_state_mut(&mut self) -> Option<&mut RenderState<BoxProtocol>>;

    /// Downcasts to SliverRenderState (if Sliver protocol).
    fn as_sliver_state(&self) -> Option<&RenderState<SliverProtocol>>;

    /// Downcasts to SliverRenderState (mutable).
    fn as_sliver_state_mut(&mut self) -> Option<&mut RenderState<SliverProtocol>>;
}

// Blanket impl for RenderState<P>
impl<P: Protocol> ProtocolState for RenderState<P> {
    fn flags(&self) -> &AtomicRenderFlags {
        &self.flags
    }

    fn offset(&self) -> Offset {
        self.offset()
    }

    fn set_offset(&self, offset: Offset) {
        self.set_offset(offset)
    }

    fn clone_state(&self) -> Box<dyn ProtocolState> {
        Box::new(self.clone())
    }

    fn protocol_id(&self) -> ProtocolId {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            ProtocolId::Box
        } else {
            ProtocolId::Sliver
        }
    }

    fn as_box_state(&self) -> Option<&RenderState<BoxProtocol>> {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            // SAFETY: We just checked TypeId matches
            Some(unsafe { &*(self as *const RenderState<P> as *const RenderState<BoxProtocol>) })
        } else {
            None
        }
    }

    fn as_box_state_mut(&mut self) -> Option<&mut RenderState<BoxProtocol>> {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            // SAFETY: We just checked TypeId matches
            Some(unsafe { &mut *(self as *mut RenderState<P> as *mut RenderState<BoxProtocol>) })
        } else {
            None
        }
    }

    fn as_sliver_state(&self) -> Option<&RenderState<SliverProtocol>> {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<SliverProtocol>() {
            // SAFETY: We just checked TypeId matches
            Some(unsafe { &*(self as *const RenderState<P> as *const RenderState<SliverProtocol>) })
        } else {
            None
        }
    }

    fn as_sliver_state_mut(&mut self) -> Option<&mut RenderState<SliverProtocol>> {
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<SliverProtocol>() {
            // SAFETY: We just checked TypeId matches
            Some(unsafe { &mut *(self as *mut RenderState<P> as *mut RenderState<SliverProtocol>) })
        } else {
            None
        }
    }
}

// ============================================================================
// RENDER ELEMENT
// ============================================================================

/// Element that owns and manages a RenderObject.
///
/// Flutter equivalent: `RenderObjectElement`
///
/// # Responsibilities
///
/// - **Lifecycle**: mount, unmount, attach, detach
/// - **Updates**: rebuild when properties change
/// - **ParentData**: setup parent data for children
/// - **Dirty tracking**: mark_needs_layout, mark_needs_paint
/// - **Tree navigation**: parent, children, depth
///
/// # Example
///
/// ```rust,ignore
/// // Create RenderElement
/// let render_obj = RenderOpacity::new(0.5);
/// let element = RenderElement::new(render_obj, ProtocolId::Box);
///
/// // Mount to tree
/// element.mount(Some(parent_id), tree);
///
/// // Update when property changes
/// if opacity_changed {
///     element.mark_needs_paint();
/// }
///
/// // Unmount when removed
/// element.unmount(tree);
/// ```
pub struct RenderElement {
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
    /// Owned render object (type-erased).
    render_object: Box<dyn RenderObject>,

    /// Protocol (Box or Sliver).
    protocol: ProtocolId,

    /// Runtime arity (child count validation).
    arity: RuntimeArity,

    // ========== Render State (Protocol-specific) ==========
    /// Protocol-specific state (geometry, constraints, flags, offset).
    ///
    /// This is Box<dyn ProtocolState> to allow Box or Sliver protocol.
    /// Contains:
    /// - AtomicRenderFlags (lock-free dirty tracking)
    /// - OnceCell<Geometry> (Size or SliverGeometry)
    /// - OnceCell<Constraints> (BoxConstraints or SliverConstraints)
    /// - AtomicOffset (paint position)
    ///
    /// Why type-erased? RenderElement needs to work with both protocols
    /// without being generic over Protocol (would infect entire tree).
    state: Box<dyn ProtocolState>,

    // ========== Lifecycle ==========
    /// Current lifecycle state.
    lifecycle: RenderLifecycle,

    // ========== ParentData ==========
    /// Parent data set by parent (for positioning, flex, etc).
    ///
    /// Flutter: setupParentData() called by parent on child
    parent_data: Option<Box<dyn ParentData>>,

    // ========== Debug ==========
    /// Debug name for diagnostics.
    debug_name: Option<&'static str>,
}

impl fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("children_count", &self.children.len())
            .field("depth", &self.depth)
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("size", &self.size)
            .field("offset", &self.offset)
            .field("lifecycle", &self.lifecycle)
            .field("flags", &self.flags.load())
            .field("debug_name", &self.debug_name())
            .finish()
    }
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl RenderElement {
    /// Creates new RenderElement with render object.
    ///
    /// Element starts in Detached state and must be mounted to tree.
    ///
    /// Flutter equivalent: Element created but not yet mounted
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render = RenderOpacity::new(0.5);
    /// let element = RenderElement::new(render, ProtocolId::Box);
    /// assert!(element.is_detached());
    /// ```
    pub fn new<R: RenderObject>(render_object: R, protocol: ProtocolId) -> Self {
        let state: Box<dyn ProtocolState> = match protocol {
            ProtocolId::Box => Box::new(RenderState::<BoxProtocol>::new()),
            ProtocolId::Sliver => Box::new(RenderState::<SliverProtocol>::new()),
        };

        Self {
            id: None,
            parent: None,
            children: Vec::new(),
            depth: 0,
            render_object: Box::new(render_object),
            protocol,
            arity: RuntimeArity::Exact(0),
            state,
            lifecycle: RenderLifecycle::Detached,
            parent_data: None,
            debug_name: None,
        }
    }

    /// Creates with specified arity.
    pub fn with_arity<R: RenderObject>(
        render_object: R,
        protocol: ProtocolId,
        arity: RuntimeArity,
    ) -> Self {
        let mut element = Self::new(render_object, protocol);
        element.arity = arity;
        element
    }

    /// Creates from boxed render object.
    pub fn from_boxed(
        render_object: Box<dyn RenderObject>,
        protocol: ProtocolId,
        arity: RuntimeArity,
    ) -> Self {
        let state: Box<dyn ProtocolState> = match protocol {
            ProtocolId::Box => Box::new(RenderState::<BoxProtocol>::new()),
            ProtocolId::Sliver => Box::new(RenderState::<SliverProtocol>::new()),
        };

        Self {
            id: None,
            parent: None,
            children: Vec::new(),
            depth: 0,
            render_object,
            protocol,
            arity,
            state,
            lifecycle: RenderLifecycle::Detached,
            parent_data: None,
            debug_name: None,
        }
    }

    /// Builder: set debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }
}

// ============================================================================
// LIFECYCLE (Flutter RenderObjectElement)
// ============================================================================

impl RenderElement {
    /// Mounts element to tree.
    ///
    /// Flutter equivalent: `mount(Element? parent, Object? newSlot)`
    ///
    /// This:
    /// 1. Sets parent and depth
    /// 2. Attaches render object to render tree
    /// 3. Calls setupParentData on render object
    /// 4. Transitions to Attached state
    ///
    /// # Flutter
    ///
    /// ```dart
    /// @override
    /// void mount(Element? parent, Object? newSlot) {
    ///   super.mount(parent, newSlot);
    ///   _renderObject = widget.createRenderObject(this);
    ///   assert(_renderObject != null);
    ///   attachRenderObject(newSlot);
    ///   super.performRebuild(); // Build children
    /// }
    /// ```
    pub fn mount(&mut self, id: ElementId, parent: Option<ElementId>) {
        debug_assert!(
            self.lifecycle.is_detached(),
            "Cannot mount: already mounted (state: {:?})",
            self.lifecycle
        );

        self.id = Some(id);
        self.parent = parent;

        // Calculate depth
        self.depth = if let Some(parent_id) = parent {
            // TODO: Get parent depth from tree
            // parent.depth + 1
            0 // Placeholder
        } else {
            0 // Root
        };

        // Attach render object
        self.attach_render_object();

        // Transition to Attached
        self.lifecycle.attach();
        self.flags.mark_needs_layout();
        self.flags.mark_needs_paint();
    }

    /// Attaches render object to render tree.
    ///
    /// Flutter equivalent: `attachRenderObject(Object? newSlot)`
    ///
    /// This inserts the render object into parent's child list.
    fn attach_render_object(&mut self) {
        // Call RenderObject.attach()
        if let Some(id) = self.id {
            self.render_object.attach(id);
        }

        // TODO: Insert into parent's render object child list
        // if let Some(parent_id) = self.parent {
        //     let parent_render = tree.get_render_element(parent_id);
        //     parent_render.insert_child_render_object(self.render_object);
        // }
    }

    /// Detaches render object from render tree.
    ///
    /// Flutter equivalent: `detachRenderObject()`
    fn detach_render_object(&mut self) {
        // Call RenderObject.detach()
        self.render_object.detach();

        // TODO: Remove from parent's render object child list
    }

    /// Unmounts element from tree.
    ///
    /// Flutter equivalent: `unmount()`
    ///
    /// This:
    /// 1. Detaches render object
    /// 2. Clears parent and children
    /// 3. Transitions to Detached state
    ///
    /// # Flutter
    ///
    /// ```dart
    /// @override
    /// void unmount() {
    ///   widget.didUnmountRenderObject(renderObject);
    ///   super.unmount();
    /// }
    /// ```
    pub fn unmount(&mut self) {
        debug_assert!(
            self.lifecycle.is_attached(),
            "Cannot unmount: not attached (state: {:?})",
            self.lifecycle
        );

        // Detach render object
        self.detach_render_object();

        // Clear state
        self.id = None;
        self.parent = None;
        self.children.clear();
        self.depth = 0;

        // Transition to Detached
        self.lifecycle.detach();
    }

    /// Updates element when properties change.
    ///
    /// Flutter equivalent: `update(RenderObjectWidget newWidget)`
    ///
    /// This is called when parent rebuilds with new configuration.
    /// RenderObject should update its properties and mark dirty.
    ///
    /// # Flutter
    ///
    /// ```dart
    /// @override
    /// void update(RenderObjectWidget newWidget) {
    ///   super.update(newWidget);
    ///   widget.updateRenderObject(this, renderObject);
    ///   _dirty = false;
    /// }
    /// ```
    ///
    /// # FLUI
    ///
    /// In FLUI, we don't have separate Widget, so this is called
    /// when RenderObject properties change:
    ///
    /// ```rust,ignore
    /// // Widget updates opacity
    /// render_opacity.set_opacity(0.8);
    ///
    /// // Element detects change and updates
    /// element.update(); // Marks needs_paint
    /// ```
    pub fn update(&mut self) {
        // In FLUI, RenderObject properties are updated directly
        // This method just ensures proper dirty marking

        // Most updates require repaint
        self.mark_needs_paint();

        // Some updates require relayout
        // (RenderObject should call mark_needs_layout directly)
    }

    /// Activates element (for reparenting).
    ///
    /// Flutter equivalent: `activate()`
    ///
    /// Called when element is moved to new location in tree.
    pub fn activate(&mut self) {
        // Re-attach to tree
        self.lifecycle.attach();
    }

    /// Deactivates element (for reparenting).
    ///
    /// Flutter equivalent: `deactivate()`
    pub fn deactivate(&mut self) {
        // Temporarily detach
        self.lifecycle.detach();
    }
}

// ============================================================================
// PARENT DATA MANAGEMENT (Flutter setupParentData)
// ============================================================================

impl RenderElement {
    /// Sets up parent data for this child.
    ///
    /// Flutter equivalent: `RenderObject.setupParentData(RenderObject child)`
    ///
    /// Called by parent when adding child. Parent creates appropriate
    /// ParentData type and attaches it to child.
    ///
    /// # Flutter
    ///
    /// ```dart
    /// // In RenderFlex (parent):
    /// @override
    /// void setupParentData(RenderObject child) {
    ///   if (child.parentData is! FlexParentData) {
    ///     child.parentData = FlexParentData();
    ///   }
    /// }
    /// ```
    ///
    /// # FLUI
    ///
    /// ```rust,ignore
    /// // Parent calls this when adding child
    /// let parent_data = FlexParentData::new();
    /// child_element.setup_parent_data(Box::new(parent_data));
    /// ```
    pub fn setup_parent_data(&mut self, parent_data: Box<dyn ParentData>) {
        self.parent_data = Some(parent_data);
    }

    /// Returns parent data (if set).
    pub fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_ref().map(|pd| &**pd)
    }

    /// Returns mutable parent data.
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_mut().map(|pd| &mut **pd)
    }

    /// Downcasts parent data to specific type.
    ///
    /// # Flutter
    ///
    /// ```dart
    /// final flexData = child.parentData as FlexParentData;
    /// flexData.flex = 2;
    /// ```
    ///
    /// # FLUI
    ///
    /// ```rust,ignore
    /// let flex_data = child.parent_data_as::<FlexParentData>()?;
    /// flex_data.set_flex(2);
    /// ```
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

impl RenderElement {
    /// Returns element ID.
    #[inline]
    pub fn id(&self) -> Option<ElementId> {
        self.id
    }

    /// Returns parent element ID.
    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Sets parent (used during reparenting).
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Returns children element IDs.
    #[inline]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Returns mutable children vector.
    #[inline]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Returns depth in tree.
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Sets depth (used during reparenting).
    #[inline]
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Adds child.
    #[inline]
    pub fn add_child(&mut self, child: ElementId) {
        self.children.push(child);
    }

    /// Removes child.
    #[inline]
    pub fn remove_child(&mut self, child: ElementId) {
        self.children.retain(|&id| id != child);
    }

    /// Returns child count.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Checks if has children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

// ============================================================================
// RENDER OBJECT ACCESS
// ============================================================================

impl RenderElement {
    /// Returns render object reference.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Returns mutable render object reference.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    /// Downcasts render object to concrete type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(opacity) = element.render_object_as::<RenderOpacity>() {
    ///     println!("Opacity: {}", opacity.opacity());
    /// }
    /// ```
    #[inline]
    pub fn render_object_as<T: RenderObject>(&self) -> Option<&T> {
        self.render_object.as_any().downcast_ref::<T>()
    }

    /// Downcasts render object to concrete type (mutable).
    #[inline]
    pub fn render_object_as_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.render_object.as_any_mut().downcast_mut::<T>()
    }

    /// Returns protocol.
    #[inline]
    pub fn protocol(&self) -> ProtocolId {
        self.protocol
    }

    /// Returns runtime arity.
    #[inline]
    pub fn arity(&self) -> RuntimeArity {
        self.arity
    }

    /// Checks if Box protocol.
    #[inline]
    pub fn is_box(&self) -> bool {
        self.protocol == ProtocolId::Box
    }

    /// Checks if Sliver protocol.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        self.protocol == ProtocolId::Sliver
    }
}

// ============================================================================
// LAYOUT CACHE
// ============================================================================

impl RenderElement {
    /// Returns computed size (Box protocol).
    #[inline]
    pub fn size(&self) -> Size {
        self.size
    }

    /// Sets size (called after layout).
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
        self.flags.mark_has_geometry();
    }

    /// Returns offset relative to parent.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets offset (called by parent during layout).
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns last box constraints.
    #[inline]
    pub fn constraints_box(&self) -> Option<BoxConstraints> {
        self.constraints_box
    }

    /// Sets box constraints (called during layout).
    #[inline]
    pub fn set_constraints_box(&mut self, constraints: BoxConstraints) {
        self.constraints_box = Some(constraints);
    }

    /// Returns last sliver constraints.
    #[inline]
    pub fn constraints_sliver(&self) -> Option<SliverConstraints> {
        self.constraints_sliver
    }

    /// Sets sliver constraints (called during layout).
    #[inline]
    pub fn set_constraints_sliver(&mut self, constraints: SliverConstraints) {
        self.constraints_sliver = Some(constraints);
    }
}

// ============================================================================
// DIRTY FLAGS (Flutter-style)
// ============================================================================

impl RenderElement {
    /// Marks element as needing layout.
    ///
    /// Flutter equivalent: `markNeedsLayout()`
    ///
    /// This propagates upward to relayout boundary or root.
    ///
    /// # Flutter
    ///
    /// ```dart
    /// void markNeedsLayout() {
    ///   if (_needsLayout) return;
    ///   if (_relayoutBoundary != this) {
    ///     markParentNeedsLayout();
    ///   } else {
    ///     _needsLayout = true;
    ///     owner!._nodesNeedingLayout.add(this);
    ///   }
    /// }
    /// ```
    pub fn mark_needs_layout(&mut self) {
        if self.flags.needs_layout() {
            return; // Already marked
        }

        self.flags.mark_needs_layout();
        self.flags.mark_needs_paint(); // Layout changes require repaint
        self.lifecycle.mark_needs_layout();

        // TODO: Check relayout boundary
        // if self.is_relayout_boundary() {
        //     pipeline.add_to_layout_queue(self.id);
        // } else {
        //     parent.mark_needs_layout();
        // }
    }

    /// Marks element as needing paint (layout still valid).
    ///
    /// Flutter equivalent: `markNeedsPaint()`
    ///
    /// This propagates upward to repaint boundary or root.
    ///
    /// # Flutter
    ///
    /// ```dart
    /// void markNeedsPaint() {
    ///   if (_needsPaint) return;
    ///   _needsPaint = true;
    ///   if (isRepaintBoundary) {
    ///     owner!._nodesNeedingPaint.add(this);
    ///   } else {
    ///     parent?.markNeedsPaint();
    ///   }
    /// }
    /// ```
    pub fn mark_needs_paint(&mut self) {
        if self.flags.needs_paint() {
            return;
        }

        self.flags.mark_needs_paint();

        if self.lifecycle.is_laid_out() {
            self.lifecycle.mark_needs_paint();
        }

        // TODO: Check repaint boundary
        // if self.is_repaint_boundary() {
        //     pipeline.add_to_paint_queue(self.id);
        // } else {
        //     parent.mark_needs_paint();
        // }
    }

    /// Marks compositing bits update needed.
    pub fn mark_needs_compositing(&mut self) {
        self.flags.mark_needs_compositing();
    }

    /// Checks if needs layout.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }

    /// Checks if needs paint.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }

    /// Checks if needs compositing.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.needs_compositing()
    }

    /// Clears needs layout flag (after layout completes).
    pub fn clear_needs_layout(&mut self) {
        self.flags.clear_needs_layout();

        if self.lifecycle.is_attached() && !self.lifecycle.is_laid_out() {
            self.lifecycle.mark_laid_out();
        }
    }

    /// Clears needs paint flag (after paint completes).
    pub fn clear_needs_paint(&mut self) {
        self.flags.clear_needs_paint();

        if self.lifecycle == RenderLifecycle::LaidOut {
            self.lifecycle.mark_painted();
        }
    }
}

// ============================================================================
// LIFECYCLE STATE
// ============================================================================

impl RenderElement {
    /// Returns current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Checks if attached to tree.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    /// Checks if detached.
    #[inline]
    pub fn is_detached(&self) -> bool {
        self.lifecycle.is_detached()
    }

    /// Checks if laid out.
    #[inline]
    pub fn is_laid_out(&self) -> bool {
        self.lifecycle.is_laid_out()
    }

    /// Checks if painted.
    #[inline]
    pub fn is_painted(&self) -> bool {
        self.lifecycle.is_painted()
    }

    /// Checks if clean (no work needed).
    #[inline]
    pub fn is_clean(&self) -> bool {
        self.lifecycle.is_clean() && self.flags.is_clean()
    }

    /// Checks if dirty (needs work).
    #[inline]
    pub fn is_dirty(&self) -> bool {
        !self.is_clean()
    }
}

// ============================================================================
// DEBUG
// ============================================================================

impl RenderElement {
    /// Returns debug name.
    pub fn debug_name(&self) -> &str {
        self.debug_name
            .unwrap_or_else(|| self.render_object.debug_name())
    }

    /// Sets debug name.
    pub fn set_debug_name(&mut self, name: &'static str) {
        self.debug_name = Some(name);
    }

    /// Returns debug description.
    pub fn debug_description(&self) -> String {
        format!("{}#{:?} ({})", self.debug_name(), self.id, self.lifecycle)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    impl RenderObject for TestRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn debug_name(&self) -> &str {
            "TestRenderObject"
        }
    }

    #[test]
    fn test_new_element() {
        let element = RenderElement::new(TestRenderObject { value: 42 }, ProtocolId::Box);

        assert!(element.is_detached());
        assert!(element.needs_layout());
        assert!(element.needs_paint());
        assert_eq!(element.child_count(), 0);
    }

    #[test]
    fn test_mount_unmount() {
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

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
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

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
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

        element.mount(ElementId::new(1), None);

        // Initially needs layout and paint
        assert!(element.needs_layout());
        assert!(element.needs_paint());

        // Clear layout
        element.clear_needs_layout();
        assert!(!element.needs_layout());
        assert!(element.needs_paint()); // Still needs paint

        // Clear paint
        element.clear_needs_paint();
        assert!(!element.needs_paint());
        assert!(element.is_clean());

        // Mark needs paint only
        element.mark_needs_paint();
        assert!(!element.needs_layout());
        assert!(element.needs_paint());
    }

    #[test]
    fn test_downcast() {
        let element = RenderElement::new(TestRenderObject { value: 42 }, ProtocolId::Box);

        let ro = element.render_object_as::<TestRenderObject>().unwrap();
        assert_eq!(ro.value, 42);
    }
}
