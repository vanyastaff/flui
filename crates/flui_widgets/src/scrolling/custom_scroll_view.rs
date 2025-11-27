//! CustomScrollView - High-level sliver-based scrolling widget
//!
//! This widget combines Scrollable and Viewport for easy sliver scrolling.

use super::{Scrollable, Viewport};
use crate::layout::ScrollController;
use flui_core::view::children::Children;
use flui_core::view::{BuildContext, IntoElement, StatelessView};
use flui_types::layout::AxisDirection;
use flui_types::painting::ClipBehavior;

/// High-level widget for sliver-based scrolling
///
/// CustomScrollView combines Scrollable (gesture/physics) and Viewport (visual)
/// into a single easy-to-use API. This is the primary way to create scrollable
/// lists, grids, and complex scrolling layouts.
///
/// # Architecture
///
/// ```text
/// CustomScrollView
///   └── Scrollable (gestures + physics)
///       └── Viewport (visual)
///           ├── SliverList
///           ├── SliverGrid
///           └── SliverAppBar
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::scrolling::CustomScrollView;
///
/// CustomScrollView::new()
///     .slivers(vec![
///         Box::new(SliverAppBar::new()
///             .title("My App")
///             .pinned(true)),
///         Box::new(SliverList::new()
///             .children(items)),
///         Box::new(SliverGrid::new()
///             .children(grid_items)),
///     ])
/// ```
pub struct CustomScrollView {
    /// Scroll axis direction
    pub axis_direction: AxisDirection,

    /// Whether to reverse scroll direction
    pub reverse: bool,

    /// Optional scroll controller
    pub controller: Option<ScrollController>,

    /// Cache extent for off-screen rendering
    pub cache_extent: f32,

    /// Clipping behavior
    pub clip_behavior: ClipBehavior,

    /// Whether physics are enabled
    pub physics_enabled: bool,

    /// Sliver children
    pub slivers: Children,
}

impl std::fmt::Debug for CustomScrollView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomScrollView")
            .field("axis_direction", &self.axis_direction)
            .field("reverse", &self.reverse)
            .field("controller", &self.controller)
            .field("cache_extent", &self.cache_extent)
            .field("clip_behavior", &self.clip_behavior)
            .field("physics_enabled", &self.physics_enabled)
            .field("slivers", &format!("[{} slivers]", self.slivers.len()))
            .finish()
    }
}

impl CustomScrollView {
    /// Create new CustomScrollView
    pub fn new() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            reverse: false,
            controller: None,
            cache_extent: 250.0,
            clip_behavior: ClipBehavior::HardEdge,
            physics_enabled: true,
            slivers: Children::new(),
        }
    }

    /// Set axis direction
    pub fn axis_direction(mut self, direction: AxisDirection) -> Self {
        self.axis_direction = direction;
        self
    }

    /// Set whether to reverse scroll direction
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Set scroll controller
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Set cache extent
    pub fn cache_extent(mut self, extent: f32) -> Self {
        self.cache_extent = extent;
        self
    }

    /// Set clip behavior
    pub fn clip_behavior(mut self, behavior: ClipBehavior) -> Self {
        self.clip_behavior = behavior;
        self
    }

    /// Enable or disable physics
    pub fn physics_enabled(mut self, enabled: bool) -> Self {
        self.physics_enabled = enabled;
        self
    }

    /// Set sliver children from a vector
    pub fn slivers_vec(mut self, slivers: Vec<impl IntoElement>) -> Self {
        self.slivers = slivers.into_iter().collect();
        self
    }

    /// Set sliver children
    pub fn slivers(mut self, slivers: Children) -> Self {
        self.slivers = slivers;
        self
    }

    /// Add a single sliver child
    pub fn add_sliver(mut self, sliver: impl IntoElement) -> Self {
        self.slivers.push(sliver);
        self
    }
}

impl Default for CustomScrollView {
    fn default() -> Self {
        Self::new()
    }
}

impl StatelessView for CustomScrollView {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        // Get scroll offset from controller if present
        let scroll_offset = self
            .controller
            .as_ref()
            .map(|c| c.offset())
            .unwrap_or(0.0);

        // Create Viewport with slivers
        let viewport = Viewport::new()
            .axis_direction(self.axis_direction)
            .scroll_offset(scroll_offset)
            .cache_extent(self.cache_extent)
            .clip_behavior(self.clip_behavior)
            .slivers(self.slivers);

        // Wrap in Scrollable for gesture handling
        let mut scrollable = Scrollable::new(viewport)
            .axis_direction(self.axis_direction)
            .reverse(self.reverse)
            .physics_enabled(self.physics_enabled);

        if let Some(controller) = self.controller {
            scrollable = scrollable.controller(controller);
        }

        scrollable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_scroll_view_new() {
        let scroll_view = CustomScrollView::new();

        assert_eq!(scroll_view.axis_direction, AxisDirection::TopToBottom);
        assert!(!scroll_view.reverse);
        assert!(scroll_view.physics_enabled);
        assert_eq!(scroll_view.cache_extent, 250.0);
        assert!(scroll_view.slivers.is_empty());
    }

    #[test]
    fn test_custom_scroll_view_builder() {
        let controller = ScrollController::new();

        let scroll_view = CustomScrollView::new()
            .axis_direction(AxisDirection::LeftToRight)
            .reverse(true)
            .controller(controller.clone())
            .cache_extent(500.0)
            .clip_behavior(ClipBehavior::AntiAlias)
            .physics_enabled(false);

        assert_eq!(scroll_view.axis_direction, AxisDirection::LeftToRight);
        assert!(scroll_view.reverse);
        assert!(!scroll_view.physics_enabled);
        assert_eq!(scroll_view.cache_extent, 500.0);
        assert_eq!(scroll_view.clip_behavior, ClipBehavior::AntiAlias);
        assert!(scroll_view.controller.is_some());
    }

    #[test]
    fn test_custom_scroll_view_default() {
        let scroll_view = CustomScrollView::default();

        assert_eq!(scroll_view.axis_direction, AxisDirection::TopToBottom);
        assert!(!scroll_view.reverse);
    }
}
