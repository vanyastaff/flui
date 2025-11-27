//! RenderSliverIgnorePointer - Ignores pointer events for sliver content

use crate::core::{HitTestContext, HitTestResult, HitTestTree, RenderSliverProxy, Single, SliverProtocol};

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

impl RenderSliverProxy for RenderSliverIgnorePointer {
    // Layout: use default proxy (passes constraints through)
    // Paint: use default proxy (passes painting through)

    // Hit test: custom implementation to ignore pointer events
    fn proxy_hit_test<T>(
        &self,
        _ctx: &HitTestContext<'_, T, Single, SliverProtocol>,
        _result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // When ignoring, don't forward hit test to child
        // Return false to indicate hit test should not continue
        !self.ignoring
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

}
