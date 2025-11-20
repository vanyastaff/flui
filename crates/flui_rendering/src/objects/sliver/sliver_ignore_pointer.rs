//! RenderSliverIgnorePointer - Ignores pointer events for sliver content

use flui_core::render::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::SliverGeometry;

/// RenderObject that makes a sliver ignore pointer events
///
/// This is useful for creating non-interactive overlays, disabled content,
/// or implementing complex hit-testing logic in scrollable containers.
///
/// # Use Cases
///
/// - Disable user interaction during loading states
/// - Create visual-only scroll content (non-interactive backgrounds)
/// - Implement custom hit-testing logic
/// - Temporarily disable sections of a list
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverIgnorePointer;
///
/// // Child sliver will not receive pointer events
/// let ignore_pointer = RenderSliverIgnorePointer::new(true);
/// ```
#[derive(Debug)]
pub struct RenderSliverIgnorePointer {
    /// Whether to ignore pointer events
    pub ignoring: bool,
    /// Whether to ignore semantics (accessibility)
    pub ignore_semantics: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverIgnorePointer {
    /// Create new sliver ignore pointer
    ///
    /// # Arguments
    /// * `ignoring` - Whether to ignore pointer events
    pub fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            ignore_semantics: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set whether to ignore pointer events
    pub fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
    }

    /// Set whether to ignore semantics
    pub fn set_ignore_semantics(&mut self, ignore: bool) {
        self.ignore_semantics = ignore;
    }

    /// Create with semantics ignored
    pub fn with_ignore_semantics(mut self) -> Self {
        self.ignore_semantics = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Check if this sliver should block hit testing
    pub fn blocks_hit_testing(&self) -> bool {
        self.ignoring
    }
}

impl Default for RenderSliverIgnorePointer {
    fn default() -> Self {
        Self::new(true) // Default to ignoring
    }
}

impl LegacySliverRender for RenderSliverIgnorePointer {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        // Pass through to child - IgnorePointer doesn't affect layout
        if let Some(child_id) = ctx.children.try_single() {
            self.sliver_geometry = ctx.tree.layout_sliver_child(child_id, ctx.constraints);
        } else {
            self.sliver_geometry = SliverGeometry::default();
        }

        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Child is painted normally, hit testing is affected separately
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
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_ignore_pointer_new() {
        let ignore = RenderSliverIgnorePointer::new(true);

        assert!(ignore.ignoring);
        assert!(!ignore.ignore_semantics);
    }

    #[test]
    fn test_render_sliver_ignore_pointer_new_not_ignoring() {
        let ignore = RenderSliverIgnorePointer::new(false);

        assert!(!ignore.ignoring);
    }

    #[test]
    fn test_set_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(false);
        ignore.set_ignoring(true);

        assert!(ignore.ignoring);
    }

    #[test]
    fn test_set_ignore_semantics() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        ignore.set_ignore_semantics(true);

        assert!(ignore.ignore_semantics);
    }

    #[test]
    fn test_with_ignore_semantics() {
        let ignore = RenderSliverIgnorePointer::new(true).with_ignore_semantics();

        assert!(ignore.ignoring);
        assert!(ignore.ignore_semantics);
    }

    #[test]
    fn test_blocks_hit_testing() {
        let ignore_true = RenderSliverIgnorePointer::new(true);
        let ignore_false = RenderSliverIgnorePointer::new(false);

        assert!(ignore_true.blocks_hit_testing());
        assert!(!ignore_false.blocks_hit_testing());
    }

    #[test]
    fn test_default_is_ignoring() {
        let ignore = RenderSliverIgnorePointer::default();

        assert!(ignore.ignoring);
    }

    #[test]
    fn test_arity_is_single_child() {
        let ignore = RenderSliverIgnorePointer::new(true);
        assert_eq!(ignore.arity(), RuntimeArity::Exact(1));
    }
}
