//! Error types for flui_rendering
//!
//! This module defines error types for rendering operations including layout failures,
//! paint errors, and parent data issues.

use flui_core::CoreError;
use std::borrow::Cow;
use thiserror::Error;

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

    /// Core error propagation
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Result type for rendering operations
pub type Result<T> = std::result::Result<T, RenderError>;

impl RenderError {
    /// Create a layout failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn layout_failed(
        render_object: &'static str,
        reason: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::LayoutFailed {
            render_object,
            reason: reason.into(),
        }
    }

    /// Create a paint failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn paint_failed(render_object: &'static str, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::PaintFailed {
            render_object,
            reason: reason.into(),
        }
    }

    /// Create an invalid parent data error
    #[must_use]
    pub fn invalid_parent_data(expected: &'static str, actual: &'static str) -> Self {
        Self::InvalidParentData { expected, actual }
    }

    /// Create a constraint violation error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn constraint_violation(details: impl Into<Cow<'static, str>>) -> Self {
        Self::ConstraintViolation {
            details: details.into(),
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
}
