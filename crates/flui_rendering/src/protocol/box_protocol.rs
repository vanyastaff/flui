//! Box protocol for 2D cartesian layout.
//!
//! This module provides the BoxProtocol and its capability implementations:
//! - [`BoxProtocol`]: Main protocol type
//! - [`BoxLayout`]: Layout capability (BoxConstraints → Size)
//! - [`BoxHitTest`]: Hit test capability (Offset → BoxHitTestResult)

use flui_types::geometry::{Matrix4, Offset, Point, Rect};
use flui_types::Size;

use crate::arity::Arity;
use crate::constraints::{BoxConstraints, Constraints};
use crate::parent_data::{BoxParentData, ParentData};
use crate::protocol::capabilities::{
    HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi,
};
use crate::protocol::protocol::{sealed, BidirectionalProtocol, Protocol, ProtocolCompatible};

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box protocol using 2D constraints and sizes.
///
/// This is the most common protocol for 2D layout with width/height constraints.
/// Used by most widgets: containers, buttons, text, images, etc.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl sealed::Sealed for BoxProtocol {}

impl Protocol for BoxProtocol {
    type Layout = BoxLayout;
    type HitTest = BoxHitTest;
    type DefaultParentData = BoxParentData;

    fn name() -> &'static str {
        "box"
    }
}

impl BidirectionalProtocol for BoxProtocol {}

// Self-compatibility
impl ProtocolCompatible<BoxProtocol> for BoxProtocol {
    fn is_compatible() -> bool {
        true
    }
}

// ============================================================================
// BOX LAYOUT CAPABILITY
// ============================================================================

/// Layout capability for box (2D) layout.
///
/// Uses `BoxConstraints` for input and `Size` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxLayout;

/// Cache key for BoxConstraints.
///
/// Uses integer representation of floats (bits) for reliable hashing.
/// This handles -0.0/+0.0 and provides exact equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoxConstraintsCacheKey {
    min_width_bits: u32,
    max_width_bits: u32,
    min_height_bits: u32,
    max_height_bits: u32,
}

impl BoxConstraintsCacheKey {
    /// Creates a cache key from constraints.
    ///
    /// Returns `None` if any value is NaN.
    pub fn from_constraints(c: &BoxConstraints) -> Option<Self> {
        // NaN check - NaN != NaN
        if c.min_width != c.min_width
            || c.max_width != c.max_width
            || c.min_height != c.min_height
            || c.max_height != c.max_height
        {
            return None;
        }

        Some(Self {
            min_width_bits: c.min_width.to_bits(),
            max_width_bits: c.max_width.to_bits(),
            min_height_bits: c.min_height.to_bits(),
            max_height_bits: c.max_height.to_bits(),
        })
    }
}

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type CacheKey = BoxConstraintsCacheKey;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        Size::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn cache_key(constraints: &Self::Constraints) -> Option<Self::CacheKey> {
        BoxConstraintsCacheKey::from_constraints(constraints)
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Box layout context implementation.
pub struct BoxLayoutCtx<'ctx, A: Arity, P: ParentData> {
    constraints: BoxConstraints,
    geometry: Option<Size>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> BoxLayoutCtx<'ctx, A, P> {
    /// Creates a new box layout context with given constraints.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            geometry: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Gets the current geometry if layout is complete.
    pub fn geometry(&self) -> Option<&Size> {
        self.geometry.as_ref()
    }
}

impl<'ctx, A: Arity, P: ParentData> LayoutContextApi<'ctx, BoxLayout, A, P>
    for BoxLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &BoxConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.geometry.is_some()
    }

    fn complete_layout(&mut self, geometry: Size) {
        self.geometry = Some(geometry);
    }

    fn child_count(&self) -> usize {
        0 // Override in actual implementation with children
    }

    fn layout_child(&mut self, _index: usize, _constraints: BoxConstraints) -> Size {
        Size::ZERO // Override in actual implementation
    }

    fn position_child(&mut self, _index: usize, _offset: Offset) {
        // Override in actual implementation
    }

    fn child_geometry(&self, _index: usize) -> Option<&Size> {
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
// BOX HIT TEST CAPABILITY
// ============================================================================

/// Hit test capability for box (2D) layout.
///
/// Uses `Offset` for position and standard hit test result.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxHitTest;

impl HitTestCapability for BoxHitTest {
    type Position = Offset;
    type Result = BoxHitTestResult;
    type Entry = BoxHitTestEntry;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxHitTestCtx<'ctx, A, P>
    where
        Self: 'ctx;
}

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

    /// Returns the number of hit entries.
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Clears all hit entries.
    pub fn clear(&mut self) {
        self.path.clear();
    }
}

/// Individual hit test entry for box protocol.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// Target identifier.
    pub target_id: u64,
    /// Transform from target to root coordinates.
    pub transform: Matrix4,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(target_id: u64, transform: Matrix4) -> Self {
        Self {
            target_id,
            transform,
        }
    }

    /// Creates a hit test entry with identity transform.
    pub fn with_id(target_id: u64) -> Self {
        Self::new(target_id, Matrix4::IDENTITY)
    }
}

/// Box hit test context implementation.
pub struct BoxHitTestCtx<'ctx, A: Arity, P: ParentData> {
    position: Offset,
    result: BoxHitTestResult,
    transform_stack: Vec<Matrix4>,
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
    pub fn current_transform(&self) -> Matrix4 {
        self.transform_stack
            .iter()
            .fold(Matrix4::IDENTITY, |acc, t| acc * *t)
    }

    /// Adds self as a hit target with the given ID.
    pub fn add_self(&mut self, target_id: u64) {
        let transform = self.current_transform();
        self.result.add(BoxHitTestEntry::new(target_id, transform));
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

    fn is_hit(&self, bounds: Rect) -> bool {
        bounds.contains(Point::new(self.position.dx, self.position.dy))
    }

    fn hit_test_child(&mut self, _index: usize, _position: Offset) -> bool {
        false // Override in actual implementation
    }

    fn push_transform(&mut self, transform: Matrix4) {
        self.transform_stack.push(transform);
    }

    fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::Leaf;

    #[test]
    fn test_box_protocol_name() {
        assert_eq!(BoxProtocol::name(), "box");
    }

    #[test]
    fn test_box_layout_default_geometry() {
        let size = BoxLayout::default_geometry();
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add(BoxHitTestEntry::with_id(1));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_box_hit_test_context() {
        let ctx: BoxHitTestCtx<'_, Leaf, BoxParentData> =
            BoxHitTestCtx::new(Offset::new(50.0, 50.0));

        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        assert!(ctx.is_hit(bounds));

        let outside = Rect::from_ltrb(100.0, 100.0, 200.0, 200.0);
        assert!(!ctx.is_hit(outside));
    }

    #[test]
    fn test_box_layout_context() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);

        assert!(!ctx.is_complete());
        assert_eq!(ctx.constraints().max_width, 100.0);

        ctx.complete_layout(Size::new(100.0, 100.0));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_box_constraints_cache_key_equality() {
        let c1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c2 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c3 = BoxConstraints::tight(Size::new(200.0, 100.0));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_box_constraints_cache_key_nan() {
        let c = BoxConstraints::new(f32::NAN, 100.0, 0.0, 100.0);
        assert!(BoxConstraintsCacheKey::from_constraints(&c).is_none());
    }

    #[test]
    fn test_box_constraints_cache_key_negative_zero() {
        // -0.0 and +0.0 should produce different cache keys (bit-exact)
        let c1 = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let c2 = BoxConstraints::new(-0.0, 100.0, 0.0, 100.0);

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();

        // They have different bits, so different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_box_constraints_cache_key_hash() {
        use std::collections::HashSet;

        let c1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c2 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c3 = BoxConstraints::tight(Size::new(200.0, 100.0));

        let key1 = BoxConstraintsCacheKey::from_constraints(&c1).unwrap();
        let key2 = BoxConstraintsCacheKey::from_constraints(&c2).unwrap();
        let key3 = BoxConstraintsCacheKey::from_constraints(&c3).unwrap();

        let mut set = HashSet::new();
        set.insert(key1);

        // key2 is equal to key1, so set size should stay 1
        set.insert(key2);
        assert_eq!(set.len(), 1);

        // key3 is different, so set size should become 2
        set.insert(key3);
        assert_eq!(set.len(), 2);
    }
}
