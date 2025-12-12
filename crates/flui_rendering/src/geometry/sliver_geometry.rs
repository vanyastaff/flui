//! Sliver geometry for scrollable content layout output

use std::fmt;

/// Layout output for sliver protocol
///
/// SliverGeometry describes the dimensions and positioning of a sliver after layout.
/// Unlike `Size` which has fixed dimensions, SliverGeometry describes extents along
/// a scroll axis and includes information about visibility, overflow, and corrections.
///
/// # Key Fields
///
/// ## Extents
/// - **scroll_extent**: Total scrollable size of this sliver
/// - **paint_extent**: Amount currently visible in viewport
/// - **max_paint_extent**: Maximum possible paint extent
///
/// ## Optional Fields
/// - **layout_extent**: Space occupied in viewport (defaults to paint_extent)
/// - **hit_test_extent**: Interaction area (defaults to paint_extent)
/// - **cache_extent**: Cached rendering area beyond viewport
///
/// # Examples
///
/// ```ignore
/// // Fully visible sliver (100px tall list item)
/// let geometry = SliverGeometry {
///     scroll_extent: 100.0,
///     paint_extent: 100.0,
///     max_paint_extent: 100.0,
///     ..Default::default()
/// };
///
/// // Partially visible sliver (scrolled halfway off screen)
/// let geometry = SliverGeometry {
///     scroll_extent: 100.0,
///     paint_extent: 50.0,      // Only 50px visible
///     max_paint_extent: 100.0,
///     visible: Some(true),
///     ..Default::default()
/// };
///
/// // Sliver with scroll correction (snapping behavior)
/// let geometry = SliverGeometry {
///     scroll_extent: 100.0,
///     paint_extent: 100.0,
///     max_paint_extent: 100.0,
///     scroll_offset_correction: Some(10.0),  // Snap by 10px
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SliverGeometry {
    /// Total extent this sliver would occupy if fully scrolled through
    ///
    /// This is the conceptual "height" or "width" of the sliver along the scroll axis.
    /// For a list of 10 items at 50px each, this would be 500px.
    pub scroll_extent: f32,

    /// Extent currently painted in the viewport
    ///
    /// This is clamped by the remaining viewport space. A 100px sliver with only
    /// 50px of viewport remaining would have paint_extent = 50px.
    pub paint_extent: f32,

    /// Offset from expected paint position (usually 0.0)
    ///
    /// Used when a sliver needs to paint before its natural position (e.g., a
    /// floating header that has scrolled up but is still visible).
    pub paint_origin: f32,

    /// Extent that occupies space in the viewport for layout purposes
    ///
    /// If None, defaults to paint_extent. Can be different for effects like
    /// floating headers (paint_extent > 0 but layout_extent = 0).
    pub layout_extent: Option<f32>,

    /// Maximum paint extent this sliver could ever have
    ///
    /// This is usually equal to scroll_extent, but can be different for
    /// infinitely scrollable content or dynamic slivers.
    pub max_paint_extent: f32,

    /// Maximum scroll obstruction extent
    ///
    /// Used for floating headers that can obstruct subsequent slivers.
    pub max_scroll_obstruction_extent: f32,

    /// Extent for hit testing
    ///
    /// If None, defaults to paint_extent. Can be larger or smaller than
    /// paint_extent for custom interaction areas.
    pub hit_test_extent: Option<f32>,

    /// Whether this sliver is visible in the viewport
    ///
    /// If None, automatically determined from paint_extent > 0.
    /// Can be explicitly set to false to skip painting even if in viewport.
    pub visible: Option<bool>,

    /// Whether this sliver paints beyond its paint_extent
    ///
    /// Set to true when clipping or overflow handling is needed.
    pub has_visual_overflow: bool,

    /// Requested scroll offset correction
    ///
    /// When set, the viewport will adjust the scroll position by this amount.
    /// Used for snapping, pinning, and other scroll behaviors.
    pub scroll_offset_correction: Option<f32>,

    /// Cache extent beyond the visible viewport
    ///
    /// If None, no caching. Otherwise specifies how much to render beyond
    /// the viewport for smoother scrolling.
    pub cache_extent: Option<f32>,
}

impl SliverGeometry {
    /// Creates zero geometry (invisible sliver)
    pub fn zero() -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            paint_origin: 0.0,
            layout_extent: None,
            max_paint_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            hit_test_extent: None,
            visible: None,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent: None,
        }
    }

    /// Returns the actual layout extent (resolving Option)
    pub fn layout_extent(&self) -> f32 {
        self.layout_extent.unwrap_or(self.paint_extent)
    }

    /// Returns the actual hit test extent (resolving Option)
    pub fn hit_test_extent(&self) -> f32 {
        self.hit_test_extent.unwrap_or(self.paint_extent)
    }

    /// Returns whether this sliver is visible
    pub fn is_visible(&self) -> bool {
        self.visible.unwrap_or(self.paint_extent > 0.0)
    }

    /// Returns whether this sliver has any scroll extent
    pub fn is_empty(&self) -> bool {
        self.scroll_extent == 0.0
    }

    /// Creates geometry for a fully visible sliver
    pub fn fully_visible(extent: f32) -> Self {
        Self {
            scroll_extent: extent,
            paint_extent: extent,
            paint_origin: 0.0,
            layout_extent: None,
            max_paint_extent: extent,
            max_scroll_obstruction_extent: 0.0,
            hit_test_extent: None,
            visible: Some(true),
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent: None,
        }
    }

    /// Creates geometry for a partially visible sliver
    pub fn partially_visible(scroll_extent: f32, paint_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: None,
            max_paint_extent: scroll_extent,
            max_scroll_obstruction_extent: 0.0,
            hit_test_extent: None,
            visible: Some(paint_extent > 0.0),
            has_visual_overflow: scroll_extent > paint_extent,
            scroll_offset_correction: None,
            cache_extent: None,
        }
    }

    /// Creates geometry with a scroll offset correction
    pub fn with_correction(extent: f32, correction: f32) -> Self {
        Self {
            scroll_extent: extent,
            paint_extent: extent,
            paint_origin: 0.0,
            layout_extent: None,
            max_paint_extent: extent,
            max_scroll_obstruction_extent: 0.0,
            hit_test_extent: None,
            visible: Some(true),
            has_visual_overflow: false,
            scroll_offset_correction: Some(correction),
            cache_extent: None,
        }
    }
}

impl Default for SliverGeometry {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for SliverGeometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SliverGeometry(scroll: {:.1}, paint: {:.1}, max: {:.1}",
            self.scroll_extent, self.paint_extent, self.max_paint_extent
        )?;

        if let Some(layout) = self.layout_extent {
            write!(f, ", layout: {:.1}", layout)?;
        }

        if self.has_visual_overflow {
            write!(f, ", overflow")?;
        }

        if let Some(correction) = self.scroll_offset_correction {
            write!(f, ", correction: {:.1}", correction)?;
        }

        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero() {
        let geo = SliverGeometry::zero();
        assert_eq!(geo.scroll_extent, 0.0);
        assert_eq!(geo.paint_extent, 0.0);
        assert!(geo.is_empty());
        assert!(!geo.is_visible());
    }

    #[test]
    fn test_fully_visible() {
        let geo = SliverGeometry::fully_visible(100.0);
        assert_eq!(geo.scroll_extent, 100.0);
        assert_eq!(geo.paint_extent, 100.0);
        assert_eq!(geo.max_paint_extent, 100.0);
        assert!(geo.is_visible());
        assert!(!geo.has_visual_overflow);
    }

    #[test]
    fn test_partially_visible() {
        let geo = SliverGeometry::partially_visible(100.0, 50.0);
        assert_eq!(geo.scroll_extent, 100.0);
        assert_eq!(geo.paint_extent, 50.0);
        assert!(geo.is_visible());
        assert!(geo.has_visual_overflow);
    }

    #[test]
    fn test_with_correction() {
        let geo = SliverGeometry::with_correction(100.0, 10.0);
        assert_eq!(geo.scroll_extent, 100.0);
        assert_eq!(geo.scroll_offset_correction, Some(10.0));
    }

    #[test]
    fn test_layout_extent() {
        let geo = SliverGeometry {
            paint_extent: 50.0,
            layout_extent: Some(30.0),
            ..Default::default()
        };
        assert_eq!(geo.layout_extent(), 30.0);

        let geo = SliverGeometry {
            paint_extent: 50.0,
            layout_extent: None,
            ..Default::default()
        };
        assert_eq!(geo.layout_extent(), 50.0);
    }
}
