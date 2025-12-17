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

use super::view::{ElementBase, View};
use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;
use std::sync::RwLock;

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
        if let Ok(guard) = ERROR_VIEW_BUILDER.read() {
            if let Some(builder) = *guard {
                return builder(error);
            }
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

impl View for ErrorView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(ErrorElement::new(self))
    }

}

// ============================================================================
// ErrorElement
// ============================================================================

/// Element for ErrorView.
///
/// This is a leaf element that doesn't have children.
pub struct ErrorElement {
    /// The current View configuration.
    view: ErrorView,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
}

impl ErrorElement {
    /// Create a new ErrorElement.
    pub fn new(view: &ErrorView) -> Self {
        Self {
            view: view.clone(),
            lifecycle: Lifecycle::Initial,
            depth: 0,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.view.message
    }

    /// Get the error details.
    pub fn details(&self) -> Option<&str> {
        self.view.details.as_deref()
    }
}

impl std::fmt::Debug for ErrorElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorElement")
            .field("message", &self.view.message)
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .finish()
    }
}

impl ElementBase for ErrorElement {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<ErrorView>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn update(&mut self, new_view: &dyn View) {
        if let Some(v) = new_view.as_any().downcast_ref::<ErrorView>() {
            self.view = v.clone();
        }
    }

    fn mark_needs_build(&mut self) {
        // ErrorElement is a leaf - nothing to rebuild
    }

    fn perform_build(&mut self) {
        // ErrorElement is a leaf - nothing to build
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {
        // No children
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
    fn test_flutter_error() {
        let error = FlutterError::with_details("Build failed", "at widget XYZ");
        assert_eq!(error.message, "Build failed");
        assert_eq!(error.details.as_deref(), Some("at widget XYZ"));
    }

    #[test]
    fn test_error_element_creation() {
        let view = ErrorView::new("Test error");
        let element = ErrorElement::new(&view);

        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert_eq!(element.message(), "Test error");
    }

    #[test]
    fn test_error_element_mount() {
        let view = ErrorView::new("Test error");
        let mut element = ErrorElement::new(&view);

        element.mount(None, 0);
        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }

    #[test]
    fn test_build_error_view_default() {
        let error = FlutterError::new("Test error");
        let view = ErrorView::build_error_view(&error);

        // Should return a boxed ErrorView
        assert!(view.as_any().downcast_ref::<ErrorView>().is_some());
    }
}
