//! Flutter-style boundary-aware dirty propagation.
//!
//! This file contains the `RenderDirtyPropagation` trait — minimal tree
//! operations needed by boundary-aware propagation. The trait shape is
//! preserved at `pub(crate)` visibility for a possible future
//! viewport-invalidation hook (see `PRESERVED_FOR` marker on the trait).
//!
//! The five `RenderState<P>::mark_needs_*` propagation methods that previously
//! lived here were deleted in U3 of the flui-rendering Phase 1 zombie cleanup
//! (plan: `docs/plans/2026-05-20-005-refactor-flui-rendering-zombie-cleanup-plan.md`)
//! because they were unreachable in production. Production dirty marking goes
//! through `PipelineOwner::add_node_needing_layout / add_node_needing_paint`
//! invoked from `flui-view` and `flui-hot-reload`, not via `RenderState`.

use flui_foundation::ElementId;

use super::RenderState;
use crate::protocol::Protocol;

// ============================================================================
// TREE OPERATIONS TRAIT
// ============================================================================

/// Tree operations needed by boundary-aware dirty propagation (preserved as cost-cheap option).
// PRESERVED_FOR: future viewport-invalidation hook (audit Step 4 item 13
// contemplates pinning down the production dirty-marking path; this trait
// shape may or may not be adopted at that time — kept as cost-cheap option,
// not as an endorsed design).
#[expect(
    dead_code,
    reason = "preserved as a cost-cheap option for a possible viewport-invalidation hook (see PRESERVED_FOR marker above); promoting to `expect` so the lint self-expires the moment a first implementer or caller appears"
)]
pub(crate) trait RenderDirtyPropagation {
    /// Gets the parent element ID, if any.
    fn parent(&self, id: ElementId) -> Option<ElementId>;

    /// Gets the render state for an element, if it exists.
    ///
    /// Returns None if:
    /// - Element doesn't exist
    /// - Element is not a render element
    /// - Protocol doesn't match
    fn get_render_state<P: Protocol>(&self, id: ElementId) -> Option<&RenderState<P>>;

    /// Registers an element that needs layout in the next frame.
    ///
    /// This is called when a relayout boundary is dirty. The pipeline
    /// owner will process all registered elements in the next frame.
    fn register_needs_layout(&mut self, id: ElementId);

    /// Registers an element that needs paint in the next frame.
    ///
    /// This is called when a repaint boundary is dirty. The pipeline
    /// owner will process all registered elements in the next frame.
    fn register_needs_paint(&mut self, id: ElementId);

    /// Registers an element that needs compositing bits update.
    ///
    /// This is called when a node's compositing status changes. The pipeline
    /// owner will process all registered elements during the compositing phase.
    fn register_needs_compositing_bits_update(&mut self, id: ElementId);

    /// Gets the RenderObject for an element to check `is_repaint_boundary`.
    ///
    /// Returns true if the element is a repaint boundary.
    fn is_repaint_boundary(&self, id: ElementId) -> bool;

    /// Gets the previous repaint boundary status (for transition detection).
    ///
    /// Returns the cached `_wasRepaintBoundary` value.
    fn was_repaint_boundary(&self, id: ElementId) -> bool;
}
