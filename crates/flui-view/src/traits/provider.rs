//! `ProviderView` - Views that provide data to descendants
//!
//! Similar to Flutter's `InheritedWidget`. Provides typed data that
//! descendant widgets can access.

use std::sync::Arc;

use flui_element::IntoElement;
use flui_element::BuildContext;

/// `ProviderView` - Views that provide data to descendants.
///
/// Similar to Flutter's `InheritedWidget`. Provides typed data that
/// descendant widgets can access via `ctx.depend_on<T>()`.
///
/// # Architecture
///
/// ```text
/// ProviderView<Theme>
///     ↓ provides Theme
/// Descendant calls ctx.depend_on<Theme>()
///     ↓ registers dependency
/// Provider updates → notify dependents → rebuild
/// ```
///
/// # Example
///
/// ```rust,ignore
/// struct ThemeProvider {
///     theme: Arc<Theme>,
///     child: Element,
/// }
///
/// impl ProviderView<Theme> for ThemeProvider {
///     fn value(&self) -> &Theme {
///         &self.theme
///     }
///
///     fn build(&mut self, _ctx: &BuildContext) -> impl IntoElement {
///         self.child.clone()
///     }
///
///     fn should_notify(&self, old: &Self) -> bool {
///         !Arc::ptr_eq(&self.theme, &old.theme)
///     }
/// }
///
/// // Usage in descendant:
/// impl StatelessView for ThemedButton {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         let theme = ctx.depend_on::<Theme>();
///         Button::new("Click").color(theme.primary_color)
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Shared state/config (theme, locale, user)
/// - Dependency injection
/// - Configuration cascading down tree
/// - Context that multiple widgets need
///
/// # When NOT to Use
///
/// - Local state → Use `StatefulView`
/// - One-off props → Pass directly
/// - Global singletons → Use static or `lazy_static`
pub trait ProviderView<T: Send + Sync + 'static>: Send + Sync + 'static {
    /// Build the child subtree.
    ///
    /// Typically just returns the child element unchanged, as the provider
    /// doesn't modify layout - it only provides data.
    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoElement;

    /// Get the value to provide (as Arc for sharing).
    ///
    /// Descendants access this via `ctx.depend_on<T>()`.
    ///
    /// The value should be wrapped in Arc for efficient sharing across dependents.
    /// Cloning an Arc is cheap (just increments the reference count).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    ///
    /// struct MyProvider {
    ///     data: Arc<MyData>,
    /// }
    ///
    /// impl ProviderView<MyData> for MyProvider {
    ///     fn value(&self) -> Arc<MyData> {
    ///         self.data.clone()  // Cheap Arc clone
    ///     }
    /// }
    /// ```
    fn value(&self) -> Arc<T>;

    /// Should notify dependents when updating?
    ///
    /// Called when provider is updated with new props. Return `true` to
    /// trigger rebuild of all dependent widgets.
    ///
    /// Default: Always returns `true` (notify on every update).
    fn should_notify(&self, _old_value: &T) -> bool {
        true
    }

    /// Initialize after element is mounted (optional).
    fn init(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when element is disposed (optional).
    fn dispose(&mut self, _ctx: &dyn BuildContext) {}
}
