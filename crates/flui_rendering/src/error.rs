//! Error types for flui_rendering
//!
//! This module defines error types for rendering operations including layout failures,
//! paint errors, and parent data issues.
//!
//! # Tracing Integration
//!
//! All error creation methods automatically emit tracing events at the appropriate level,
//! providing observability without requiring manual logging at call sites.

use flui_foundation::ElementId;
use std::borrow::Cow;
use thiserror::Error;
use tracing::{error, warn};

/// Rendering-specific error type
///
/// All fallible rendering operations return `Result<T, RenderError>`.
#[derive(Error, Debug, Clone)]
pub enum RenderError {
    /// Layout computation failed
    #[error("Layout failed for {render_object}: {reason}")]
    LayoutFailed {
        /// Render object type name
        render_object: &'static str,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Paint operation failed
    #[error("Paint failed for {render_object}: {reason}")]
    PaintFailed {
        /// Render object type name
        render_object: &'static str,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Invalid parent data
    #[error("Invalid parent data: expected {expected}, got {actual}")]
    InvalidParentData {
        /// Expected parent data type
        expected: &'static str,
        /// Actual parent data type
        actual: &'static str,
    },

    /// Constraint violation
    #[error("Constraint violation: {details}")]
    ConstraintViolation {
        /// Violation details
        details: Cow<'static, str>,
    },

    /// Element not found
    #[error("Element not found: {0:?}")]
    ElementNotFound(ElementId),

    /// Not a render element
    #[error("Element {0:?} is not a render element")]
    NotRenderElement(ElementId),

    /// Hit test failed
    #[error("Hit test failed: {reason}")]
    HitTestFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Unsupported protocol
    #[error("Unsupported protocol: expected {expected}, found {found}")]
    UnsupportedProtocol {
        /// Expected protocol
        expected: &'static str,
        /// Found protocol
        found: &'static str,
    },

    /// Protocol mismatch (dynamic protocol detection)
    #[error("Protocol mismatch: expected {expected}, got {actual}")]
    ProtocolMismatch {
        /// Expected protocol
        expected: String,
        /// Actual protocol
        actual: String,
    },

    /// Generic layout error (for simple error messages)
    #[error("Layout error: {0}")]
    Layout(String),

    /// Arity validation error (child count constraints)
    #[error("Arity error: {0}")]
    Arity(#[from] flui_tree::ArityError),
}

/// Result type for rendering operations
pub type Result<T> = std::result::Result<T, RenderError>;

impl RenderError {
    /// Create a layout failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    /// Automatically emits a tracing error event.
    #[must_use]
    pub fn layout_failed(
        render_object: &'static str,
        reason: impl Into<Cow<'static, str>>,
    ) -> Self {
        let reason = reason.into();
        error!(
            render_object = render_object,
            reason = %reason,
            "layout failed"
        );
        Self::LayoutFailed {
            render_object,
            reason,
        }
    }

    /// Create a paint failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    /// Automatically emits a tracing error event.
    #[must_use]
    pub fn paint_failed(render_object: &'static str, reason: impl Into<Cow<'static, str>>) -> Self {
        let reason = reason.into();
        error!(
            render_object = render_object,
            reason = %reason,
            "paint failed"
        );
        Self::PaintFailed {
            render_object,
            reason,
        }
    }

    /// Create an invalid parent data error
    ///
    /// Automatically emits a tracing error event.
    #[must_use]
    pub fn invalid_parent_data(expected: &'static str, actual: &'static str) -> Self {
        error!(
            expected = expected,
            actual = actual,
            "invalid parent data type"
        );
        Self::InvalidParentData { expected, actual }
    }

    /// Create a constraint violation error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    /// Automatically emits a tracing warning event (violations may be recoverable).
    #[must_use]
    pub fn constraint_violation(details: impl Into<Cow<'static, str>>) -> Self {
        let details = details.into();
        warn!(details = %details, "constraint violation");
        Self::ConstraintViolation { details }
    }

    /// Create an element not found error
    ///
    /// Automatically emits a tracing error event.
    #[must_use]
    pub fn element_not_found(id: ElementId) -> Self {
        error!(element_id = %id.get(), "element not found");
        Self::ElementNotFound(id)
    }

    /// Create a not render element error
    ///
    /// Automatically emits a tracing error event.
    #[must_use]
    pub fn not_render_element(id: ElementId) -> Self {
        error!(element_id = %id.get(), "element is not a render element");
        Self::NotRenderElement(id)
    }

    /// Create a hit test failed error
    ///
    /// Automatically emits a tracing warning event (hit test failures are often expected).
    #[must_use]
    pub fn hit_test_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        let reason = reason.into();
        warn!(reason = %reason, "hit test failed");
        Self::HitTestFailed { reason }
    }

    /// Returns true if this error is recoverable and should not abort rendering.
    ///
    /// Recoverable errors include constraint violations, hit test failures,
    /// and arity errors, which can often be handled gracefully by using fallback values.
    #[must_use]
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ConstraintViolation { .. } | Self::HitTestFailed { .. } | Self::Arity(_)
        )
    }

    /// Returns the element ID associated with this error, if any.
    #[must_use]
    pub fn element_id(&self) -> Option<ElementId> {
        match self {
            Self::ElementNotFound(id) | Self::NotRenderElement(id) => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_failed() {
        let err = RenderError::layout_failed("RenderFlex", "invalid constraints");
        assert!(err.to_string().contains("RenderFlex"));
        assert!(err.to_string().contains("invalid constraints"));
    }

    #[test]
    fn test_paint_failed() {
        let err = RenderError::paint_failed("RenderOpacity", "invalid alpha");
        assert!(err.to_string().contains("RenderOpacity"));
        assert!(err.to_string().contains("invalid alpha"));
    }

    #[test]
    fn test_invalid_parent_data() {
        let err = RenderError::invalid_parent_data("FlexParentData", "StackParentData");
        assert!(err.to_string().contains("FlexParentData"));
        assert!(err.to_string().contains("StackParentData"));
    }

    #[test]
    fn test_constraint_violation() {
        let err = RenderError::constraint_violation("min width exceeds max width");
        assert!(err.to_string().contains("min width exceeds max width"));
    }

    #[test]
    fn test_element_not_found() {
        let err = RenderError::element_not_found(ElementId::new(42));
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn test_is_recoverable() {
        // Recoverable errors
        assert!(RenderError::constraint_violation("test").is_recoverable());
        assert!(RenderError::hit_test_failed("test").is_recoverable());

        // Non-recoverable errors
        assert!(!RenderError::layout_failed("Test", "test").is_recoverable());
        assert!(!RenderError::paint_failed("Test", "test").is_recoverable());
        assert!(!RenderError::element_not_found(ElementId::new(1)).is_recoverable());
    }

    #[test]
    fn test_element_id_extraction() {
        let id = ElementId::new(42);

        assert_eq!(RenderError::element_not_found(id).element_id(), Some(id));
        assert_eq!(RenderError::not_render_element(id).element_id(), Some(id));
        assert_eq!(
            RenderError::layout_failed("Test", "test").element_id(),
            None
        );
    }
}
