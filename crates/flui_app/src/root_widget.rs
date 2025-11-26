//! RootWidget - Internal root widget wrapper
//!
//! This is analogous to Flutter's RootWidget and _RenderObjectToWidgetAdapter.
//! It wraps the user's root app and provides the framework infrastructure.
//!
//! # Architecture
//!
//! ```text
//! RootWidget (internal framework widget)
//!   └─ MediaQueryProvider (window size, DPI)
//!       └─ ErrorBoundary (catch errors in user code)
//!           └─ Performance overlay (if enabled, TODO)
//!               └─ User's app (WidgetsApp, MaterialApp, or custom)
//! ```
//!
//! # Design Notes
//!
//! This widget is NOT part of the public API. Users should use `run_app()` which
//! automatically wraps their app in RootWidget.

use crate::error_handling::ErrorBoundary;
use crate::providers::MediaQueryProvider;
use crate::AppBinding;
use flui_core::BuildContext;
use flui_view::{IntoElement, Provider, Stateful, StatelessView};

/// RootWidget - Internal framework widget that wraps the user's app
///
/// Provides:
/// - Error boundary for catching panics in user code
/// - Framework initialization
/// - Performance overlay (optional)
///
/// # Example
///
/// ```rust,ignore
/// // Users don't create this directly - run_app() does it for them
/// let root = RootWidget::new(MyApp);
/// ```
#[derive(Debug, Clone)]
pub struct RootWidget<V>
where
    V: StatelessView + Clone,
{
    /// The user's root app widget
    app: V,

    /// Enable performance overlay (FPS counter, frame timing)
    #[allow(dead_code)]
    show_performance_overlay: bool,
}

impl<V> RootWidget<V>
where
    V: StatelessView + Clone,
{
    /// Create a new RootWidget wrapping the user's app
    ///
    /// # Parameters
    ///
    /// - `app`: User's root widget (typically WidgetsApp or MaterialApp)
    pub fn new(app: V) -> Self {
        Self {
            app,
            show_performance_overlay: false,
        }
    }

    /// Enable performance overlay
    ///
    /// Shows FPS counter and frame timing information.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_performance_overlay(mut self, enabled: bool) -> Self {
        self.show_performance_overlay = enabled;
        self
    }
}

impl<V> StatelessView for RootWidget<V>
where
    V: StatelessView + Clone,
{
    fn build(self, ctx: &dyn BuildContext) -> impl IntoElement {
        // Get media query data from binding
        let binding =
            AppBinding::instance().expect("AppBinding must be initialized before RootWidget");
        let media_data = binding.media_query();

        // Convert app to Element
        let app_element = self.app.build(ctx).into_element();

        // Wrap in framework infrastructure:
        // 1. ErrorBoundary (error handling)
        let error_boundary = ErrorBoundary::new(app_element).on_error(|error| {
            tracing::error!(
                message = %error.message,
                details = ?error.details,
                "Error in widget tree"
            );
        });

        let error_boundary_element = Stateful(error_boundary).into_element();

        // 2. MediaQueryProvider (window size, DPI)
        // TODO: Add performance overlay if enabled

        Provider::new(MediaQueryProvider::new(media_data, error_boundary_element))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::Element;

    #[derive(Debug, Clone)]
    struct TestApp;

    impl StatelessView for TestApp {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            Element::empty()
        }
    }

    #[test]
    fn test_root_widget_creation() {
        let root = RootWidget::new(TestApp);
        assert!(!root.show_performance_overlay);
    }

    #[test]
    fn test_with_performance_overlay() {
        let root = RootWidget::new(TestApp).with_performance_overlay(true);
        assert!(root.show_performance_overlay);
    }
}
