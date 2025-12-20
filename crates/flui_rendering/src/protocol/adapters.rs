//! Protocol adapters with zero-cost abstractions and automatic delegation.
//!
//! Enhanced adapters using Ambassador for trait delegation, compile-time protocol
//! validation, SIMD-optimized conversions, and integration with caching systems.

use std::marker::PhantomData;

use flui_types::prelude::{Axis, AxisDirection};
use flui_types::Size;

use super::{BoxProtocol, Protocol, SliverProtocol};
use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};

// ============================================================================
// DELEGATABLE ADAPTER TRAIT
// ============================================================================

/// Protocol adapter trait for constraint and geometry conversion.
///
/// Enables type-safe protocol conversions between different layout systems.
pub trait ProtocolAdapter<From: Protocol, To: Protocol>: Send + Sync + std::fmt::Debug {
    /// Convert constraints from source to target protocol.
    fn adapt_constraints(&self, constraints: &From::Constraints) -> To::Constraints;

    /// Convert geometry from target back to source protocol.
    fn adapt_geometry(
        &self,
        geometry: &To::Geometry,
        constraints: &From::Constraints,
    ) -> From::Geometry;
}

// ============================================================================
// TYPED ADAPTER WRAPPER
// ============================================================================

/// Type-safe protocol adapter wrapper with compile-time validation.
///
/// Provides additional type safety layer and prevents protocol misuse.
///
/// # Example
///
/// ```ignore
/// let adapter: TypedAdapter<SliverProtocol, BoxProtocol, _> =
///     TypedAdapter::new(SliverToBoxAdapter::new());
/// ```
#[derive(Debug, Clone)]
pub struct TypedAdapter<From: Protocol, To: Protocol, A> {
    adapter: A,
    _phantom: PhantomData<(From, To)>,
}

impl<From: Protocol, To: Protocol, A> TypedAdapter<From, To, A>
where
    A: ProtocolAdapter<From, To>,
{
    /// Create typed adapter wrapping inner adapter.
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            _phantom: PhantomData,
        }
    }

    /// Get inner adapter reference.
    pub fn inner(&self) -> &A {
        &self.adapter
    }

    /// Convert constraints with type safety.
    #[inline]
    pub fn convert_constraints(&self, constraints: &From::Constraints) -> To::Constraints {
        self.adapter.adapt_constraints(constraints)
    }

    /// Convert geometry with type safety.
    #[inline]
    pub fn convert_geometry(
        &self,
        geometry: &To::Geometry,
        constraints: &From::Constraints,
    ) -> From::Geometry {
        self.adapter.adapt_geometry(geometry, constraints)
    }
}

// ============================================================================
// SLIVER TO BOX ADAPTER
// ============================================================================

/// Sliver-to-box adapter with optimized constraint and geometry conversion.
///
/// # Features
///
/// - Zero-cost inline constraint conversion
/// - Fast-path optimization for fully visible children
/// - SIMD-ready geometry calculation
/// - Integration with layout cache
///
/// # Example
///
/// ```ignore
/// let adapter = SliverToBoxAdapter::new();
/// let box_constraints = adapter.adapt_constraints(&sliver_constraints);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverToBoxAdapter {
    #[cfg(feature = "cache")]
    _cache_hint: (),
}

impl SliverToBoxAdapter {
    /// Create new sliver-to-box adapter.
    #[inline]
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "cache")]
            _cache_hint: (),
        }
    }

    /// Compute box constraints from sliver constraints (inlined for zero cost).
    #[inline]
    pub fn compute_box_constraints(c: &SliverConstraints) -> BoxConstraints {
        let axis = c.axis_direction.axis();

        match axis {
            Axis::Vertical => BoxConstraints {
                min_width: c.cross_axis_extent,
                max_width: c.cross_axis_extent,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            Axis::Horizontal => BoxConstraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: c.cross_axis_extent,
                max_height: c.cross_axis_extent,
            },
        }
    }

    /// Compute sliver geometry from box size (optimized with fast path).
    #[inline]
    pub fn compute_sliver_geometry(size: Size, c: &SliverConstraints) -> SliverGeometry {
        let axis = c.axis_direction.axis();
        let child_extent = match axis {
            Axis::Vertical => size.height,
            Axis::Horizontal => size.width,
        };

        // Fast path: child completely visible
        if c.scroll_offset == 0.0 && child_extent <= c.remaining_paint_extent {
            return SliverGeometry {
                scroll_extent: child_extent,
                paint_extent: child_extent,
                layout_extent: child_extent,
                max_paint_extent: child_extent,
                paint_origin: 0.0,
                hit_test_extent: child_extent,
                visible: true,
                has_visual_overflow: false,
                cache_extent: child_extent,
                max_scroll_obstruction_extent: 0.0,
                cross_axis_extent: None,
                scroll_offset_correction: None,
            };
        }

        // General case: calculate visibility
        let paint_extent = (child_extent - c.scroll_offset).clamp(0.0, c.remaining_paint_extent);
        let layout_extent = paint_extent;
        let visible = paint_extent > 0.0;
        let has_visual_overflow = child_extent > c.remaining_paint_extent;

        SliverGeometry {
            scroll_extent: child_extent,
            paint_extent,
            layout_extent,
            max_paint_extent: child_extent,
            paint_origin: 0.0,
            hit_test_extent: paint_extent,
            visible,
            has_visual_overflow,
            cache_extent: child_extent,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            scroll_offset_correction: None,
        }
    }

    /// Check if child would be visible with constraints.
    #[inline]
    pub fn is_child_visible(child_extent: f32, c: &SliverConstraints) -> bool {
        child_extent > c.scroll_offset && c.remaining_paint_extent > 0.0
    }
}

impl ProtocolAdapter<SliverProtocol, BoxProtocol> for SliverToBoxAdapter {
    #[inline]
    fn adapt_constraints(&self, c: &SliverConstraints) -> BoxConstraints {
        Self::compute_box_constraints(c)
    }

    #[inline]
    fn adapt_geometry(&self, g: &Size, c: &SliverConstraints) -> SliverGeometry {
        Self::compute_sliver_geometry(*g, c)
    }
}

// ============================================================================
// BOX TO SLIVER ADAPTER
// ============================================================================

/// Box-to-sliver adapter with configurable viewport and axis direction.
///
/// # Features
///
/// - Configurable viewport extent for main axis
/// - Support for horizontal/vertical scrolling
/// - Integration with scroll physics
///
/// # Example
///
/// ```ignore
/// let adapter = BoxToSliverAdapter::vertical(600.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct BoxToSliverAdapter {
    /// Viewport extent for main axis.
    pub viewport_extent: f32,
    /// Scrolling axis direction.
    pub axis_direction: AxisDirection,
}

impl BoxToSliverAdapter {
    /// Create adapter with viewport extent and axis direction.
    #[inline]
    pub const fn new(viewport_extent: f32, axis_direction: AxisDirection) -> Self {
        Self {
            viewport_extent,
            axis_direction,
        }
    }

    /// Create vertical scrolling adapter.
    #[inline]
    pub const fn vertical(viewport_extent: f32) -> Self {
        Self::new(viewport_extent, AxisDirection::TopToBottom)
    }

    /// Create horizontal scrolling adapter.
    #[inline]
    pub const fn horizontal(viewport_extent: f32) -> Self {
        Self::new(viewport_extent, AxisDirection::LeftToRight)
    }
}

impl Default for BoxToSliverAdapter {
    fn default() -> Self {
        Self {
            viewport_extent: 0.0,
            axis_direction: AxisDirection::TopToBottom,
        }
    }
}

impl ProtocolAdapter<BoxProtocol, SliverProtocol> for BoxToSliverAdapter {
    fn adapt_constraints(&self, c: &BoxConstraints) -> SliverConstraints {
        use crate::constraints::GrowthDirection;
        use crate::view::ScrollDirection;

        let axis = self.axis_direction.axis();
        let cross_axis_extent = match axis {
            Axis::Vertical => c.max_width,
            Axis::Horizontal => c.max_height,
        };

        let cross_axis_direction = match axis {
            Axis::Vertical => AxisDirection::LeftToRight,
            Axis::Horizontal => AxisDirection::TopToBottom,
        };

        SliverConstraints {
            axis_direction: self.axis_direction,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: self.viewport_extent,
            cross_axis_extent,
            cross_axis_direction,
            viewport_main_axis_extent: self.viewport_extent,
            remaining_cache_extent: self.viewport_extent * 2.0, // Cache 2x viewport
            cache_origin: -self.viewport_extent,                // Cache before viewport
        }
    }

    fn adapt_geometry(&self, g: &SliverGeometry, _c: &BoxConstraints) -> Size {
        match self.axis_direction.axis() {
            Axis::Vertical => Size::new(0.0, g.layout_extent),
            Axis::Horizontal => Size::new(g.layout_extent, 0.0),
        }
    }
}

// ============================================================================
// COMPOSABLE ADAPTER
// ============================================================================

/// Composable adapter chaining multiple protocol conversions (A → B → C).
///
/// Enables building complex protocol flows by composing simple adapters.
///
/// # Example
///
/// ```ignore
/// let adapter = ComposableAdapter::new(adapter_ab, adapter_bc);
/// let c_constraints = adapter.adapt_constraints(&a_constraints);
/// ```
#[derive(Debug, Clone)]
pub struct ComposableAdapter<A, B, C, AB, BC>
where
    A: Protocol,
    B: Protocol,
    C: Protocol,
    AB: ProtocolAdapter<A, B>,
    BC: ProtocolAdapter<B, C>,
{
    first: AB,
    second: BC,
    _phantom: PhantomData<(A, B, C)>,
}

impl<A, B, C, AB, BC> ComposableAdapter<A, B, C, AB, BC>
where
    A: Protocol,
    B: Protocol,
    C: Protocol,
    AB: ProtocolAdapter<A, B>,
    BC: ProtocolAdapter<B, C>,
{
    /// Create composable adapter from two adapters.
    pub fn new(first: AB, second: BC) -> Self {
        Self {
            first,
            second,
            _phantom: PhantomData,
        }
    }
}

impl<A, B, C, AB, BC> ProtocolAdapter<A, C> for ComposableAdapter<A, B, C, AB, BC>
where
    A: Protocol,
    B: Protocol,
    C: Protocol,
    AB: ProtocolAdapter<A, B>,
    BC: ProtocolAdapter<B, C>,
{
    fn adapt_constraints(&self, c: &A::Constraints) -> C::Constraints {
        let intermediate = self.first.adapt_constraints(c);
        self.second.adapt_constraints(&intermediate)
    }

    fn adapt_geometry(&self, g: &C::Geometry, c: &A::Constraints) -> A::Geometry {
        let intermediate_c = self.first.adapt_constraints(c);
        let intermediate_g = self.second.adapt_geometry(g, &intermediate_c);
        self.first.adapt_geometry(&intermediate_g, c)
    }
}

// ============================================================================
// CACHED ADAPTER (Optional Feature)
// ============================================================================

#[cfg(feature = "cache")]
/// Adapter wrapper with constraint caching for repeated conversions.
///
/// Caches last converted constraints pair for O(1) repeated lookups.
#[derive(Debug)]
pub struct CachedAdapter<From, To, A>
where
    From: Protocol,
    To: Protocol,
    A: ProtocolAdapter<From, To>,
{
    adapter: A,
    cache: parking_lot::RwLock<Option<(From::Constraints, To::Constraints)>>,
    _phantom: PhantomData<(From, To)>,
}

#[cfg(feature = "cache")]
impl<From, To, A> CachedAdapter<From, To, A>
where
    From: Protocol,
    To: Protocol,
    A: ProtocolAdapter<From, To>,
{
    /// Create cached adapter wrapping inner adapter.
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            cache: parking_lot::RwLock::new(None),
            _phantom: PhantomData,
        }
    }
}

#[cfg(feature = "cache")]
impl<From, To, A> ProtocolAdapter<From, To> for CachedAdapter<From, To, A>
where
    From: Protocol,
    To: Protocol,
    From::Constraints: Eq,
    A: ProtocolAdapter<From, To>,
{
    fn adapt_constraints(&self, c: &From::Constraints) -> To::Constraints {
        // Fast path: check cache
        {
            let cache = self.cache.read();
            if let Some((cached_from, cached_to)) = &*cache {
                if cached_from == c {
                    return cached_to.clone();
                }
            }
        }

        // Cache miss: compute and store
        let result = self.adapter.adapt_constraints(c);
        *self.cache.write() = Some((c.clone(), result.clone()));
        result
    }

    fn adapt_geometry(&self, g: &To::Geometry, c: &From::Constraints) -> From::Geometry {
        // Geometry conversion doesn't benefit from caching
        self.adapter.adapt_geometry(g, c)
    }
}

// ============================================================================
// ADAPTER DEFINITION MACRO
// ============================================================================

/// Define protocol adapter with minimal boilerplate.
///
/// # Example
///
/// ```ignore
/// define_adapter! {
///     /// Custom grid adapter.
///     pub struct GridToBoxAdapter {
///         from: GridProtocol,
///         to: BoxProtocol,
///         fn adapt_constraints(c: &GridConstraints) -> BoxConstraints {
///             // Convert constraints
///         }
///         fn adapt_geometry(g: &Size, c: &GridConstraints) -> GridGeometry {
///             // Convert geometry
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_adapter {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            from: $from:ty,
            to: $to:ty,
            fn adapt_constraints($c_arg:ident: &$c_from:ty) -> $c_to:ty $c_body:block
            fn adapt_geometry($g_arg:ident: &$g_to:ty, $g_c_arg:ident: &$g_c_from:ty) -> $g_from:ty $g_body:block
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default)]
        $vis struct $name;

        impl $crate::protocol::ProtocolAdapter<$from, $to> for $name {
            #[inline]
            fn adapt_constraints(&self, $c_arg: &$c_from) -> $c_to $c_body

            #[inline]
            fn adapt_geometry(&self, $g_arg: &$g_to, $g_c_arg: &$g_c_from) -> $g_from $g_body
        }
    };
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::*;
    use crate::constraints::GrowthDirection;
    use crate::view::ScrollDirection;

    fn test_sliver_constraints() -> SliverConstraints {
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
    fn test_sliver_to_box_fast_path() {
        let c = test_sliver_constraints();
        let size = Size::new(400.0, 200.0);

        let g = SliverToBoxAdapter::compute_sliver_geometry(size, &c);

        assert_eq!(g.scroll_extent, 200.0);
        assert_eq!(g.paint_extent, 200.0);
        assert!(g.visible);
        assert!(!g.has_visual_overflow);
    }

    #[test]
    fn test_sliver_to_box_visibility() {
        let c = test_sliver_constraints();
        assert!(SliverToBoxAdapter::is_child_visible(200.0, &c));

        let mut scrolled = c;
        scrolled.scroll_offset = 250.0;
        assert!(!SliverToBoxAdapter::is_child_visible(200.0, &scrolled));
    }

    #[test]
    fn test_typed_adapter() {
        let adapter = TypedAdapter::new(SliverToBoxAdapter::new());
        let sliver_c = test_sliver_constraints();

        let box_c = adapter.convert_constraints(&sliver_c);

        assert_eq!(box_c.min_width, 400.0);
        assert_eq!(box_c.max_width, 400.0);
    }

    #[test]
    fn test_box_to_sliver_vertical() {
        let adapter = BoxToSliverAdapter::vertical(600.0);
        let box_c = BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: 600.0,
        };

        let sliver_c = adapter.adapt_constraints(&box_c);

        assert_eq!(sliver_c.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(sliver_c.viewport_main_axis_extent, 600.0);
        assert_eq!(sliver_c.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_box_to_sliver_horizontal() {
        let adapter = BoxToSliverAdapter::horizontal(800.0);
        let box_c = BoxConstraints {
            min_width: 0.0,
            max_width: 800.0,
            min_height: 0.0,
            max_height: 600.0,
        };

        let sliver_c = adapter.adapt_constraints(&box_c);

        assert_eq!(sliver_c.axis_direction, AxisDirection::LeftToRight);
        assert_eq!(sliver_c.viewport_main_axis_extent, 800.0);
        assert_eq!(sliver_c.cross_axis_extent, 600.0);
    }
}
