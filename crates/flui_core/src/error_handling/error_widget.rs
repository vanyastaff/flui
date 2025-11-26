//! ErrorWidget - Widget that displays error information
//!
//! Analogous to Flutter's ErrorWidget, this widget displays error details
//! when a build error occurs.

use crate::{BuildContext, Element};
use flui_view::{IntoElement, StatelessView};

/// Error information displayed by ErrorWidget
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    /// Error message
    pub message: String,

    /// Optional stack trace or additional details
    pub details: Option<String>,

    /// Whether this is a debug build (show more details)
    pub is_debug: bool,
}

impl ErrorInfo {
    /// Create error info from a panic message
    pub fn from_panic(msg: String) -> Self {
        Self {
            message: msg,
            details: None,
            is_debug: cfg!(debug_assertions),
        }
    }

    /// Create error info with details
    pub fn with_details(msg: String, details: String) -> Self {
        Self {
            message: msg,
            details: Some(details),
            is_debug: cfg!(debug_assertions),
        }
    }
}

/// ErrorWidget - Displays error information
///
/// This widget is shown when a build error occurs in an ErrorBoundary.
/// Analogous to Flutter's ErrorWidget.
///
/// # Example
///
/// ```rust,ignore
/// ErrorWidget::new(ErrorInfo::from_panic("Widget build failed".to_string()))
/// ```
#[derive(Debug, Clone)]
pub struct ErrorWidget {
    /// Error information
    error: ErrorInfo,
}

impl ErrorWidget {
    /// Create a new error widget
    pub fn new(error: ErrorInfo) -> Self {
        Self { error }
    }
}

impl StatelessView for ErrorWidget {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        // Log error with full details
        if self.error.is_debug {
            tracing::error!(
                message = %self.error.message,
                details = ?self.error.details,
                "╔══════════════════════════════════════════════════════════════"
            );
            tracing::error!("║ ERROR IN WIDGET TREE");
            tracing::error!("║ {}", self.error.message);
            if let Some(ref details) = self.error.details {
                tracing::error!("║");
                tracing::error!("║ Details:");
                for line in details.lines() {
                    tracing::error!("║   {}", line);
                }
            }
            tracing::error!("╚══════════════════════════════════════════════════════════════");
        } else {
            tracing::error!(
                message = %self.error.message,
                "Error in widget tree"
            );
        }

        // TODO: Replace with RenderErrorBox when render integration is ready
        // For now, return empty element
        // The visual error display will be implemented in a future update
        Element::empty()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_info_from_panic() {
        let info = ErrorInfo::from_panic("Test panic".to_string());
        assert_eq!(info.message, "Test panic");
        assert!(info.details.is_none());
    }

    #[test]
    fn test_error_info_with_details() {
        let info =
            ErrorInfo::with_details("Test error".to_string(), "Stack trace here".to_string());
        assert_eq!(info.message, "Test error");
        assert_eq!(info.details, Some("Stack trace here".to_string()));
    }

    #[test]
    fn test_error_widget_creation() {
        let widget = ErrorWidget::new(ErrorInfo::from_panic("Test".to_string()));
        assert_eq!(widget.error.message, "Test");
    }
}
