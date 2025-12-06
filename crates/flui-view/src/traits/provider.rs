//! `ProviderView` - Views that provide data to descendants
//!
//! Similar to Flutter's `InheritedWidget`. Provides typed data that
//! descendant widgets can access.
//!
//! # Lifecycle
//!
//! `ProviderView` follows Flutter-like lifecycle:
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
//! │  └──────┬──────────────────┘   change (nested providers)    │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌──────────────┐                                           │
//! │  │    build     │◄──────────────────┐                       │
//! │  └──────┬───────┘                   │                       │
//! │         │                           │                       │
//! │         ▼                           │ (update with new      │
//! │  ┌───────────────┐                  │  value)               │
//! │  │ should_notify │ ← Compare values │                       │
//! │  └──────┬────────┘                  │                       │
//! │         │ true                      │                       │
//! │         ▼                           │                       │
//! │  ┌───────────────────┐              │                       │
//! │  │ notify dependents │──────────────┘                       │
//! │  └──────┬────────────┘                                      │
//! │         │                                                    │
//! │         ▼ (element unmounted)                               │
//! │  ┌──────────────┐                                           │
//! │  │   dispose    │ ← Clean up, clear dependents              │
//! │  └──────────────┘                                           │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;

use crate::{BuildContext, IntoView};

/// `ProviderView` - Views that provide data to descendants.
///
/// This is equivalent to Flutter's `InheritedWidget`. Provides typed data that
/// descendant widgets can access via `ctx.depend_on<T>()`.
///
/// # Performance Tips
///
/// - Override `should_notify()` to avoid unnecessary rebuilds of dependents
/// - Use `Arc<T>` for provided values to avoid clones (already done in trait design)
/// - Keep provided types small - they're cloned for each dependent
/// - Consider fine-grained providers (multiple small providers) vs one large provider
/// - Use structural comparison in `should_notify()` for better performance
///
/// # Architecture
///
/// ```text
/// ProviderView<Theme>
///     ↓ provides Theme
/// Descendant calls ctx.depend_on<Theme>()
///     ↓ registers dependency
/// Provider updates → should_notify() → rebuild dependents
/// ```
///
/// # Example
///
/// ```rust,ignore
/// struct ThemeProvider {
///     theme: Arc<Theme>,
///     child: Box<dyn ViewObject>,
/// }
///
/// impl ProviderView<Theme> for ThemeProvider {
///     fn value(&self) -> Arc<Theme> {
///         Arc::clone(&self.theme)
///     }
///
///     fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
///         // Return child
///     }
///
///     fn should_notify(&self, old: &Theme) -> bool {
///         // Only notify if color actually changed
///         self.theme.primary_color != old.primary_color
///     }
/// }
///
/// // Usage in descendant:
/// impl StatelessView for ThemedButton {
///     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
///         let theme = ctx.depend_on::<Theme>()
///             .expect("ThemeProvider not found");
///         Button::new("Click").color(theme.primary_color)
///     }
/// }
/// ```
///
/// # Optimization: `should_notify`
///
/// Override [`should_notify`](Self::should_notify) to avoid unnecessary rebuilds:
///
/// ```rust,ignore
/// fn should_notify(&self, old: &MyData) -> bool {
///     // Only notify if relevant fields changed
///     self.data.version != old.version
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
    /// Typically just returns the child view object unchanged, as the provider
    /// doesn't modify layout - it only provides data.
    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoView;

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
    ///         Arc::clone(&self.data)  // Cheap Arc clone
    ///     }
    /// }
    /// ```
    fn value(&self) -> Arc<T>;

    /// Should notify dependents when value changes?
    ///
    /// Called when provider is updated with new props. Return `true` to
    /// trigger rebuild of all dependent widgets, `false` to skip notification.
    ///
    /// **Flutter equivalent:** `InheritedWidget.updateShouldNotify()`
    ///
    /// # Optimization
    ///
    /// Override this method to avoid unnecessary rebuilds. For example, if your
    /// provider wraps a large data structure but only certain fields are relevant:
    ///
    /// ```rust,ignore
    /// fn should_notify(&self, old: &Config) -> bool {
    ///     // Only notify if user-visible config changed
    ///     self.config.theme != old.theme || self.config.locale != old.locale
    /// }
    /// ```
    ///
    /// # Default
    ///
    /// Default implementation returns `true` (always notify).
    #[allow(unused_variables)]
    fn should_notify(&self, old_value: &T) -> bool {
        true
    }

    // ========== LIFECYCLE METHODS ==========

    /// Initialize after element is mounted.
    ///
    /// Called once after the element has been inserted into the tree.
    ///
    /// **Flutter equivalent:** Similar to `State.initState()`
    #[allow(unused_variables)]
    fn init(&mut self, ctx: &dyn BuildContext) {}

    /// Called when an inherited widget dependency changes.
    ///
    /// Providers can depend on other providers.
    ///
    /// **Flutter equivalent:** `State.didChangeDependencies()`
    #[allow(unused_variables)]
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is temporarily removed from the tree.
    ///
    /// **Flutter equivalent:** `State.deactivate()`
    #[allow(unused_variables)]
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is reinserted after being deactivated.
    ///
    /// **Flutter equivalent:** `State.activate()`
    #[allow(unused_variables)]
    fn activate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when element is permanently removed.
    ///
    /// Clean up resources here. Dependents list is automatically cleared.
    ///
    /// **Flutter equivalent:** `State.dispose()`
    #[allow(unused_variables)]
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
}
