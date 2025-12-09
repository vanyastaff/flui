//! `IntoViewConfig` trait - Convert views into immutable configurations.
//!
//! # Overview
//!
//! The `IntoViewConfig` trait provides a way to convert view types into
//! `ViewConfig` instances without creating live `ViewObject` state immediately.
//!
//! This enables:
//! - **Hot-reload**: Recreate view objects from configuration
//! - **Lazy mounting**: Delay state creation until mount time
//! - **Flutter-like API**: Pass views as config, not pre-created objects
//!
//! # Comparison with IntoView
//!
//! | Trait | Converts To | When to Use |
//! |-------|------------|-------------|
//! | `IntoView` | `Box<dyn ViewObject>` | Immediate object creation |
//! | `IntoViewConfig` | `ViewConfig` | Deferred mounting, hot-reload |
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_view::{IntoViewConfig, StatelessView, BuildContext};
//!
//! #[derive(Clone)]
//! struct MyView {
//!     value: i32,
//! }
//!
//! impl StatelessView for MyView {
//!     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(format!("Value: {}", self.value))
//!     }
//! }
//!
//! // Convert to ViewConfig (no ViewObject created yet!)
//! let config = MyView { value: 42 }.into_view_config();
//!
//! // Later, create ViewHandle and mount
//! let handle = ViewHandle::new(config);
//! let mounted = handle.mount(None);
//! ```
//!
//! # Design
//!
//! This trait is automatically implemented for all view types that implement
//! the view traits (`StatelessView`, `StatefulView`, etc.) via blanket impls.

use crate::handle::ViewConfig;
use crate::traits::{StatefulView, StatelessView};
use crate::wrappers::{StatefulViewWrapper, StatelessViewWrapper};

/// Converts a view into an immutable `ViewConfig`.
///
/// This trait enables delayed mounting of views by storing configuration
/// instead of creating live `ViewObject` instances immediately.
///
/// # Implementations
///
/// This trait is implemented via blanket impls for:
/// - Types implementing `StatelessView`
/// - Types implementing `StatefulView`
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{IntoViewConfig, StatelessView};
///
/// #[derive(Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
///
/// // Convert to config without creating ViewObject
/// let config = Greeting { name: "World".into() }.into_view_config();
/// ```
pub trait IntoViewConfig: Send + 'static {
    /// Convert this view into a `ViewConfig`.
    ///
    /// This creates an immutable configuration that can be used to create
    /// `ViewHandle` instances later.
    fn into_view_config(self) -> ViewConfig;
}

// ============================================================================
// BLANKET IMPL FOR STATELESS VIEWS
// ============================================================================

/// Blanket implementation for all `StatelessView` types.
///
/// This automatically implements `IntoViewConfig` for any type that implements
/// `StatelessView` and is `Clone + Send + Sync + 'static`.
///
/// # Factory Function
///
/// The factory function creates a `StatelessViewWrapper` which implements
/// `ViewObject` and delegates to the original stateless view's `build` method.
impl<V> IntoViewConfig for V
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        ViewConfig::new_with_factory(self, |v: &V| {
            Box::new(StatelessViewWrapper::new(v.clone()))
        })
    }
}

// Note: We cannot provide a blanket impl for StatefulView here because it
// would conflict with the StatelessView blanket impl. StatefulView requires
// explicit wrapper creation with state management.
//
// Users should use the `Stateful(view)` wrapper for stateful views:
//
// ```rust,ignore
// use flui_view::{Stateful, StatefulView};
//
// let config = Stateful(MyStatefulView { ... }).into_view_config();
// ```

// ============================================================================
// IMPL FOR STATEFUL VIEW WRAPPER
// ============================================================================

/// Implementation for `StatefulViewWrapper`.
///
/// This allows stateful views to be converted to `ViewConfig` when wrapped
/// in the `Stateful` helper:
///
/// ```rust,ignore
/// use flui_view::{Stateful, StatefulView, IntoViewConfig};
///
/// #[derive(Clone)]
/// struct Counter {
///     initial_count: i32,
/// }
///
/// impl StatefulView for Counter {
///     type State = i32;
///     // ...
/// }
///
/// let config = Stateful(Counter { initial_count: 0 }).into_view_config();
/// ```
impl<V> IntoViewConfig for StatefulViewWrapper<V>
where
    V: StatefulView + Clone + Send + Sync + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        let view = self.into_inner();
        ViewConfig::new_with_factory(view, |v: &V| {
            Box::new(StatefulViewWrapper::new(v.clone()))
        })
    }
}

// ============================================================================
// IMPL FOR STATELESS VIEW WRAPPER
// ============================================================================

/// Implementation for `StatelessViewWrapper`.
///
/// This allows explicit use of the `Stateless` wrapper:
///
/// ```rust,ignore
/// use flui_view::{Stateless, StatelessView, IntoViewConfig};
///
/// let config = Stateless(MyView { ... }).into_view_config();
/// ```
impl<V> IntoViewConfig for StatelessViewWrapper<V>
where
    V: StatelessView + Clone + Send + Sync + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        let view = self.into_inner();
        ViewConfig::new_with_factory(view, |v: &V| {
            Box::new(StatelessViewWrapper::new(v.clone()))
        })
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViewMode;

    // Helper test view
    #[derive(Clone)]
    struct TestView {
        value: i32,
    }

    impl StatelessView for TestView {
        fn build(self, _ctx: &dyn crate::BuildContext) -> impl crate::IntoView {
            crate::EmptyView
        }
    }

    #[test]
    fn test_into_view_config_stateless() {
        let view = TestView { value: 42 };
        let config = view.into_view_config();

        // Create ViewObject from config
        let view_obj = config.create_view_object();
        assert_eq!(view_obj.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_into_view_config_can_update() {
        let view1 = TestView { value: 1 };
        let view2 = TestView { value: 2 };

        let config1 = view1.into_view_config();
        let config2 = view2.into_view_config();

        // Same type, should be able to update
        assert!(config1.can_update(&config2));
    }

    #[test]
    fn test_config_creates_fresh_objects() {
        let view = TestView { value: 99 };
        let config = view.into_view_config();

        // Create two objects from same config
        let obj1 = config.create_view_object();
        let obj2 = config.create_view_object();

        // They should be independent instances
        assert_eq!(obj1.mode(), ViewMode::Stateless);
        assert_eq!(obj2.mode(), ViewMode::Stateless);
    }
}
