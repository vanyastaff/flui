//! Stateless view trait.
//!
//! For simple views without persistent state or lifecycle.

use crate::element::IntoElement;
use crate::view::{BuildContext, Stateless, View};

/// Stateless view - simple views without persistent state.
///
/// Similar to Flutter's `StatelessWidget`. Views are consumed during build
/// and cannot be rebuilt. Perfect for pure composition.
///
/// # Lifecycle
///
/// - **Created**: View struct instantiated
/// - **Build**: `build()` called once, view consumed
/// - **Done**: No rebuild, no lifecycle hooks
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         Text::new(format!("Hello, {}", self.name))
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Pure UI composition
/// - No user interaction
/// - Props don't change
/// - Simple leaf widgets
///
/// # When NOT to Use
///
/// - Need to store state → Use `StatefulView`
/// - Need subscriptions → Use `AnimatedView`
/// - Need lifecycle → Use `StatefulView` or `ProxyView`
pub trait StatelessView: Send + 'static {
    /// Build UI from this view.
    ///
    /// View is consumed (moved) during build. Cannot be called again.
    ///
    /// # Parameters
    ///
    /// - `self`: Consumed view (moved)
    /// - `ctx`: Build context for tree queries and hooks
    ///
    /// # Return
    ///
    /// Any type implementing `IntoElement` (View, RenderObject, Element, etc)
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

/// Auto-implement `View<Stateless>` for all `StatelessView`.
///
/// This allows `StatelessView` to integrate with the internal protocol system.
impl<V> View<Stateless> for V
where
    V: StatelessView,
{
    fn _build(&mut self, _ctx: &BuildContext) -> crate::element::Element {
        // Note: This is never called directly.
        // StatelessViewWrapper handles the consumption of the view.
        unreachable!("StatelessView::_build should not be called directly")
    }
}
