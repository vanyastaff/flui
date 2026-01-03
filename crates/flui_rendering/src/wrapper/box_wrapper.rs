//! Wrapper types that bridge RenderBox/RenderSliver to RenderObject.
//!
//! This module provides wrapper types that convert typed render traits
//! into `RenderObject` for storage in `RenderTree`:
//!
//! - `BoxWrapper<T: RenderBox>` - For box layout render objects
//! - `SliverWrapper<T: RenderSliver>` - For sliver layout render objects (future)

use flui_foundation::{Diagnosticable, DiagnosticsBuilder, RenderId};
use flui_types::{Offset, Rect, Size};

use crate::children_access::ChildState;
use crate::constraints::BoxConstraints;
use crate::context::{CanvasContext, LayoutContext};
use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};
use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;
use crate::protocol::BoxProtocol;
use crate::traits::{RenderBox, RenderObject};

// ============================================================================
// BoxWrapper
// ============================================================================

/// Wrapper that bridges a typed `RenderBox` to `RenderObject`.
///
/// `BoxWrapper` stores:
/// - The inner `RenderBox` implementation
/// - `ChildState` for each child (size, offset, parent_data)
/// - RenderObject state (depth, needs_layout, needs_paint, etc.)
///
/// When `layout_without_resize()` is called, `BoxWrapper`:
/// 1. Creates a `BoxLayoutContext` with constraints
/// 2. Calls `inner.perform_layout(ctx)`
/// 3. Stores the resulting size
///
/// # Type Parameters
///
/// - `T`: The inner RenderBox type
///
/// # Example
///
/// ```ignore
/// use flui_rendering::wrapper::BoxWrapper;
/// use flui_rendering::traits::RenderBox;
///
/// struct MyColoredBox { size: Size }
///
/// impl RenderBox for MyColoredBox {
///     type Arity = Leaf;
///     type ParentData = BoxParentData;
///     // ... implementation
/// }
///
/// let wrapper = BoxWrapper::new(MyColoredBox { size: Size::new(100.0, 50.0) });
/// // wrapper implements RenderObject and can be stored in RenderTree
/// ```
pub struct BoxWrapper<T: RenderBox> {
    /// The inner RenderBox implementation.
    inner: T,

    /// Child states (size, offset, parent_data).
    children: Vec<ChildState<T::ParentData>>,

    /// Child render IDs (for RenderTree lookup).
    child_ids: Vec<RenderId>,

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

    /// Cached constraints from parent.
    cached_constraints: Option<BoxConstraints>,

    /// Parent data set by parent.
    parent_data: Option<Box<dyn ParentData>>,
}

// Safety: BoxWrapper manages raw pointers carefully
unsafe impl<T: RenderBox + Send> Send for BoxWrapper<T> {}
unsafe impl<T: RenderBox + Sync> Sync for BoxWrapper<T> {}

impl<T: RenderBox> BoxWrapper<T> {
    /// Creates a new BoxWrapper around an inner RenderBox.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            children: Vec::new(),
            child_ids: Vec::new(),
            depth: 0,
            parent: None,
            owner: None,
            needs_layout: true,
            needs_paint: true,
            needs_compositing_bits_update: false,
            is_repaint_boundary: false,
            was_repaint_boundary: false,
            needs_compositing: false,
            cached_constraints: None,
            parent_data: None,
        }
    }

    /// Creates a BoxWrapper with repaint boundary enabled.
    pub fn with_repaint_boundary(inner: T) -> Self {
        let mut wrapper = Self::new(inner);
        wrapper.is_repaint_boundary = true;
        wrapper
    }

    /// Returns a reference to the inner RenderBox.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner RenderBox.
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
        "box"
    }
}

// ============================================================================
// RenderObject Implementation for BoxWrapper
// ============================================================================

impl<T: RenderBox> RenderObject for BoxWrapper<T> {
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
        // TODO: Implement proper relayout boundary logic
        false
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
        self.cached_constraints = Some(constraints);
        self.layout_without_resize();
    }

    fn layout_without_resize(&mut self) {
        // Get constraints (should be cached from parent's layout call)
        let constraints = self
            .cached_constraints
            .unwrap_or_else(|| BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY)));

        // Create the layout context for BoxProtocol
        use crate::protocol::BoxLayoutCtx;

        let inner_ctx = BoxLayoutCtx::<T::Arity, T::ParentData>::new(constraints);
        let mut ctx = LayoutContext::<BoxProtocol, T::Arity, T::ParentData>::new(inner_ctx);

        // Call the RenderBox's perform_layout
        self.inner.perform_layout(&mut ctx);

        // Clear dirty flag
        self.needs_layout = false;
    }

    fn cached_constraints(&self) -> Option<BoxConstraints> {
        self.cached_constraints
    }

    fn set_cached_constraints(&mut self, constraints: BoxConstraints) {
        self.cached_constraints = Some(constraints);
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
        // The RenderTree handles child traversal
    }

    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // Children are stored in RenderTree, not here
    }

    fn paint_bounds(&self) -> Rect {
        self.inner.box_paint_bounds()
    }

    fn paint(&self, context: &mut CanvasContext, offset: Offset) {
        // TODO: Create BoxPaintContext and call inner.paint(ctx)
        let _ = (context, offset);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl<T: RenderBox> std::fmt::Debug for BoxWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxWrapper")
            .field("inner", &self.inner)
            .field("child_count", &self.children.len())
            .field("depth", &self.depth)
            .field("needs_layout", &self.needs_layout)
            .field("needs_paint", &self.needs_paint)
            .finish()
    }
}

impl<T: RenderBox> Diagnosticable for BoxWrapper<T> {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("protocol", "box");
        builder.add("type", std::any::type_name::<T>());
        builder.add("size", format!("{:?}", self.inner.size()));
        builder.add("needs_layout", self.needs_layout);
        builder.add("needs_paint", self.needs_paint);
        builder.add("child_count", self.children.len());
    }
}

// ============================================================================
// HitTestTarget Implementation
// ============================================================================

impl<T: RenderBox + 'static> HitTestTarget for BoxWrapper<T> {
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
    use crate::context::BoxLayoutContext;
    use crate::parent_data::BoxParentData;

    /// Simple test RenderBox implementation
    #[derive(Debug)]
    struct TestBox {
        size: Size,
    }

    impl TestBox {
        fn new(size: Size) -> Self {
            Self { size }
        }
    }

    impl RenderBox for TestBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
            let constrained = ctx.constrain(self.size);
            self.size = constrained;
            ctx.complete_with_size(constrained);
        }

        fn size(&self) -> Size {
            self.size
        }

        fn set_size(&mut self, size: Size) {
            self.size = size;
        }

        fn paint(&mut self, _ctx: &mut crate::context::BoxPaintContext<'_, Leaf, BoxParentData>) {
            // No-op for test
        }

        fn hit_test(
            &self,
            _ctx: &mut crate::context::BoxHitTestContext<'_, Leaf, BoxParentData>,
        ) -> bool {
            false
        }
    }

    #[test]
    fn test_box_wrapper_creation() {
        let inner = TestBox::new(Size::new(100.0, 50.0));
        let wrapper = BoxWrapper::new(inner);

        assert_eq!(wrapper.inner().size(), Size::new(100.0, 50.0));
        assert!(wrapper.needs_layout());
        assert!(wrapper.needs_paint());
        assert_eq!(wrapper.child_count(), 0);
        assert_eq!(wrapper.protocol_name(), "box");
    }

    #[test]
    fn test_box_wrapper_add_remove_child() {
        let inner = TestBox::new(Size::new(100.0, 50.0));
        let mut wrapper = BoxWrapper::new(inner);

        let child_id = RenderId::new(1);
        wrapper.add_child(child_id);

        assert_eq!(wrapper.child_count(), 1);
        assert_eq!(wrapper.child_ids(), &[child_id]);

        assert!(wrapper.remove_child(child_id));
        assert_eq!(wrapper.child_count(), 0);
    }

    #[test]
    fn test_box_wrapper_layout() {
        let inner = TestBox::new(Size::new(200.0, 100.0));
        let mut wrapper = BoxWrapper::new(inner);

        let constraints = BoxConstraints::tight(Size::new(150.0, 75.0));
        wrapper.layout(constraints, true);

        assert!(!wrapper.needs_layout());
        assert_eq!(wrapper.cached_constraints(), Some(constraints));
        // Size should be constrained
        assert_eq!(wrapper.inner().size(), Size::new(150.0, 75.0));
    }

    #[test]
    fn test_box_wrapper_paint_bounds() {
        let inner = TestBox::new(Size::new(100.0, 50.0));
        let wrapper = BoxWrapper::new(inner);

        let bounds = wrapper.paint_bounds();
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 50.0);
    }

    #[test]
    fn test_box_wrapper_depth() {
        let inner = TestBox::new(Size::new(100.0, 50.0));
        let mut wrapper = BoxWrapper::new(inner);

        assert_eq!(wrapper.depth(), 0);
        wrapper.set_depth(5);
        assert_eq!(wrapper.depth(), 5);
    }
}
