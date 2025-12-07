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
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_tree::{
    HitTestVisitable, LayoutVisitable, PaintVisitable, RenderTreeAccess, TreeNav, TreeRead,
};
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::core::unified::{Constraints, Geometry};
use crate::core::{HitTestTree, LayoutTree, PaintTree, RenderStateExt};
use crate::error::RenderError;
use flui_types::Axis;

// ============================================================================
// RENDER TREE STORAGE TRAIT
// ============================================================================

/// Requirements for storage that can be used with `RenderTree<T>`.
///
/// This trait combines the necessary capabilities from `flui-tree`:
/// - `TreeRead` - Access nodes by ID
/// - `TreeNav` - Parent/child navigation
/// - `RenderTreeAccess` - Access to render objects and state
///
/// Any type implementing these traits (like `ElementTree`) can be
/// wrapped in `RenderTree<T>`.
///
/// All required functionality is provided by the composed traits.
pub trait RenderTreeStorage: TreeRead + TreeNav + RenderTreeAccess {}

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
    ///
    /// Supports both Box and Sliver protocols via `RenderStateExt`.
    fn get_element_offset(&self, id: ElementId) -> Option<Offset> {
        self.storage
            .render_state(id)
            .and_then(|state| state.offset())
    }

    /// Gets hit test bounds for an element based on its protocol type.
    ///
    /// Supports both Box and Sliver protocols:
    /// - Box: Uses geometry (Size) directly
    /// - Sliver: Computes bounds from hit_test_extent and cross_axis_extent
    fn get_hit_test_bounds(&self, id: ElementId) -> flui_types::Rect {
        self.storage
            .render_state(id)
            .and_then(|state| {
                // Try Box protocol first (most common)
                if let Some(size) = state.box_geometry() {
                    return Some(flui_types::Rect::from_min_size(Offset::ZERO, size));
                }

                // Try Sliver protocol
                if let Some(sliver_state) = state.as_sliver_state() {
                    let geometry = sliver_state.geometry().unwrap_or(SliverGeometry::zero());
                    let bounds = compute_sliver_hit_bounds(
                        &geometry,
                        sliver_state.constraints(),
                        Axis::Vertical,
                    );
                    return Some(bounds);
                }

                None
            })
            .unwrap_or(flui_types::Rect::ZERO)
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
                if let Some(box_state) = state.as_box_state() {
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
        // Validate render element exists
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Check cached geometry if layout not needed
        if let Some(state) = self.storage.render_state(id) {
            if !state.needs_layout() {
                if let Some(size) = state.box_geometry() {
                    return Ok(size);
                }
            }
            state.clear_needs_layout();
        }

        // SAFETY: Callback-based layout pattern
        //
        // We use a raw pointer to create a recursive callback:
        // 1. Callback captures raw pointer to self (not &mut self)
        // 2. This allows us to borrow render_element mutably
        // 3. When callback is invoked, it calls perform_layout on OTHER elements
        //
        // This is safe because:
        // - Parent element (id) and child elements are DISJOINT in the tree
        // - No aliasing: we never access parent while children are being laid out
        // - Rust's tree invariant guarantees no cycles
        // - Raw pointer is only used within this scope
        unsafe {
            let self_ptr = self as *mut Self;

            // Create callback that uses raw pointer for recursion
            let mut layout_child = |child_id: ElementId, child_constraints: BoxConstraints| {
                (*self_ptr).perform_layout(child_id, child_constraints)
            };

            // Now we can borrow render_element without conflict
            let render_element = self
                .storage
                .render_object_mut(id)
                .and_then(|obj| obj.downcast_mut::<crate::core::RenderElement>())
                .ok_or_else(|| RenderError::not_render_element(id))?;

            // Call perform_layout with callback - NO tree parameter needed!
            let size = render_element
                .render_object_mut()
                .perform_layout(id, constraints, &mut layout_child)?;

            // Cache the result
            if let Some(state) = self.storage.render_state(id) {
                if let Some(box_state) = state.as_box_state() {
                    box_state.set_constraints(constraints);
                    box_state.set_size(size);
                }
            }

            Ok(size)
        }
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        // Validate render element exists
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Check cached geometry if layout not needed
        if let Some(state) = self.storage.render_state(id) {
            if !state.needs_layout() {
                if let Some(sliver_state) = state.as_sliver_state() {
                    if let Some(geom) = sliver_state.geometry() {
                        return Ok(geom);
                    }
                }
            }
            state.clear_needs_layout();
        }

        // SAFETY: Same callback pattern as perform_layout()
        // See detailed safety comments in perform_layout() above.
        unsafe {
            let self_ptr = self as *mut Self;

            // Create callback for laying out sliver children
            let layout_sliver_child =
                |child_id: ElementId, child_constraints: SliverConstraints| {
                    (*self_ptr).perform_sliver_layout(child_id, child_constraints)
                };

            // Borrow render_element without conflict
            let render_element = self
                .storage
                .render_object_mut(id)
                .and_then(|obj| obj.downcast_mut::<crate::core::RenderElement>())
                .ok_or_else(|| RenderError::not_render_element(id))?;

            // TODO: Add perform_sliver_layout to RenderObject trait
            // For now, return zero geometry as placeholder
            let geom = SliverGeometry::zero();
            let _ = (layout_sliver_child, render_element); // Silence unused warnings

            // Cache the result
            if let Some(state) = self.storage.render_state(id) {
                if let Some(sliver_state) = state.as_sliver_state() {
                    sliver_state.set_constraints(constraints);
                    sliver_state.set_sliver_geometry(geom);
                }
            }

            Ok(geom)
        }
    }

    fn set_offset(&mut self, id: ElementId, offset: Offset) {
        if let Some(box_state) = self.storage.render_state(id).and_then(|s| s.as_box_state()) {
            box_state.set_offset(offset);
        }
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        self.storage
            .render_state(id)
            .and_then(|state| state.offset())
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        self.needs_layout.insert(id);

        // Also mark the render state flag
        if let Some(flags) = self.storage.render_state(id).and_then(|s| s.render_flags()) {
            flags.mark_needs_layout();
        }
    }

    fn needs_layout(&self, id: ElementId) -> bool {
        if self.needs_layout.contains(&id) {
            return true;
        }

        // Also check render state flag
        self.storage
            .render_state(id)
            .map(|state| state.needs_layout())
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
    fn perform_paint(&mut self, id: ElementId, _offset: Offset) -> Result<Canvas, RenderError> {
        // Get render object
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        // Clear needs_paint flag
        if let Some(state) = self.storage.render_state(id) {
            state.clear_needs_paint();
        }

        // TODO: Actually call render_object.paint() with proper context
        // For now, return empty canvas
        let canvas = Canvas::new();

        Ok(canvas)
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.insert(id);

        // Also mark the render state flag
        if let Some(flags) = self.storage.render_state(id).and_then(|s| s.render_flags()) {
            flags.mark_needs_paint();
        }
    }

    fn needs_paint(&self, id: ElementId) -> bool {
        if self.needs_paint.contains(&id) {
            return true;
        }

        self.storage
            .render_state(id)
            .map(|state| state.needs_paint())
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
        // Get render object - must exist for hit testing
        let _render_obj = match self.storage.render_object(id) {
            Some(obj) => obj,
            None => return false,
        };

        // Get bounds based on protocol type (Box or Sliver)
        let bounds = self.get_hit_test_bounds(id);

        // Check if position is within bounds
        if !bounds.contains(position) {
            return false;
        }

        // Hit test children first (front to back)
        // Use TreeNav::children() instead of custom children_vec()
        let children: Vec<_> = self.storage.children(id).collect();
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
// FLUI-TREE VISITOR TRAIT IMPLEMENTATIONS
// ============================================================================
//
// These traits provide unified protocol support using Constraints/Geometry enums.
// They enable generic tree traversal algorithms that work with both Box and Sliver protocols.

impl<T: RenderTreeStorage> LayoutVisitable for RenderTree<T> {
    type Constraints = Constraints;
    type Geometry = Geometry;
    type Position = Offset;

    fn layout_element(&mut self, id: ElementId, constraints: Self::Constraints) -> Self::Geometry {
        // Dispatch to protocol-specific LayoutTree methods based on constraint type
        match constraints {
            Constraints::Box(box_c) => {
                // Call Box protocol layout
                match self.perform_layout(id, box_c) {
                    Ok(size) => Geometry::Box(size),
                    Err(_) => Geometry::zero(crate::core::ProtocolId::Box),
                }
            }
            Constraints::Sliver(sliver_c) => {
                // Call Sliver protocol layout
                match self.perform_sliver_layout(id, sliver_c) {
                    Ok(geom) => Geometry::Sliver(geom),
                    Err(_) => Geometry::zero(crate::core::ProtocolId::Sliver),
                }
            }
        }
    }

    fn set_position(&mut self, id: ElementId, position: Self::Position) {
        // Delegate to existing LayoutTree implementation
        self.set_offset(id, position);
    }

    fn get_position(&self, id: ElementId) -> Option<Self::Position> {
        // Delegate to existing LayoutTree implementation
        LayoutTree::get_offset(self, id)
    }

    fn get_geometry(&self, id: ElementId) -> Option<Self::Geometry> {
        self.storage.render_state(id).and_then(|state| {
            // Try Box protocol first (most common)
            if let Some(size) = state.box_geometry() {
                return Some(Geometry::Box(size));
            }

            // Try Sliver protocol
            if let Some(sliver_state) = state.as_sliver_state() {
                if let Some(geom) = sliver_state.geometry() {
                    return Some(Geometry::Sliver(geom));
                }
            }

            None
        })
    }
}

impl<T: RenderTreeStorage> PaintVisitable for RenderTree<T> {
    type Position = Offset;
    type PaintResult = ();

    fn paint_element(&mut self, id: ElementId, position: Self::Position) -> Self::PaintResult {
        // Get geometry for this element
        let geometry = self.get_geometry(id);

        // Get reference to the element
        let element_result = self
            .storage
            .render_object(id)
            .and_then(|obj| obj.downcast_ref::<crate::core::RenderElement>());

        if let (Some(element), Some(geom)) = (element_result, geometry) {
            // TODO: Get actual canvas from somewhere - for now we skip painting
            // In real implementation, paint() would need &mut Canvas parameter
            let _ = element;
            let _ = geom;
            let _ = position;
            // element.paint(id, position, &geom, canvas, self as &dyn PaintTree).ok();
        }
    }

    fn combine_paint_results(&self, _results: Vec<Self::PaintResult>) -> Self::PaintResult {
        // Nothing to combine for () result type
    }
}

impl<T: RenderTreeStorage> HitTestVisitable for RenderTree<T> {
    type Position = Offset;
    type HitResult = HitTestResult;

    fn hit_test_element(
        &self,
        id: ElementId,
        position: Self::Position,
        result: &mut Self::HitResult,
    ) -> bool {
        // Get the bounds for this element
        let bounds = self.get_hit_test_bounds(id);

        // Check if position is within bounds
        if bounds.contains(position.to_point()) {
            // Add to hit test result
            let entry = HitTestEntry::new(id, position, bounds);
            result.add(entry);
            true
        } else {
            false
        }
    }

    fn transform_position_for_child(
        &self,
        _parent: ElementId,
        child: ElementId,
        position: Self::Position,
    ) -> Self::Position {
        // Get child offset and transform position
        if let Some(child_offset) = self.get_element_offset(child) {
            position - child_offset
        } else {
            position
        }
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

// ============================================================================
// HELPER FUNCTIONS FOR HIT TESTING
// ============================================================================

/// Computes hit test bounds for a Sliver element.
///
/// # Arguments
///
/// * `geometry` - The sliver geometry containing hit_test_extent
/// * `constraints` - Optional constraints for cross_axis_extent
/// * `default_axis` - Default axis if constraints not available
///
/// # Returns
///
/// A `Rect` representing the hit testable area, or `Rect::ZERO` if not hit testable.
#[inline]
pub fn compute_sliver_hit_bounds(
    geometry: &SliverGeometry,
    constraints: Option<&SliverConstraints>,
    default_axis: Axis,
) -> flui_types::Rect {
    if !geometry.is_hit_testable() {
        return flui_types::Rect::ZERO;
    }

    let cross_axis_extent = constraints.map(|c| c.cross_axis_extent).unwrap_or(0.0);
    let hit_extent = geometry.hit_test_extent();
    let axis = constraints.map(|c| c.axis).unwrap_or(default_axis);

    let size = match axis {
        Axis::Vertical => Size::new(cross_axis_extent, hit_extent),
        Axis::Horizontal => Size::new(hit_extent, cross_axis_extent),
    };

    flui_types::Rect::from_min_size(Offset::ZERO, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::AxisDirection;

    // Note: We cannot create a MockStorage that implements TreeRead/TreeNav
    // because they are sealed traits. Tests with actual storage (ElementTree)
    // should be in integration tests.

    // ========================================================================
    // SLIVER HIT TEST BOUNDS TESTS
    // ========================================================================

    #[test]
    fn test_compute_sliver_hit_bounds_vertical() {
        let geometry = SliverGeometry::new(200.0, 100.0, 0.0);
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            100.0,
            600.0,
            300.0, // cross_axis_extent
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Vertical);

        // Vertical axis: width = cross_axis_extent, height = hit_test_extent
        assert_eq!(bounds.width(), 300.0);
        assert_eq!(bounds.height(), 100.0); // hit_test_extent defaults to paint_extent
    }

    #[test]
    fn test_compute_sliver_hit_bounds_horizontal() {
        let geometry = SliverGeometry::new(200.0, 100.0, 0.0);
        let constraints = SliverConstraints::new(
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            Axis::Horizontal,
            0.0,
            100.0,
            800.0,
            400.0, // cross_axis_extent
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Horizontal);

        // Horizontal axis: width = hit_test_extent, height = cross_axis_extent
        assert_eq!(bounds.width(), 100.0); // hit_test_extent defaults to paint_extent
        assert_eq!(bounds.height(), 400.0);
    }

    #[test]
    fn test_compute_sliver_hit_bounds_with_explicit_hit_extent() {
        let geometry = SliverGeometry::new(200.0, 100.0, 0.0).with_hit_test_extent(80.0);
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            100.0,
            600.0,
            300.0,
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Vertical);

        assert_eq!(bounds.width(), 300.0);
        assert_eq!(bounds.height(), 80.0); // explicit hit_test_extent
    }

    #[test]
    fn test_compute_sliver_hit_bounds_not_hit_testable() {
        // Zero geometry is not hit testable
        let geometry = SliverGeometry::zero();
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            100.0,
            600.0,
            300.0,
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Vertical);

        assert_eq!(bounds, flui_types::Rect::ZERO);
    }

    #[test]
    fn test_compute_sliver_hit_bounds_invisible() {
        let geometry = SliverGeometry::new(200.0, 100.0, 0.0).with_visible(false);
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            100.0,
            600.0,
            300.0,
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Vertical);

        // Invisible sliver is not hit testable
        assert_eq!(bounds, flui_types::Rect::ZERO);
    }

    #[test]
    fn test_compute_sliver_hit_bounds_no_constraints() {
        let geometry = SliverGeometry::new(200.0, 100.0, 0.0);

        // Without constraints, uses default axis and zero cross_axis_extent
        let bounds = compute_sliver_hit_bounds(&geometry, None, Axis::Vertical);

        assert_eq!(bounds.width(), 0.0); // no cross_axis_extent
        assert_eq!(bounds.height(), 100.0);
    }

    // ========================================================================
    // DIRTY SET TESTS
    // ========================================================================

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
