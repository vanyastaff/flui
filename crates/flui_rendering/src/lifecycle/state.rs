//! Unified render object state storage.
//!
//! This module provides [`RenderObjectState`] - a compact struct that stores
//! all common state for render objects, following Flutter's pattern where
//! attach state is determined by owner presence.
//!
//! # Flutter Approach
//!
//! In Flutter, render objects determine attachment via `owner != null`:
//! - `bool get attached => _owner != null;`
//!
//! Dirty flags are separate boolean fields. We use `RenderObjectFlags` to
//! pack these efficiently.

use std::sync::Arc;

use parking_lot::RwLock;

use super::{DirtyFlags, RelayoutBoundary, RenderObjectFlags};
use crate::pipeline::PipelineOwner;
use crate::traits::RenderObject;

// ============================================================================
// RenderObjectState
// ============================================================================

/// Unified state storage for render objects.
///
/// This struct combines dirty flags with tree position information,
/// providing a single source of truth for render object state.
///
/// # Flutter Approach
///
/// Attachment is determined by owner presence (`owner != null`), not a
/// separate lifecycle flag. This matches Flutter exactly:
///
/// ```dart
/// bool get attached => _owner != null;
/// ```
///
/// # Memory Layout
///
/// ```text
/// RenderObjectState (48 bytes on 64-bit):
/// - flags: RenderObjectFlags (2 bytes)
/// - depth: u16 (2 bytes)
/// - padding: 4 bytes
/// - owner: Option<Arc<RwLock<PipelineOwner>>> (8 bytes)
/// - parent: Option<*const dyn RenderObject> (16 bytes - wide pointer)
/// - node_id: usize (8 bytes)
/// ```
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
    /// Combined dirty flags and relayout boundary (2 bytes).
    flags: RenderObjectFlags,

    /// Depth in the render tree (root = 0).
    /// Using u16 allows trees up to 65535 levels deep.
    depth: u16,

    /// The pipeline owner that manages this render object.
    /// Attachment is determined by `owner.is_some()`.
    owner: Option<Arc<RwLock<PipelineOwner>>>,

    /// Pointer to parent render object (wide pointer with vtable).
    /// Using raw pointer to avoid circular references.
    /// Safety: Only valid while attached to tree.
    parent: Option<*const dyn RenderObject>,

    /// Unique identifier for this node in the pipeline owner's dirty lists.
    node_id: usize,

    /// Whether this object has been disposed.
    disposed: bool,
}

impl std::fmt::Debug for RenderObjectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderObjectState")
            .field("flags", &self.flags)
            .field("depth", &self.depth)
            .field("owner", &self.owner.as_ref().map(|_| "..."))
            .field("parent", &self.parent.map(|p| p as *const ()))
            .field("node_id", &self.node_id)
            .field("disposed", &self.disposed)
            .finish()
    }
}

// Safety: RenderObjectState is Send + Sync because:
// - RenderObjectFlags is Copy and contains no pointers
// - depth is Copy
// - Arc<RwLock<PipelineOwner>> is Send + Sync
// - parent is only dereferenced while attached (single-threaded tree ops)
// - node_id is Copy
// - disposed is Copy
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
            flags: RenderObjectFlags::new(),
            depth: 0,
            owner: None,
            parent: None,
            node_id: 0,
            disposed: false,
        }
    }

    /// Creates a new render object state with a specific node ID.
    #[inline]
    pub fn with_node_id(node_id: usize) -> Self {
        Self {
            flags: RenderObjectFlags::new(),
            depth: 0,
            owner: None,
            parent: None,
            node_id,
            disposed: false,
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
    ///
    /// # Flutter Equivalence
    /// `bool get attached => _owner != null;`
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
    // Flags Access
    // ========================================================================

    /// Returns the dirty flags.
    #[inline]
    pub fn dirty_flags(&self) -> DirtyFlags {
        self.flags.dirty()
    }

    /// Returns the underlying render object flags.
    #[inline]
    pub fn flags(&self) -> &RenderObjectFlags {
        &self.flags
    }

    /// Returns mutable access to render object flags.
    #[inline]
    pub fn flags_mut(&mut self) -> &mut RenderObjectFlags {
        &mut self.flags
    }

    // ========================================================================
    // Dirty State Queries (Delegated to flags)
    // ========================================================================

    /// Returns whether layout is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsLayout`
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }

    /// Returns whether paint is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsPaint`
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }

    /// Returns whether compositing bits update is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositingBitsUpdate`
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.flags.needs_compositing_bits_update()
    }

    /// Returns whether semantics update is needed.
    ///
    /// # Flutter Equivalence
    /// Part of semantics system
    #[inline]
    pub fn needs_semantics_update(&self) -> bool {
        self.flags.needs_semantics_update()
    }

    /// Returns whether this is a relayout boundary.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary`
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.is_relayout_boundary()
    }

    /// Returns whether this is a repaint boundary.
    ///
    /// # Flutter Equivalence
    /// `isRepaintBoundary`
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.flags.is_repaint_boundary()
    }

    /// Returns whether compositing is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositing`
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.needs_compositing()
    }

    /// Returns the relayout boundary state.
    #[inline]
    pub fn relayout_boundary(&self) -> RelayoutBoundary {
        self.flags.relayout_boundary()
    }

    /// Returns whether the object is disposed.
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    // ========================================================================
    // Attach / Detach
    // ========================================================================

    /// Attaches this render object to a pipeline owner.
    ///
    /// # Flutter Equivalence
    /// `attach(PipelineOwner owner)` sets `_owner = owner`
    pub fn attach(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        debug_assert!(
            self.owner.is_none(),
            "Cannot attach: already attached to a pipeline owner"
        );
        debug_assert!(!self.disposed, "Cannot attach: object is disposed");

        self.owner = Some(owner);

        // Schedule initial layout if needed
        if self.needs_layout() {
            self.schedule_layout_with_owner();
        }
        if self.needs_paint() {
            self.schedule_paint_with_owner();
        }
        if self.needs_compositing_bits_update() {
            self.schedule_compositing_bits_with_owner();
        }
    }

    /// Detaches this render object from its pipeline owner.
    ///
    /// # Flutter Equivalence
    /// `detach()` sets `_owner = null`
    pub fn detach(&mut self) {
        debug_assert!(
            self.owner.is_some(),
            "Cannot detach: not attached to a pipeline owner"
        );

        self.owner = None;
        // Clear relayout boundary on detach (Flutter does this in dropChild)
        self.flags.clear_relayout_boundary();
    }

    /// Disposes this render object.
    ///
    /// # Flutter Equivalence
    /// `dispose()` - marks object as disposed
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.owner = None;
        self.parent = None;
    }

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this object as needing layout.
    ///
    /// If attached, also schedules with the pipeline owner.
    ///
    /// # Flutter Equivalence
    /// `markNeedsLayout()`
    pub fn mark_needs_layout(&mut self) {
        self.flags.mark_needs_layout();
        self.schedule_layout_with_owner();
    }

    /// Marks this object as needing paint.
    ///
    /// If attached, also schedules with the pipeline owner.
    ///
    /// # Flutter Equivalence
    /// `markNeedsPaint()`
    pub fn mark_needs_paint(&mut self) {
        self.flags.mark_needs_paint();
        self.schedule_paint_with_owner();
    }

    /// Marks compositing bits as needing update.
    ///
    /// # Flutter Equivalence
    /// `markNeedsCompositingBitsUpdate()`
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.flags.mark_needs_compositing_bits_update();
        self.schedule_compositing_bits_with_owner();
    }

    /// Marks semantics as needing update.
    ///
    /// # Flutter Equivalence
    /// Part of semantics system
    pub fn mark_needs_semantics_update(&mut self) {
        self.flags.mark_needs_semantics_update();
        self.schedule_semantics_with_owner();
    }

    // ========================================================================
    // Clearing Dirty State
    // ========================================================================

    /// Clears the needs_layout state after layout completes.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.flags.clear_needs_layout();
    }

    /// Clears the needs_paint state after paint completes.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.flags.clear_needs_paint();
    }

    /// Clears the needs_compositing_bits_update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.flags.clear_needs_compositing_bits_update();
    }

    /// Clears the needs_semantics_update flag.
    #[inline]
    pub fn clear_needs_semantics_update(&mut self) {
        self.flags.clear_needs_semantics_update();
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Sets the relayout boundary state.
    ///
    /// # Flutter Equivalence
    /// `_isRelayoutBoundary = ...`
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        self.flags.set_relayout_boundary(if is_boundary {
            RelayoutBoundary::Yes
        } else {
            RelayoutBoundary::No
        });
    }

    /// Sets whether this is a repaint boundary.
    ///
    /// # Flutter Equivalence
    /// `isRepaintBoundary` (typically overridden)
    #[inline]
    pub fn set_repaint_boundary(&mut self, is_boundary: bool) {
        self.flags.set_repaint_boundary(is_boundary);
    }

    /// Sets whether compositing is needed.
    ///
    /// # Flutter Equivalence
    /// `_needsCompositing = ...`
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        self.flags.set_needs_compositing(needs);
    }

    /// Syncs was_repaint_boundary to match current repaint_boundary.
    ///
    /// Called at the end of paint.
    ///
    /// # Flutter Equivalence
    /// `_wasRepaintBoundary = isRepaintBoundary;`
    #[inline]
    pub fn sync_was_repaint_boundary(&mut self) {
        self.flags.sync_was_repaint_boundary();
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
        // Verify size is reasonable (should be around 40-48 bytes on 64-bit)
        let size = std::mem::size_of::<RenderObjectState>();
        assert!(size <= 56, "RenderObjectState is too large: {} bytes", size);
    }

    #[test]
    fn test_render_object_state_default() {
        let state = RenderObjectState::new();
        assert_eq!(state.depth(), 0);
        assert!(state.owner().is_none());
        assert!(state.parent_ptr().is_none());
        assert!(!state.is_attached());
        // New objects need layout and paint
        assert!(state.needs_layout());
        assert!(state.needs_paint());
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

        // Initially detached
        assert!(!state.is_attached());

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

        // Clear initial paint request
        owner.write().flush_paint();

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
    fn test_render_object_state_repaint_boundary_sync() {
        let mut state = RenderObjectState::new();

        state.set_repaint_boundary(true);
        assert!(state.is_repaint_boundary());
        assert!(!state.flags().was_repaint_boundary());

        state.sync_was_repaint_boundary();
        assert!(state.flags().was_repaint_boundary());

        state.set_repaint_boundary(false);
        assert!(!state.is_repaint_boundary());
        assert!(state.flags().was_repaint_boundary()); // Still true until next sync
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RenderObjectState>();
    }
}
