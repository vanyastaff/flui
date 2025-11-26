//! ErrorBoundary - Catches and displays errors in widget tree
//!
//! Analogous to Flutter's ErrorWidget/Error Boundary pattern.

use super::{ErrorInfo, ErrorWidget};
use crate::{BuildContext, Element};
use flui_view::{IntoElement, StatefulView, StatelessView};
use parking_lot::RwLock;
use std::sync::Arc;

/// ErrorBoundary state
#[derive(Debug)]
pub struct ErrorBoundaryState {
    /// Current error (if any)
    error: Arc<RwLock<Option<ErrorInfo>>>,
}

impl ErrorBoundaryState {
    /// Create new state
    fn new() -> Self {
        Self {
            error: Arc::new(RwLock::new(None)),
        }
    }

    /// Set error
    pub fn set_error(&self, error: ErrorInfo) {
        *self.error.write() = Some(error);
    }

    /// Clear error
    pub fn clear_error(&self) {
        *self.error.write() = None;
    }

    /// Get current error
    pub fn has_error(&self) -> bool {
        self.error.read().is_some()
    }

    /// Get error info
    pub fn get_error(&self) -> Option<ErrorInfo> {
        self.error.read().clone()
    }
}

/// Type alias for error handler callback
type ErrorHandler = Arc<dyn Fn(&ErrorInfo) + Send + Sync>;

/// ErrorBoundary - Catches and displays errors from child widgets
///
/// This widget wraps a child and displays an ErrorWidget if the child
/// throws an error during build/layout/paint.
///
/// Analogous to Flutter's ErrorWidget mechanism and React's Error Boundaries.
///
/// # Example
///
/// ```rust,ignore
/// ErrorBoundary::new(my_child_widget)
/// ```
///
/// # Architecture
///
/// ```text
/// ErrorBoundary
///   ├─ No error: Shows child widget
///   └─ Error: Shows ErrorWidget with error details
/// ```
pub struct ErrorBoundary {
    /// Child element to render (wrapped in Arc for sharing)
    child: Arc<RwLock<Option<Element>>>,

    /// Error handler callback (optional)
    /// Called when an error occurs
    on_error: Option<ErrorHandler>,
}

impl ErrorBoundary {
    /// Create a new error boundary
    ///
    /// # Parameters
    ///
    /// - `child`: Child element to wrap
    pub fn new(child: Element) -> Self {
        Self {
            child: Arc::new(RwLock::new(Some(child))),
            on_error: None,
        }
    }

    /// Set error handler callback
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ErrorBoundary::new(child)
    ///     .on_error(|error| {
    ///         eprintln!("Error caught: {}", error.message);
    ///     })
    /// ```
    #[must_use]
    pub fn on_error<F>(mut self, handler: F) -> Self
    where
        F: Fn(&ErrorInfo) + Send + Sync + 'static,
    {
        self.on_error = Some(Arc::new(handler));
        self
    }
}

impl std::fmt::Debug for ErrorBoundary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorBoundary")
            .field("has_child", &self.child.read().is_some())
            .field("has_handler", &self.on_error.is_some())
            .finish()
    }
}

impl StatefulView for ErrorBoundary {
    type State = ErrorBoundaryState;

    fn create_state(&self) -> Self::State {
        ErrorBoundaryState::new()
    }

    fn build(&self, state: &mut Self::State, _ctx: &dyn BuildContext) -> impl IntoElement {
        // Check if we have an error
        if let Some(error) = state.get_error() {
            // Call error handler if set
            if let Some(ref handler) = self.on_error {
                handler(&error);
            }

            // Show error widget
            return ErrorWidget::new(error).build(_ctx).into_element();
        }

        // No error - show child
        // Take child from Arc<RwLock<Option<Element>>>
        self.child
            .write()
            .take()
            .unwrap_or_else(|| {
                tracing::warn!("ErrorBoundary: child already taken, returning empty");
                Element::empty()
            })
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Find nearest ErrorBoundary ancestor and set error
///
/// This is a helper function for the framework to report errors to the
/// nearest error boundary in the tree.
///
/// # Implementation Note
///
/// This function needs access to the ElementTree to walk up the tree
/// and find ErrorBoundary elements. It should be called from PipelineOwner
/// or similar framework code that has tree access.
///
/// # Parameters
///
/// - `ctx`: Build context
/// - `error`: Error information
///
/// # Returns
///
/// Returns true if an ErrorBoundary was found and error was set.
pub fn report_error_to_boundary(ctx: &dyn BuildContext, error: ErrorInfo) -> bool {
    // TODO: Implement tree walking to find nearest ErrorBoundary
    // This requires access to ElementTree which isn't available from BuildContext
    // The framework should call this with proper tree access
    tracing::error!(
        message = %error.message,
        details = ?error.details,
        element_id = ?ctx.element_id(),
        "Error in widget tree (ErrorBoundary search not yet implemented)"
    );
    false
}

/// Handle panic in widget build
///
/// This is called by the framework when a panic occurs during build.
/// It creates an ErrorInfo from the panic payload.
///
/// # Parameters
///
/// - `panic_info`: Panic payload (usually &str or String)
///
/// # Returns
///
/// ErrorInfo suitable for displaying in ErrorWidget
pub fn handle_build_panic(panic_info: &dyn std::any::Any) -> ErrorInfo {
    // Try to extract panic message
    let message = if let Some(s) = panic_info.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_info.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown panic (no message)".to_string()
    };

    ErrorInfo::from_panic(message)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_boundary_state() {
        let state = ErrorBoundaryState::new();
        assert!(!state.has_error());

        state.set_error(ErrorInfo::from_panic("Test error".to_string()));
        assert!(state.has_error());

        let error = state.get_error().unwrap();
        assert_eq!(error.message, "Test error");

        state.clear_error();
        assert!(!state.has_error());
    }

    #[test]
    fn test_error_boundary_creation() {
        let boundary = ErrorBoundary::new(Element::empty());
        assert!(boundary.child.read().is_some());
        assert!(boundary.on_error.is_none());
    }

    #[test]
    fn test_error_boundary_with_handler() {
        let boundary = ErrorBoundary::new(Element::empty())
            .on_error(|error| {
                println!("Error: {}", error.message);
            });
        assert!(boundary.on_error.is_some());
    }
}
