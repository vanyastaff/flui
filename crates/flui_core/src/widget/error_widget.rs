//! ErrorWidget - displays exceptions gracefully
//!
//! This widget displays error messages with different styles for debug and release modes.
//! In debug mode, it shows a red background with detailed error information.
//! In release mode, it shows a simple gray box.

use crate::context::Context;
use crate::widget::any_widget::AnyWidget;
use crate::widget::traits::StatelessWidget;
use std::fmt;
use std::sync::Arc;

/// Widget that displays an error message
///
/// # Debug vs Release Mode
///
/// - **Debug**: Red background with error message and details
/// - **Release**: Simple gray box (no error details exposed)
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::widget::ErrorWidget;
///
/// // Create from any error
/// let widget = ErrorWidget::new("Failed to load data");
///
/// // With additional details
/// let widget = ErrorWidget::new("Network error")
///     .with_details("Connection refused on port 8080");
/// ```
#[derive(Clone)]
pub struct ErrorWidget {
    message: String,
    details: Option<String>,
    error: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl fmt::Debug for ErrorWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErrorWidget")
            .field("message", &self.message)
            .field("details", &self.details)
            .field("has_error", &self.error.is_some())
            .finish()
    }
}

impl ErrorWidget {
    /// Create ErrorWidget with a message
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
            error: None,
        }
    }

    /// Create from an Error trait object
    pub fn from_error(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        let message = error.to_string();
        Self {
            message: message.clone(),
            details: None,
            error: Some(Arc::new(error)),
        }
    }

    /// Add error details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Get error message
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get error details
    pub fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    /// Get original error
    pub fn error(&self) -> Option<&(dyn std::error::Error + Send + Sync)> {
        self.error.as_ref().map(|e| &**e as &(dyn std::error::Error + Send + Sync))
    }
}

impl StatelessWidget for ErrorWidget {
    fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
        // TODO: Implement actual UI rendering
        // For now, return self to maintain widget tree structure
        // When flui_widgets is available, this should return a Container with Text
        Box::new(ErrorWidget {
            message: self.message.clone(),
            details: self.details.clone(),
            error: self.error.clone(),
        })
    }
}

// Widget trait is implemented automatically via blanket impl for StatelessWidget

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_widget_creation() {
        let widget = ErrorWidget::new("Test error");
        assert_eq!(widget.message(), "Test error");
        assert_eq!(widget.details(), None);
    }

    #[test]
    fn test_error_widget_with_details() {
        let widget = ErrorWidget::new("Test error")
            .with_details("Additional info");

        assert_eq!(widget.message(), "Test error");
        assert_eq!(widget.details(), Some("Additional info"));
    }

    #[test]
    fn test_error_widget_from_error() {
        use std::io;
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let widget = ErrorWidget::from_error(io_error);

        assert!(widget.message().contains("file not found"));
        assert!(widget.error().is_some());
    }

    #[test]
    fn test_error_widget_build() {
        let widget = ErrorWidget::new("Test error");
        let context = create_test_context();

        // Should return a widget
        let built = widget.build(&context);
        assert!(built.type_name().contains("Error"));
    }

    #[test]
    fn test_error_widget_clone() {
        let widget = ErrorWidget::new("Test error").with_details("Details");
        let cloned = widget.clone();

        assert_eq!(widget.message(), cloned.message());
        assert_eq!(widget.details(), cloned.details());
    }

    // Helper to create test context
    fn create_test_context() -> Context {
        use crate::element::ElementId;
        use crate::tree::ElementTree;
        use std::sync::{Arc, RwLock};

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();

        Context::new(element_id, tree)
    }
}
