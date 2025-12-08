//! RenderSliverAppBar - Floating and pinned app bar for scrollable content
//!
//! Implements Flutter's SliverAppBar that provides Material Design app bar behavior
//! in scrollable viewports. Supports pinned (sticky), floating (appears on scroll up),
//! and snap (full/hidden only) modes for rich scrolling interactions.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverAppBar` | `RenderSliverPersistentHeader` + `FlexibleSpaceBar` logic |
//! | `expanded_height` | `expandedHeight` property |
//! | `collapsed_height` | `collapsedHeight` / `toolbarHeight` |
//! | `pinned` | `pinned` property |
//! | `floating` | `floating` property |
//! | `snap` | `snap` property |
//! | `stretch` | `stretch` property |
//!
//! # App Bar Behaviors
//!
//! ## Pinned Mode
//! ```text
//! scroll_offset = 0:    [████████████] (200px expanded)
//! scroll_offset = 100:  [██████] (100px shrinking)
//! scroll_offset = 200+: [███] (56px collapsed, STAYS VISIBLE)
//! ```
//!
//! ## Floating Mode (simplified)
//! ```text
//! Scroll down:  [████████████] → [      ] (hides)
//! Scroll up:    [      ] → [████████████] (appears immediately)
//! ```
//!
//! ## Normal Mode
//! ```text
//! scroll_offset = 0:    [████████████] (200px)
//! scroll_offset = 100:  [██████] (100px)
//! scroll_offset = 200+: [      ] (0px, hidden)
//! ```
//!
//! # Layout Protocol
//!
//! 1. **Calculate effective height based on mode**
//!    - Pinned: Always collapsed_height (sticky)
//!    - Floating: Full expanded_height (simplified)
//!    - Normal: expanded_height - scroll_offset (shrinks)
//!
//! 2. **Calculate paint extent**
//!    - Pinned: min(collapsed_height, remaining_extent)
//!    - Normal: min(expanded_height - scroll_offset, remaining_extent)
//!
//! 3. **Calculate scroll extent**
//!    - Pinned: expanded_height - collapsed_height
//!    - Normal: expanded_height
//!
//! 4. **Calculate layout extent**
//!    - Pinned: collapsed_height (affects following slivers)
//!    - Normal: paint_extent (shrinks with scroll)
//!
//! # Paint Protocol
//!
//! 1. **Check visibility**
//!    - Only paint if geometry.visible
//!
//! 2. **Paint child content**
//!    - Child is app bar content (toolbar, title, flexible space)
//!    - Painted at current offset
//!
//! # Performance
//!
//! - **Layout**: O(1) - geometry calculation only
//! - **Paint**: O(1) + child paint - simple visibility check
//! - **Memory**: 32 bytes (heights + flags + geometry cache)
//!
//! # Use Cases
//!
//! - **Material app bars**: Standard Material Design headers
//! - **Collapsing toolbars**: Expand/collapse with scroll
//! - **Search bars**: Appear on scroll up for easy access
//! - **Navigation headers**: Persistent navigation with scroll
//! - **Hero headers**: Large headers that shrink on scroll
//! - **Sticky headers**: Pinned section headers
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPersistentHeader**: AppBar is specialized for toolbars, PersistentHeader is generic
//! - **vs SliverPinnedPersistentHeader**: PinnedHeader is always pinned, AppBar is configurable
//! - **vs SliverFloatingPersistentHeader**: FloatingHeader is always floating, AppBar is configurable
//! - **vs BoxAppBar**: SliverAppBar integrates with viewport, BoxAppBar is static
//!
//! # Implementation Status
//!
//! **IMPORTANT**: Current implementation has issues:
//! 1. **Child is never laid out** - layout() doesn't call layout_child(), so app bar content
//!    is not sized or positioned. This is incomplete.
//! 2. **Floating behavior is simplified** - doesn't track scroll direction or velocity
//! 3. **Line 148**: _effective_height calculated but never used (dead code)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverAppBar;
//!
//! // Pinned app bar (always visible, collapses to 56px)
//! let pinned = RenderSliverAppBar::new(200.0)
//!     .with_pinned(true);
//!
//! // Floating app bar (appears on scroll up)
//! let floating = RenderSliverAppBar::new(150.0)
//!     .with_floating(true);
//!
//! // Pinned + Floating (Material Design standard)
//! let material = RenderSliverAppBar::new(200.0)
//!     .with_pinned(true)
//!     .with_floating(true);
//!
//! // Snap behavior (no partial visibility)
//! let snap = RenderSliverAppBar::new(200.0)
//!     .with_floating(true)
//!     .with_snap(true);
//!
//! // Custom collapsed height
//! let mut app_bar = RenderSliverAppBar::new(250.0);
//! app_bar.set_collapsed_height(80.0);
//! ```

use flui_rendering::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry, BoxConstraints};

/// RenderObject for Material Design app bar with scroll effects.
///
/// Provides configurable scroll behaviors: pinned (sticky), floating (appears on scroll up),
/// snap (full/hidden only), and stretch (overscroll). Supports expanding/collapsing toolbar
/// with smooth transitions between expanded and collapsed states.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child (app bar content).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Scroll-Responsive Header** - Calculates own geometry based on scroll offset
/// and mode flags, doesn't layout child (currently incomplete). Geometry changes
/// affect viewport layout and paint behavior.
///
/// # Behavior Modes
///
/// | Mode | Scroll Down | Scroll Up | Always Visible | Notes |
/// |------|-------------|-----------|----------------|-------|
/// | Normal | Shrinks | Grows | No | Disappears when scrolled past |
/// | Pinned | Shrinks to min | Grows | Yes (min height) | Sticky header |
/// | Floating | Hides | Appears | No | Immediate response |
/// | Snap | Hides | Shows | No | No partial states |
/// | Pinned+Floating | Shrinks to min | Full height | Yes | Material standard |
///
/// # Use Cases
///
/// - **Material app bars**: Standard Android/web app headers
/// - **Collapsing toolbars**: Large headers with title/image
/// - **Search bars**: Floating search that appears on scroll
/// - **Navigation headers**: Sticky navigation with content below
/// - **Hero sections**: Large hero images that collapse
/// - **Section headers**: Pinned headers with rich content
///
/// # Flutter Compliance
///
/// Partially matches Flutter's SliverAppBar:
/// - Calculates geometry based on scroll offset ✅
/// - Supports pinned, floating, snap, stretch flags ✅
/// - Expands/collapses between two heights ✅
/// - **INCOMPLETE**: Doesn't layout child content ❌
/// - **SIMPLIFIED**: Floating doesn't track scroll direction ⚠️
///
/// # Implementation Issues
///
/// **CRITICAL**: Child is never laid out! The layout() method only calculates
/// this sliver's geometry but doesn't call layout_child() to size the app bar
/// content. This means child size is undefined. Full implementation needs to
/// layout child with BoxConstraints derived from current scroll state.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverAppBar;
///
/// // Material Design standard (pinned + floating)
/// let app_bar = RenderSliverAppBar::new(200.0)
///     .with_pinned(true)
///     .with_floating(true)
///     .with_snap(true);
///
/// // Large collapsing header
/// let hero = RenderSliverAppBar::new(300.0);
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

}

impl Default for RenderSliverAppBar {
    fn default() -> Self {
        Self::new(200.0) // Default expanded height
    }
}

impl RenderObject for RenderSliverAppBar {}

impl RenderSliver<Single> for RenderSliverAppBar {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();
        let constraints = ctx.constraints;
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate effective height based on mode
        let effective_height = if self.pinned {
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
        };

        // Layout child with box constraints matching effective height
        let box_constraints = BoxConstraints::new(
            0.0,
            constraints.cross_axis_extent,
            effective_height,
            effective_height,
        );
        ctx.tree_mut().perform_layout(child_id, box_constraints)?;

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

        self.sliver_geometry = SliverGeometry {
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
        };

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();
            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
                *ctx.canvas = child_canvas;
            }
        }
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
