//! RenderTree storage wrapper - adds rendering capabilities to any tree storage.
//!
//! This module provides `RenderTreeWrapper<T>`, a wrapper that implements
//! `LayoutTree`, `PaintTree`, and `HitTestTree` for any compatible storage.
//!
//! # Architecture
//!
//! ```text
//! RenderTreeWrapper<T>
//!   ├── storage: T (ElementTree or any RenderTreeStorage)
//!   ├── render_objects: RenderTree (separate storage for RenderObjects)
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
//! - `RenderTreeWrapper<T>` in `flui_rendering` (rendering operations)
//! - No circular dependencies

use std::any::Any;
use std::collections::HashSet;

use crate::tree::{
    HitTestVisitable, LayoutVisitable, PaintVisitable, RenderTreeAccess, RenderTreeExt,
};
use flui_foundation::ElementId;
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_tree::{TreeNav, TreeRead};
use flui_types::{Axis, BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::error::RenderError;
use crate::hit_test_tree::HitTestTree;
use crate::layout_tree::LayoutTree;
use crate::paint_tree::PaintTree;
use crate::protocol::ProtocolId;
use crate::state::RenderStateExt;
use crate::unified::{Constraints, Geometry};

// ============================================================================
// RENDER TREE STORAGE TRAIT
// ============================================================================

/// Requirements for storage that can be used with `RenderTreeWrapper<T>`.
///
/// This trait combines the necessary capabilities from `flui-tree`:
/// - `TreeRead` - Access nodes by ID
/// - `TreeNav` - Parent/child navigation
/// - `RenderTreeAccess` - Access to render objects and state
pub trait RenderTreeStorage: TreeRead<ElementId> + TreeNav<ElementId> + RenderTreeAccess {}

// ============================================================================
// RENDER TREE WRAPPER
// ============================================================================

/// Wrapper that adds rendering capabilities to any compatible storage.
///
/// `RenderTreeWrapper<T>` takes ownership of a storage type (like `ElementTree`)
/// and provides implementations of `LayoutTree`, `PaintTree`, and `HitTestTree`.
///
/// # Four-Tree Architecture
///
/// This wrapper coordinates between:
/// - `storage`: ElementTree (stores Elements with ViewId/RenderId references)
/// - `render_objects`: RenderTree (stores actual RenderObjects)
#[derive(Debug)]
pub struct RenderTreeWrapper<T: RenderTreeStorage> {
    /// Underlying storage (e.g., ElementTree) - stores Elements
    storage: T,

    /// Separate RenderObject tree (four-tree architecture)
    render_objects: crate::tree::RenderTree,

    /// Elements that need layout in the next frame.
    needs_layout: HashSet<ElementId>,

    /// Elements that need paint in the next frame.
    needs_paint: HashSet<ElementId>,

    /// Elements that need compositing bits update.
    needs_compositing: HashSet<ElementId>,

    /// Elements that need semantics update.
    needs_semantics: HashSet<ElementId>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<T: RenderTreeStorage> RenderTreeWrapper<T> {
    /// Creates a new RenderTreeWrapper wrapping the given storage.
    pub fn new(storage: T) -> Self {
        Self {
            storage,
            render_objects: crate::tree::RenderTree::new(),
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing: HashSet::new(),
            needs_semantics: HashSet::new(),
        }
    }

    /// Creates with pre-allocated capacity for dirty sets.
    pub fn with_capacity(storage: T, capacity: usize) -> Self {
        Self {
            storage,
            render_objects: crate::tree::RenderTree::with_capacity(capacity),
            needs_layout: HashSet::with_capacity(capacity),
            needs_paint: HashSet::with_capacity(capacity),
            needs_compositing: HashSet::with_capacity(capacity),
            needs_semantics: HashSet::with_capacity(capacity),
        }
    }

    /// Unwraps, returning the underlying storage and RenderObject tree.
    pub fn into_inner(self) -> (T, crate::tree::RenderTree) {
        (self.storage, self.render_objects)
    }

    /// Returns a reference to the RenderObject tree.
    #[inline]
    pub fn render_objects(&self) -> &crate::tree::RenderTree {
        &self.render_objects
    }

    /// Returns a mutable reference to the RenderObject tree.
    #[inline]
    pub fn render_objects_mut(&mut self) -> &mut crate::tree::RenderTree {
        &mut self.render_objects
    }
}

// ============================================================================
// STORAGE ACCESS
// ============================================================================

impl<T: RenderTreeStorage> RenderTreeWrapper<T> {
    #[inline]
    pub fn storage(&self) -> &T {
        &self.storage
    }

    #[inline]
    pub fn storage_mut(&mut self) -> &mut T {
        &mut self.storage
    }
}

// ============================================================================
// DIRTY SET ACCESS
// ============================================================================

impl<T: RenderTreeStorage> RenderTreeWrapper<T> {
    fn get_element_offset(&self, id: ElementId) -> Option<Offset> {
        self.storage
            .render_state(id)
            .and_then(|state| state.offset())
    }

    fn get_hit_test_bounds(&self, id: ElementId) -> flui_types::Rect {
        self.storage
            .render_state(id)
            .and_then(|state| {
                if let Some(size) = state.box_geometry() {
                    return Some(flui_types::Rect::from_min_size(Offset::ZERO, size));
                }

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

    #[inline]
    pub fn elements_needing_layout(&self) -> &HashSet<ElementId> {
        &self.needs_layout
    }

    #[inline]
    pub fn elements_needing_paint(&self) -> &HashSet<ElementId> {
        &self.needs_paint
    }

    #[inline]
    pub fn elements_needing_compositing(&self) -> &HashSet<ElementId> {
        &self.needs_compositing
    }

    #[inline]
    pub fn elements_needing_semantics(&self) -> &HashSet<ElementId> {
        &self.needs_semantics
    }

    pub fn clear_dirty_sets(&mut self) {
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing.clear();
        self.needs_semantics.clear();
    }

    #[inline]
    pub fn has_pending_work(&self) -> bool {
        !self.needs_layout.is_empty()
            || !self.needs_paint.is_empty()
            || !self.needs_compositing.is_empty()
            || !self.needs_semantics.is_empty()
    }
}

// ============================================================================
// FLUSH METHODS
// ============================================================================

impl<T: RenderTreeStorage> RenderTreeWrapper<T> {
    /// Processes all elements needing layout.
    pub fn flush_layout(&mut self, root_constraints: BoxConstraints) -> Result<Size, RenderError> {
        let elements: Vec<_> = self.needs_layout.drain().collect();
        let mut root_size = Size::ZERO;

        for id in elements {
            match self.perform_layout(id, root_constraints) {
                Ok(size) => {
                    root_size = size;
                }
                Err(e) => {
                    tracing::warn!("Layout failed for {:?}: {:?}", id, e);
                }
            }
        }

        Ok(root_size)
    }

    /// Processes all elements needing paint.
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
    pub fn flush_compositing_bits(&mut self) {
        let elements: Vec<_> = self.needs_compositing.drain().collect();

        for id in elements {
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

impl<T: RenderTreeStorage> LayoutTree for RenderTreeWrapper<T> {
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError> {
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        if let Some(state) = self.storage.render_state(id) {
            if !state.needs_layout() {
                if let Some(size) = state.box_geometry() {
                    return Ok(size);
                }
            }
            state.clear_needs_layout();
        }

        // TODO: Implement actual layout via RenderTree
        // For now, return constraint's smallest size
        let size = constraints.smallest();

        if let Some(state) = self.storage.render_state(id) {
            if let Some(box_state) = state.as_box_state() {
                box_state.set_constraints(constraints);
                box_state.set_size(size);
            }
        }

        Ok(size)
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

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

        let geom = SliverGeometry::zero();

        if let Some(state) = self.storage.render_state(id) {
            if let Some(sliver_state) = state.as_sliver_state() {
                sliver_state.set_constraints(constraints);
                sliver_state.set_sliver_geometry(geom);
            }
        }

        Ok(geom)
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

        if let Some(flags) = self.storage.render_state(id).and_then(|s| s.render_flags()) {
            flags.mark_needs_layout();
        }
    }

    fn needs_layout(&self, id: ElementId) -> bool {
        if self.needs_layout.contains(&id) {
            return true;
        }

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
    }
}

// ============================================================================
// PAINT TREE IMPLEMENTATION
// ============================================================================

impl<T: RenderTreeStorage> PaintTree for RenderTreeWrapper<T> {
    fn perform_paint(&mut self, id: ElementId, _offset: Offset) -> Result<Canvas, RenderError> {
        let _render_obj = self
            .storage
            .render_object(id)
            .ok_or_else(|| RenderError::not_render_element(id))?;

        if let Some(state) = self.storage.render_state(id) {
            state.clear_needs_paint();
        }

        Ok(Canvas::new())
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.insert(id);

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

impl<T: RenderTreeStorage> HitTestTree for RenderTreeWrapper<T> {
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool {
        let _render_obj = match self.storage.render_object(id) {
            Some(obj) => obj,
            None => return false,
        };

        let bounds = self.get_hit_test_bounds(id);

        if !bounds.contains(position) {
            return false;
        }

        let children: Vec<_> = self.storage.children(id).collect();
        for child_id in children.into_iter().rev() {
            let child_offset = self.get_element_offset(child_id).unwrap_or(Offset::ZERO);
            let child_position = position - child_offset;

            if self.hit_test(child_id, child_position, result) {
                return true;
            }
        }

        let entry = HitTestEntry::new(id, position, bounds);
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
// VISITOR TRAIT IMPLEMENTATIONS
// ============================================================================

impl<T: RenderTreeStorage> LayoutVisitable for RenderTreeWrapper<T> {
    type Constraints = Constraints;
    type Geometry = Geometry;
    type Position = Offset;

    fn layout_element(&mut self, id: ElementId, constraints: Self::Constraints) -> Self::Geometry {
        match constraints {
            Constraints::Box(box_c) => match self.perform_layout(id, box_c) {
                Ok(size) => Geometry::Box(size),
                Err(_) => Geometry::zero(ProtocolId::Box),
            },
            Constraints::Sliver(sliver_c) => match self.perform_sliver_layout(id, sliver_c) {
                Ok(geom) => Geometry::Sliver(geom),
                Err(_) => Geometry::zero(ProtocolId::Sliver),
            },
        }
    }

    fn set_position(&mut self, id: ElementId, position: Self::Position) {
        self.set_offset(id, position);
    }

    fn get_position(&self, id: ElementId) -> Option<Self::Position> {
        LayoutTree::get_offset(self, id)
    }

    fn get_geometry(&self, id: ElementId) -> Option<Self::Geometry> {
        self.storage.render_state(id).and_then(|state| {
            if let Some(size) = state.box_geometry() {
                return Some(Geometry::Box(size));
            }

            if let Some(sliver_state) = state.as_sliver_state() {
                if let Some(geom) = sliver_state.geometry() {
                    return Some(Geometry::Sliver(geom));
                }
            }

            None
        })
    }
}

impl<T: RenderTreeStorage> PaintVisitable for RenderTreeWrapper<T> {
    type Position = Offset;
    type PaintResult = ();

    fn paint_element(&mut self, id: ElementId, position: Self::Position) -> Self::PaintResult {
        let _ = self.perform_paint(id, position);
    }

    fn combine_paint_results(&self, _results: Vec<Self::PaintResult>) -> Self::PaintResult {}
}

impl<T: RenderTreeStorage> HitTestVisitable for RenderTreeWrapper<T> {
    type Position = Offset;
    type HitResult = HitTestResult;

    fn hit_test_element(
        &self,
        id: ElementId,
        position: Self::Position,
        result: &mut Self::HitResult,
    ) -> bool {
        let bounds = self.get_hit_test_bounds(id);

        if bounds.contains(position.to_point()) {
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
        if let Some(child_offset) = self.get_element_offset(child) {
            position - child_offset
        } else {
            position
        }
    }
}

// ============================================================================
// DEREF
// ============================================================================

impl<T: RenderTreeStorage> std::ops::Deref for RenderTreeWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl<T: RenderTreeStorage> std::ops::DerefMut for RenderTreeWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}

impl<T: RenderTreeStorage + Default> Default for RenderTreeWrapper<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Computes hit test bounds for a Sliver element.
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::AxisDirection;

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
            300.0,
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Vertical);

        assert_eq!(bounds.width(), 300.0);
        assert_eq!(bounds.height(), 100.0);
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
            400.0,
        );

        let bounds = compute_sliver_hit_bounds(&geometry, Some(&constraints), Axis::Horizontal);

        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 400.0);
    }

    #[test]
    fn test_compute_sliver_hit_bounds_not_hit_testable() {
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
    fn test_dirty_set_operations() {
        let mut needs_layout: HashSet<ElementId> = HashSet::new();
        let mut needs_paint: HashSet<ElementId> = HashSet::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        needs_layout.insert(id1);
        needs_paint.insert(id2);

        assert!(needs_layout.contains(&id1));
        assert!(!needs_layout.contains(&id2));
        assert!(needs_paint.contains(&id2));

        let has_work = !needs_layout.is_empty() || !needs_paint.is_empty();
        assert!(has_work);

        needs_layout.clear();
        needs_paint.clear();

        let has_work = !needs_layout.is_empty() || !needs_paint.is_empty();
        assert!(!has_work);
    }
}
