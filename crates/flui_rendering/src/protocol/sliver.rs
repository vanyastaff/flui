//! SliverProtocol - Scrollable content layout with viewport awareness.
//!
//! Enhanced sliver protocol with viewport-aware constraints, scroll correction,
//! cache extent management, and tight integration with flui-elite optimizations.

use flui_types::prelude::{Axis, AxisDirection};
use flui_types::{Offset, Rect};
use std::hash::{Hash, Hasher};

use super::base::{
    Canvas, HitTestContext, HitTestTarget, LayoutContext, PaintContext, Protocol, ProtocolId,
    Sealed,
};
use crate::constraints::{SliverConstraints, SliverGeometry};
use crate::parent_data::SliverParentData;

// ============================================================================
// RENDER SLIVER TRAIT (stub for Protocol::Object)
// ============================================================================

/// Trait for sliver render objects (scrollable content).
///
/// This is a stub trait for Protocol::Object type. Full implementation
/// will be added when sliver rendering is implemented.
pub trait RenderSliver: Send + Sync + 'static {
    /// Performs sliver layout with constraints.
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;
}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Scrollable content protocol with viewport-aware constraints and optimization.
///
/// # Features
///
/// - Viewport-aware layout with scroll state tracking
/// - Cache extent management for smooth scrolling
/// - Scroll correction support for dynamic content
/// - Overlap handling for pinned/floating headers
/// - GAT-based contexts with lifetime safety
///
/// # Layout Model
///
/// 1. Parent viewport passes `SliverConstraints` with scroll state
/// 2. Child computes visible portion and consumed space
/// 3. Child returns `SliverGeometry` with extents and visibility
/// 4. Parent composites geometries for complete scrollable view
///
/// # Key Concepts
///
/// - **Scroll Offset**: Distance content has scrolled
/// - **Viewport Extent**: Visible area size
/// - **Remaining Paint Extent**: Space available for this sliver
/// - **Cache Extent**: Extra area to pre-render for performance
/// - **Scroll Correction**: Scroll position adjustment after layout
///
/// # Example
///
/// ```ignore
/// use flui_rendering::protocol::{SliverProtocol, Protocol};
/// use flui_elite::prelude::*;
///
/// let arena: EliteArena<Box<dyn RenderSliver>, RenderId> = EliteArena::new();
/// let constraints = SliverConstraints::default();
/// let is_valid = SliverProtocol::validate_constraints(&constraints);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverProtocol;

impl Sealed for SliverProtocol {}

// ============================================================================
// CORE IMPLEMENTATION
// ============================================================================

impl Protocol for SliverProtocol {
    type Object = dyn RenderSliver;
    type Constraints = SliverConstraints;
    type ParentData = SliverParentData;
    type Geometry = SliverGeometry;

    type LayoutContext<'ctx>
        = SliverLayoutContext<'ctx>
    where
        Self: 'ctx;
    type PaintContext<'ctx>
        = SliverPaintContext<'ctx>
    where
        Self: 'ctx;
    type HitTestContext<'ctx>
        = SliverHitTestContext<'ctx>
    where
        Self: 'ctx;

    fn name() -> &'static str {
        "sliver"
    }

    fn default_geometry() -> SliverGeometry {
        SliverGeometry::zero()
    }

    fn validate_constraints(c: &Self::Constraints) -> bool {
        // All extents must be non-negative
        c.viewport_main_axis_extent >= 0.0
            && c.remaining_paint_extent >= 0.0
            && c.cross_axis_extent >= 0.0
            && c.remaining_cache_extent >= 0.0
            && c.scroll_offset >= 0.0
            // Overlap can be negative (pushes content down) but shouldn't exceed viewport
            && c.overlap.abs() <= c.viewport_main_axis_extent
    }

    fn normalize_constraints(mut c: Self::Constraints) -> Self::Constraints {
        // Round to 2 decimal places for consistent cache keys
        let round = |v: f32| (v * 100.0).round() / 100.0;

        c.scroll_offset = round(c.scroll_offset);
        c.preceding_scroll_extent = round(c.preceding_scroll_extent);
        c.overlap = round(c.overlap);
        c.remaining_paint_extent = round(c.remaining_paint_extent);
        c.cross_axis_extent = round(c.cross_axis_extent);
        c.viewport_main_axis_extent = round(c.viewport_main_axis_extent);
        c.remaining_cache_extent = round(c.remaining_cache_extent);
        c.cache_origin = round(c.cache_origin);

        c
    }
}

impl SliverProtocol {
    /// Get protocol ID (sliver = 2).
    pub const fn protocol_id() -> ProtocolId {
        ProtocolId::new(2)
    }
}

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// Sliver layout context with viewport information and child access.
pub struct SliverLayoutContext<'ctx> {
    constraints: SliverConstraints,
    is_complete: bool,
    geometry: Option<SliverGeometry>,
    children: Option<&'ctx mut dyn SliverChildAccessor>,
}

impl<'ctx> SliverLayoutContext<'ctx> {
    /// Create new context.
    pub fn new(constraints: SliverConstraints) -> Self {
        Self::with_children_optional(constraints, None)
    }

    /// Create context with child accessor.
    pub fn with_children(
        constraints: SliverConstraints,
        children: &'ctx mut dyn SliverChildAccessor,
    ) -> Self {
        Self::with_children_optional(constraints, Some(children))
    }

    /// Internal: unified constructor.
    fn with_children_optional(
        constraints: SliverConstraints,
        children: Option<&'ctx mut dyn SliverChildAccessor>,
    ) -> Self {
        Self {
            constraints,
            is_complete: false,
            geometry: None,
            children,
        }
    }

    /// Get scrolling axis.
    #[inline]
    pub fn axis(&self) -> Axis {
        self.constraints.axis_direction.axis()
    }

    /// Check if scrolling in reverse direction.
    #[inline]
    pub fn is_reverse(&self) -> bool {
        self.constraints.axis_direction.is_reverse()
    }

    /// Get viewport main axis extent.
    #[inline]
    pub fn viewport_extent(&self) -> f32 {
        self.constraints.viewport_main_axis_extent
    }

    /// Get remaining paint extent.
    #[inline]
    pub fn remaining_paint_extent(&self) -> f32 {
        self.constraints.remaining_paint_extent
    }

    /// Get remaining cache extent.
    #[inline]
    pub fn remaining_cache_extent(&self) -> f32 {
        self.constraints.remaining_cache_extent
    }

    /// Calculate child constraints after consuming space.
    pub fn child_constraints(
        &self,
        consumed_scroll_extent: f32,
        consumed_paint_extent: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            axis_direction: self.constraints.axis_direction,
            growth_direction: self.constraints.growth_direction,
            user_scroll_direction: self.constraints.user_scroll_direction,
            scroll_offset: (self.constraints.scroll_offset - consumed_scroll_extent).max(0.0),
            preceding_scroll_extent: self.constraints.preceding_scroll_extent
                + consumed_scroll_extent,
            overlap: self.constraints.overlap,
            remaining_paint_extent: (self.constraints.remaining_paint_extent
                - consumed_paint_extent)
                .max(0.0),
            cross_axis_extent: self.constraints.cross_axis_extent,
            cross_axis_direction: self.constraints.cross_axis_direction,
            viewport_main_axis_extent: self.constraints.viewport_main_axis_extent,
            remaining_cache_extent: self.constraints.remaining_cache_extent,
            cache_origin: self.constraints.cache_origin,
        }
    }

    /// Layout child at index with constraints.
    pub fn layout_child(
        &mut self,
        index: usize,
        constraints: SliverConstraints,
    ) -> Option<SliverGeometry> {
        self.children.as_mut()?.layout_child(index, constraints)
    }
}

impl<'ctx> LayoutContext<'ctx, SliverProtocol> for SliverLayoutContext<'ctx> {
    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn complete_layout(&mut self, geometry: SliverGeometry) {
        self.geometry = Some(geometry);
        self.is_complete = true;
    }

    fn child_count(&self) -> usize {
        self.children.as_ref().map_or(0, |c| c.child_count())
    }
}

/// Accessor trait for sliver child layout operations.
pub trait SliverChildAccessor: Send + Sync {
    /// Get number of children.
    fn child_count(&self) -> usize;

    /// Layout child with constraints, returning computed geometry.
    fn layout_child(
        &mut self,
        index: usize,
        constraints: SliverConstraints,
    ) -> Option<SliverGeometry>;

    /// Get child parent data (read-only).
    fn parent_data(&self, index: usize) -> Option<&SliverParentData>;

    /// Get mutable child parent data.
    fn parent_data_mut(&mut self, index: usize) -> Option<&mut SliverParentData>;
}

// ============================================================================
// PAINT CONTEXT
// ============================================================================

/// Sliver paint context with viewport clipping and transform tracking.
pub struct SliverPaintContext<'ctx> {
    canvas: &'ctx mut dyn Canvas,
    viewport_clip: Rect,
    transform_depth: usize,
    clip_depth: usize,
}

impl<'ctx> SliverPaintContext<'ctx> {
    /// Create new paint context with viewport clip.
    pub fn new(canvas: &'ctx mut dyn Canvas, viewport_clip: Rect) -> Self {
        Self {
            canvas,
            viewport_clip,
            transform_depth: 0,
            clip_depth: 0,
        }
    }

    /// Get viewport clip rectangle.
    pub fn viewport_clip(&self) -> Rect {
        self.viewport_clip
    }

    /// Check if rectangle is visible in viewport.
    pub fn is_visible(&self, rect: Rect) -> bool {
        self.viewport_clip.intersects(&rect)
    }
}

impl<'ctx> PaintContext<'ctx, SliverProtocol> for SliverPaintContext<'ctx> {
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

/// Sliver hit test context with scroll offset awareness.
pub struct SliverHitTestContext<'ctx> {
    position: Offset,
    scroll_offset: f32,
    axis_direction: AxisDirection,
    results: &'ctx mut Vec<Box<dyn HitTestTarget>>,
}

impl<'ctx> SliverHitTestContext<'ctx> {
    /// Create new hit test context.
    pub fn new(
        position: Offset,
        scroll_offset: f32,
        axis_direction: AxisDirection,
        results: &'ctx mut Vec<Box<dyn HitTestTarget>>,
    ) -> Self {
        Self {
            position,
            scroll_offset,
            axis_direction,
            results,
        }
    }

    /// Get main axis position adjusted for scroll offset.
    pub fn main_axis_position(&self) -> f32 {
        match self.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                self.position.dy + self.scroll_offset
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                self.position.dx + self.scroll_offset
            }
        }
    }
}

impl<'ctx> HitTestContext<'ctx, SliverProtocol> for SliverHitTestContext<'ctx> {
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

impl Hash for SliverConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash direction enums as integers
        (self.axis_direction as u8).hash(state);
        (self.growth_direction as u8).hash(state);
        (self.user_scroll_direction as u8).hash(state);
        (self.cross_axis_direction as u8).hash(state);

        // Hash extents as bits to avoid float comparison issues
        self.scroll_offset.to_bits().hash(state);
        self.preceding_scroll_extent.to_bits().hash(state);
        self.overlap.to_bits().hash(state);
        self.remaining_paint_extent.to_bits().hash(state);
        self.cross_axis_extent.to_bits().hash(state);
        self.viewport_main_axis_extent.to_bits().hash(state);
        self.remaining_cache_extent.to_bits().hash(state);
        self.cache_origin.to_bits().hash(state);
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Calculate paint extent (visible portion) of sliver.
#[inline]
pub fn calculate_paint_extent(
    scroll_extent: f32,
    scroll_offset: f32,
    remaining_paint_extent: f32,
) -> f32 {
    (scroll_extent - scroll_offset).clamp(0.0, remaining_paint_extent)
}

/// Calculate cache extent for pre-rendering.
pub fn calculate_cache_extent(
    scroll_extent: f32,
    scroll_offset: f32,
    remaining_cache_extent: f32,
    cache_origin: f32,
) -> f32 {
    let start = scroll_offset + cache_origin;
    let end = scroll_offset + remaining_cache_extent;

    if end <= 0.0 || start >= scroll_extent {
        0.0 // Completely outside cache window
    } else {
        (end.min(scroll_extent) - start.max(0.0)).max(0.0)
    }
}

/// Check if sliver is visible given constraints.
#[inline]
pub fn is_visible(scroll_extent: f32, scroll_offset: f32, remaining_paint_extent: f32) -> bool {
    calculate_paint_extent(scroll_extent, scroll_offset, remaining_paint_extent) > 0.0
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::GrowthDirection;
    use crate::view::ScrollDirection;

    fn test_constraints() -> SliverConstraints {
        SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 850.0,
            cache_origin: -250.0,
        }
    }

    #[test]
    fn test_protocol_basics() {
        assert_eq!(SliverProtocol::name(), "sliver");
        assert_eq!(SliverProtocol::protocol_id().get(), 2);

        let geom = SliverProtocol::default_geometry();
        assert_eq!(geom.scroll_extent, 0.0);
        assert!(!geom.visible);
    }

    #[test]
    fn test_constraint_validation() {
        assert!(SliverProtocol::validate_constraints(&test_constraints()));

        let mut invalid = test_constraints();
        invalid.viewport_main_axis_extent = -100.0;
        assert!(!SliverProtocol::validate_constraints(&invalid));
    }

    #[test]
    fn test_constraint_normalization() {
        let mut c = test_constraints();
        c.scroll_offset = 123.456789;
        c.remaining_paint_extent = 456.789012;

        let normalized = SliverProtocol::normalize_constraints(c);

        assert_eq!(normalized.scroll_offset, 123.46);
        assert_eq!(normalized.remaining_paint_extent, 456.79);
    }

    #[test]
    fn test_layout_context() {
        let constraints = test_constraints();
        let mut ctx = SliverLayoutContext::new(constraints);

        assert_eq!(ctx.axis(), Axis::Vertical);
        assert!(!ctx.is_reverse());
        assert_eq!(ctx.viewport_extent(), 600.0);
        assert!(!ctx.is_complete());

        ctx.complete_layout(SliverGeometry::zero());
        assert!(ctx.is_complete());
    }

    #[test]
    fn test_child_constraints() {
        let ctx = SliverLayoutContext::new(test_constraints());
        let child = ctx.child_constraints(100.0, 100.0);

        assert_eq!(child.scroll_offset, 0.0); // max(0.0 - 100.0, 0.0)
        assert_eq!(child.preceding_scroll_extent, 100.0);
        assert_eq!(child.remaining_paint_extent, 500.0); // 600.0 - 100.0
    }

    #[test]
    fn test_calculate_paint_extent() {
        assert_eq!(calculate_paint_extent(200.0, 0.0, 600.0), 200.0); // Fully visible
        assert_eq!(calculate_paint_extent(200.0, 100.0, 600.0), 100.0); // Partially scrolled
        assert_eq!(calculate_paint_extent(200.0, 250.0, 600.0), 0.0); // Scrolled out
        assert_eq!(calculate_paint_extent(1000.0, 0.0, 600.0), 600.0); // Clamped
    }

    #[test]
    fn test_is_visible() {
        assert!(is_visible(200.0, 0.0, 600.0)); // Fully visible
        assert!(is_visible(200.0, 100.0, 600.0)); // Partially visible
        assert!(!is_visible(200.0, 250.0, 600.0)); // Scrolled out
    }
}
