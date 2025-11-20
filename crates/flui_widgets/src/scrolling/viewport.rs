//! Viewport - Visual container for sliver children
//!
//! This widget creates a RenderViewport and manages sliver layout.

use flui_core::view::{BuildContext, IntoElement, View};
use flui_rendering::objects::{ClipBehavior, RenderViewport};
use flui_types::layout::AxisDirection;

/// Viewport widget for displaying sliver children
///
/// The Viewport is responsible for:
/// - Creating RenderViewport (render layer)
/// - Converting scroll offset to sliver constraints
/// - Managing sliver children layout
/// - Clipping content to viewport bounds
///
/// # Architecture
///
/// ```text
/// Viewport (Widget)
///   └── RenderViewport (RenderObject)
///       ├── SliverList (child)
///       ├── SliverGrid (child)
///       └── SliverAppBar (child)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::scrolling::Viewport;
/// use flui_types::layout::AxisDirection;
///
/// Viewport::new()
///     .axis_direction(AxisDirection::TopToBottom)
///     .scroll_offset(100.0)
///     .slivers(vec![
///         Box::new(SliverList::new()),
///         Box::new(SliverGrid::new()),
///     ])
/// ```
#[derive(Clone)]
pub struct Viewport {
    /// Scroll axis direction
    pub axis_direction: AxisDirection,

    /// Current scroll offset
    pub scroll_offset: f32,

    /// Cache extent for off-screen rendering
    pub cache_extent: f32,

    /// Clipping behavior
    pub clip_behavior: ClipBehavior,

    /// Sliver children
    pub slivers: Vec<Box<dyn >>,
}

impl std::fmt::Debug for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewport")
            .field("axis_direction", &self.axis_direction)
            .field("scroll_offset", &self.scroll_offset)
            .field("cache_extent", &self.cache_extent)
            .field("clip_behavior", &self.clip_behavior)
            .field("slivers", &format!("[{} slivers]", self.slivers.len()))
            .finish()
    }
}

impl Viewport {
    /// Create new viewport
    pub fn new() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            scroll_offset: 0.0,
            cache_extent: 250.0,
            clip_behavior: ClipBehavior::HardEdge,
            slivers: Vec::new(),
        }
    }

    /// Set axis direction
    pub fn axis_direction(mut self, direction: AxisDirection) -> Self {
        self.axis_direction = direction;
        self
    }

    /// Set scroll offset
    pub fn scroll_offset(mut self, offset: f32) -> Self {
        self.scroll_offset = offset;
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

    /// Set sliver children
    pub fn slivers(mut self, slivers: Vec<Box<dyn >>) -> Self {
        self.slivers = slivers;
        self
    }

    /// Add a single sliver child
    pub fn add_sliver(mut self, sliver: impl View + 'static) -> Self {
        self.slivers.push(Box::new(sliver));
        self
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

impl View for Viewport {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        // Calculate viewport main axis extent from scroll offset
        // In a real implementation, this would come from parent constraints
        let viewport_main_axis_extent = 600.0; // Placeholder

        // Create RenderViewport
        let mut render_viewport = RenderViewport::new(
            self.axis_direction,
            viewport_main_axis_extent,
            self.scroll_offset,
        );

        // Configure viewport
        render_viewport.set_cache_extent(self.cache_extent);
        render_viewport.set_clip_behavior(self.clip_behavior);

        // Return render object with sliver children
        (render_viewport, self.slivers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_viewport_new() {
        let viewport = Viewport::new();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.cache_extent, 250.0);
        assert_eq!(viewport.clip_behavior, ClipBehavior::HardEdge);
        assert!(viewport.slivers.is_empty());
    }

    #[test]
    fn test_viewport_builder() {
        let viewport = Viewport::new()
            .axis_direction(AxisDirection::LeftToRight)
            .scroll_offset(100.0)
            .cache_extent(500.0)
            .clip_behavior(ClipBehavior::AntiAlias);

        assert_eq!(viewport.axis_direction, AxisDirection::LeftToRight);
        assert_eq!(viewport.scroll_offset, 100.0);
        assert_eq!(viewport.cache_extent, 500.0);
        assert_eq!(viewport.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_viewport_default() {
        let viewport = Viewport::default();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
    }
}
