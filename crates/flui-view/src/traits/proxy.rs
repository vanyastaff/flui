//! `ProxyView` - Views that wrap single child without changing layout
//!
//! For views that add behavior, metadata, or event handling without
//! affecting layout.
//!
//! # Lifecycle
//!
//! `ProxyView` follows Flutter-like lifecycle (simplified from `StatefulView`):
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     LIFECYCLE DIAGRAM                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  ┌──────────────┐                                           │
//! │  │     init     │ ← Called once when element is mounted     │
//! │  └──────┬───────┘                                           │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────┐                                │
//! │  │ did_change_dependencies │ ← Called when dependencies     │
//! │  └──────┬──────────────────┘   change (InheritedWidget)     │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌───────────────────┐                                      │
//! │  │ before_child_build│                                      │
//! │  └──────┬────────────┘                                      │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌──────────────┐                                           │
//! │  │  build_child │◄──────────────────┐                       │
//! │  └──────┬───────┘                   │                       │
//! │         │                           │                       │
//! │         ▼                           │ (rebuild)             │
//! │  ┌──────────────────┐               │                       │
//! │  │ after_child_build│───────────────┘                       │
//! │  └──────┬───────────┘                                       │
//! │         │                                                    │
//! │         ▼ (element unmounted)                               │
//! │  ┌──────────────┐                                           │
//! │  │   dispose    │ ← Clean up resources                      │
//! │  └──────────────┘                                           │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```

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

    // ========== LIFECYCLE METHODS ==========

    /// Initialize after element is mounted.
    ///
    /// Called once after the element has been inserted into the tree.
    /// Use this to:
    /// - Subscribe to streams or listeners
    /// - Initialize resources
    /// - Access inherited widgets (via `ctx.depend_on<T>()`)
    ///
    /// **Flutter equivalent:** Similar to `State.initState()`
    #[allow(unused_variables)]
    fn init(&mut self, ctx: &dyn BuildContext) {}

    /// Called when an inherited widget dependency changes.
    ///
    /// This method is called:
    /// - Immediately after [`init`](Self::init)
    /// - Whenever an `InheritedWidget` that this view depends on changes
    ///
    /// **Flutter equivalent:** `State.didChangeDependencies()`
    #[allow(unused_variables)]
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}

    /// Called before child builds.
    ///
    /// Use to set up context or metadata before child tree is built.
    #[allow(unused_variables)]
    fn before_child_build(&mut self, ctx: &dyn BuildContext) {}

    /// Called after child builds.
    ///
    /// Use to clean up context after child tree is built.
    #[allow(unused_variables)]
    fn after_child_build(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is temporarily removed from the tree.
    ///
    /// This happens when the element might be reinserted later.
    ///
    /// **Flutter equivalent:** `State.deactivate()`
    #[allow(unused_variables)]
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is reinserted after being deactivated.
    ///
    /// Opposite of [`deactivate`](Self::deactivate).
    ///
    /// **Flutter equivalent:** `State.activate()`
    #[allow(unused_variables)]
    fn activate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when element is permanently removed.
    ///
    /// Clean up resources, cancel subscriptions here.
    ///
    /// **Flutter equivalent:** `State.dispose()`
    #[allow(unused_variables)]
    fn dispose(&mut self, ctx: &dyn BuildContext) {}

    // ========== EVENT HANDLING ==========

    /// Handle event before passing to child.
    ///
    /// Return `true` to stop event propagation, `false` to continue.
    #[allow(unused_variables)]
    fn handle_event(&mut self, event: &Event, ctx: &dyn BuildContext) -> bool {
        false // Default: don't block events
    }
}
