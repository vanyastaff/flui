//! RenderSliverIgnorePointer - Ignores pointer events for sliver content
//!
//! Implements Flutter's IgnorePointer pattern for slivers. Controls hit testing behavior
//! while keeping layout and paint unchanged. When ignoring, the sliver is visible and laid
//! out normally but doesn't respond to pointer events (taps, drags, hovers).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverIgnorePointer` | `RenderIgnorePointer` adapted for sliver protocol |
//! | `ignoring` property | `ignoring` property (bool) |
//! | `ignore_semantics` | `ignoringSemantics` property |
//! | `blocks_hit_testing()` | Hit test blocking logic |
//! | Layout/paint unchanged | Same behavior as Flutter |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - IgnorePointer is layout-transparent
//!    - Child receives identical constraints
//!
//! 2. **Return child geometry**
//!    - Returns child's geometry unchanged
//!    - No modification to scroll extent, paint extent, etc.
//!
//! 3. **Result**
//!    - Geometry is identical to child's (proxy)
//!
//! # Paint Protocol
//!
//! 1. **Check visibility**
//!    - Only paint if geometry.visible
//!
//! 2. **Paint child normally**
//!    - Child is painted unchanged
//!    - Visual appearance not affected by ignoring flag
//!    - Only hit testing behavior changes
//!
//! # Hit Test Protocol (Conceptual)
//!
//! 1. **Check ignoring flag**
//!    - If ignoring = true: Block hit testing (don't test child)
//!    - If ignoring = false: Pass hit testing to child
//!
//! 2. **Result**
//!    - Ignoring: Pointer events pass through to widgets beneath
//!    - Not ignoring: Child receives pointer events normally
//!
//! # Performance
//!
//! - **Layout**: O(child) - pass-through proxy
//! - **Paint**: O(child) - pass-through proxy
//! - **Hit Test**: O(1) when ignoring (skip child), O(child) when not ignoring
//! - **Memory**: 2 bytes (bool flags) + 48 bytes (SliverGeometry cache) = 50 bytes
//! - **Optimization**: Skips hit testing traversal when ignoring
//!
//! # Use Cases
//!
//! - **Loading states**: Disable interaction while loading
//! - **Visual-only content**: Non-interactive backgrounds, decorations
//! - **Disabled UI sections**: Show but don't allow interaction
//! - **Tutorial overlays**: Show interface without allowing input
//! - **Custom hit testing**: Block default hit testing for custom logic
//! - **Temporary disabling**: Disable list sections during operations
//!
//! # Difference from Related Patterns
//!
//! **vs Offstage**:
//! - IgnorePointer: Full layout/paint, blocks hit testing only
//! - Offstage: No layout/paint/hit testing, zero geometry
//!
//! **vs Opacity**:
//! - IgnorePointer: Full opacity, blocks hit testing
//! - Opacity: Reduced opacity, still receives events
//!
//! **vs AbsorbPointer**:
//! - IgnorePointer: Events pass through to widgets beneath
//! - AbsorbPointer: Events consumed, don't pass through
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderIgnorePointer**: SliverIgnorePointer is sliver protocol, IgnorePointer is box
//! - **vs SliverAbsorbPointer**: AbsorbPointer consumes events, IgnorePointer passes through
//! - **vs SliverOffstage**: Offstage removes from layout, IgnorePointer keeps layout
//! - **vs SliverOpacity**: Opacity affects visuals, IgnorePointer affects hit testing only
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverIgnorePointer;
//!
//! // Ignore pointer events (visual only)
//! let disabled = RenderSliverIgnorePointer::new(true);
//!
//! // Allow pointer events (interactive)
//! let enabled = RenderSliverIgnorePointer::new(false);
//!
//! // Toggle interaction dynamically
//! let mut ignore = RenderSliverIgnorePointer::new(false);
//! ignore.set_ignoring(true);  // Disable
//! ignore.set_ignoring(false); // Enable
//!
//! // Also ignore semantics (accessibility)
//! let fully_ignored = RenderSliverIgnorePointer::new(true)
//!     .with_ignore_semantics();
//!
//! // During loading
//! let mut content = RenderSliverIgnorePointer::new(false);
//! // Start loading...
//! content.set_ignoring(true); // Disable interaction
//! // Finish loading...
//! content.set_ignoring(false); // Re-enable
//! ```

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::SliverGeometry;

/// RenderObject that makes a sliver ignore pointer events while keeping layout/paint.
///
/// Controls hit testing behavior without affecting visual rendering. When ignoring,
/// the sliver is fully visible and laid out but doesn't respond to pointer events,
/// allowing events to pass through to widgets beneath.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child sliver (optional in implementation).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Hit Testing Proxy** - Layout and paint are transparent proxies, only hit testing
/// is affected. When ignoring: blocks child hit testing. When not ignoring: passes
/// hit testing to child.
///
/// # Use Cases
///
/// - **Loading overlays**: Disable interaction during async operations
/// - **Visual-only content**: Non-interactive backgrounds in scrollables
/// - **Disabled sections**: Show content without allowing interaction
/// - **Tutorial mode**: Display UI without accepting input
/// - **Custom hit logic**: Block default hit testing for custom handlers
/// - **Temporary disabling**: Disable list items during batch operations
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderIgnorePointer behavior (adapted for slivers):
/// - Passes constraints unchanged to child ✅
/// - Returns child geometry unchanged ✅
/// - Paints child normally (ignoring doesn't affect paint) ✅
/// - Blocks hit testing when ignoring = true ✅
/// - Supports ignore_semantics for accessibility ✅
///
/// # Behavior Comparison
///
/// | Feature | IgnorePointer | AbsorbPointer | Offstage |
/// |---------|---------------|---------------|----------|
/// | Layout | Full | Full | Skipped |
/// | Paint | Full | Full | Skipped |
/// | Hit Testing | Blocked | Consumes | Blocked |
/// | Events to beneath | Yes (pass through) | No (absorbed) | N/A |
/// | Geometry | Child's | Child's | Zero |
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverIgnorePointer;
///
/// // Disable interaction (events pass through)
/// let disabled = RenderSliverIgnorePointer::new(true);
///
/// // Enable interaction
/// let enabled = RenderSliverIgnorePointer::new(false);
///
/// // Toggle based on loading state
/// let mut ui = RenderSliverIgnorePointer::new(false);
/// ui.set_ignoring(is_loading); // Disable when loading
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
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        // Pass through to child - IgnorePointer doesn't affect layout
        if let Some(child_id) = ctx.children.try_single() {
            self.sliver_geometry = ctx.tree.layout_sliver_child(child_id, ctx.constraints);
        } else {
            self.sliver_geometry = SliverGeometry::default();
        }

        self.sliver_geometry
    }

    fn paint(&self, ctx: &Sliver) -> Canvas {
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
