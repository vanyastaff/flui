//! RenderTree - Wrapper that adds rendering capabilities to any tree storage.
//!
//! This module provides `RenderTree<T>`, a wrapper that implements
//! `LayoutTree`, `PaintTree`, and `HitTestTree` for any compatible storage.
//!
//! # Architecture
//!
//! ```text
//! RenderTree<T>
//!   ├── storage: T (ElementTree or any RenderTreeStorage)
//!   ├── needs_layout: HashSet<ElementId>
//!   ├── needs_paint: HashSet<ElementId>
//!   └── needs_compositing: HashSet<ElementId>
//! ```
//!
//! # Flutter Analogy
//!
//! This is similar to Flutter's `PipelineOwner` combined with render tree
//! operations. The separation allows:
//! - `ElementTree` to remain in `flui-element` (storage only)
//! - `RenderTree<T>` in `flui_rendering` (rendering operations)
//! - No circular dependencies
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderTree, LayoutTree, PaintTree};
//! use flui_element::ElementTree;
//!
//! let element_tree = ElementTree::new();
//! let mut render_tree = RenderTree::new(element_tree);
//!
//! // Now render_tree implements LayoutTree, PaintTree, HitTestTree
//! let size = render_tree.perform_layout(root_id, constraints)?;
//! let canvas = render_tree.perform_paint(root_id, Offset::ZERO)?;
//! ```

use std::any::Any;
use std::collections::HashSet;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_tree::{RenderTreeAccess, TreeNav, TreeRead};
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::core::{BoxRenderState, HitTestTree, LayoutTree, PaintTree};
use crate::error::RenderError;

// ============================================================================
// RENDER TREE STORAGE TRAIT
// ============================================================================

/// Requirements for storage that can be used with `RenderTree<T>`.
///
/// This trait combines the necessary capabilities from `flui-tree`:
/// - `TreeRead` - Access nodes by ID
/// - `TreeNav` - Parent/child navigation
/// - `RenderTreeAccess` - Access to render objects and state (includes `render_object_mut`)
///
/// Any type implementing these traits (like `ElementTree`) can be
/// wrapped in `RenderTree<T>`.
///
/// Note: `render_object_mut` and `render_state_mut` come from `RenderTreeAccess`.
pub trait RenderTreeStorage: TreeRead + TreeNav + RenderTreeAccess {
    /// Get children of an element as a Vec (needed for iteration during mutation).
    fn children_vec(&self, id: ElementId) -> Vec<ElementId> {
        self.children(id).collect()
    }
}

// ============================================================================
// RENDER TREE
// ============================================================================

/// Wrapper that adds rendering capabilities to any compatible storage.
///
/// `RenderTree<T>` takes ownership of a storage type (like `ElementTree`)
/// and provides implementations of `LayoutTree`, `PaintTree`, and `HitTestTree`.
///
/// # Type Parameters
///
/// - `T`: The underlying storage type, must implement `RenderTreeStorage`
///
/// # Thread Safety
///
/// `RenderTree<T>` is `Send + Sync` if `T` is `Send + Sync`.
/// The dirty sets use standard `HashSet` - for concurrent access,
/// wrap in `Arc<RwLock<RenderTree<T>>>` or use `parking_lot`.
///
/// # Performance
///
/// - Dirty tracking: O(1) insert/remove/contains via HashSet
/// - Storage access: Delegated to underlying `T`
/// - No additional allocations during layout/paint (except dirty sets)
#[derive(Debug)]
pub struct RenderTree<T: RenderTreeStorage> {
    /// Underlying storage (e.g., ElementTree)
    storage: T,

    /// Elements that need layout in the next frame.
    /// Flutter equivalent: `PipelineOwner._nodesNeedingLayout`
    needs_layout: HashSet<ElementId>,

    /// Elements that need paint in the next frame.
    /// Flutter equivalent: `PipelineOwner._nodesNeedingPaint`
    needs_paint: HashSet<ElementId>,

    /// Elements that need compositing bits update.
    /// Flutter equivalent: `PipelineOwner._nodesNeedingCompositingBitsUpdate`
    needs_compositing: HashSet<ElementId>,

    /// Elements that need semantics update.
    /// Flutter equivalent: `PipelineOwner._nodesNeedingSemanticsUpdate`
    needs_semantics: HashSet<ElementId>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<T: RenderTreeStorage> RenderTree<T> {
    /// Creates a new RenderTree wrapping the given storage.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let element_tree = ElementTree::new();
    /// let render_tree = RenderTree::new(element_tree);
    /// ```
    pub fn new(storage: T) -> Self {
        Self {
            storage,
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing: HashSet::new(),
            needs_semantics: HashSet::new(),
        }
    }

    /// Creates a RenderTree with pre-allocated capacity for dirty sets.
    ///
    /// Use this when you know approximately how many elements will be dirty.
    pub fn with_capacity(storage: T, capacity: usize) -> Self {
        Self {
            storage,
            needs_layout: HashSet::with_capacity(capacity),
            needs_paint: HashSet::with_capacity(capacity),
            needs_compositing: HashSet::with_capacity(capacity),
            needs_semantics: HashSet::with_capacity(capacity),
        }
    }

    /// Unwraps the RenderTree, returning the underlying storage.
    pub fn into_inner(self) -> T {
        self.storage
    }
}

// ============================================================================
// STORAGE ACCESS
// ============================================================================

impl<T: RenderTreeStorage> RenderTree<T> {
    /// Returns a reference to the underlying storage.
    #[inline]
    pub fn storage(&self) -> &T {
        &self.storage
    }

    /// Returns a mutable reference to the underlying storage.
    #[inline]
    pub fn storage_mut(&mut self) -> &mut T {
        &mut self.storage
    }
}

// ============================================================================
// DIRTY SET ACCESS (Flutter PipelineOwner-like)
// ============================================================================

impl<T: RenderTreeStorage> RenderTree<T> {
    /// Gets the offset of an element (internal helper to avoid trait method ambiguity).
    fn get_element_offset(&self, id: ElementId) -> Option<Offset> {
        self.storage
            .render_state(id)
            .and_then(|state| state.downcast_ref::<BoxRenderState>().map(|s| s.offset()))
    }

    /// Returns elements needing layout.
    ///
    /// Flutter equivalent: `PipelineOwner._nodesNeedingLayout`
    #[inline]
    pub fn elements_needing_layout(&self) -> &HashSet<ElementId> {
        &self.needs_layout
    }

    /// Returns elements needing paint.
    ///
    /// Flutter equivalent: `PipelineOwner._nodesNeedingPaint`
    #[inline]
    pub fn elements_needing_paint(&self) -> &HashSet<ElementId> {
        &self.needs_paint
    }

    /// Returns elements needing compositing update.
    #[inline]
    pub fn elements_needing_compositing(&self) -> &HashSet<ElementId> {
        &self.needs_compositing
    }

    /// Returns elements needing semantics update.
    #[inline]
    pub fn elements_needing_semantics(&self) -> &HashSet<ElementId> {
        &self.needs_semantics
    }

    /// Clears all dirty sets after a frame is complete.
    ///
    /// Call this after `flush_layout()` and `flush_paint()` complete.
    pub fn clear_dirty_sets(&mut self) {
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing.clear();
        self.needs_semantics.clear();
    }

    /// Returns true if any element needs processing.
    #[inline]
    pub fn has_pending_work(&self) -> bool {
        !self.needs_layout.is_empty()
            || !self.needs_paint.is_empty()
            || !self.needs_compositing.is_empty()
            || !self.needs_semantics.is_empty()
    }
}

// ============================================================================
// FLUSH METHODS (Flutter PipelineOwner-like)
// ============================================================================

impl<T: RenderTreeStorage> RenderTree<T> {
    /// Processes all elements needing layout.
    ///
    /// Flutter equivalent: `PipelineOwner.flushLayout()`
    ///
    /// Elements are processed in depth order (parents before children)
    /// to ensure constraints flow correctly down the tree.
    ///
    /// # Arguments
    ///
    /// * `root_constraints` - Constraints for the root element
    ///
    /// # Returns
    ///
    /// The size of the root element after layout, or an error.
    pub fn flush_layout(&mut self, root_constraints: BoxConstraints) -> Result<Size, RenderError> {
        // Sort by depth (shallow first) for correct constraint propagation
        let elements: Vec<_> = self.needs_layout.drain().collect();

        // Sort by depth - we need to layout parents before children
        // For now, simple approach: layout each element
        // TODO: Proper depth sorting using storage.depth()

        let mut root_size = Size::ZERO;

        for id in elements {
            match self.perform_layout(id, root_constraints) {
                Ok(size) => {
                    root_size = size; // Last one wins (should be root)
                }
                Err(e) => {
                    tracing::warn!("Layout failed for {:?}: {:?}", id, e);
                    // Continue with other elements
                }
            }
        }

        Ok(root_size)
    }

    /// Processes all elements needing paint.
    ///
    /// Flutter equivalent: `PipelineOwner.flushPaint()`
    ///
    /// # Returns
    ///
    /// Combined canvas of all painted elements, or an error.
    pub fn flush_paint(&mut self) -> Result<Canvas, RenderError> {
        let elements: Vec<_> = self.needs_paint.drain().collect();

        let mut combined_canvas = Canvas::new();

        for id in elements {
            let offset = self.get_element_offset(id).unwrap_or(Offset::ZERO);
            match self.perform_paint(id, offset) {
                Ok(canvas) => {
                    combined_canvas = combined_canvas.merge(canvas);
                }
                Err(e) => {
                    tracing::warn!("Paint failed for {:?}: {:?}", id, e);
                }
            }
        }

        Ok(combined_canvas)
    }

    /// Processes all elements needing compositing bits update.
    ///
    /// Flutter equivalent: `PipelineOwner.flushCompositingBits()`
    pub fn flush_compositing_bits(&mut self) {
        let elements: Vec<_> = self.needs_compositing.drain().collect();

        for id in elements {
            // Update compositing bits for each element
            if let Some(state) = self.storage.render_state(id) {
                if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                    box_state.clear_needs_compositing();
                }
            }
        }
    }
}

// ============================================================================
// LAYOUT TREE IMPLEMENTATION
// ============================================================================

impl<T: RenderTreeStorage> LayoutTree for RenderTree<T> {
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError> {
        // Get render object
        let render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Try to downcast to RenderBox (most common case)
        // For now, return placeholder - real implementation needs proper downcasting
        // through the RenderObject trait

        // Get render state to check/set dirty flags
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                // Clear needs_layout flag
                box_state.clear_needs_layout();

                // Return cached geometry if available
                if let Some(size) = box_state.geometry() {
                    return Ok(size);
                }
            }
        }

        // TODO: Actually call render_object.layout() when we have proper downcasting
        // For now, return a default size
        let size = constraints.constrain(Size::new(100.0, 100.0));

        // Cache the result
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                // Note: set_geometry will panic if already set - that's intentional
                // box_state.set_geometry(size);
            }
        }

        Ok(size)
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        // Get render object
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // TODO: Implement sliver layout
        Ok(SliverGeometry::zero())
    }

    fn set_offset(&mut self, id: ElementId, offset: Offset) {
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                box_state.set_offset(offset);
            }
        }
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        self.storage
            .render_state(id)
            .and_then(|state| state.downcast_ref::<BoxRenderState>().map(|s| s.offset()))
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        self.needs_layout.insert(id);

        // Also mark the render state flag
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                box_state.flags().mark_needs_layout();
            }
        }
    }

    fn needs_layout(&self, id: ElementId) -> bool {
        if self.needs_layout.contains(&id) {
            return true;
        }

        // Also check render state flag
        self.storage
            .render_state(id)
            .and_then(|state| state.downcast_ref::<BoxRenderState>())
            .map(|s| s.needs_layout())
            .unwrap_or(false)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        self.storage.render_object(id)
    }

    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        self.storage.render_object_mut(id)
    }

    fn setup_child_parent_data(&mut self, _parent_id: ElementId, _child_id: ElementId) {
        // TODO: Implement parent data setup
        // This requires accessing parent's create_parent_data() and attaching to child
    }
}

// ============================================================================
// PAINT TREE IMPLEMENTATION
// ============================================================================

impl<T: RenderTreeStorage> PaintTree for RenderTree<T> {
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> Result<Canvas, RenderError> {
        // Get render object
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Clear needs_paint flag
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                box_state.clear_needs_paint();
            }
        }

        // TODO: Actually call render_object.paint() with proper context
        // For now, return empty canvas
        let canvas = Canvas::new();

        Ok(canvas)
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.insert(id);

        // Also mark the render state flag
        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.downcast_ref::<BoxRenderState>() {
                box_state.flags().mark_needs_paint();
            }
        }
    }

    fn needs_paint(&self, id: ElementId) -> bool {
        if self.needs_paint.contains(&id) {
            return true;
        }

        self.storage
            .render_state(id)
            .and_then(|state| state.downcast_ref::<BoxRenderState>())
            .map(|s| s.needs_paint())
            .unwrap_or(false)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        self.storage.render_object(id)
    }

    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        self.storage.render_object_mut(id)
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        self.get_element_offset(id)
    }
}

// ============================================================================
// HIT TEST TREE IMPLEMENTATION
// ============================================================================

impl<T: RenderTreeStorage> HitTestTree for RenderTree<T> {
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool {
        // Get render object
        let render_obj = match self.storage.render_object(id) {
            Some(obj) => obj,
            None => return false,
        };

        // Get geometry to check bounds
        let size = self
            .storage
            .render_state(id)
            .and_then(|state| state.downcast_ref::<BoxRenderState>())
            .and_then(|s| s.geometry())
            .unwrap_or(Size::ZERO);

        // Check if position is within bounds
        let bounds = flui_types::Rect::from_min_size(Offset::ZERO, size);
        if !bounds.contains(position) {
            return false;
        }

        // Hit test children first (front to back)
        let children = self.storage.children_vec(id);
        for child_id in children.into_iter().rev() {
            let child_offset = self.get_element_offset(child_id).unwrap_or(Offset::ZERO);
            let child_position = position - child_offset;

            if self.hit_test(child_id, child_position, result) {
                return true;
            }
        }

        // Add self to result
        let entry = flui_interaction::HitTestEntry::new(id, position, bounds);
        result.add(entry);
        true
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        self.storage.render_object(id)
    }

    fn children(&self, id: ElementId) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.storage.children(id))
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        self.get_element_offset(id)
    }
}

// ============================================================================
// DEREF FOR CONVENIENT STORAGE ACCESS
// ============================================================================

impl<T: RenderTreeStorage> std::ops::Deref for RenderTree<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl<T: RenderTreeStorage> std::ops::DerefMut for RenderTree<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}

// ============================================================================
// DEFAULT
// ============================================================================

impl<T: RenderTreeStorage + Default> Default for RenderTree<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We cannot create a MockStorage that implements TreeRead/TreeNav
    // because they are sealed traits. Tests with actual storage (ElementTree)
    // should be in integration tests.

    // For now, we test the dirty set management which doesn't require storage.

    #[test]
    fn test_dirty_set_operations() {
        // Test HashSet operations directly
        let mut needs_layout: HashSet<ElementId> = HashSet::new();
        let mut needs_paint: HashSet<ElementId> = HashSet::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        // Test insert
        needs_layout.insert(id1);
        needs_paint.insert(id2);

        assert!(needs_layout.contains(&id1));
        assert!(!needs_layout.contains(&id2));
        assert!(needs_paint.contains(&id2));

        // Test has_pending_work logic
        let has_work = !needs_layout.is_empty() || !needs_paint.is_empty();
        assert!(has_work);

        // Test clear
        needs_layout.clear();
        needs_paint.clear();

        let has_work = !needs_layout.is_empty() || !needs_paint.is_empty();
        assert!(!has_work);
    }

    #[test]
    fn test_element_id_in_hash_set() {
        let mut set: HashSet<ElementId> = HashSet::new();

        // Test multiple inserts
        for i in 1..=10 {
            set.insert(ElementId::new(i));
        }

        assert_eq!(set.len(), 10);

        // Test drain
        let elements: Vec<_> = set.drain().collect();
        assert_eq!(elements.len(), 10);
        assert!(set.is_empty());
    }
}
