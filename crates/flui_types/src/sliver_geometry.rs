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

    /// The maximum amount of scroll extent that could be scrolled
    ///
    /// For most slivers, this is scroll_extent. But for slivers
    /// that can shrink (like SliverFillRemaining), it might be less.
    pub max_scroll_obsolescence: f32,

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

    /// Whether hit testing should be performed
    ///
    /// False for slivers that are completely off-screen or transparent.
    pub hit_test_extent: Option<f32>,

    /// Whether this sliver contributes to the viewport's max scroll extent
    pub scroll_offset_correction: Option<f32>,
}

impl SliverGeometry {
    /// Create geometry for a fully scrolled sliver
    pub fn zero() -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            paint_origin: 0.0,
            layout_extent: 0.0,
            max_paint_extent: 0.0,
            max_scroll_obsolescence: 0.0,
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

    /// Check if sliver is scrolled off-screen
    pub fn is_scrolled_off_screen(&self) -> bool {
        !self.visible
    }

    /// Get the trailing scroll offset
    ///
    /// This is the scroll offset at the trailing edge of this sliver.
    pub fn trailing_scroll_offset(&self, leading_scroll_offset: f32) -> f32 {
        leading_scroll_offset + self.scroll_extent
    }
}

impl Default for SliverGeometry {
    fn default() -> Self {
        Self::zero()
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
    fn test_default() {
        let geometry = SliverGeometry::default();

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }
}
