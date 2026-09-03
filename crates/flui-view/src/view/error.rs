//! ErrorView - Widget displayed when build fails.
//!
//! When an error occurs during build, the broken widget is replaced
//! by an ErrorView. This provides visual feedback and debugging information.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `ErrorWidget` class.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::ErrorView;
//!
//! // Create from error message
//! let error = ErrorView::new("Failed to build widget");
//!
//! // Create with details
//! let error = ErrorView::with_details(
//!     "Build failed",
//!     Some("Stack trace here...".to_string()),
//! );
//! ```

use std::sync::RwLock;

use super::view::View;

/// Factory function type for creating custom error widgets.
///
/// This allows applications to customize how errors are displayed.
pub type ErrorViewBuilder = fn(&FlutterError) -> Box<dyn View>;

/// Global configurable factory for ErrorView.
///
/// Applications can set this to customize error display.
static ERROR_VIEW_BUILDER: RwLock<Option<ErrorViewBuilder>> = RwLock::new(None);

/// Set the global error view builder.
///
/// When an error occurs during build, this factory is used to create
/// the error widget. If not set, the default ErrorView is used.
pub fn set_error_view_builder(builder: ErrorViewBuilder) {
    if let Ok(mut guard) = ERROR_VIEW_BUILDER.write() {
        *guard = Some(builder);
    }
}

/// Clear the global error view builder.
pub fn clear_error_view_builder() {
    if let Ok(mut guard) = ERROR_VIEW_BUILDER.write() {
        *guard = None;
    }
}

/// Error details for framework errors.
#[derive(Debug, Clone)]
pub struct FlutterError {
    /// The error message.
    pub message: String,
    /// Optional stack trace or additional details.
    pub details: Option<String>,
    /// The exception that caused the error, if any.
    pub exception: Option<String>,
}

impl FlutterError {
    /// Create a new error with just a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
            exception: None,
        }
    }

    /// Create a new error with message and details.
    pub fn with_details(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: Some(details.into()),
            exception: None,
        }
    }

    /// Create from an exception.
    pub fn from_exception(exception: &dyn std::fmt::Debug) -> Self {
        Self {
            message: format!("{exception:?}"),
            details: None,
            exception: Some(format!("{exception:?}")),
        }
    }

    /// Create a `FlutterError` from a panic payload caught by
    /// [`std::panic::catch_unwind`].
    ///
    /// A panic payload is a `Box<dyn Any + Send>`. The common shapes are
    /// `&'static str` (from `panic!("literal")`) and `String` (from
    /// `panic!("{}", formatted)`); anything else (a custom panic value)
    /// cannot be rendered and falls back to a generic message.
    ///
    /// `context` describes *what* was building when the panic happened
    /// (e.g. `"building StatelessElement"`) and is stored as the error
    /// `details` so the rendered [`ErrorView`] carries a breadcrumb.
    ///
    /// Flutter parity: `ComponentElement.performRebuild`
    /// (`framework.dart:5823-5834`) funnels the caught exception through
    /// `_reportException` into `ErrorWidget.builder`.
    pub fn from_panic(payload: &(dyn std::any::Any + Send), context: impl Into<String>) -> Self {
        let message = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic during build (non-string payload)".to_string()
        };
        Self {
            message,
            details: Some(context.into()),
            exception: None,
        }
    }
}

impl std::fmt::Display for FlutterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, "\n{details}")?;
        }
        Ok(())
    }
}

impl std::error::Error for FlutterError {}

/// A View that displays an error message.
///
/// This is used when a widget fails to build. It displays the error
/// message in debug mode and a gray background in release mode.
///
/// # Customization
///
/// Use [`set_error_view_builder`] to customize how errors are displayed.
#[derive(Clone)]
pub struct ErrorView {
    /// The error message to display.
    pub message: String,
    /// Optional additional details.
    pub details: Option<String>,
}

impl ErrorView {
    /// Create an ErrorView with just a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    /// Create an ErrorView with message and details.
    pub fn with_details(message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            message: message.into(),
            details,
        }
    }

    /// Create an ErrorView from a FlutterError.
    pub fn from_error(error: &FlutterError) -> Self {
        Self {
            message: error.message.clone(),
            details: error.details.clone(),
        }
    }

    /// Build an error view using the global builder or default.
    pub fn build_error_view(error: &FlutterError) -> Box<dyn View> {
        // Check for custom builder
        if let Ok(guard) = ERROR_VIEW_BUILDER.read()
            && let Some(builder) = *guard
        {
            return builder(error);
        }

        // Default: use ErrorView
        Box::new(Self::from_error(error))
    }
}

impl std::fmt::Debug for ErrorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorView")
            .field("message", &self.message)
            .field("details", &self.details)
            .finish()
    }
}

impl crate::view::RenderView for ErrorView {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderErrorBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        flui_objects::RenderErrorBox::new(self.message.clone(), self.details.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_error(self.message.clone(), self.details.clone())
    }
}

impl View for ErrorView {
    /// A render element over [`RenderErrorBox`](flui_objects::RenderErrorBox).
    ///
    /// The error view owns a render node on purpose: it stands in for a
    /// subtree that failed to build, and every place that subtree could sit
    /// — a lazy sliver child in particular, which must carry a render node to
    /// be laid out at all — needs something with size and paint there.
    /// Flutter's `ErrorWidget` is a `RenderErrorBox` for the same reason.
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_view_creation() {
        let error = ErrorView::new("Test error");
        assert_eq!(error.message, "Test error");
        assert!(error.details.is_none());
    }

    #[test]
    fn test_error_view_with_details() {
        let error = ErrorView::with_details("Test error", Some("Stack trace".to_string()));
        assert_eq!(error.message, "Test error");
        assert_eq!(error.details.as_deref(), Some("Stack trace"));
    }

    #[test]
    fn error_view_is_a_render_element_over_an_error_box() {
        let view = ErrorView::new("boom");
        assert!(
            matches!(
                view.create_element(),
                crate::element::ElementKind::RenderVariable(_)
            ),
            "the error view must own a render node so a failed subtree still has size and paint"
        );
        let ctx = crate::RenderObjectContext::new(None);
        let render_object =
            <ErrorView as crate::view::RenderView>::create_render_object(&view, &ctx);
        assert_eq!(render_object.message(), "boom");
    }
}
