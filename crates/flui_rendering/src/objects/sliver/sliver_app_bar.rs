//! RenderSliverAppBar - Floating and pinned app bar for scrollable content

use crate::core::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for an app bar that can float, pin, or scroll away
///
/// SliverAppBar provides three main behaviors:
/// - **Pinned**: Always visible at the top (like a sticky header)
/// - **Floating**: Appears immediately on scroll up
/// - **Snap**: Snaps completely in/out (no partial visibility)
///
/// # Use Cases
///
/// - Material Design app bars
/// - Collapsing toolbars
/// - Search bars that appear on scroll
/// - Navigation headers
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverAppBar;
///
/// // Pinned app bar (always visible)
/// let pinned = RenderSliverAppBar::new(60.0)
///     .with_pinned(true);
///
/// // Floating app bar (appears on scroll up)
/// let floating = RenderSliverAppBar::new(60.0)
///     .with_floating(true);
/// ```
#[derive(Debug)]
pub struct RenderSliverAppBar {
    /// Expanded height (when not scrolled)
    pub expanded_height: f32,
    /// Collapsed height (minimum height)
    pub collapsed_height: f32,
    /// Whether app bar is pinned (always visible)
    pub pinned: bool,
    /// Whether app bar floats (appears on scroll up)
    pub floating: bool,
    /// Whether app bar snaps (no partial visibility)
    pub snap: bool,
    /// Stretch mode (allows overscroll stretch)
    pub stretch: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverAppBar {
    /// Create new sliver app bar
    ///
    /// # Arguments
    /// * `expanded_height` - Height when fully expanded
    pub fn new(expanded_height: f32) -> Self {
        Self {
            expanded_height,
            collapsed_height: 56.0, // Material Design standard
            pinned: false,
            floating: false,
            snap: false,
            stretch: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set collapsed height
    pub fn set_collapsed_height(&mut self, height: f32) {
        self.collapsed_height = height;
    }

    /// Set pinned behavior
    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    /// Set floating behavior
    pub fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
    }

    /// Set snap behavior
    pub fn set_snap(&mut self, snap: bool) {
        self.snap = snap;
    }

    /// Set stretch behavior
    pub fn set_stretch(&mut self, stretch: bool) {
        self.stretch = stretch;
    }

    /// Create with pinned behavior
    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Create with floating behavior
    pub fn with_floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }

    /// Create with snap behavior
    pub fn with_snap(mut self, snap: bool) -> Self {
        self.snap = snap;
        self
    }

    /// Create with stretch behavior
    pub fn with_stretch(mut self, stretch: bool) -> Self {
        self.stretch = stretch;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate effective height based on scroll offset
    fn calculate_effective_height(&self, scroll_offset: f32) -> f32 {
        if self.pinned {
            // Pinned: Always at collapsed height (minimum)
            self.collapsed_height
        } else if self.floating {
            // Floating: Full height when scrolling up, collapses when scrolling down
            // In real implementation, this depends on scroll direction and velocity
            self.expanded_height
        } else {
            // Normal: Shrinks as user scrolls
            let available = self.expanded_height - scroll_offset;
            available.max(0.0)
        }
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
    ) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let _effective_height = self.calculate_effective_height(scroll_offset);

        // Calculate how much we actually paint
        let paint_extent = if self.pinned {
            // Pinned: Always paint collapsed height
            self.collapsed_height.min(remaining_extent)
        } else {
            // Calculate based on scroll position
            let visible_height = (self.expanded_height - scroll_offset).max(0.0);
            visible_height.min(remaining_extent)
        };

        // Scroll extent is the expanded height (how much scrollable space we consume)
        let scroll_extent = if self.pinned {
            self.expanded_height - self.collapsed_height
        } else {
            self.expanded_height
        };

        // Layout extent is what affects following slivers
        let layout_extent = if self.pinned {
            self.collapsed_height.min(remaining_extent)
        } else {
            paint_extent
        };

        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: self.expanded_height,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if scroll_extent > 0.0 {
                (paint_extent / scroll_extent).min(1.0)
            } else {
                1.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverAppBar {
    fn default() -> Self {
        Self::new(200.0) // Default expanded height
    }
}

impl LegacySliverRender for RenderSliverAppBar {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&ctx.constraints);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Paint child if present and visible
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                return ctx.tree.paint_child(child_id, ctx.offset);
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child (app bar content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_app_bar_new() {
        let app_bar = RenderSliverAppBar::new(200.0);

        assert_eq!(app_bar.expanded_height, 200.0);
        assert_eq!(app_bar.collapsed_height, 56.0);
        assert!(!app_bar.pinned);
        assert!(!app_bar.floating);
        assert!(!app_bar.snap);
        assert!(!app_bar.stretch);
    }

    #[test]
    fn test_render_sliver_app_bar_default() {
        let app_bar = RenderSliverAppBar::default();

        assert_eq!(app_bar.expanded_height, 200.0);
        assert_eq!(app_bar.collapsed_height, 56.0);
    }

    #[test]
    fn test_set_collapsed_height() {
        let mut app_bar = RenderSliverAppBar::new(200.0);
        app_bar.set_collapsed_height(80.0);

        assert_eq!(app_bar.collapsed_height, 80.0);
    }

    #[test]
    fn test_with_pinned() {
        let app_bar = RenderSliverAppBar::new(200.0).with_pinned(true);

        assert!(app_bar.pinned);
    }

    #[test]
    fn test_with_floating() {
        let app_bar = RenderSliverAppBar::new(200.0).with_floating(true);

        assert!(app_bar.floating);
    }

    #[test]
    fn test_with_snap() {
        let app_bar = RenderSliverAppBar::new(200.0).with_snap(true);

        assert!(app_bar.snap);
    }

    #[test]
    fn test_with_stretch() {
        let app_bar = RenderSliverAppBar::new(200.0).with_stretch(true);

        assert!(app_bar.stretch);
    }

    #[test]
    fn test_calculate_effective_height_normal() {
        let app_bar = RenderSliverAppBar::new(200.0);

        // Not scrolled yet
        assert_eq!(app_bar.calculate_effective_height(0.0), 200.0);

        // Scrolled 50px
        assert_eq!(app_bar.calculate_effective_height(50.0), 150.0);

        // Scrolled past app bar
        assert_eq!(app_bar.calculate_effective_height(250.0), 0.0);
    }

    #[test]
    fn test_calculate_effective_height_pinned() {
        let app_bar = RenderSliverAppBar::new(200.0).with_pinned(true);

        // Always collapsed height when pinned
        assert_eq!(app_bar.calculate_effective_height(0.0), 56.0);
        assert_eq!(app_bar.calculate_effective_height(100.0), 56.0);
        assert_eq!(app_bar.calculate_effective_height(500.0), 56.0);
    }

    #[test]
    fn test_calculate_effective_height_floating() {
        let app_bar = RenderSliverAppBar::new(200.0).with_floating(true);

        // Floating shows full height (simplified - real impl depends on scroll direction)
        assert_eq!(app_bar.calculate_effective_height(0.0), 200.0);
        assert_eq!(app_bar.calculate_effective_height(100.0), 200.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let app_bar = RenderSliverAppBar::new(200.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = app_bar.calculate_sliver_geometry(&constraints);

        // Full app bar visible
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let app_bar = RenderSliverAppBar::new(200.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled 100px
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = app_bar.calculate_sliver_geometry(&constraints);

        // Half visible (200 - 100 = 100px)
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 100.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past() {
        let app_bar = RenderSliverAppBar::new(200.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = app_bar.calculate_sliver_geometry(&constraints);

        // Not visible
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_pinned() {
        let app_bar = RenderSliverAppBar::new(200.0).with_pinned(true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = app_bar.calculate_sliver_geometry(&constraints);

        // Still visible at collapsed height when pinned
        assert_eq!(geometry.scroll_extent, 144.0); // 200 - 56
        assert_eq!(geometry.paint_extent, 56.0); // Collapsed height
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let app_bar = RenderSliverAppBar::new(200.0);
        assert_eq!(app_bar.arity(), RuntimeArity::Exact(1));
    }
}
