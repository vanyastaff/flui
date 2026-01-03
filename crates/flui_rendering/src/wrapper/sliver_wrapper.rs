//! SliverWrapper - bridges RenderSliver to RenderObject.
//!
//! This module provides the `SliverWrapper<T>` type that converts a typed
//! `RenderSliver` implementation into a `RenderObject` for storage in `RenderTree`.

use flui_foundation::{Diagnosticable, DiagnosticsBuilder, RenderId};
use flui_types::{Offset, Rect, Size};

use crate::children_access::ChildState;
use crate::constraints::{BoxConstraints, SliverConstraints};
use crate::context::{CanvasContext, LayoutContext};
use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};
use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;
use crate::protocol::SliverProtocol;
use crate::traits::{RenderObject, RenderSliver};

// ============================================================================
// SliverWrapper
// ============================================================================

/// Wrapper that bridges a typed `RenderSliver` to `RenderObject`.
///
/// `SliverWrapper` stores:
/// - The inner `RenderSliver` implementation
/// - `ChildState` for each child (geometry, offset, parent_data)
/// - RenderObject state (depth, needs_layout, needs_paint, etc.)
///
/// When `layout_without_resize()` is called, `SliverWrapper`:
/// 1. Creates a `SliverLayoutContext` with constraints
/// 2. Calls `inner.perform_layout(ctx)`
/// 3. Stores the resulting geometry
///
/// # Type Parameters
///
/// - `T`: The inner RenderSliver type
///
/// # Example
///
/// ```ignore
/// use flui_rendering::wrapper::SliverWrapper;
/// use flui_rendering::traits::RenderSliver;
///
/// struct MySliverList { /* ... */ }
///
/// impl RenderSliver for MySliverList {
///     type Arity = Variable;
///     type ParentData = SliverMultiBoxAdaptorParentData;
///     // ... implementation
/// }
///
/// let wrapper = SliverWrapper::new(MySliverList { /* ... */ });
/// // wrapper implements RenderObject and can be stored in RenderTree
/// ```
pub struct SliverWrapper<T: RenderSliver> {
    /// The inner RenderSliver implementation.
    inner: T,

    /// Child states (geometry, offset, parent_data).
    children: Vec<ChildState<T::ParentData>>,

    /// Child render IDs (for RenderTree lookup).
    child_ids: Vec<RenderId>,

    /// Cached sliver constraints from parent.
    cached_sliver_constraints: Option<SliverConstraints>,

    // ========================================================================
    // RenderObject State
    // ========================================================================
    /// Depth in the render tree.
    depth: usize,

    /// Parent pointer (raw for trait object compatibility).
    parent: Option<*const dyn RenderObject>,

    /// Pipeline owner pointer.
    owner: Option<*const PipelineOwner>,

    /// Whether layout is needed.
    needs_layout: bool,

    /// Whether paint is needed.
    needs_paint: bool,

    /// Whether compositing bits update is needed.
    needs_compositing_bits_update: bool,

    /// Whether this is a repaint boundary.
    is_repaint_boundary: bool,

    /// Whether this was a repaint boundary.
    was_repaint_boundary: bool,

    /// Whether compositing is needed.
    needs_compositing: bool,

    /// Parent data set by parent.
    parent_data: Option<Box<dyn ParentData>>,
}

// Safety: SliverWrapper manages raw pointers carefully
unsafe impl<T: RenderSliver + Send> Send for SliverWrapper<T> {}
unsafe impl<T: RenderSliver + Sync> Sync for SliverWrapper<T> {}

impl<T: RenderSliver> SliverWrapper<T> {
    /// Creates a new SliverWrapper around an inner RenderSliver.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            children: Vec::new(),
            child_ids: Vec::new(),
            cached_sliver_constraints: None,
            depth: 0,
            parent: None,
            owner: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            is_repaint_boundary: false,
            was_repaint_boundary: false,
            needs_compositing: false,
            parent_data: None,
        }
    }

    /// Creates a SliverWrapper with repaint boundary enabled.
    pub fn with_repaint_boundary(inner: T) -> Self {
        let mut wrapper = Self::new(inner);
        wrapper.is_repaint_boundary = true;
        wrapper
    }

    /// Returns a reference to the inner RenderSliver.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner RenderSliver.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Adds a child with the given RenderId.
    pub fn add_child(&mut self, child_id: RenderId) {
        self.child_ids.push(child_id);
        self.children.push(ChildState::new(child_id));
    }

    /// Removes a child by RenderId.
    pub fn remove_child(&mut self, child_id: RenderId) -> bool {
        if let Some(pos) = self.child_ids.iter().position(|&id| id == child_id) {
            self.child_ids.remove(pos);
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns the child IDs.
    pub fn child_ids(&self) -> &[RenderId] {
        &self.child_ids
    }

    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns the protocol name for this wrapper.
    pub fn protocol_name(&self) -> &'static str {
        "sliver"
    }

    /// Returns the cached sliver constraints.
    pub fn cached_sliver_constraints(&self) -> Option<&SliverConstraints> {
        self.cached_sliver_constraints.as_ref()
    }

    /// Performs layout with sliver constraints.
    pub fn layout_sliver(&mut self, constraints: SliverConstraints) {
        self.cached_sliver_constraints = Some(constraints.clone());
        self.layout_sliver_without_resize();
    }

    /// Performs layout using cached sliver constraints.
    fn layout_sliver_without_resize(&mut self) {
        let constraints = self
            .cached_sliver_constraints
            .clone()
            .unwrap_or_else(SliverConstraints::default);

        // Create the layout context for SliverProtocol
        use crate::protocol::SliverLayoutCtx;

        let inner_ctx = SliverLayoutCtx::<T::Arity, T::ParentData>::new(constraints);
        let mut ctx = LayoutContext::<SliverProtocol, T::Arity, T::ParentData>::new(inner_ctx);

        // Call the RenderSliver's perform_layout
        self.inner.perform_layout(&mut ctx);

        // Clear dirty flag
        self.needs_layout = false;
    }
}

// ============================================================================
// RenderObject Implementation for SliverWrapper
// ============================================================================

impl<T: RenderSliver> RenderObject for SliverWrapper<T> {
    fn depth(&self) -> usize {
        self.depth
    }

    fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        self.owner.map(|p| unsafe { &*p })
    }

    fn set_parent(&mut self, parent: Option<*const dyn RenderObject>) {
        self.parent = parent;
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        self.owner = Some(owner as *const PipelineOwner);
    }

    fn detach(&mut self) {
        self.owner = None;
    }

    fn adopt_child(&mut self, child: &mut dyn RenderObject) {
        self.setup_parent_data(child);
        self.needs_layout = true;
        self.needs_compositing_bits_update = true;
        child.set_parent(Some(self as *const dyn RenderObject));
        if let Some(owner) = self.owner() {
            child.attach(owner);
        }
        self.redepth_child(child);
    }

    fn drop_child(&mut self, child: &mut dyn RenderObject) {
        child.set_parent(None);
        if self.attached() {
            child.detach();
        }
        self.needs_layout = true;
        self.needs_compositing_bits_update = true;
    }

    fn redepth_child(&mut self, child: &mut dyn RenderObject) {
        if child.depth() <= self.depth {
            child.set_depth(self.depth + 1);
            child.redepth_children();
        }
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint
    }

    fn needs_compositing_bits_update(&self) -> bool {
        self.needs_compositing_bits_update
    }

    fn is_relayout_boundary(&self) -> bool {
        // Slivers are typically relayout boundaries
        true
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    fn mark_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = true;
    }

    fn mark_needs_semantics_update(&mut self) {
        // TODO: Implement semantics
    }

    fn clear_needs_layout(&mut self) {
        self.needs_layout = false;
    }

    fn clear_needs_paint(&mut self) {
        self.needs_paint = false;
    }

    fn clear_needs_compositing_bits_update(&mut self) {
        self.needs_compositing_bits_update = false;
    }

    fn layout(&mut self, constraints: BoxConstraints, _parent_uses_size: bool) {
        // Slivers don't use BoxConstraints directly.
        // This is called when a sliver is in a box context (rare).
        // Convert to sliver constraints with default values.
        let sliver_constraints = SliverConstraints {
            cross_axis_extent: constraints.max_width,
            viewport_main_axis_extent: constraints.max_height,
            ..Default::default()
        };
        self.layout_sliver(sliver_constraints);
    }

    fn layout_without_resize(&mut self) {
        self.layout_sliver_without_resize();
    }

    fn cached_constraints(&self) -> Option<BoxConstraints> {
        // Convert cached sliver constraints to box constraints for compatibility
        self.cached_sliver_constraints.as_ref().map(|sc| {
            BoxConstraints::tight(Size::new(
                sc.cross_axis_extent,
                sc.viewport_main_axis_extent,
            ))
        })
    }

    fn set_cached_constraints(&mut self, constraints: BoxConstraints) {
        // Convert box constraints to sliver constraints
        self.cached_sliver_constraints = Some(SliverConstraints {
            cross_axis_extent: constraints.max_width,
            viewport_main_axis_extent: constraints.max_height,
            ..Default::default()
        });
    }

    fn mark_parent_needs_layout(&mut self) {
        // TODO: Propagate to parent
    }

    fn schedule_initial_layout(&mut self) {
        self.needs_layout = true;
    }

    fn schedule_initial_paint(&mut self) {
        self.needs_paint = true;
    }

    fn is_repaint_boundary(&self) -> bool {
        self.is_repaint_boundary
    }

    fn was_repaint_boundary(&self) -> bool {
        self.was_repaint_boundary
    }

    fn set_was_repaint_boundary(&mut self, value: bool) {
        self.was_repaint_boundary = value;
    }

    fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    fn set_needs_compositing(&mut self, value: bool) {
        self.needs_compositing = value;
    }

    fn parent_data(&self) -> Option<&dyn ParentData> {
        self.parent_data.as_ref().map(|p| p.as_ref())
    }

    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.parent_data.as_mut().map(|p| p.as_mut())
    }

    fn set_parent_data(&mut self, data: Box<dyn ParentData>) {
        self.parent_data = Some(data);
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // Children are stored in RenderTree, not here
    }

    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // Children are stored in RenderTree, not here
    }

    fn paint_bounds(&self) -> Rect {
        self.inner.sliver_paint_bounds()
    }

    fn paint(&self, context: &mut CanvasContext, offset: Offset) {
        // TODO: Create SliverPaintContext and call inner.paint(ctx)
        let _ = (context, offset);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl<T: RenderSliver> std::fmt::Debug for SliverWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverWrapper")
            .field("inner", &self.inner)
            .field("child_count", &self.children.len())
            .field("depth", &self.depth)
            .field("needs_layout", &self.needs_layout)
            .field("needs_paint", &self.needs_paint)
            .finish()
    }
}

impl<T: RenderSliver> Diagnosticable for SliverWrapper<T> {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("protocol", "sliver");
        builder.add("type", std::any::type_name::<T>());
        builder.add("geometry", format!("{:?}", self.inner.geometry()));
        builder.add("needs_layout", self.needs_layout);
        builder.add("needs_paint", self.needs_paint);
        builder.add("child_count", self.children.len());
    }
}

// ============================================================================
// HitTestTarget Implementation
// ============================================================================

impl<T: RenderSliver + 'static> HitTestTarget for SliverWrapper<T> {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::Leaf;
    use crate::constraints::SliverGeometry;
    use crate::context::SliverLayoutContext;
    use crate::parent_data::SliverParentData;

    /// Simple test RenderSliver implementation
    #[derive(Debug)]
    struct TestSliver {
        geometry: SliverGeometry,
        constraints: SliverConstraints,
    }

    impl TestSliver {
        fn new() -> Self {
            Self {
                geometry: SliverGeometry::ZERO,
                constraints: SliverConstraints::default(),
            }
        }
    }

    impl RenderSliver for TestSliver {
        type Arity = Leaf;
        type ParentData = SliverParentData;

        fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, SliverParentData>) {
            self.constraints = ctx.constraints().clone();
            let geometry = SliverGeometry {
                scroll_extent: 100.0,
                paint_extent: ctx.constraints().remaining_paint_extent.min(100.0),
                max_paint_extent: 100.0,
                ..Default::default()
            };
            self.geometry = geometry.clone();
            ctx.complete(geometry);
        }

        fn geometry(&self) -> &SliverGeometry {
            &self.geometry
        }

        fn constraints(&self) -> &SliverConstraints {
            &self.constraints
        }

        fn set_geometry(&mut self, geometry: SliverGeometry) {
            self.geometry = geometry;
        }

        fn paint(
            &mut self,
            _ctx: &mut crate::context::SliverPaintContext<'_, Leaf, SliverParentData>,
        ) {
            // No-op for test
        }

        fn hit_test(
            &self,
            _ctx: &mut crate::context::SliverHitTestContext<'_, Leaf, SliverParentData>,
        ) -> bool {
            false
        }
    }

    #[test]
    fn test_sliver_wrapper_creation() {
        let inner = TestSliver::new();
        let wrapper = SliverWrapper::new(inner);

        assert!(wrapper.needs_layout());
        assert!(wrapper.needs_paint());
        assert_eq!(wrapper.child_count(), 0);
        assert_eq!(wrapper.protocol_name(), "sliver");
    }

    #[test]
    fn test_sliver_wrapper_add_remove_child() {
        let inner = TestSliver::new();
        let mut wrapper = SliverWrapper::new(inner);

        let child_id = RenderId::new(1);
        wrapper.add_child(child_id);

        assert_eq!(wrapper.child_count(), 1);
        assert_eq!(wrapper.child_ids(), &[child_id]);

        assert!(wrapper.remove_child(child_id));
        assert_eq!(wrapper.child_count(), 0);
    }

    #[test]
    fn test_sliver_wrapper_layout() {
        let inner = TestSliver::new();
        let mut wrapper = SliverWrapper::new(inner);

        let constraints = SliverConstraints {
            cross_axis_extent: 400.0,
            viewport_main_axis_extent: 800.0,
            remaining_paint_extent: 800.0,
            ..Default::default()
        };
        wrapper.layout_sliver(constraints.clone());

        assert!(!wrapper.needs_layout());
        assert_eq!(wrapper.cached_sliver_constraints(), Some(&constraints));
        // Geometry should be set
        assert_eq!(wrapper.inner().geometry().scroll_extent, 100.0);
    }

    #[test]
    fn test_sliver_wrapper_depth() {
        let inner = TestSliver::new();
        let mut wrapper = SliverWrapper::new(inner);

        assert_eq!(wrapper.depth(), 0);
        wrapper.set_depth(5);
        assert_eq!(wrapper.depth(), 5);
    }

    #[test]
    fn test_sliver_wrapper_is_relayout_boundary() {
        let inner = TestSliver::new();
        let wrapper = SliverWrapper::new(inner);

        // Slivers are typically relayout boundaries
        assert!(wrapper.is_relayout_boundary());
    }
}
