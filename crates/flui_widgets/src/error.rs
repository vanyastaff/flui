//! Error types for flui_widgets
//!
//! This module defines error types for widget operations including build failures,
//! configuration errors, and constraint violations.

use flui_core::CoreError;
use std::borrow::Cow;
use thiserror::Error;

/// Widget-specific error type
///
/// All fallible widget operations return `Result<T, WidgetError>`.
#[derive(Error, Debug, Clone)]
pub enum WidgetError {
    /// Widget build failed
    #[error("Failed to build widget '{widget_name}': {reason}")]
    BuildFailed {
        /// Widget type name
        widget_name: &'static str,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Invalid widget configuration
    #[error("Invalid configuration for {widget_name}: {reason}")]
    InvalidConfiguration {
        /// Widget type name
        widget_name: &'static str,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Constraint violation
    #[error("Constraint violation in {widget_name}: {details}")]
    ConstraintViolation {
        /// Widget type name
        widget_name: &'static str,
        /// Details about the violation
        details: Cow<'static, str>,
    },

    /// Core error propagation
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Result type for widget operations
pub type Result<T> = std::result::Result<T, WidgetError>;

impl WidgetError {
    /// Create a build failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn build_failed(widget_name: &'static str, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::BuildFailed {
            widget_name,
            reason: reason.into(),
        }
    }

    /// Create an invalid configuration error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_configuration(
        widget_name: &'static str,
        reason: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::InvalidConfiguration {
            widget_name,
            reason: reason.into(),
        }
    }

    /// Create a constraint violation error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn constraint_violation(
        widget_name: &'static str,
        details: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::ConstraintViolation {
            widget_name,
            details: details.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_failed() {
        let err = WidgetError::build_failed("Container", "invalid child");
        assert!(err.to_string().contains("Container"));
        assert!(err.to_string().contains("invalid child"));
    }

    #[test]
    fn test_invalid_configuration() {
        let err = WidgetError::invalid_configuration("SizedBox", "negative width");
        assert!(err.to_string().contains("SizedBox"));
        assert!(err.to_string().contains("negative width"));
    }

    #[test]
    fn test_constraint_violation() {
        let err = WidgetError::constraint_violation("Padding", "width exceeds max");
        assert!(err.to_string().contains("Padding"));
        assert!(err.to_string().contains("width exceeds max"));
    }

    #[test]
    fn test_cow_string_static() {
        let err = WidgetError::build_failed("Test", "static");
        match err {
            WidgetError::BuildFailed { reason, .. } => {
                assert_eq!(reason, "static");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_cow_string_dynamic() {
        let dynamic = format!("dynamic {}", 42);
        let err = WidgetError::build_failed("Test", dynamic.clone());
        match err {
            WidgetError::BuildFailed { reason, .. } => {
                assert_eq!(reason.as_ref(), &dynamic);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
