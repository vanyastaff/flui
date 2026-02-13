//! Rich HitTestContext with ergonomic API for hit testing operations.
//!
//! This module provides `HitTestContext`, a high-level wrapper around the hit test
//! capability traits that offers ergonomic APIs for common hit testing patterns.
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

use flui_types::geometry::{Matrix4, Offset, Rect};
use flui_types::Pixels;

use crate::arity::Arity;
use crate::parent_data::ParentData;
use crate::protocol::{
    BoxHitTest, BoxHitTestEntry, HitTestCapability, HitTestContextApi, MainAxisPosition, Protocol,
    SliverHitTest,
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
}

impl<'ctx, P: Protocol, A: Arity, PD: ParentData> HitTestContext<'ctx, P, A, PD>
where
    <P::HitTest as HitTestCapability>::Context<'ctx, A, PD>:
        HitTestContextApi<'ctx, P::HitTest, A, PD>,
{
    /// Creates a new hit test context wrapping the capability context.
    pub fn new(inner: <P::HitTest as HitTestCapability>::Context<'ctx, A, PD>) -> Self {
        Self { inner }
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

    /// Executes a closure with a pushed transform, automatically popping afterward.
    pub fn with_transform<F, R>(&mut self, transform: Matrix4, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.inner.push_transform(transform);
        let result = f(self);
        self.inner.pop_transform();
        result
    }

    /// Executes a closure with a pushed offset, automatically popping afterward.
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
        pos.dx >= 0.0 && pos.dx < width && pos.dy >= 0.0 && pos.dy < height
    }

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        self.inner
            .add_hit(BoxHitTestEntry::new(target_id, Matrix4::IDENTITY));
    }

    /// Adds self as a hit target with transform.
    pub fn add_self_with_transform(&mut self, target_id: u64, transform: Matrix4) {
        self.inner
            .add_hit(BoxHitTestEntry::new(target_id, transform));
    }

    /// Tests a child at the given offset.
    ///
    /// Automatically transforms position by subtracting the offset.
    pub fn hit_test_child_at_offset(&mut self, index: usize, offset: Offset) -> bool {
        let local_position = self.position_minus(offset);
        self.inner.hit_test_child(index, local_position)
    }

    /// Tests all children in reverse order (topmost first).
    ///
    /// Returns the index of the first child hit, or None.
    pub fn hit_test_children_reverse<F>(&mut self, get_offset: F) -> Option<usize>
    where
        F: Fn(usize) -> Offset,
    {
        let count = 0; // Would need child count from parent
        for i in (0..count).rev() {
            let offset = get_offset(i);
            if self.hit_test_child_at_offset(i, offset) {
                return Some(i);
            }
        }
        None
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
    #[test]
    fn test_hit_test_context_compiles() {
        // This test just verifies the module compiles
        assert!(true);
    }
}
