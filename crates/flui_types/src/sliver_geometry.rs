//! SliverGeometry - Layout output for slivers

/// Describes the amount of space occupied by a sliver
///
/// After layout, each sliver produces a SliverGeometry that describes
/// how much space it consumed and other properties needed by the viewport.
///
/// # Example
///
/// ```rust,ignore
/// use flui_types::SliverGeometry;
///
/// let geometry = SliverGeometry {
///     scroll_extent: 1000.0,
///     paint_extent: 600.0,
///     max_paint_extent: 1000.0,
///     visible: true,
///     has_visual_overflow: false,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverGeometry {
    /// The (estimated) total scrollable extent of this sliver
    ///
    /// This is the total amount of space this sliver would occupy
    /// if it were fully scrolled into view. For a list with 100 items
    /// of 50px each, this would be 5000px.
    pub scroll_extent: f32,

    /// The amount of currently visible visual space
    ///
    /// This is how much space the sliver actually painted.
    /// Must be <= remaining_paint_extent from constraints.
    pub paint_extent: f32,

    /// The distance from the first visible part to the first painted part
    ///
    /// This is non-zero when the sliver has parts scrolled out of view.
    /// For example, if the sliver starts at scroll offset 100, but the
    /// viewport starts at 0, this would be 100.
    pub paint_origin: f32,

    /// The amount that was actually laid out
    ///
    /// This includes both visible and cached (but not visible) parts.
    /// Must be >= paint_extent.
    pub layout_extent: f32,

    /// The maximum paint extent this sliver could have
    ///
    /// For fixed-size slivers, this equals scroll_extent.
    /// For expanding slivers (like fill remaining), this is infinity.
    pub max_paint_extent: f32,

    /// The maximum extent by which this sliver can reduce the scrollable area
    ///
    /// This is used for pinned headers that obstruct the viewport.
    /// For a pinned header with height 56.0, this would be 56.0.
    /// For non-pinned slivers, this is 0.0.
    pub max_scroll_obstruction_extent: f32,

    /// Fraction of the sliver currently visible (0.0 to 1.0)
    ///
    /// Used for effects like fade-in as content scrolls into view.
    pub visible_fraction: f32,

    /// Hit test extent in the cross axis
    ///
    /// Typically equals cross_axis_extent from constraints.
    pub cross_axis_extent: f32,

    /// Cache extent consumed by this sliver
    ///
    /// This is how much of the cache extent this sliver used up.
    pub cache_extent: f32,

    /// Whether this sliver is currently visible
    pub visible: bool,

    /// Whether the sliver has content overflow
    ///
    /// True if the sliver wanted to paint more than paint_extent allowed.
    pub has_visual_overflow: bool,

    /// The extent used for hit testing
    ///
    /// None means use paint_extent for hit testing.
    /// Some(value) specifies a custom hit test extent.
    pub hit_test_extent: Option<f32>,

    /// Scroll offset correction requested by this sliver
    ///
    /// If set, the viewport should adjust the scroll offset by this amount
    /// and re-run layout. Used when content changes during layout.
    pub scroll_offset_correction: Option<f32>,
}

impl SliverGeometry {
    /// Create geometry for a fully scrolled sliver (zero extent)
    pub const fn zero() -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            paint_origin: 0.0,
            layout_extent: 0.0,
            max_paint_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            visible_fraction: 0.0,
            cross_axis_extent: 0.0,
            cache_extent: 0.0,
            visible: false,
            has_visual_overflow: false,
            hit_test_extent: None,
            scroll_offset_correction: None,
        }
    }

    /// Create geometry for a simple visible sliver
    pub fn simple(scroll_extent: f32, paint_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            visible: paint_extent > 0.0,
            visible_fraction: if scroll_extent > 0.0 {
                (paint_extent / scroll_extent).min(1.0)
            } else {
                0.0
            },
            ..Self::zero()
        }
    }

    /// Create geometry for a pinned sliver (e.g., pinned header)
    ///
    /// Pinned slivers obstruct the viewport and reduce scrollable area.
    pub fn pinned(extent: f32) -> Self {
        Self {
            scroll_extent: extent,
            paint_extent: extent,
            layout_extent: extent,
            max_paint_extent: extent,
            max_scroll_obstruction_extent: extent,
            visible: true,
            visible_fraction: 1.0,
            ..Self::zero()
        }
    }

    /// Check if sliver is scrolled off-screen
    #[inline]
    pub fn is_scrolled_off_screen(&self) -> bool {
        !self.visible
    }

    /// Get the trailing scroll offset
    ///
    /// This is the scroll offset at the trailing edge of this sliver.
    #[inline]
    pub fn trailing_scroll_offset(&self, leading_scroll_offset: f32) -> f32 {
        leading_scroll_offset + self.scroll_extent
    }

    /// Get the effective hit test extent
    ///
    /// Returns `hit_test_extent` if set, otherwise `paint_extent`.
    #[inline]
    pub fn effective_hit_test_extent(&self) -> f32 {
        self.hit_test_extent.unwrap_or(self.paint_extent)
    }

    /// Check if geometry is valid
    ///
    /// Returns true if all extents are non-negative and finite.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.scroll_extent >= 0.0
            && self.paint_extent >= 0.0
            && self.layout_extent >= 0.0
            && self.max_paint_extent >= 0.0
            && self.scroll_extent.is_finite()
            && self.paint_extent.is_finite()
            && self.layout_extent.is_finite()
    }

    /// Create a builder for constructing SliverGeometry
    pub fn builder() -> SliverGeometryBuilder {
        SliverGeometryBuilder::new()
    }
}

impl Default for SliverGeometry {
    fn default() -> Self {
        Self::zero()
    }
}

/// Builder for constructing SliverGeometry with a fluent API
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverGeometryBuilder {
    geometry: SliverGeometry,
}

impl SliverGeometryBuilder {
    /// Create a new builder with default values
    pub const fn new() -> Self {
        Self {
            geometry: SliverGeometry::zero(),
        }
    }

    /// Set the scroll extent
    pub const fn scroll_extent(mut self, value: f32) -> Self {
        self.geometry.scroll_extent = value;
        self
    }

    /// Set the paint extent
    pub const fn paint_extent(mut self, value: f32) -> Self {
        self.geometry.paint_extent = value;
        self
    }

    /// Set the paint origin
    pub const fn paint_origin(mut self, value: f32) -> Self {
        self.geometry.paint_origin = value;
        self
    }

    /// Set the layout extent
    pub const fn layout_extent(mut self, value: f32) -> Self {
        self.geometry.layout_extent = value;
        self
    }

    /// Set the max paint extent
    pub const fn max_paint_extent(mut self, value: f32) -> Self {
        self.geometry.max_paint_extent = value;
        self
    }

    /// Set the max scroll obstruction extent
    pub const fn max_scroll_obstruction_extent(mut self, value: f32) -> Self {
        self.geometry.max_scroll_obstruction_extent = value;
        self
    }

    /// Set the cache extent
    pub const fn cache_extent(mut self, value: f32) -> Self {
        self.geometry.cache_extent = value;
        self
    }

    /// Set the visible flag
    pub const fn visible(mut self, value: bool) -> Self {
        self.geometry.visible = value;
        self
    }

    /// Set the has_visual_overflow flag
    pub const fn has_visual_overflow(mut self, value: bool) -> Self {
        self.geometry.has_visual_overflow = value;
        self
    }

    /// Set the hit test extent
    pub const fn hit_test_extent(mut self, value: f32) -> Self {
        self.geometry.hit_test_extent = Some(value);
        self
    }

    /// Set the scroll offset correction
    pub const fn scroll_offset_correction(mut self, value: f32) -> Self {
        self.geometry.scroll_offset_correction = Some(value);
        self
    }

    /// Build the SliverGeometry
    pub fn build(mut self) -> SliverGeometry {
        // Auto-calculate visible_fraction if not explicitly set
        if self.geometry.scroll_extent > 0.0 {
            self.geometry.visible_fraction =
                (self.geometry.paint_extent / self.geometry.scroll_extent).min(1.0);
        }
        // Auto-set visible based on paint_extent
        if self.geometry.paint_extent > 0.0 {
            self.geometry.visible = true;
        }
        self.geometry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero() {
        let geometry = SliverGeometry::zero();

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 0.0);
        assert!(!geometry.visible);
        assert!(!geometry.has_visual_overflow);
    }

    #[test]
    fn test_simple() {
        let geometry = SliverGeometry::simple(1000.0, 600.0);

        assert_eq!(geometry.scroll_extent, 1000.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert_eq!(geometry.layout_extent, 600.0);
        assert_eq!(geometry.max_paint_extent, 1000.0);
        assert!(geometry.visible);
        assert!((geometry.visible_fraction - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_pinned() {
        let geometry = SliverGeometry::pinned(56.0);

        assert_eq!(geometry.scroll_extent, 56.0);
        assert_eq!(geometry.paint_extent, 56.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 56.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_simple_full_visible() {
        let geometry = SliverGeometry::simple(500.0, 500.0);

        assert_eq!(geometry.visible_fraction, 1.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_simple_not_visible() {
        let geometry = SliverGeometry::simple(1000.0, 0.0);

        assert!(!geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.0);
    }

    #[test]
    fn test_is_scrolled_off_screen() {
        let mut geometry = SliverGeometry::default();

        assert!(geometry.is_scrolled_off_screen());

        geometry.visible = true;
        assert!(!geometry.is_scrolled_off_screen());
    }

    #[test]
    fn test_trailing_scroll_offset() {
        let geometry = SliverGeometry {
            scroll_extent: 500.0,
            ..Default::default()
        };

        assert_eq!(geometry.trailing_scroll_offset(100.0), 600.0);
        assert_eq!(geometry.trailing_scroll_offset(0.0), 500.0);
    }

    #[test]
    fn test_effective_hit_test_extent() {
        let mut geometry = SliverGeometry {
            paint_extent: 100.0,
            ..Default::default()
        };
        assert_eq!(geometry.effective_hit_test_extent(), 100.0);

        geometry.hit_test_extent = Some(50.0);
        assert_eq!(geometry.effective_hit_test_extent(), 50.0);
    }

    #[test]
    fn test_is_valid() {
        let geometry = SliverGeometry::simple(100.0, 50.0);
        assert!(geometry.is_valid());

        let invalid = SliverGeometry {
            scroll_extent: -10.0,
            ..Default::default()
        };
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_builder() {
        let geometry = SliverGeometry::builder()
            .scroll_extent(1000.0)
            .paint_extent(600.0)
            .layout_extent(600.0)
            .max_paint_extent(1000.0)
            .build();

        assert_eq!(geometry.scroll_extent, 1000.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert!((geometry.visible_fraction - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_builder_with_obstruction() {
        let geometry = SliverGeometry::builder()
            .scroll_extent(56.0)
            .paint_extent(56.0)
            .max_scroll_obstruction_extent(56.0)
            .build();

        assert_eq!(geometry.max_scroll_obstruction_extent, 56.0);
    }

    #[test]
    fn test_default() {
        let geometry = SliverGeometry::default();

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }
}
