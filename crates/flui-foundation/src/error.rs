//! Error types for FLUI Foundation
//!
//! This module provides standardized error handling for foundation operations.
//! All errors are designed to be composable and provide rich debugging information.

use std::fmt;
use thiserror::Error;

/// The main error type for FLUI Foundation operations.
///
/// This type provides comprehensive error information with support for
/// error chaining and debugging context.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{FoundationError, Result};
///
/// fn example_operation() -> Result<()> {
///     Err(FoundationError::InvalidId {
///         id: 0,
///         context: "ElementId cannot be zero".into()
///     })
/// }
/// ```
#[derive(Error, Debug, Clone)]
#[must_use = "errors should be handled or propagated"]
pub enum FoundationError {
    /// An invalid ID was provided.
    #[error("Invalid ID: {id} - {context}")]
    InvalidId {
        /// The invalid ID value
        id: u64,
        /// Additional context about why the ID is invalid
        context: String,
    },

    /// An invalid key was provided.
    #[error("Invalid key: {context}")]
    InvalidKey {
        /// Context about the invalid key
        context: String,
    },

    /// A listener operation failed.
    #[error("Listener error: {operation} - {context}")]
    ListenerError {
        /// The operation that failed (add, remove, notify)
        operation: String,
        /// Additional context
        context: String,
    },

    /// A diagnostics operation failed.
    #[error("Diagnostics error: {context}")]
    DiagnosticsError {
        /// Context about the diagnostics failure
        context: String,
    },

    /// A notification operation failed.
    #[error("Notification error: {notification_type} - {context}")]
    NotificationError {
        /// The type of notification that failed
        notification_type: String,
        /// Additional context
        context: String,
    },

    /// An atomic operation failed.
    #[error("Atomic operation failed: {operation} - {context}")]
    AtomicError {
        /// The atomic operation that failed
        operation: String,
        /// Additional context
        context: String,
    },

    /// Serialization or deserialization failed.
    #[cfg(feature = "serde")]
    #[error("Serialization error: {context}")]
    SerializationError {
        /// Context about the serialization failure
        context: String,
    },

    /// A generic foundation error with custom message.
    #[error("Foundation error: {message}")]
    Generic {
        /// The error message
        message: String,
    },
}

impl FoundationError {
    /// Creates a new invalid ID error.
    pub fn invalid_id(id: u64, context: impl Into<String>) -> Self {
        Self::InvalidId {
            id,
            context: context.into(),
        }
    }

    /// Creates a new invalid key error.
    pub fn invalid_key(context: impl Into<String>) -> Self {
        Self::InvalidKey {
            context: context.into(),
        }
    }

    /// Creates a new listener error.
    pub fn listener_error(operation: impl Into<String>, context: impl Into<String>) -> Self {
        Self::ListenerError {
            operation: operation.into(),
            context: context.into(),
        }
    }

    /// Creates a new diagnostics error.
    pub fn diagnostics_error(context: impl Into<String>) -> Self {
        Self::DiagnosticsError {
            context: context.into(),
        }
    }

    /// Creates a new notification error.
    pub fn notification_error(
        notification_type: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::NotificationError {
            notification_type: notification_type.into(),
            context: context.into(),
        }
    }

    /// Creates a new atomic error.
    pub fn atomic_error(operation: impl Into<String>, context: impl Into<String>) -> Self {
        Self::AtomicError {
            operation: operation.into(),
            context: context.into(),
        }
    }

    /// Creates a new serialization error.
    #[cfg(feature = "serde")]
    pub fn serialization_error(context: impl Into<String>) -> Self {
        Self::SerializationError {
            context: context.into(),
        }
    }

    /// Creates a new generic error.
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic {
            message: message.into(),
        }
    }

    /// Returns the error category as a string.
    #[must_use]
    pub const fn category(&self) -> &'static str {
        match self {
            Self::InvalidId { .. } => "invalid_id",
            Self::InvalidKey { .. } => "invalid_key",
            Self::ListenerError { .. } => "listener",
            Self::DiagnosticsError { .. } => "diagnostics",
            Self::NotificationError { .. } => "notification",
            Self::AtomicError { .. } => "atomic",
            #[cfg(feature = "serde")]
            Self::SerializationError { .. } => "serialization",
            Self::Generic { .. } => "generic",
        }
    }

    /// Returns whether this error is recoverable.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        match self {
            Self::InvalidId { .. } => false,        // Programming error
            Self::InvalidKey { .. } => false,       // Programming error
            Self::ListenerError { .. } => true,     // Can retry listener operations
            Self::DiagnosticsError { .. } => true,  // Diagnostics failures are non-critical
            Self::NotificationError { .. } => true, // Can retry notifications
            Self::AtomicError { .. } => true,       // Can retry atomic operations
            #[cfg(feature = "serde")]
            Self::SerializationError { .. } => false, // Data format issue
            Self::Generic { .. } => true,           // Depends on context, default to recoverable
        }
    }
}

/// A type alias for `Result<T, FoundationError>`.
///
/// This is the standard result type used throughout FLUI Foundation.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::Result;
///
/// fn example() -> Result<i32> {
///     Ok(42)
/// }
/// ```
pub type Result<T> = std::result::Result<T, FoundationError>;

// ============================================================================
// ERROR CONVERSION UTILITIES
// ============================================================================

/// Provides convenient error conversion utilities.
pub trait ErrorContext<T> {
    /// Adds context to an error result.
    ///
    /// # Errors
    ///
    /// Returns a `FoundationError::Generic` with the context prepended to the original error.
    fn with_context(self, context: impl Into<String>) -> Result<T>;

    /// Adds context to an error result using a closure.
    ///
    /// # Errors
    ///
    /// Returns a `FoundationError::Generic` with the context prepended to the original error.
    fn with_context_fn<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T, E> ErrorContext<T> for std::result::Result<T, E>
where
    E: fmt::Display,
{
    fn with_context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|e| FoundationError::generic(format!("{}: {}", context.into(), e)))
    }

    fn with_context_fn<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| FoundationError::generic(format!("{}: {}", f(), e)))
    }
}

impl<T> ErrorContext<T> for Option<T> {
    fn with_context(self, context: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| FoundationError::generic(context.into()))
    }

    fn with_context_fn<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.ok_or_else(|| FoundationError::generic(f()))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = FoundationError::invalid_id(0, "zero is not allowed");
        assert_eq!(err.category(), "invalid_id");
        assert!(!err.is_recoverable());
        assert!(err.to_string().contains("Invalid ID: 0"));
    }

    #[test]
    fn test_error_context() {
        let result: std::result::Result<(), &str> = Err("original error");
        let with_context = result.with_context("additional context");

        assert!(with_context.is_err());
        let error_str = with_context.unwrap_err().to_string();
        assert!(error_str.contains("additional context"));
        assert!(error_str.contains("original error"));
    }

    #[test]
    fn test_option_context() {
        let option: Option<i32> = None;
        let result = option.with_context("value was None");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("value was None"));
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            FoundationError::invalid_id(1, "test").category(),
            "invalid_id"
        );
        assert_eq!(
            FoundationError::invalid_key("test").category(),
            "invalid_key"
        );
        assert_eq!(
            FoundationError::listener_error("add", "test").category(),
            "listener"
        );
        assert_eq!(
            FoundationError::diagnostics_error("test").category(),
            "diagnostics"
        );
    }

    #[test]
    fn test_recoverable_errors() {
        assert!(!FoundationError::invalid_id(0, "test").is_recoverable());
        assert!(!FoundationError::invalid_key("test").is_recoverable());
        assert!(FoundationError::listener_error("add", "test").is_recoverable());
        assert!(FoundationError::diagnostics_error("test").is_recoverable());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_error() {
        let err = FoundationError::serialization_error("failed to serialize");
        assert_eq!(err.category(), "serialization");
        assert!(!err.is_recoverable());
    }
}
