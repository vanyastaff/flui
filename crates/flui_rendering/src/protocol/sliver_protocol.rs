//! Sliver protocol for scrollable viewport layout.
//!
//! This module provides the SliverProtocol and its capability implementations:
//! - [`SliverProtocol`]: Main protocol type for scrollable content
//! - [`SliverLayout`]: Layout capability (SliverConstraints → SliverGeometry)
//! - [`SliverHitTest`]: Hit test capability (MainAxisPosition → SliverHitTestResult)

use flui_types::geometry::{Matrix4, Offset, Rect};

use crate::arity::Arity;
use crate::constraints::{Constraints, SliverConstraints, SliverGeometry};
use crate::parent_data::{ParentData, SliverParentData};
use crate::protocol::box_protocol::BoxProtocol;
use crate::protocol::capabilities::{
    HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi,
};
use crate::protocol::protocol::{sealed, Protocol, ProtocolCompatible};

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Sliver protocol for scrollable viewport children.
///
/// Slivers are laid out along a single scrolling axis with viewport constraints.
/// Used by scrollable widgets: ListView, GridView, CustomScrollView, etc.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverProtocol;

impl sealed::Sealed for SliverProtocol {}

impl Protocol for SliverProtocol {
    type Layout = SliverLayout;
    type HitTest = SliverHitTest;
    type DefaultParentData = SliverParentData;

    fn name() -> &'static str {
        "sliver"
    }
}

// Self-compatibility
impl ProtocolCompatible<SliverProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// Box and Sliver can be adapted together
impl ProtocolCompatible<BoxProtocol> for SliverProtocol {
    fn is_compatible() -> bool {
        true
    }
}

impl ProtocolCompatible<SliverProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// ============================================================================
// SLIVER LAYOUT CAPABILITY
// ============================================================================

/// Layout capability for sliver (scrollable) layout.
///
/// Uses `SliverConstraints` for input and `SliverGeometry` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverLayout;

/// Cache key for SliverConstraints.
///
/// Uses integer representation of floats (bits) for reliable hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliverConstraintsCacheKey {
    axis_direction: u8,
    growth_direction: u8,
    cross_axis_extent_bits: u32,
    viewport_main_axis_extent_bits: u32,
    scroll_offset_bits: u32,
    remaining_paint_extent_bits: u32,
    overlap_bits: u32,
    remaining_cache_extent_bits: u32,
    cache_origin_bits: u32,
    preceding_scroll_extent_bits: u32,
}

impl SliverConstraintsCacheKey {
    /// Creates a cache key from constraints.
    ///
    /// Returns `None` if any float value is NaN.
    pub fn from_constraints(c: &SliverConstraints) -> Option<Self> {
        // NaN check helper
        let is_nan = |v: f32| v != v;

        if is_nan(c.cross_axis_extent)
            || is_nan(c.viewport_main_axis_extent)
            || is_nan(c.scroll_offset)
            || is_nan(c.remaining_paint_extent)
            || is_nan(c.overlap)
            || is_nan(c.remaining_cache_extent)
            || is_nan(c.cache_origin)
            || is_nan(c.preceding_scroll_extent)
        {
            return None;
        }

        Some(Self {
            axis_direction: c.axis_direction as u8,
            growth_direction: c.growth_direction as u8,
            cross_axis_extent_bits: c.cross_axis_extent.to_bits(),
            viewport_main_axis_extent_bits: c.viewport_main_axis_extent.to_bits(),
            scroll_offset_bits: c.scroll_offset.to_bits(),
            remaining_paint_extent_bits: c.remaining_paint_extent.to_bits(),
            overlap_bits: c.overlap.to_bits(),
            remaining_cache_extent_bits: c.remaining_cache_extent.to_bits(),
            cache_origin_bits: c.cache_origin.to_bits(),
            preceding_scroll_extent_bits: c.preceding_scroll_extent.to_bits(),
        })
    }
}

impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type CacheKey = SliverConstraintsCacheKey;
    type Context<'ctx, A: Arity, P: ParentData + Default>
        = SliverLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        SliverGeometry::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey> {
        SliverConstraintsCacheKey::from_constraints(constraints)
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Sliver layout context implementation.
pub struct SliverLayoutCtx<'ctx, A: Arity, P: ParentData> {
    constraints: SliverConstraints,
    geometry: Option<SliverGeometry>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> SliverLayoutCtx<'ctx, A, P> {
    /// Creates a new sliver layout context with given constraints.
    pub fn new(constraints: SliverConstraints) -> Self {
        Self {
            constraints,
            geometry: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Gets the current geometry if layout is complete.
    pub fn geometry(&self) -> Option<&SliverGeometry> {
        self.geometry.as_ref()
    }

    // ════════════════════════════════════════════════════════════════════════
    // SLIVER-SPECIFIC HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the scroll offset from constraints.
    pub fn scroll_offset(&self) -> f32 {
        self.constraints.scroll_offset
    }

    /// Gets the remaining paint extent.
    pub fn remaining_paint_extent(&self) -> f32 {
        self.constraints.remaining_paint_extent
    }

    /// Gets the viewport main axis extent.
    pub fn viewport_main_axis_extent(&self) -> f32 {
        self.constraints.viewport_main_axis_extent
    }

    /// Gets the cross axis extent.
    pub fn cross_axis_extent(&self) -> f32 {
        self.constraints.cross_axis_extent
    }
}

impl<'ctx, A: Arity, P: ParentData> LayoutContextApi<'ctx, SliverLayout, A, P>
    for SliverLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.geometry.is_some()
    }

    fn complete_layout(&mut self, geometry: SliverGeometry) {
        self.geometry = Some(geometry);
    }

    fn child_count(&self) -> usize {
        0 // Override in actual implementation
    }

    fn layout_child(&mut self, _index: usize, _constraints: SliverConstraints) -> SliverGeometry {
        SliverGeometry::ZERO // Override in actual implementation
    }

    fn position_child(&mut self, _index: usize, _offset: Offset) {
        // Override in actual implementation
    }

    fn child_geometry(&self, _index: usize) -> Option<&SliverGeometry> {
        None // Override in actual implementation
    }

    fn child_parent_data(&self, _index: usize) -> Option<&P> {
        None // Override in actual implementation
    }

    fn child_parent_data_mut(&mut self, _index: usize) -> Option<&mut P> {
        None // Override in actual implementation
    }
}

// ============================================================================
// SLIVER HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for sliver (scrollable) layout.
///
/// Uses main axis position for hit testing along scroll direction.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverHitTest;

impl HitTestCapability for SliverHitTest {
    type Position = MainAxisPosition;
    type Result = SliverHitTestResult;
    type Entry = SliverHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = SliverHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

/// Main axis position for sliver hit testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MainAxisPosition {
    /// Position along the main (scroll) axis.
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

    /// Creates from an offset assuming vertical scrolling.
    pub fn from_vertical_offset(offset: Offset) -> Self {
        Self::new(offset.dy, offset.dx)
    }

    /// Creates from an offset assuming horizontal scrolling.
    pub fn from_horizontal_offset(offset: Offset) -> Self {
        Self::new(offset.dx, offset.dy)
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

    /// Returns whether any targets were hit.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the number of hit entries.
    pub fn len(&self) -> usize {
        self.path.len()
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

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        self.result
            .add(SliverHitTestEntry::new(target_id, self.position.main_axis));
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

    fn is_hit(&self, bounds: Rect) -> bool {
        // For slivers, check if main axis position is within bounds height
        self.position.main_axis >= 0.0 && self.position.main_axis <= bounds.height()
    }

    fn hit_test_child(&mut self, _index: usize, _position: MainAxisPosition) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, _transform: Matrix4) {
        // Slivers typically use main axis offset instead of full transforms
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
    fn test_sliver_protocol_name() {
        assert_eq!(SliverProtocol::name(), "sliver");
    }

    #[test]
    fn test_sliver_layout_default_geometry() {
        let geometry = SliverLayout::default_geometry();
        assert_eq!(geometry, SliverGeometry::ZERO);
    }

    #[test]
    fn test_main_axis_position() {
        let pos = MainAxisPosition::new(100.0, 50.0);
        assert_eq!(pos.main_axis, 100.0);
        assert_eq!(pos.cross_axis, 50.0);
    }

    #[test]
    fn test_sliver_hit_test_result() {
        let mut result = SliverHitTestResult::new();
        assert!(result.is_empty());

        result.add(SliverHitTestEntry::new(1, 100.0));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_protocol_compatibility() {
        use crate::protocol::protocol::ProtocolCompatible;

        assert!(<SliverProtocol as ProtocolCompatible<SliverProtocol>>::is_compatible());
        assert!(<SliverProtocol as ProtocolCompatible<BoxProtocol>>::is_compatible());
        assert!(<BoxProtocol as ProtocolCompatible<SliverProtocol>>::is_compatible());
    }
}
