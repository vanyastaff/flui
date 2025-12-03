//! `ProxyView` - Views that wrap single child without changing layout
//!
//! For views that add behavior, metadata, or event handling without
//! affecting layout.

use flui_types::Event;

use crate::{BuildContext, IntoView};

/// `ProxyView` - Views that wrap a single child.
///
/// Similar to `RenderProxyBox` in render system. Delegates layout to child
/// while adding behavior, metadata, or event handling.
///
/// # Purpose
///
/// Proxy views are lightweight wrappers that:
/// - Don't change layout (child determines size/position)
/// - Add behavior (event handling, focus, etc)
/// - Add metadata (semantics, accessibility)
/// - Optimize rendering (repaint boundaries)
///
/// # Architecture
///
/// ```text
/// ProxyView → build_child() → Child
///     ↓ wraps                    ↓ determines layout
/// Adds behavior               Actual UI
/// ```
///
/// # Example: Event Blocking
///
/// ```rust,ignore
/// struct IgnorePointer {
///     ignoring: bool,
///     child: Box<dyn ViewObject>,
/// }
///
/// impl ProxyView for IgnorePointer {
///     fn build_child(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
///         // Return child
///     }
///
///     fn handle_event(&mut self, _event: &Event, _ctx: &dyn BuildContext) -> bool {
///         self.ignoring  // Block events if ignoring
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Event interception (`IgnorePointer`, `GestureDetector`)
/// - Accessibility (Semantics, `ExcludeSemantics`)
/// - Focus management (Focus, `FocusScope`)
/// - Optimization hints (`RepaintBoundary`)
/// - Metadata (Tooltip, Hero)
///
/// # When NOT to Use
///
/// - Custom layout → Use render object
/// - Multiple children → Use `StatelessView` or render object
/// - Need state → Use `StatefulView`
pub trait ProxyView: Send + Sync + 'static {
    /// Build the child view object.
    ///
    /// Required method. Must return exactly one child.
    fn build_child(&mut self, ctx: &dyn BuildContext) -> impl IntoView;

    /// Called before child builds (optional).
    ///
    /// Use to set up context or metadata before child tree is built.
    fn before_child_build(&mut self, _ctx: &dyn BuildContext) {}

    /// Called after child builds (optional).
    ///
    /// Use to clean up context after child tree is built.
    fn after_child_build(&mut self, _ctx: &dyn BuildContext) {}

    /// Handle event before passing to child (optional).
    ///
    /// Return `true` to stop event propagation, `false` to continue.
    fn handle_event(&mut self, _event: &Event, _ctx: &dyn BuildContext) -> bool {
        false // Default: don't block events
    }

    /// Initialize after element is mounted (optional).
    fn init(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when element is disposed (optional).
    fn dispose(&mut self, _ctx: &dyn BuildContext) {}
}
