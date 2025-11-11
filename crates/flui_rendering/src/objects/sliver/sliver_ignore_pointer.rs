//! RenderSliverIgnorePointer - Ignores pointer events for sliver content

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;

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
    child_size: Size,
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
            child_size: Size::ZERO,
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

impl Render for RenderSliverIgnorePointer {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // IgnorePointer doesn't affect layout, pass through to child
        // In real implementation, child would be laid out here
        self.child_size = Size::new(
            constraints.max_width,
            constraints.max_height,
        );

        self.child_size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // Child is painted normally, hit testing is affected separately
        // TODO: Paint child (visibility unaffected)

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child sliver
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
        assert!(!ignore.ignore_semantics);
    }

    #[test]
    fn test_render_sliver_ignore_pointer_default() {
        let ignore = RenderSliverIgnorePointer::default();

        assert!(ignore.ignoring);
        assert!(!ignore.ignore_semantics);
    }

    #[test]
    fn test_set_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        ignore.set_ignoring(false);

        assert!(!ignore.ignoring);
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
    fn test_blocks_hit_testing_when_ignoring() {
        let ignore = RenderSliverIgnorePointer::new(true);

        assert!(ignore.blocks_hit_testing());
    }

    #[test]
    fn test_does_not_block_hit_testing_when_not_ignoring() {
        let ignore = RenderSliverIgnorePointer::new(false);

        assert!(!ignore.blocks_hit_testing());
    }

    #[test]
    fn test_toggle_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(false);
        assert!(!ignore.blocks_hit_testing());

        ignore.set_ignoring(true);
        assert!(ignore.blocks_hit_testing());

        ignore.set_ignoring(false);
        assert!(!ignore.blocks_hit_testing());
    }

    #[test]
    fn test_semantics_independent_of_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        ignore.set_ignore_semantics(true);

        assert!(ignore.ignoring);
        assert!(ignore.ignore_semantics);

        ignore.set_ignoring(false);
        assert!(!ignore.ignoring);
        assert!(ignore.ignore_semantics); // Still ignoring semantics
    }

    #[test]
    fn test_arity_is_single_child() {
        let ignore = RenderSliverIgnorePointer::new(true);
        assert_eq!(ignore.arity(), Arity::Exact(1));
    }
}
