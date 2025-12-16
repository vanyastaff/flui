//! Base render object implementation.
//!
//! This module provides [`BaseRenderObject`] - a reusable struct that implements
//! common render object functionality using [`RenderObjectState`].

use std::sync::Arc;

use parking_lot::RwLock;

use super::RenderObjectState;
use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;

// ============================================================================
// BaseRenderObject
// ============================================================================

/// Base implementation of common render object functionality.
///
/// This struct provides a reusable implementation of the state management
/// portions of [`RenderObject`]. Concrete render objects can embed this
/// struct and delegate lifecycle methods to it.
///
/// # Usage Pattern
///
/// ```ignore
/// pub struct MyRenderBox {
///     base: BaseRenderObject,
///     // ... rendering-specific fields
///     size: Size,
/// }
///
/// impl MyRenderBox {
///     pub fn new() -> Self {
///         Self {
///             base: BaseRenderObject::new(),
///             size: Size::ZERO,
///         }
///     }
///
///     // Delegate RenderObject methods to base
///     pub fn mark_needs_layout(&mut self) {
///         self.base.mark_needs_layout();
///     }
/// }
/// ```
///
/// # Memory Layout
///
/// `BaseRenderObject` is designed to be compact:
/// - `state`: RenderObjectState (~48 bytes)
/// - `parent_data`: Option<Box<dyn ParentData>> (16 bytes)
/// - `debug_creator`: Option<String> (24 bytes)
///
/// Total: ~88 bytes base overhead per render object.
#[derive(Debug)]
pub struct BaseRenderObject {
    /// Unified lifecycle and tree state.
    state: RenderObjectState,

    /// Parent data set by the parent render object.
    parent_data: Option<Box<dyn ParentData>>,

    /// Debug information about who created this object.
    debug_creator: Option<String>,
}

impl Default for BaseRenderObject {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseRenderObject {
    /// Creates a new base render object.
    #[inline]
    pub fn new() -> Self {
        Self {
            state: RenderObjectState::new(),
            parent_data: None,
            debug_creator: None,
        }
    }

    /// Creates a new base render object with a specific node ID.
    #[inline]
    pub fn with_node_id(node_id: usize) -> Self {
        Self {
            state: RenderObjectState::with_node_id(node_id),
            parent_data: None,
            debug_creator: None,
        }
    }

    // ========================================================================
    // State Access
    // ========================================================================

    /// Returns the internal state.
    #[inline]
    pub fn state(&self) -> &RenderObjectState {
        &self.state
    }

    /// Returns mutable access to the internal state.
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderObjectState {
        &mut self.state
    }

    // ========================================================================
    // Tree Structure (Delegated)
    // ========================================================================

    /// Returns the depth in the render tree.
    #[inline]
    pub fn depth(&self) -> usize {
        self.state.depth()
    }

    /// Sets the depth in the render tree.
    #[inline]
    pub fn set_depth(&mut self, depth: usize) {
        self.state.set_depth(depth);
    }

    /// Returns the pipeline owner, if attached.
    #[inline]
    pub fn owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        self.state.owner()
    }

    /// Returns whether this object is attached to a pipeline owner.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.state.is_attached()
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Attaches this render object to a pipeline owner.
    pub fn attach(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        self.state.attach(owner);
    }

    /// Detaches this render object from its pipeline owner.
    pub fn detach(&mut self) {
        self.state.detach();
    }

    /// Disposes this render object.
    pub fn dispose(&mut self) {
        self.state.dispose();
        self.parent_data = None;
    }

    // ========================================================================
    // Dirty State Queries
    // ========================================================================

    /// Returns whether layout is needed.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.state.needs_layout()
    }

    /// Returns whether paint is needed.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.state.needs_paint()
    }

    /// Returns whether compositing bits update is needed.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        self.state.needs_compositing_bits_update()
    }

    /// Returns whether this is a relayout boundary.
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.state.is_relayout_boundary()
    }

    /// Returns whether this is a repaint boundary.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.state.is_repaint_boundary()
    }

    /// Returns whether compositing is needed.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.state.needs_compositing()
    }

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this object as needing layout.
    pub fn mark_needs_layout(&mut self) {
        self.state.mark_needs_layout();
    }

    /// Marks this object as needing paint.
    pub fn mark_needs_paint(&mut self) {
        self.state.mark_needs_paint();
    }

    /// Marks compositing bits as needing update.
    pub fn mark_needs_compositing_bits_update(&mut self) {
        self.state.mark_needs_compositing_bits_update();
    }

    /// Marks semantics as needing update.
    pub fn mark_needs_semantics_update(&mut self) {
        self.state.mark_needs_semantics_update();
    }

    // ========================================================================
    // Clearing Dirty State
    // ========================================================================

    /// Clears the needs_layout state after layout completes.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.state.clear_needs_layout();
    }

    /// Clears the needs_paint state after paint completes.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.state.clear_needs_paint();
    }

    /// Clears the needs_compositing_bits_update flag.
    #[inline]
    pub fn clear_needs_compositing_bits_update(&mut self) {
        self.state.clear_needs_compositing_bits_update();
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Sets whether this is a relayout boundary.
    #[inline]
    pub fn set_relayout_boundary(&mut self, is_boundary: bool) {
        self.state.set_relayout_boundary(is_boundary);
    }

    /// Sets whether this is a repaint boundary.
    #[inline]
    pub fn set_repaint_boundary(&mut self, is_boundary: bool) {
        self.state.set_repaint_boundary(is_boundary);
    }

    /// Sets whether compositing is needed.
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        self.state.set_needs_compositing(needs);
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Returns the parent data, if any.
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_deref()
    }

    /// Returns mutable parent data, if any.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_deref_mut()
    }

    /// Sets the parent data.
    #[inline]
    pub fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.parent_data = Some(data);
    }

    /// Takes the parent data, leaving None.
    #[inline]
    pub fn take_parent_data(&mut self) -> Option<Box<dyn ParentData>> {
        self.parent_data.take()
    }

    // ========================================================================
    // Debug Information
    // ========================================================================

    /// Returns the debug creator string, if set.
    #[inline]
    pub fn debug_creator(&self) -> Option<&str> {
        self.debug_creator.as_deref()
    }

    /// Sets the debug creator string.
    #[inline]
    pub fn set_debug_creator(&mut self, creator: Option<String>) {
        self.debug_creator = creator;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_render_object_size() {
        let size = std::mem::size_of::<BaseRenderObject>();
        // Should be reasonably compact
        assert!(size <= 104, "BaseRenderObject is too large: {} bytes", size);
    }

    #[test]
    fn test_base_render_object_default() {
        let base = BaseRenderObject::new();
        assert_eq!(base.depth(), 0);
        assert!(!base.is_attached());
        assert!(base.parent_data().is_none());
        assert!(base.debug_creator().is_none());
        // New objects need layout and paint
        assert!(base.needs_layout());
        assert!(base.needs_paint());
    }

    #[test]
    fn test_base_render_object_with_node_id() {
        let base = BaseRenderObject::with_node_id(42);
        assert_eq!(base.state().node_id(), 42);
    }

    #[test]
    fn test_base_render_object_lifecycle() {
        let mut base = BaseRenderObject::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Attach
        base.attach(owner.clone());
        assert!(base.is_attached());
        assert!(base.needs_layout());

        // Detach
        base.detach();
        assert!(!base.is_attached());
    }

    #[test]
    fn test_base_render_object_dirty_marking() {
        let mut base = BaseRenderObject::with_node_id(1);
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));

        base.attach(owner.clone());
        base.clear_needs_layout();

        // Mark needs layout again
        base.mark_needs_layout();
        assert!(base.needs_layout());

        // Clear and mark needs paint
        base.clear_needs_layout();
        base.mark_needs_paint();
        assert!(base.needs_paint());
    }

    #[test]
    fn test_base_render_object_debug_creator() {
        let mut base = BaseRenderObject::new();

        base.set_debug_creator(Some("TestWidget".to_string()));
        assert_eq!(base.debug_creator(), Some("TestWidget"));

        base.set_debug_creator(None);
        assert!(base.debug_creator().is_none());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BaseRenderObject>();
    }
}
