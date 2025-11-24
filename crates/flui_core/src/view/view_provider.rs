//! Provider view trait.
//!
//! For views that provide data to descendants (like InheritedWidget).

use crate::element::IntoElement;
use crate::view::{BuildContext, Provider, View};

/// Provider view - views that provide data to descendants.
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
/// # Lifecycle
///
/// 1. **Created**: Provider with value instantiated
/// 2. **Mounted**: Value registered in context
/// 3. **Dependents**: Descendants register dependencies
/// 4. **Updated**: New value → notify dependents
/// 5. **Disposed**: Value unregistered from context
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
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
///         Button::new("Click")
///             .color(theme.primary_color)
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
/// - Global singletons → Use static or lazy_static
///
/// # Comparison to Flutter
///
/// | Flutter | FLUI |
/// |---------|------|
/// | `InheritedWidget` | `ProviderView` |
/// | `of(context)` | `ctx.depend_on<T>()` |
/// | `updateShouldNotify` | `should_notify()` |
/// | Type parameter on widget | Type parameter on trait |
pub trait ProviderView<T: Send + 'static>: Clone + Send + 'static {
    /// Build the child subtree.
    ///
    /// Typically just returns the child element unchanged, as the provider
    /// doesn't modify layout - it only provides data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn build(&mut self, _ctx: &BuildContext) -> impl IntoElement {
    ///     self.child.clone()
    /// }
    /// ```
    fn build(&mut self, ctx: &BuildContext) -> impl IntoElement;

    /// Get the value to provide.
    ///
    /// Descendants access this via `ctx.depend_on<T>()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn value(&self) -> &Theme {
    ///     &self.theme
    /// }
    /// ```
    fn value(&self) -> &T;

    /// Should notify dependents when updating?
    ///
    /// Called when provider is updated with new props. Return `true` to
    /// trigger rebuild of all dependent widgets.
    ///
    /// # Default
    ///
    /// Always returns `true` (notify on every update).
    ///
    /// # Optimization
    ///
    /// Override to prevent unnecessary rebuilds:
    ///
    /// ```rust,ignore
    /// fn should_notify(&self, old: &Self) -> bool {
    ///     !Arc::ptr_eq(&self.theme, &old.theme)  // Only if Arc changed
    /// }
    /// ```
    fn should_notify(&self, _old: &Self) -> bool {
        true
    }
}

/// Auto-implement `View<Provider<T>>` for all `ProviderView<T>`.
///
/// This allows `ProviderView` to integrate with the internal protocol system.
impl<V, T> View<Provider<T>> for V
where
    V: ProviderView<T>,
    T: Send + 'static,
{
    fn _build(&mut self, ctx: &BuildContext) -> crate::element::Element {
        self.build(ctx).into_element()
    }
}
