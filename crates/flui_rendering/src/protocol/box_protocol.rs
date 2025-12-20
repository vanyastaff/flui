//! BoxProtocol - 2D Cartesian layout protocol with intrinsic sizing.
//!
//! Enhanced box protocol featuring intrinsic dimensions, baseline alignment,
//! bidirectional layout, and tight integration with flui-elite arena system.

use flui_types::{Offset, Rect, Size};
use std::hash::{Hash, Hasher};

use super::base::{
    BaselineProtocol, BidirectionalProtocol, Canvas, HitTestContext, HitTestTarget,
    IntrinsicProtocol, LayoutContext, PaintContext, Protocol, ProtocolId, Sealed,
};
use crate::constraints::BoxConstraints;
use crate::parent_data::BoxParentData;
use crate::traits::RenderObject;

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// 2D Cartesian layout protocol with comprehensive feature support.
///
/// # Features
///
/// - Intrinsic sizing queries for optimal layout decisions
/// - Baseline alignment for text and inline content
/// - Bidirectional layout support
/// - GAT-based contexts with lifetime safety
/// - Constraint normalization for reliable caching
///
/// # Layout Model
///
/// 1. Parent passes `BoxConstraints` (min/max width and height)
/// 2. Child queries intrinsic sizes if needed (optional optimization)
/// 3. Child computes size within constraints
/// 4. Child returns `Size` as layout result
/// 5. Parent positions child using `Offset` stored in parent data
///
/// # Example
///
/// ```ignore
/// use flui_rendering::protocol::{BoxProtocol, Protocol};
/// use flui_elite::prelude::*;
///
/// let arena: EliteArena<Box<dyn RenderBox>, RenderId> = EliteArena::new();
/// let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
/// let normalized = BoxProtocol::normalize_constraints(constraints);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxProtocol;

impl Sealed for BoxProtocol {}

// ============================================================================
// CORE IMPLEMENTATION
// ============================================================================

impl Protocol for BoxProtocol {
    type Object = dyn RenderObject;
    type Constraints = BoxConstraints;
    type ParentData = BoxParentData;
    type Geometry = Size;

    type LayoutContext<'ctx>
        = BoxLayoutContext<'ctx>
    where
        Self: 'ctx;
    type PaintContext<'ctx>
        = BoxPaintContext<'ctx>
    where
        Self: 'ctx;
    type HitTestContext<'ctx>
        = BoxHitTestContext<'ctx>
    where
        Self: 'ctx;

    fn name() -> &'static str {
        "box"
    }

    fn default_geometry() -> Size {
        Size::ZERO
    }

    fn validate_constraints(c: &Self::Constraints) -> bool {
        c.is_normalized()
            && c.min_width >= 0.0
            && c.min_height >= 0.0
            && c.max_width >= c.min_width
            && c.max_height >= c.min_height
    }

    fn normalize_constraints(mut c: Self::Constraints) -> Self::Constraints {
        // Round to 2 decimal places for consistent cache keys
        let round = |v: f32| (v * 100.0).round() / 100.0;
        c.min_width = round(c.min_width);
        c.max_width = round(c.max_width);
        c.min_height = round(c.min_height);
        c.max_height = round(c.max_height);
        c
    }
}

impl BidirectionalProtocol for BoxProtocol {}

impl IntrinsicProtocol for BoxProtocol {
    fn compute_min_intrinsic_main_axis(&self, _cross_axis: f32) -> f32 {
        0.0 // Override in specific render objects
    }

    fn compute_max_intrinsic_main_axis(&self, _cross_axis: f32) -> f32 {
        f32::INFINITY // Override in specific render objects
    }
}

impl BaselineProtocol for BoxProtocol {
    fn get_distance_to_baseline(&self) -> Option<f32> {
        None // Override in text/inline render objects
    }
}

impl BoxProtocol {
    /// Get protocol ID (box = 1).
    pub const fn protocol_id() -> ProtocolId {
        ProtocolId::new(1)
    }
}

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// Box layout context providing child access and constraint helpers.
pub struct BoxLayoutContext<'ctx> {
    constraints: BoxConstraints,
    is_complete: bool,
    geometry: Option<Size>,
    children: Option<&'ctx mut dyn ChildAccessor>,
}

impl<'ctx> BoxLayoutContext<'ctx> {
    /// Create new context.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self::with_children_optional(constraints, None)
    }

    /// Create context with child accessor.
    pub fn with_children(
        constraints: BoxConstraints,
        children: &'ctx mut dyn ChildAccessor,
    ) -> Self {
        Self::with_children_optional(constraints, Some(children))
    }

    /// Internal: unified constructor.
    fn with_children_optional(
        constraints: BoxConstraints,
        children: Option<&'ctx mut dyn ChildAccessor>,
    ) -> Self {
        Self {
            constraints,
            is_complete: false,
            geometry: None,
            children,
        }
    }

    /// Get maximum allowed size.
    #[inline]
    pub fn biggest_size(&self) -> Size {
        Size::new(self.constraints.max_width, self.constraints.max_height)
    }

    /// Get minimum required size.
    #[inline]
    pub fn smallest_size(&self) -> Size {
        Size::new(self.constraints.min_width, self.constraints.min_height)
    }

    /// Constrain size to fit within constraints.
    #[inline]
    pub fn constrain_size(&self, size: Size) -> Size {
        self.constraints.constrain(size)
    }

    /// Check if constraints are tight (fixed size).
    #[inline]
    pub fn is_tight(&self) -> bool {
        self.constraints.is_tight()
    }

    /// Layout child at index with given constraints.
    pub fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Option<Size> {
        self.children.as_mut()?.layout_child(index, constraints)
    }

    /// Set child position in parent data.
    pub fn position_child(&mut self, index: usize, offset: Offset) {
        if let Some(children) = self.children.as_mut() {
            children.position_child(index, offset);
        }
    }
}

impl<'ctx> LayoutContext<'ctx, BoxProtocol> for BoxLayoutContext<'ctx> {
    fn constraints(&self) -> &BoxConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn complete_layout(&mut self, geometry: Size) {
        self.geometry = Some(geometry);
        self.is_complete = true;
    }

    fn child_count(&self) -> usize {
        self.children.as_ref().map_or(0, |c| c.child_count())
    }
}

/// Accessor trait for child layout operations.
pub trait ChildAccessor: Send + Sync {
    /// Get number of children.
    fn child_count(&self) -> usize;

    /// Layout child with constraints, returning computed size.
    fn layout_child(&mut self, index: usize, constraints: BoxConstraints) -> Option<Size>;

    /// Set child position in parent data.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// Get child parent data (read-only).
    fn parent_data(&self, index: usize) -> Option<&BoxParentData>;

    /// Get mutable child parent data.
    fn parent_data_mut(&mut self, index: usize) -> Option<&mut BoxParentData>;
}

// ============================================================================
// PAINT CONTEXT
// ============================================================================

/// Box paint context tracking transform and clip depth.
pub struct BoxPaintContext<'ctx> {
    canvas: &'ctx mut dyn Canvas,
    transform_depth: usize,
    clip_depth: usize,
}

impl<'ctx> BoxPaintContext<'ctx> {
    /// Create new paint context.
    pub fn new(canvas: &'ctx mut dyn Canvas) -> Self {
        Self {
            canvas,
            transform_depth: 0,
            clip_depth: 0,
        }
    }

    /// Get current transform stack depth.
    pub fn transform_depth(&self) -> usize {
        self.transform_depth
    }

    /// Get current clip stack depth.
    pub fn clip_depth(&self) -> usize {
        self.clip_depth
    }
}

impl<'ctx> PaintContext<'ctx, BoxProtocol> for BoxPaintContext<'ctx> {
    fn canvas(&mut self) -> &mut dyn Canvas {
        self.canvas
    }

    fn push_transform(&mut self, _transform: flui_types::Matrix4) {
        self.transform_depth += 1;
    }

    fn pop_transform(&mut self) {
        self.transform_depth = self.transform_depth.saturating_sub(1);
    }

    fn push_clip_rect(&mut self, _rect: Rect) {
        self.clip_depth += 1;
    }

    fn pop_clip(&mut self) {
        self.clip_depth = self.clip_depth.saturating_sub(1);
    }
}

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// Box hit test context with position and result collection.
pub struct BoxHitTestContext<'ctx> {
    position: Offset,
    results: &'ctx mut Vec<Box<dyn HitTestTarget>>,
}

impl<'ctx> BoxHitTestContext<'ctx> {
    /// Create new hit test context.
    pub fn new(position: Offset, results: &'ctx mut Vec<Box<dyn HitTestTarget>>) -> Self {
        Self { position, results }
    }
}

impl<'ctx> HitTestContext<'ctx, BoxProtocol> for BoxHitTestContext<'ctx> {
    fn position(&self) -> Offset {
        self.position
    }

    fn add_hit(&mut self, target: impl HitTestTarget + 'static) {
        self.results.push(Box::new(target));
    }

    fn is_hit(&self, bounds: Rect) -> bool {
        bounds.contains(self.position)
    }
}

// ============================================================================
// CONSTRAINTS HASH IMPL
// ============================================================================

impl Hash for BoxConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash as bits to avoid float comparison issues
        self.min_width.to_bits().hash(state);
        self.max_width.to_bits().hash(state);
        self.min_height.to_bits().hash(state);
        self.max_height.to_bits().hash(state);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_basics() {
        assert_eq!(BoxProtocol::name(), "box");
        assert_eq!(BoxProtocol::default_geometry(), Size::ZERO);
        assert_eq!(BoxProtocol::protocol_id().get(), 1);
    }

    #[test]
    fn test_constraint_validation() {
        let valid = BoxConstraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        };
        assert!(BoxProtocol::validate_constraints(&valid));

        let invalid = BoxConstraints {
            min_width: 100.0,
            max_width: 50.0, // max < min
            min_height: 0.0,
            max_height: 100.0,
        };
        assert!(!BoxProtocol::validate_constraints(&invalid));
    }

    #[test]
    fn test_constraint_normalization() {
        let c = BoxConstraints {
            min_width: 10.123456,
            max_width: 100.987654,
            min_height: 20.555555,
            max_height: 200.444444,
        };

        let normalized = BoxProtocol::normalize_constraints(c);

        assert_eq!(normalized.min_width, 10.12);
        assert_eq!(normalized.max_width, 100.99);
        assert_eq!(normalized.min_height, 20.56);
        assert_eq!(normalized.max_height, 200.44);
    }

    #[test]
    fn test_layout_context() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut ctx = BoxLayoutContext::new(constraints);

        assert_eq!(ctx.constraints().max_width, 100.0);
        assert!(!ctx.is_complete());
        assert!(ctx.is_tight());

        ctx.complete_layout(Size::new(100.0, 100.0));
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_context_helpers() {
        let c = BoxConstraints {
            min_width: 50.0,
            max_width: 200.0,
            min_height: 50.0,
            max_height: 200.0,
        };
        let ctx = BoxLayoutContext::new(c);

        assert_eq!(ctx.biggest_size(), Size::new(200.0, 200.0));
        assert_eq!(ctx.smallest_size(), Size::new(50.0, 50.0));

        let constrained = ctx.constrain_size(Size::new(300.0, 300.0));
        assert_eq!(constrained, Size::new(200.0, 200.0));
    }
}
