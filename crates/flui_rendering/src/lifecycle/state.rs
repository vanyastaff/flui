//! Unified render object state storage.
//!
//! This module provides [`RenderObjectState`] - a compact struct that stores
//! all common state for render objects, replacing scattered fields with a
//! unified, memory-efficient representation.

use std::sync::Arc;

use parking_lot::RwLock;

use super::{DirtyFlags, RenderLifecycle, RenderState};
use crate::pipeline::PipelineOwner;
use crate::traits::RenderObject;

// ============================================================================
// RenderObjectState
// ============================================================================

/// Unified state storage for render objects.
///
/// This struct combines lifecycle state with tree position information,
/// providing a single source of truth for render object state.
///
/// # Memory Layout
///
/// ```text
/// RenderObjectState (48 bytes on 64-bit):
/// - render_state: RenderState (2 bytes)
/// - depth: u16 (2 bytes)
/// - padding: 4 bytes
/// - owner: Option<Arc<RwLock<PipelineOwner>>> (8 bytes)
/// - parent: Option<*const dyn RenderObject> (16 bytes - wide pointer)
/// - node_id: usize (8 bytes)
/// ```
///
/// Compare to storing these fields separately with individual booleans:
/// - needs_layout: bool (1 byte + padding)
/// - needs_paint: bool (1 byte + padding)
/// - needs_compositing_bits_update: bool (1 byte + padding)
/// - needs_semantics_update: bool (1 byte + padding)
/// - is_relayout_boundary: bool (1 byte + padding)
/// - is_repaint_boundary: bool (1 byte + padding)
/// - ... many more fields
///
/// # Usage
///
/// ```ignore
/// pub struct MyRenderBox {
///     state: RenderObjectState,
///     // ... other fields
/// }
///
/// impl MyRenderBox {
///     pub fn mark_needs_layout(&mut self) {
///         self.state.mark_needs_layout();
///     }
/// }
/// ```
pub struct RenderObjectState {
    /// Combined lifecycle and dirty flags (2 bytes).
    render_state: RenderState,

    /// Depth in the render tree (root = 0).
    /// Using u16 allows trees up to 65535 levels deep.
    depth: u16,

    /// The pipeline owner that manages this render object.
    owner: Option<Arc<RwLock<PipelineOwner>>>,

    /// Pointer to parent render object (wide pointer with vtable).
    /// Using raw pointer to avoid circular references.
    /// Safety: Only valid while attached to tree.
    parent: Option<*const dyn RenderObject>,

    /// Unique identifier for this node in the pipeline owner's dirty lists.
    node_id: usize,
}

impl std::fmt::Debug for RenderObjectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderObjectState")
            .field("render_state", &self.render_state)
            .field("depth", &self.depth)
            .field("owner", &self.owner.as_ref().map(|_| "..."))
            .field("parent", &self.parent.map(|p| p as *const ()))
            .field("node_id", &self.node_id)
            .finish()
    }
}

// Safety: RenderObjectState is Send + Sync because:
// - RenderState is Copy and contains no pointers
// - depth is Copy
// - Arc<RwLock<PipelineOwner>> is Send + Sync
// - parent is only dereferenced while attached (single-threaded tree ops)
// - node_id is Copy
unsafe impl Send for RenderObjectState {}
unsafe impl Sync for RenderObjectState {}

impl Default for RenderObjectState {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObjectState {
    /// Creates a new render object state in detached state.
    #[inline]
    pub fn new() -> Self {
        Self {
            render_state: RenderState::new(),
            depth: 0,
            owner: None,
            parent: None,
            node_id: 0,
        }
    }

    /// Creates a new render object state with a specific node ID.
    #[inline]
    pub fn with_node_id(node_id: usize) -> Self {
        Self {
            render_state: RenderState::new(),
            depth: 0,
            owner: None,
            parent: None,
            node_id,
        }
    }

    // ========================================================================
    // Node Identity
    // ========================================================================

    /// Returns the unique node ID.
    #[inline]
    pub fn node_id(&self) -> usize {
        self.node_id
    }

    /// Sets the node ID.
    #[inline]
    pub fn set_node_id(&mut self, id: usize) {
        self.node_id = id;
    }

    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Returns the depth in the render tree.
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth as usize
    }

    /// Sets the depth in the render tree.
    #[inline]
    pub fn set_depth(&mut self, depth: usize) {
        debug_assert!(depth <= u16::MAX as usize, "Tree depth exceeds u16::MAX");
        self.depth = depth as u16;
    }

    /// Returns the pipeline owner, if attached.
    #[inline]
    pub fn owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        self.owner.as_ref()
    }

    /// Returns whether this object is attached to a pipeline owner.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.owner.is_some()
    }

    /// Returns the parent pointer, if any.
    ///
    /// # Safety
    ///
    /// The returned pointer is only valid while the render object is attached
    /// to the tree. Do not store or dereference after detachment.
    #[inline]
    pub fn parent_ptr(&self) -> Option<*const dyn RenderObject> {
        self.parent
    }

    /// Sets the parent pointer.
    ///
    /// Pass `None` to clear the parent reference.
    #[inline]
    pub fn set_parent_ptr(&mut self, parent: Option<*const dyn RenderObject>) {
        self.parent = parent;
    }

    // ========================================================================
    // Lifecycle Access
    // ========================================================================

    /// Returns the current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.render_state.lifecycle()
    }

    /// Returns the dirty flags.
    #[inline]
    pub fn flags(&self) -> DirtyFlags {
        self.render_state.flags()
    }

    /// Returns the underlying render state.
    #[inline]
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Returns mutable access to render state.
    #[inline]
    pub fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    // ========================================================================
    // Lifecycle Queries (Delegated)
    // ========================================================================

    /// Returns whether layout is needed.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.render_state.needs_layout()
    }

    /// Returns whether paint is needed.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.render_state.needs_paint()
    }

    /// Returns whether compositing bits update is needed.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.render_state.needs_compositing_bits_update()
    }

    /// Returns whether semantics update is needed.
    #[inline]
    pub fn needs_semantics_update(&self) -> bool {
        self.render_state.needs_semantics_update()
    }

    /// Returns whether this is a relayout boundary.
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.render_state.is_relayout_boundary()
    }

    /// Returns whether this is a repaint boundary.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.render_state.is_repaint_boundary()
    }

    /// Returns whether compositing is needed.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.render_state.needs_compositing()
    }

    /// Returns whether the object is disposed.
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.render_state.is_disposed()
    }

    // ========================================================================
    // Attach / Detach
    // ========================================================================

    /// Attaches this render object to a pipeline owner.
    ///
    /// This transitions the lifecycle to Attached, then NeedsLayout,
    /// and schedules initial layout/paint if needed.
    pub fn attach(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        debug_assert!(
            self.owner.is_none(),
            "Cannot attach: already attached to a pipeline owner"
        );

        self.owner = Some(owner);
        self.render_state.attach();

        // Schedule initial layout
        self.schedule_layout_with_owner();
    }

    /// Detaches this render object from its pipeline owner.
    pub fn detach(&mut self) {
        debug_assert!(
            self.owner.is_some(),
            "Cannot detach: not attached to a pipeline owner"
        );

        self.owner = None;
        self.render_state.detach();
    }

    /// Disposes this render object.
    pub fn dispose(&mut self) {
        self.render_state.dispose();
        self.owner = None;
        self.parent = None;
    }

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this object as needing layout.
    ///
    /// If attached, also schedules with the pipeline owner.
    pub fn mark_needs_layout(&mut self) {
        self.render_state.mark_needs_layout();
        self.schedule_layout_with_owner();
    }

    /// Marks this object as needing paint.
    ///
    /// If attached, also schedules with the pipeline owner.
    pub fn mark_needs_paint(&mut self) {
        self.render_state.mark_needs_paint();
        self.schedule_paint_with_owner();
    }

    /// Marks compositing bits as needing update.
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.render_state.mark_needs_compositing_bits_update();
        self.schedule_compositing_bits_with_owner();
    }

    /// Marks semantics as needing update.
    pub fn mark_needs_semantics_update(&mut self) {
        self.render_state.mark_needs_semantics();
        self.schedule_semantics_with_owner();
    }

    // ========================================================================
    // Clearing Dirty State
    // ========================================================================

    /// Clears the needs_layout state after layout completes.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.render_state.clear_needs_layout();
    }

    /// Clears the needs_paint state after paint completes.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.render_state.clear_needs_paint();
    }

    /// Clears the needs_compositing_bits_update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.render_state.clear_needs_compositing_bits_update();
    }

    /// Clears the needs_semantics_update flag.
    #[inline]
    pub fn clear_needs_semantics_update(&mut self) {
        self.render_state.clear_needs_semantics();
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Sets whether this is a relayout boundary.
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        self.render_state.set_relayout_boundary(is_boundary);
    }

    /// Sets whether this is a repaint boundary.
    #[inline]
    pub fn set_repaint_boundary(&mut self, is_boundary: bool) {
        self.render_state.set_repaint_boundary(is_boundary);
    }

    /// Sets whether compositing is needed.
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        self.render_state.set_needs_compositing(needs);
    }

    // ========================================================================
    // Pipeline Owner Scheduling
    // ========================================================================

    /// Schedules layout with the pipeline owner if attached.
    fn schedule_layout_with_owner(&self) {
        if let Some(owner) = &self.owner {
            owner
                .write()
                .add_node_needing_layout(self.node_id, self.depth as usize);
        }
    }

    /// Schedules paint with the pipeline owner if attached.
    fn schedule_paint_with_owner(&self) {
        if let Some(owner) = &self.owner {
            owner
                .write()
                .add_node_needing_paint(self.node_id, self.depth as usize);
        }
    }

    /// Schedules compositing bits update with the pipeline owner if attached.
    fn schedule_compositing_bits_with_owner(&self) {
        if let Some(owner) = &self.owner {
            owner
                .write()
                .add_node_needing_compositing_bits_update(self.node_id, self.depth as usize);
        }
    }

    /// Schedules semantics update with the pipeline owner if attached.
    fn schedule_semantics_with_owner(&self) {
        if let Some(owner) = &self.owner {
            owner
                .write()
                .add_node_needing_semantics(self.node_id, self.depth as usize);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_object_state_size() {
        // Verify size is reasonable (should be around 32-40 bytes on 64-bit)
        let size = std::mem::size_of::<RenderObjectState>();
        assert!(size <= 48, "RenderObjectState is too large: {} bytes", size);
    }

    #[test]
    fn test_render_object_state_default() {
        let state = RenderObjectState::new();
        assert_eq!(state.depth(), 0);
        assert!(state.owner().is_none());
        assert!(state.parent_ptr().is_none());
        assert!(!state.is_attached());
        assert!(state.lifecycle().is_detached());
    }

    #[test]
    fn test_render_object_state_with_node_id() {
        let state = RenderObjectState::with_node_id(42);
        assert_eq!(state.node_id(), 42);
    }

    #[test]
    fn test_render_object_state_depth() {
        let mut state = RenderObjectState::new();
        state.set_depth(10);
        assert_eq!(state.depth(), 10);

        state.set_depth(65535);
        assert_eq!(state.depth(), 65535);
    }

    #[test]
    fn test_render_object_state_attach_detach() {
        let mut state = RenderObjectState::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Attach
        state.attach(owner.clone());
        assert!(state.is_attached());
        assert!(state.needs_layout());

        // Verify scheduled with owner
        assert_eq!(owner.read().nodes_needing_layout().len(), 1);

        // Detach
        state.detach();
        assert!(!state.is_attached());
    }

    #[test]
    fn test_render_object_state_mark_needs_layout() {
        let mut state = RenderObjectState::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        state.attach(owner.clone());

        // Clear initial layout request
        owner.write().flush_layout();
        state.clear_needs_layout();
        assert!(!state.needs_layout());

        // Mark needs layout
        state.mark_needs_layout();
        assert!(state.needs_layout());
        assert_eq!(owner.read().nodes_needing_layout().len(), 1);
    }

    #[test]
    fn test_render_object_state_mark_needs_paint() {
        let mut state = RenderObjectState::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        state.attach(owner.clone());
        state.clear_needs_layout();

        // Transition to LaidOut first
        state.render_state_mut().clear_needs_layout();

        // Mark needs paint
        state.mark_needs_paint();
        assert!(state.needs_paint());
        assert_eq!(owner.read().nodes_needing_paint().len(), 1);
    }

    #[test]
    fn test_render_object_state_boundaries() {
        let mut state = RenderObjectState::new();

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_repaint_boundary(true);
        assert!(state.is_repaint_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
        assert!(state.is_repaint_boundary());
    }

    #[test]
    fn test_render_object_state_dispose() {
        let mut state = RenderObjectState::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        state.attach(owner);
        state.dispose();

        assert!(state.is_disposed());
        assert!(state.owner().is_none());
        assert!(state.parent_ptr().is_none());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RenderObjectState>();
    }
}
