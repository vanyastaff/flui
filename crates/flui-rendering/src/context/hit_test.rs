//! Rich HitTestContext with ergonomic API for hit testing operations.
//!
//! This module provides `HitTestContext`, a high-level wrapper around the hit
//! test capability traits that offers ergonomic APIs for common hit testing
//! patterns.
//!
//! # Features
//!
//! - **Position Access**: Easy access to hit position in local coordinates
//! - **Hit Detection**: Simplified bounds checking and hit detection
//! - **Child Testing**: Helper methods for testing children with transforms
//! - **Result Management**: Easy hit entry addition and result access
//!
//! # Example
//!
//! ```ignore
//! fn hit_test(&self, ctx: &mut HitTestContext<BoxProtocol, Single, BoxParentData>) -> bool {
//!     // Quick bounds check
//!     if !ctx.is_within_bounds(self.size.as_rect()) {
//!         return false;
//!     }
//!
//!     // Test children first (reverse paint order)
//!     for i in (0..ctx.child_count()).rev() {
//!         if ctx.hit_test_child_at_offset(i, child_offsets[i]) {
//!             return true;
//!         }
//!     }
//!
//!     // Add ourselves as a hit target
//!     ctx.add_self(self.id());
//!     true
//! }
//! ```

use flui_foundation::RenderId;
use flui_tree::Arity;
use flui_types::{
    Pixels, Size,
    geometry::{Matrix4, Offset, Rect},
};

use crate::{
    parent_data::ParentData,
    protocol::{
        BoxHitTest, BoxHitTestEntry, HitTestCapability, HitTestContextApi, MainAxisPosition,
        Protocol, SliverHitTest,
    },
};

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// Rich hit test context with ergonomic API for common hit testing patterns.
///
/// This context wraps the underlying capability context and provides:
/// - Easy position access and bounds checking
/// - Child hit testing with automatic transforms
/// - Result management helpers
pub struct HitTestContext<'ctx, P: Protocol, A: Arity, PD: ParentData> {
    /// The underlying hit test context from the capability
    inner: <P::HitTest as HitTestCapability>::Context<'ctx, A, PD>,
    /// The node's laid-out size in local pixels, resolved by the driver
    /// from [`RenderState`](crate::storage::RenderState) (geometry's sole
    /// owner). The box protocol uses it for the default bounds gate
    /// (`is_within_own_size`); the sliver protocol leaves it `Size::ZERO`
    /// (its hit gate is driver-owned). 2B field dedup: render objects no
    /// longer cache their own size.
    own_size: Size,
    /// Whether this render object should be appended to the global hit-test
    /// path even when its `hit_test` return value keeps sibling traversal open.
    self_hit_entry_registered: bool,
}

impl<P: Protocol, A: Arity, PD: ParentData> std::fmt::Debug for HitTestContext<'_, P, A, PD> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `inner` is a capability-GAT context (may hold live driver callbacks);
        // report only the driver-resolved scalars.
        f.debug_struct("HitTestContext")
            .field("own_size", &self.own_size)
            .field("self_hit_entry_registered", &self.self_hit_entry_registered)
            .finish_non_exhaustive()
    }
}

impl<'ctx, P: Protocol, A: Arity, PD: ParentData> HitTestContext<'ctx, P, A, PD>
where
    <P::HitTest as HitTestCapability>::Context<'ctx, A, PD>:
        HitTestContextApi<'ctx, P::HitTest, A, PD>,
{
    /// Creates a new hit test context wrapping the capability context.
    ///
    /// `own_size` is the node's laid-out size from `RenderState`; the
    /// box bounds gate (`is_within_own_size`) reads it. Sliver callers
    /// pass `Size::ZERO` (unused — the driver owns the sliver gate).
    pub fn new(
        inner: <P::HitTest as HitTestCapability>::Context<'ctx, A, PD>,
        own_size: Size,
    ) -> Self {
        Self {
            inner,
            own_size,
            self_hit_entry_registered: false,
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // POSITION ACCESS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the hit test position in local coordinates.
    pub fn position(&self) -> &<P::HitTest as HitTestCapability>::Position {
        self.inner.position()
    }

    // ════════════════════════════════════════════════════════════════════════
    // HIT DETECTION
    // ════════════════════════════════════════════════════════════════════════

    /// Checks if position is within the given bounds.
    pub fn is_within_bounds(&self, bounds: Rect) -> bool {
        self.inner.is_hit(bounds)
    }

    /// Alias for is_within_bounds for semantic clarity.
    pub fn is_hit(&self, bounds: Rect) -> bool {
        self.inner.is_hit(bounds)
    }

    // ════════════════════════════════════════════════════════════════════════
    // RESULT MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the hit test result.
    pub fn result(&self) -> &<P::HitTest as HitTestCapability>::Result {
        self.inner.result()
    }

    /// Gets mutable reference to hit test result.
    pub fn result_mut(&mut self) -> &mut <P::HitTest as HitTestCapability>::Result {
        self.inner.result_mut()
    }

    /// Adds a hit entry to the result.
    pub fn add_hit(&mut self, entry: <P::HitTest as HitTestCapability>::Entry) {
        self.inner.add_hit(entry);
    }

    /// Requests that the pipeline append this render object to the global
    /// hit-test path even if this object's `hit_test` returns `false`.
    ///
    /// This models Flutter's `HitTestBehavior::Translucent` side effect:
    /// receive the event, but keep testing siblings visually behind this node.
    pub fn register_self_hit_entry(&mut self) {
        self.self_hit_entry_registered = true;
    }

    pub(crate) fn self_hit_entry_registered(&self) -> bool {
        self.self_hit_entry_registered
    }

    // ════════════════════════════════════════════════════════════════════════
    // CHILD TESTING
    // ════════════════════════════════════════════════════════════════════════

    /// Tests a child for hits with position transformation.
    pub fn hit_test_child(
        &mut self,
        index: usize,
        position: <P::HitTest as HitTestCapability>::Position,
    ) -> bool {
        self.inner.hit_test_child(index, position)
    }

    /// Tests a child at its laid-out position (`RenderState.offset`)
    /// — the parent supplies no offset, the driver resolves it. THE
    /// way to hit-test children positioned during layout; parents no
    /// longer mirror offsets in their own fields.
    pub fn hit_test_child_at_layout_offset(&mut self, index: usize) -> bool {
        self.inner.hit_test_child_at_layout_offset(index)
    }

    // ════════════════════════════════════════════════════════════════════════
    // TRANSFORM MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════

    /// Adds a transform to the hit test path.
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.inner.push_transform(transform);
    }

    /// Removes the most recent transform.
    pub fn pop_transform(&mut self) {
        self.inner.pop_transform();
    }

    /// Adds an offset transform.
    pub fn push_offset(&mut self, offset: Offset) {
        self.inner.push_offset(offset);
    }

    /// Executes a closure with a pushed transform, automatically popping
    /// afterward.
    pub fn with_transform<F, R>(&mut self, transform: Matrix4, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.inner.push_transform(transform);
        let result = f(self);
        self.inner.pop_transform();
        result
    }

    /// Executes a closure with a pushed offset, automatically popping
    /// afterward.
    pub fn with_offset<F, R>(&mut self, offset: Offset, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.inner.push_offset(offset);
        let result = f(self);
        self.inner.pop_transform();
        result
    }

    // ════════════════════════════════════════════════════════════════════════
    // INNER ACCESS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the underlying context for advanced operations.
    pub fn inner(&self) -> &<P::HitTest as HitTestCapability>::Context<'ctx, A, PD> {
        &self.inner
    }

    /// Gets mutable access to the underlying context.
    pub fn inner_mut(&mut self) -> &mut <P::HitTest as HitTestCapability>::Context<'ctx, A, PD> {
        &mut self.inner
    }
}

// ============================================================================
// BOX-SPECIFIC EXTENSIONS
// ============================================================================

use crate::protocol::BoxProtocol;

impl<'ctx, A: Arity, PD: ParentData> HitTestContext<'ctx, BoxProtocol, A, PD>
where
    <BoxHitTest as HitTestCapability>::Context<'ctx, A, PD>:
        HitTestContextApi<'ctx, BoxHitTest, A, PD>,
{
    // ════════════════════════════════════════════════════════════════════════
    // BOX POSITION HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the X coordinate of the hit position.
    pub fn x(&self) -> Pixels {
        self.inner.position().dx
    }

    /// Gets the Y coordinate of the hit position.
    pub fn y(&self) -> Pixels {
        self.inner.position().dy
    }

    /// Gets the hit position as an Offset.
    pub fn offset(&self) -> Offset {
        *self.inner.position()
    }

    /// Returns position translated by the given offset.
    pub fn position_minus(&self, offset: Offset) -> Offset {
        *self.inner.position() - offset
    }

    // ════════════════════════════════════════════════════════════════════════
    // BOX HIT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Checks if position is within a rectangle at origin with given size.
    pub fn is_within_size(&self, width: Pixels, height: Pixels) -> bool {
        let pos = self.inner.position();
        pos.dx >= Pixels::ZERO && pos.dx < width && pos.dy >= Pixels::ZERO && pos.dy < height
    }

    /// The node's laid-out size, resolved by the driver from
    /// `RenderState` (2B field dedup — objects no longer cache it).
    pub fn own_size(&self) -> Size {
        self.own_size
    }

    /// Checks if the hit position is within this node's own laid-out
    /// bounds — the default [`RenderBox::hit_test`](crate::traits::RenderBox::hit_test)
    /// gate. Equivalent to `is_within_size(own_size.width, own_size.height)`
    /// but reads the driver-supplied size instead of a per-object field.
    pub fn is_within_own_size(&self) -> bool {
        self.is_within_size(self.own_size.width, self.own_size.height)
    }

    /// Adds self as a hit target with the given render ID.
    pub fn add_self(&mut self, target_id: RenderId) {
        self.inner
            .add_hit(BoxHitTestEntry::new(target_id.as_u64(), Matrix4::IDENTITY));
    }

    /// Adds self as a hit target with transform.
    pub fn add_self_with_transform(&mut self, target_id: RenderId, transform: Matrix4) {
        self.inner
            .add_hit(BoxHitTestEntry::new(target_id.as_u64(), transform));
    }

    /// Tests a child at the given offset.
    ///
    /// Automatically transforms position by subtracting the offset.
    pub fn hit_test_child_at_offset(&mut self, index: usize, offset: Offset) -> bool {
        let local_position = self.position_minus(offset);
        self.inner.hit_test_child(index, local_position)
    }
}

// ============================================================================
// SLIVER-SPECIFIC EXTENSIONS
// ============================================================================

use crate::protocol::SliverProtocol;

impl<'ctx, A: Arity, PD: ParentData> HitTestContext<'ctx, SliverProtocol, A, PD>
where
    <SliverHitTest as HitTestCapability>::Context<'ctx, A, PD>:
        HitTestContextApi<'ctx, SliverHitTest, A, PD>,
{
    // ════════════════════════════════════════════════════════════════════════
    // SLIVER POSITION HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the main axis position.
    pub fn main_axis(&self) -> f32 {
        self.inner.position().main_axis
    }

    /// Gets the cross axis position.
    pub fn cross_axis(&self) -> f32 {
        self.inner.position().cross_axis
    }

    /// Gets the position as MainAxisPosition.
    pub fn main_axis_position(&self) -> MainAxisPosition {
        *self.inner.position()
    }

    // ════════════════════════════════════════════════════════════════════════
    // SLIVER HIT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Checks if main axis position is within range.
    pub fn is_within_main_axis_range(&self, start: f32, end: f32) -> bool {
        let pos = self.main_axis();
        pos >= start && pos < end
    }

    /// Checks if cross axis position is within range.
    pub fn is_within_cross_axis_range(&self, start: f32, end: f32) -> bool {
        let pos = self.cross_axis();
        pos >= start && pos < end
    }

    /// Translates position along main axis.
    pub fn position_minus_main_axis(&self, offset: f32) -> MainAxisPosition {
        MainAxisPosition::new(self.main_axis() - offset, self.cross_axis())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;
    use flui_tree::Leaf;
    use flui_types::geometry::Offset;

    use crate::{
        parent_data::BoxParentData,
        protocol::{BoxHitTestCtx, BoxProtocol, HitTestContextApi},
    };

    use super::HitTestContext;

    #[test]
    fn test_hit_test_context_compiles() {
        // This test just verifies the module compiles — empty body is enough
        // because failure surfaces at `cargo build`, not at assert time.
    }

    /// Exercises `HitTestContext<BoxProtocol>::add_self` end-to-end.
    ///
    /// Constructs a real `HitTestContext`, calls `add_self(id)`, then asserts
    /// the entry written into the inner result carries `target_id == id.as_u64()`.
    /// A regression in the body (wrong accessor or cast) would fail this test.
    #[test]
    fn add_self_writes_render_id_as_u64_into_hit_result() {
        let id = RenderId::new(7);
        let inner: BoxHitTestCtx<'_, Leaf, BoxParentData> = BoxHitTestCtx::new(Offset::ZERO);
        let mut ctx: HitTestContext<'_, BoxProtocol, Leaf, BoxParentData> =
            HitTestContext::new(inner, flui_types::Size::ZERO);

        ctx.add_self(id);

        let entries = &ctx.inner().result().path;
        assert_eq!(entries.len(), 1, "exactly one entry after add_self");
        assert_eq!(
            entries[0].target_id,
            id.as_u64(),
            "stored target_id must equal id.as_u64()"
        );
    }
}
