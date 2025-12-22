//! Hit test capability trait for protocol composition.
//!
//! This module defines the `HitTestCapability` trait which groups:
//! - Position (hit test input position)
//! - Result (hit test result accumulator)
//! - Entry (individual hit test entry)
//! - HitTestContext (GAT for hit test operations)

use crate::arity::Arity;
use crate::parent_data::ParentData;
use flui_types::Offset;
use std::fmt::Debug;

// ============================================================================
// HIT TEST CAPABILITY
// ============================================================================

/// Capability trait for hit testing operations.
///
/// Groups all types related to hit testing: position input,
/// result accumulator, entry type, and the hit test context GAT.
///
/// # Type Parameters
///
/// - `Position`: Hit test position type (usually Offset or custom)
/// - `Result`: Accumulator for hit test results
/// - `Entry`: Individual hit test entry (what gets added to result)
/// - `Context<'ctx, A, P>`: Hit test context with lifetime, arity, and parent data
///
/// # Example
///
/// ```ignore
/// pub struct BoxHitTest;
///
/// impl HitTestCapability for BoxHitTest {
///     type Position = Offset;
///     type Result = BoxHitTestResult;
///     type Entry = BoxHitTestEntry;
///     type Context<'ctx, A: Arity, P: ParentData> = BoxHitTestContext<'ctx, A, P>
///     where Self: 'ctx;
/// }
/// ```
pub trait HitTestCapability: Send + Sync + 'static {
    /// Hit test position type.
    ///
    /// Usually `Offset` for 2D, could be custom for slivers.
    type Position: Clone + Debug + Default + Send + Sync + 'static;

    /// Hit test result accumulator.
    ///
    /// Collects all hit entries during traversal.
    type Result: Default + Send + Sync + 'static;

    /// Individual hit test entry.
    ///
    /// Represents a single hit target with transform info.
    type Entry: Clone + Debug + Send + Sync + 'static;

    /// Hit test context with Generic Associated Type.
    ///
    /// Provides access to position, result, and child hit testing.
    type Context<'ctx, A: Arity, P: ParentData>: HitTestContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;
}

// ============================================================================
// HIT TEST CONTEXT API
// ============================================================================

/// API for hit test context operations.
///
/// Provides access to position and hit test operations.
/// Implemented by protocol-specific hit test contexts.
pub trait HitTestContextApi<'ctx, H: HitTestCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the hit test position in local coordinates.
    fn position(&self) -> &H::Position;

    /// Gets the hit test result accumulator.
    fn result(&self) -> &H::Result;

    /// Gets mutable reference to hit test result.
    fn result_mut(&mut self) -> &mut H::Result;

    /// Adds a hit entry to the result.
    fn add_hit(&mut self, entry: H::Entry);

    /// Checks if position is inside the given bounds.
    fn is_hit(&self, bounds: flui_types::Rect) -> bool;

    /// Tests a child for hits with position transformation.
    ///
    /// Returns `true` if child was hit.
    fn hit_test_child(&mut self, index: usize, position: H::Position) -> bool;

    /// Adds a transform to the hit test path.
    fn push_transform(&mut self, transform: flui_types::Matrix4);

    /// Removes the most recent transform.
    fn pop_transform(&mut self);

    /// Adds an offset transform (convenience method).
    fn push_offset(&mut self, offset: Offset) {
        self.push_transform(flui_types::Matrix4::translation(offset.dx, offset.dy, 0.0));
    }
}

// ============================================================================
// BOX HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for box (2D) layout.
///
/// Uses `Offset` for position and standard hit test result.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxHitTest;

/// Hit test result for box protocol.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    /// Path of hit test entries from leaf to root.
    pub path: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self { path: Vec::new() }
    }

    /// Adds an entry to the hit test path.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.path.push(entry);
    }

    /// Returns whether any targets were hit.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}

/// Individual hit test entry for box protocol.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Transform from target to root coordinates.
    pub transform: flui_types::Matrix4,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(target_id: u64, transform: flui_types::Matrix4) -> Self {
        Self {
            target_id,
            transform,
        }
    }
}

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Box hit test context implementation.
pub struct BoxHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: Offset,
    result: BoxHitTestResult,
    transform_stack: Vec<flui_types::Matrix4>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> BoxHitTestCtx<'ctx, A, P> {
    /// Creates a new box hit test context.
    pub fn new(position: Offset) -> Self {
        Self {
            position,
            result: BoxHitTestResult::new(),
            transform_stack: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the current accumulated transform.
    pub fn current_transform(&self) -> flui_types::Matrix4 {
        self.transform_stack
            .iter()
            .fold(flui_types::Matrix4::IDENTITY, |acc, t| acc * *t)
    }
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, BoxHitTest, A, P>
    for BoxHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &Offset {
        &self.position
    }

    fn result(&self) -> &BoxHitTestResult {
        &self.result
    }

    fn result_mut(&mut self) -> &mut BoxHitTestResult {
        &mut self.result
    }

    fn add_hit(&mut self, entry: BoxHitTestEntry) {
        self.result.add(entry);
    }

    fn is_hit(&self, bounds: flui_types::Rect) -> bool {
        bounds.contains(flui_types::Point::new(self.position.dx, self.position.dy))
    }

    fn hit_test_child(&mut self, _index: usize, _position: Offset) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, transform: flui_types::Matrix4) {
        self.transform_stack.push(transform);
    }

    fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }
}

// ============================================================================
// SLIVER HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for sliver (scrollable) layout.
///
/// Uses main axis position for hit testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverHitTest;

/// Main axis position for sliver hit testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MainAxisPosition {
    /// Position along the main axis.
    pub main_axis: f32,
    /// Position along the cross axis.
    pub cross_axis: f32,
}

impl MainAxisPosition {
    /// Creates a new main axis position.
    pub fn new(main_axis: f32, cross_axis: f32) -> Self {
        Self {
            main_axis,
            cross_axis,
        }
    }
}

/// Hit test result for sliver protocol.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    /// Path of hit test entries.
    pub path: Vec<SliverHitTestEntry>,
}

impl SliverHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self { path: Vec::new() }
    }

    /// Adds an entry to the hit test path.
    pub fn add(&mut self, entry: SliverHitTestEntry) {
        self.path.push(entry);
    }
}

/// Individual hit test entry for sliver protocol.
#[derive(Debug, Clone)]
pub struct SliverHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Main axis position where hit occurred.
    pub main_axis_position: f32,
}

impl SliverHitTestEntry {
    /// Creates a new sliver hit test entry.
    pub fn new(target_id: u64, main_axis_position: f32) -> Self {
        Self {
            target_id,
            main_axis_position,
        }
    }
}

impl HitTestCapability for SliverHitTest {
    type Position = MainAxisPosition;
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = SliverHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Sliver hit test context implementation.
pub struct SliverHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: MainAxisPosition,
    result: SliverHitTestResult,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> SliverHitTestCtx<'ctx, A, P> {
    /// Creates a new sliver hit test context.
    pub fn new(position: MainAxisPosition) -> Self {
        Self {
            position,
            result: SliverHitTestResult::new(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'ctx, A: Arity, P: ParentData> HitTestContextApi<'ctx, SliverHitTest, A, P>
    for SliverHitTestCtx<'ctx, A, P>
{
    fn position(&self) -> &MainAxisPosition {
        &self.position
    }

    fn result(&self) -> &SliverHitTestResult {
        &self.result
    }

    fn result_mut(&mut self) -> &mut SliverHitTestResult {
        &mut self.result
    }

    fn add_hit(&mut self, entry: SliverHitTestEntry) {
        self.result.add(entry);
    }

    fn is_hit(&self, bounds: flui_types::Rect) -> bool {
        // For slivers, check if main axis position is within bounds
        self.position.main_axis >= 0.0 && self.position.main_axis <= bounds.height()
    }

    fn hit_test_child(&mut self, _index: usize, _position: MainAxisPosition) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, _transform: flui_types::Matrix4) {
        // Slivers typically use main axis offset instead
    }

    fn pop_transform(&mut self) {
        // No-op for basic sliver hit test
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add(BoxHitTestEntry::new(1, flui_types::Matrix4::IDENTITY));
        assert!(!result.is_empty());
        assert_eq!(result.path.len(), 1);
    }

    #[test]
    fn test_main_axis_position() {
        let pos = MainAxisPosition::new(100.0, 50.0);
        assert_eq!(pos.main_axis, 100.0);
        assert_eq!(pos.cross_axis, 50.0);
    }

    #[test]
    fn test_box_hit_test_context() {
        use crate::arity::Leaf;
        use crate::parent_data::BoxParentData;

        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(50.0, 50.0));

        let bounds = flui_types::Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        assert!(ctx.is_hit(bounds));

        let outside_bounds = flui_types::Rect::from_ltrb(100.0, 100.0, 200.0, 200.0);
        assert!(!ctx.is_hit(outside_bounds));
    }
}
