//! ErrorWidget - displays exceptions gracefully
//!
//! This widget displays error messages with different styles for debug and release modes.
//! In debug mode, it shows a red background with detailed error information.
//! In release mode, it shows a simple gray box.
//!
//! # Phase 3.3: Enhanced Error Handling
//!
//! Added global error widget builder support for customizable error displays.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use crate::context::Context;
use crate::widget::DynWidget;
use crate::widget::traits::StatelessWidget;

/// Error details for error widget display (Phase 3.3)
///
/// Provides structured information about errors that occurred during
/// widget tree building.
#[derive(Debug, Clone)]
pub struct ErrorDetails {
    /// The error message
    pub exception: String,

    /// Context where the error occurred (e.g., "building ComponentElement<MyWidget>")
    pub context: String,

    /// Widget type that caused the error
    pub widget_type: String,

    /// Optional stack trace (if available)
    pub stack_trace: Option<String>,
}

impl ErrorDetails {
    /// Create new error details
    pub fn new(exception: String, context: String, widget_type: String) -> Self {
        Self {
            exception,
            context,
            widget_type,
            stack_trace: None,
        }
    }

    /// Add stack trace to error details (builder pattern)
    #[must_use]
    pub fn with_stack_trace(mut self, stack_trace: String) -> Self {
        self.stack_trace = Some(stack_trace);
        self
    }
}

impl fmt::Display for ErrorDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Error: {}", self.exception)?;
        writeln!(f, "Context: {}", self.context)?;
        writeln!(f, "Widget: {}", self.widget_type)?;
        if let Some(ref trace) = self.stack_trace {
            writeln!(f, "\nStack trace:\n{}", trace)?;
        }
        Ok(())
    }
}

/// Builder function type for creating error widgets (Phase 3.3)
///
/// This allows customizing how errors are displayed throughout the application.
pub type ErrorWidgetBuilder = Box<dyn Fn(ErrorDetails) -> Box<dyn DynWidget> + Send + Sync>;

/// Global error widget builder (Phase 3.3)
static ERROR_WIDGET_BUILDER: OnceLock<ErrorWidgetBuilder> = OnceLock::new();

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
#[derive(Debug, Clone)]
pub struct ErrorWidget {
    message: String,
    details: Option<String>,
    /// Arc allows cloning, but we ignore it in PartialEq/Hash
    /// since errors aren't comparable
    error: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl ErrorWidget {
    /// Create ErrorWidget with a message
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
            error: None,
        }
    }

    /// Create from an Error trait object
    #[must_use]
    pub fn from_error(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        let message = error.to_string();
        Self {
            message: message.clone(),
            details: None,
            error: Some(Arc::new(error)),
        }
    }

    /// Add error details (builder pattern)
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Get error message
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get error details
    #[must_use]
    pub fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    /// Get original error
    #[must_use]
    pub fn error(&self) -> Option<&(dyn std::error::Error + Send + Sync)> {
        self.error.as_ref().map(|e| &**e as &(dyn std::error::Error + Send + Sync))
    }

    /// Check if error details are present
    #[must_use]
    pub fn has_details(&self) -> bool {
        self.details.is_some()
    }

    /// Check if original error is present
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    // ========== Phase 3.3: Global Error Widget Builder ==========

    /// Set the global error widget builder (Phase 3.3)
    ///
    /// This allows customizing how errors are displayed throughout the application.
    /// Call this once at app startup to set your custom error widget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::{ErrorWidget, ErrorDetails};
    ///
    /// // Set custom error widget builder
    /// ErrorWidget::set_builder(Box::new(|details| {
    ///     Box::new(MyCustomErrorWidget::new(details))
    /// }));
    /// ```
    ///
    /// # Note
    ///
    /// This can only be set once. Subsequent calls will be ignored with a warning.
    pub fn set_builder(builder: ErrorWidgetBuilder) {
        if ERROR_WIDGET_BUILDER.set(builder).is_err() {
            // Note: Builder already set, subsequent calls are ignored
            // In debug builds, you might want to add logging here
        }
    }

    /// Get the global error widget builder (Phase 3.3)
    ///
    /// Returns the custom builder if set, otherwise returns the default builder
    /// which creates a standard ErrorWidget.
    #[must_use]
    pub fn builder() -> &'static ErrorWidgetBuilder {
        ERROR_WIDGET_BUILDER.get_or_init(|| {
            // Default builder - creates standard ErrorWidget
            Box::new(|details| {
                Box::new(ErrorWidget::new(details.exception)
                    .with_details(format!("{}\nWidget: {}", details.context, details.widget_type)))
            })
        })
    }

    /// Create an error widget from error details (Phase 3.3)
    ///
    /// This uses the global error widget builder to create the widget.
    /// Use this when you want to use the custom error widget builder.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::ErrorDetails;
    ///
    /// let details = ErrorDetails::new(
    ///     "Failed to load data".to_string(),
    ///     "building MyWidget".to_string(),
    ///     "MyWidget".to_string(),
    /// );
    ///
    /// let error_widget = ErrorWidget::from_details(details);
    /// ```
    #[must_use]
    pub fn from_details(details: ErrorDetails) -> Box<dyn DynWidget> {
        (Self::builder())(details)
    }
}

// ========== Trait Implementations ==========

impl PartialEq for ErrorWidget {
    /// Compare ErrorWidgets by message and details
    ///
    /// Note: Ignores the `error` field since trait objects can't be compared
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message && self.details == other.details
    }
}

impl Eq for ErrorWidget {}

impl Hash for ErrorWidget {
    /// Hash by message and details only
    ///
    /// Note: Ignores the `error` field since trait objects can't be hashed
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.message.hash(state);
        self.details.hash(state);
    }
}

impl StatelessWidget for ErrorWidget {
    fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
        // TODO: Implement actual UI rendering
        // For now, return self to maintain widget tree structure
        // When flui_widgets is available, this should return a Container with Text
        Box::new(Self {
            message: self.message.clone(),
            details: self.details.clone(),
            error: self.error.clone(),
        })
    }
}

impl fmt::Display for ErrorWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error: {}", self.message)?;
        if let Some(details) = &self.details {
            write!(f, " ({})", details)?;
        }
        Ok(())
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
        assert!(!widget.has_details());
        assert!(!widget.has_error());
    }

    #[test]
    fn test_error_widget_with_details() {
        let widget = ErrorWidget::new("Test error")
            .with_details("Additional info");

        assert_eq!(widget.message(), "Test error");
        assert_eq!(widget.details(), Some("Additional info"));
        assert!(widget.has_details());
    }

    #[test]
    fn test_error_widget_from_error() {
        use std::io;
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let widget = ErrorWidget::from_error(io_error);

        assert!(widget.message().contains("file not found"));
        assert!(widget.has_error());
    }

    #[test]
    fn test_error_widget_equality() {
        let widget1 = ErrorWidget::new("Error").with_details("Details");
        let widget2 = ErrorWidget::new("Error").with_details("Details");
        let widget3 = ErrorWidget::new("Different");

        assert_eq!(widget1, widget2);
        assert_ne!(widget1, widget3);
    }

    #[test]
    fn test_error_widget_hash() {
        use std::collections::HashSet;

        let widget1 = ErrorWidget::new("Error 1");
        let widget2 = ErrorWidget::new("Error 2");
        let widget3 = ErrorWidget::new("Error 1"); // Same as widget1

        let mut set = HashSet::new();
        set.insert(widget1.clone());
        set.insert(widget2);

        assert!(set.contains(&widget3)); // Should find widget1 equivalent
    }

    #[test]
    fn test_error_widget_display() {
        let widget = ErrorWidget::new("Test error");
        assert_eq!(widget.to_string(), "Error: Test error");

        let widget = ErrorWidget::new("Test error")
            .with_details("Extra info");
        assert_eq!(widget.to_string(), "Error: Test error (Extra info)");
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

        assert_eq!(widget, cloned);
        assert_eq!(widget.message(), cloned.message());
        assert_eq!(widget.details(), cloned.details());
    }

    // Helper to create test context
    fn create_test_context() -> Context {
        use crate::ElementId;
        use crate::tree::ElementTree;
        use std::sync::Arc;
        use parking_lot::RwLock;

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();

        Context::new(tree, element_id)
    }
}