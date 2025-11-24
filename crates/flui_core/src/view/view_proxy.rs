//! Proxy view trait.
//!
//! For views that wrap single child without changing layout.

use crate::element::IntoElement;
use crate::view::{BuildContext, Proxy, View};
use flui_types::Event;

/// Proxy view - views that wrap a single child.
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
/// # Lifecycle
///
/// 1. **Created**: Proxy view instantiated
/// 2. **Mounted**: `init()` called
/// 3. **Build**: `build_child()` returns child element
/// 4. **Events**: `handle_event()` intercepts events
/// 5. **Disposed**: `dispose()` called for cleanup
///
/// # Example: Event Blocking
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct IgnorePointer {
///     ignoring: bool,
///     child: Element,
/// }
///
/// impl ProxyView for IgnorePointer {
///     fn build_child(&mut self, _ctx: &BuildContext) -> impl IntoElement {
///         self.child.clone()
///     }
///
///     fn handle_event(&mut self, _event: &Event, _ctx: &BuildContext) -> bool {
///         self.ignoring  // Block events if ignoring
///     }
/// }
/// ```
///
/// # Example: Semantics
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct Semantics {
///     label: String,
///     child: Element,
/// }
///
/// impl ProxyView for Semantics {
///     fn build_child(&mut self, _ctx: &BuildContext) -> impl IntoElement {
///         self.child.clone()
///     }
///
///     fn before_child_build(&mut self, ctx: &BuildContext) {
///         ctx.set_semantics_label(&self.label);
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Event interception (IgnorePointer, GestureDetector)
/// - Accessibility (Semantics, ExcludeSemantics)
/// - Focus management (Focus, FocusScope)
/// - Optimization hints (RepaintBoundary)
/// - Metadata (Tooltip, Hero)
///
/// # When NOT to Use
///
/// - Custom layout → Use render object
/// - Multiple children → Use `StatelessView` or render object
/// - Need state → Use `StatefulView`
///
/// # Comparison to Render System
///
/// | Render | View |
/// |--------|------|
/// | `RenderProxyBox` | `ProxyView` |
/// | Delegates layout | Delegates to child |
/// | Single child | Single child |
/// | Can intercept paint | Can intercept events |
pub trait ProxyView: Clone + Send + 'static {
    /// Build the child element.
    ///
    /// Required method. Must return exactly one child element.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn build_child(&mut self, _ctx: &BuildContext) -> impl IntoElement {
    ///     self.child.clone()
    /// }
    /// ```
    fn build_child(&mut self, ctx: &BuildContext) -> impl IntoElement;

    /// Called before child builds (optional).
    ///
    /// Use to set up context or metadata before child tree is built.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn before_child_build(&mut self, ctx: &BuildContext) {
    ///     ctx.push_layer();  // Setup
    /// }
    /// ```
    fn before_child_build(&mut self, _ctx: &BuildContext) {}

    /// Called after child builds (optional).
    ///
    /// Use to clean up context after child tree is built.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn after_child_build(&mut self, ctx: &BuildContext) {
    ///     ctx.pop_layer();  // Cleanup
    /// }
    /// ```
    fn after_child_build(&mut self, _ctx: &BuildContext) {}

    /// Handle event before passing to child (optional).
    ///
    /// Return `true` to stop event propagation, `false` to continue.
    ///
    /// # Example: Block events
    ///
    /// ```rust,ignore
    /// fn handle_event(&mut self, _event: &Event, _ctx: &BuildContext) -> bool {
    ///     self.ignoring  // Block if ignoring
    /// }
    /// ```
    ///
    /// # Example: Observe events
    ///
    /// ```rust,ignore
    /// fn handle_event(&mut self, event: &Event, _ctx: &BuildContext) -> bool {
    ///     if let Event::Tap = event {
    ///         self.on_tap();
    ///     }
    ///     false  // Don't block, just observe
    /// }
    /// ```
    fn handle_event(&mut self, _event: &Event, _ctx: &BuildContext) -> bool {
        false  // Default: don't block events
    }

    /// Initialize after element is mounted (optional).
    ///
    /// Use for setup that requires element to be in tree.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn init(&mut self, ctx: &BuildContext) {
    ///     if self.autofocus {
    ///         ctx.request_focus();
    ///     }
    /// }
    /// ```
    fn init(&mut self, _ctx: &BuildContext) {}

    /// Called when element is disposed (optional).
    ///
    /// Use for cleanup.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn dispose(&mut self, ctx: &BuildContext) {
    ///     ctx.remove_overlay();
    /// }
    /// ```
    fn dispose(&mut self, _ctx: &BuildContext) {}
}

/// Auto-implement `View<Proxy>` for all `ProxyView`.
///
/// This allows `ProxyView` to integrate with the internal protocol system.
impl<V> View<Proxy> for V
where
    V: ProxyView,
{
    fn _build(&mut self, ctx: &BuildContext) -> crate::element::Element {
        self.before_child_build(ctx);
        let child = self.build_child(ctx).into_element();
        self.after_child_build(ctx);
        child
    }
}
