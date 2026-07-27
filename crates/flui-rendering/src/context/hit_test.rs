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

    /// Tests a child at a caller-supplied position.
    ///
    /// # Caller contract
    ///
    /// `position` must ALREADY be local to the child: this method hands it
    /// over verbatim, so any reframing the caller performed to compute it
    /// is invisible here — it does not, by itself, tell the driver to
    /// record anything on the `HitTestResult` transform stack. Whether the
    /// walk pushes something for THIS call depends on whether a transform
    /// was pushed on this context before calling (see below) and, absent
    /// that, on the parent/child pairing:
    ///
    /// - box parent → any child, and sliver parent → sliver child: with no
    ///   ctx-level push in effect, the driver pushes nothing on this path.
    ///   Sound only when the child is committed at `Offset::ZERO` (a
    ///   transparent passthrough that never repositions its child); a
    ///   nonzero offset here silently yields an un-localized position, and
    ///   the sliver walk carries a `debug_assert!` for exactly that.
    /// - sliver parent → box child: the driver DOES push the child's paint
    ///   offset, because crossing into a box frame always decomposes the
    ///   position into box-local coordinates.
    ///
    /// Prefer
    /// [`hit_test_child_at_layout_offset`](Self::hit_test_child_at_layout_offset)
    /// whenever the child has a real laid-out offset — it lets the driver
    /// resolve and push the offset uniformly and cannot silently deliver an
    /// un-localized position. Reach for `push_offset`/`push_transform`/
    /// `with_transform`/`with_offset` (below) plus this method only when the
    /// child's effective position was computed some OTHER way — e.g. a
    /// paint-time-only offset that never lands in `RenderState.offset`
    /// (`RenderFractionalTranslation`), an inverted delegate-supplied matrix
    /// (`RenderFlow`), or an inverted scale/alignment matrix
    /// (`RenderFittedBox`).
    ///
    /// # Ctx-level pushes ARE forwarded to the driver
    ///
    /// A transform pushed on this context (`push_offset`, `push_transform`,
    /// or the `with_offset`/`with_transform` scope helpers) before calling
    /// this method is folded into the shared `HitTestResult` for the
    /// duration of the recursive call: the driver pushes its INVERSE
    /// (mirroring `hit_test_transform`'s own forward-then-invert handling),
    /// so `HitTestEntry.transform` composes correctly. Only the box
    /// protocol does this — `BoxHitTestContext`'s ctx-level stack reaches
    /// the driver; the sliver protocol's ctx-level stack stays a no-op
    /// (main-axis position covers its needs).
    pub fn hit_test_child(
        &mut self,
        index: usize,
        position: <P::HitTest as HitTestCapability>::Position,
    ) -> bool {
        self.inner.hit_test_child(index, position)
    }

    /// Tests a child at its laid-out position (`RenderState.offset`)
    /// — the parent supplies no offset, the driver resolves it and pushes
    /// it onto the transform stack. THE safe way to hit-test children
    /// positioned during layout; parents no longer mirror offsets in their
    /// own fields, and unlike [`hit_test_child`](Self::hit_test_child) this
    /// cannot forget the push.
    pub fn hit_test_child_at_layout_offset(&mut self, index: usize) -> bool {
        self.inner.hit_test_child_at_layout_offset(index)
    }

    // ════════════════════════════════════════════════════════════════════════
    // TRANSFORM MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════
    //
    // Unlike the driver-level `HitTestResult::push_transform`/`push_offset`
    // (RAW primitives — the caller supplies the ALREADY-INVERTED,
    // global-to-local value; see `crates/flui-interaction/docs/HIT_TESTING.md`),
    // these ctx-level methods take the FORWARD, paint-direction transform —
    // the same value a render object already computes to know where its
    // child sits (`RenderFractionalTranslation`'s pixel offset,
    // `RenderFlow`'s per-child delegate matrix). The bridge from this
    // context to the driver — `hit_test_child`/`hit_test_child_at_layout_offset`
    // above — inverts it for you when composing the real
    // `HitTestEntry.transform`, mirroring `HitTestResult::with_paint_transform`.
    // A push here only takes effect for hit-test calls made on THIS context
    // before the matching pop; it does not retroactively affect entries
    // already added.

    /// Pushes a FORWARD (paint-direction) transform, applied to the next
    /// `hit_test_child`/`hit_test_child_at_layout_offset` call on this
    /// context (see the section doc above). Must be paired with
    /// [`pop_transform`](Self::pop_transform) — prefer
    /// [`with_transform`](Self::with_transform), which pairs them for you.
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.inner.push_transform(transform);
    }

    /// Removes the most recently pushed transform or offset.
    pub fn pop_transform(&mut self) {
        self.inner.pop_transform();
    }

    /// Pushes a FORWARD (paint-direction) offset — see
    /// [`push_transform`](Self::push_transform)'s doc; a pure translation
    /// convenience over the same mechanism.
    pub fn push_offset(&mut self, offset: Offset) {
        self.inner.push_offset(offset);
    }

    /// Runs `f` with `transform` pushed (see
    /// [`push_transform`](Self::push_transform)) and pops it before
    /// returning.
    pub fn with_transform<F, R>(&mut self, transform: Matrix4, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.inner.push_transform(transform);
        let result = f(self);
        self.inner.pop_transform();
        result
    }

    /// Runs `f` with `offset` pushed (see
    /// [`push_offset`](Self::push_offset)) and pops it before returning.
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
    /// Automatically transforms position by subtracting the offset — and,
    /// mirroring what [`hit_test_child_at_layout_offset`](Self::hit_test_child_at_layout_offset)
    /// does for a `RenderState`-owned offset, records that same offset on
    /// the ctx-level transform stack
    /// (see the "Ctx-level pushes ARE forwarded to the driver" section on
    /// [`hit_test_child`](Self::hit_test_child)) so it reaches
    /// `HitTestEntry.transform`. The push happens unconditionally — this
    /// method always moves `position` into the child's frame (even when
    /// `offset` is `Offset::ZERO`, a no-op translation), so per the rule
    /// this API follows throughout (push the child offset iff the position
    /// handed to the child was moved into the child's frame), it must
    /// always push. A previous version of this method moved the position
    /// but never pushed, which silently dropped it from the recorded
    /// `HitTestEntry.transform` — see
    /// `crates/flui-interaction/docs/HIT_TESTING.md`.
    pub fn hit_test_child_at_offset(&mut self, index: usize, offset: Offset) -> bool {
        let local_position = self.position_minus(offset);
        // A zero offset is no frame change, so there is nothing to record —
        // and skipping it keeps the stack EMPTY, which is what lets
        // `local_transform_for_driver` return `None` and the driver avoid a
        // `with_paint_transform(IDENTITY)` (a determinant plus a full 4x4
        // inverse) per level. Most callers pass `Offset::ZERO`: the proxy
        // render objects (opacity, repaint boundary, listener, mouse region,
        // absorb/ignore pointer, ...) sit on the pointer hot path and stack
        // several deep in an ordinary tree.
        if offset == Offset::ZERO {
            return self.inner.hit_test_child(index, local_position);
        }
        self.push_offset(offset);
        let hit = self.inner.hit_test_child(index, local_position);
        self.pop_transform();
        hit
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
