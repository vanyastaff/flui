//! Debug assertions and error handling utilities.
//!
//! This module provides assertion macros and error types for development-time
//! checks that are stripped in release builds.
//!
//! # Example
//!
//! ```rust
//! use flui_foundation::assert::{FluiError};
//! use flui_foundation::debug_assert_valid;
//!
//! fn layout_child(width: f64) {
//!     debug_assert_valid!(width >= 0.0, "Width must be non-negative, got {}", width);
//!     // ... layout logic
//! }
//! ```

use std::fmt;

/// A rich diagnostic error with contextual information.
///
/// `FluiError` captures not just an error message but also contextual
/// information about where and why the error occurred.
///
/// # Example
///
/// ```rust
/// use flui_foundation::assert::FluiError;
///
/// fn validate_constraints(min: f64, max: f64) -> Result<(), FluiError> {
///     if min > max {
///         return Err(FluiError::new(
///             "Invalid constraints",
///             format!("min ({}) cannot be greater than max ({})", min, max),
///         ));
///     }
///     Ok(())
/// }
/// ```
#[derive(Clone)]
#[must_use = "errors should be handled or reported"]
pub struct FluiError {
    /// Short summary of the error
    summary: String,
    /// Detailed message explaining the error
    message: String,
    /// Optional stack of contextual information
    context: Vec<String>,
    /// Optional library/module where error originated
    library: Option<String>,
}

impl FluiError {
    /// Create a new `FluiError` with summary and message.
    pub fn new(summary: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            message: message.into(),
            context: Vec::new(),
            library: None,
        }
    }

    /// Create an error from just a message (summary is derived).
    pub fn from_message(message: impl Into<String>) -> Self {
        let msg = message.into();
        let summary = msg.lines().next().unwrap_or(&msg).to_string();
        Self {
            summary,
            message: msg,
            context: Vec::new(),
            library: None,
        }
    }

    /// Add contextual information to the error.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context.push(context.into());
        self
    }

    /// Set the library/module name where this error originated.
    pub fn with_library(mut self, library: impl Into<String>) -> Self {
        self.library = Some(library.into());
        self
    }

    /// Get the error summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Get the full error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the context stack.
    #[must_use]
    pub fn context(&self) -> &[String] {
        &self.context
    }

    /// Get the library name if set.
    #[must_use]
    pub fn library(&self) -> Option<&str> {
        self.library.as_deref()
    }

    /// Format the error with full diagnostic information.
    #[must_use]
    pub fn to_diagnostic_string(&self) -> String {
        use std::fmt::Write;

        let mut output = String::new();

        // Header
        output.push_str("══════════════════════════════════════════════════════════════\n");
        if let Some(lib) = &self.library {
            let _ = writeln!(output, "Exception caught by {lib} library");
        }
        let _ = writeln!(output, "  {}", self.summary);
        output.push_str("══════════════════════════════════════════════════════════════\n");

        // Message
        output.push_str(&self.message);
        output.push('\n');

        // Context
        if !self.context.is_empty() {
            output.push_str("\nContext:\n");
            for (i, ctx) in self.context.iter().enumerate() {
                let _ = writeln!(output, "  {}. {ctx}", i + 1);
            }
        }

        output.push_str("══════════════════════════════════════════════════════════════\n");
        output
    }
}

impl fmt::Debug for FluiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FluiError")
            .field("summary", &self.summary)
            .field("message", &self.message)
            .field("context", &self.context)
            .field("library", &self.library)
            .finish()
    }
}

impl fmt::Display for FluiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            // Use {:#} for full diagnostic output
            write!(f, "{}", self.to_diagnostic_string())
        } else {
            // Default: just summary and message
            write!(f, "{}: {}", self.summary, self.message)
        }
    }
}

impl std::error::Error for FluiError {}

/// Error handler type for catching `FluiError`s.
pub type ErrorHandler = Box<dyn Fn(&FluiError) + Send + Sync>;

/// Debug-only assertion that validates a condition with a formatted message.
///
/// This macro is a no-op in release builds, allowing expensive checks
/// during development without runtime cost in production.
///
/// # Example
///
/// ```rust
/// use flui_foundation::debug_assert_valid;
///
/// fn process_value(value: i32) {
///     debug_assert_valid!(value > 0, "Value must be positive, got {}", value);
///     debug_assert_valid!(value < 100, "Value must be less than 100");
/// }
///
/// process_value(50); // OK
/// ```
#[macro_export]
macro_rules! debug_assert_valid {
    ($cond:expr, $($arg:tt)+) => {
        if cfg!(debug_assertions) && !$cond {
            panic!($($arg)+);
        }
    };
    ($cond:expr) => {
        if cfg!(debug_assertions) && !$cond {
            panic!(concat!("Assertion failed: ", stringify!($cond)));
        }
    };
}

/// Debug-only assertion that a value is within a range.
///
/// # Example
///
/// ```rust
/// use flui_foundation::debug_assert_range;
///
/// fn set_opacity(value: f64) {
///     debug_assert_range!(value, 0.0..=1.0, "opacity");
///     // ... set opacity
/// }
///
/// set_opacity(0.5); // OK
/// ```
#[macro_export]
macro_rules! debug_assert_range {
    ($value:expr, $range:expr, $name:expr) => {
        if cfg!(debug_assertions) {
            let value = $value;
            let range = $range;
            assert!(
                range.contains(&value),
                "{} must be in range {:?}, got {}",
                $name,
                range,
                value
            );
        }
    };
}

/// Debug-only assertion that a value is finite (not NaN or infinite).
///
/// # Example
///
/// ```rust
/// use flui_foundation::debug_assert_finite;
///
/// fn set_size(width: f64, height: f64) {
///     debug_assert_finite!(width, "width");
///     debug_assert_finite!(height, "height");
/// }
///
/// set_size(100.0, 200.0); // OK
/// ```
#[macro_export]
macro_rules! debug_assert_finite {
    ($value:expr, $name:expr) => {
        if cfg!(debug_assertions) {
            let value: f64 = $value;
            assert!(value.is_finite(), "{} must be finite, got {}", $name, value);
        }
    };
}

/// Debug-only assertion that a value is not NaN.
///
/// # Example
///
/// ```rust
/// use flui_foundation::debug_assert_not_nan;
///
/// fn calculate_ratio(a: f64, b: f64) -> f64 {
///     let result = a / b;
///     debug_assert_not_nan!(result, "ratio");
///     result
/// }
///
/// let r = calculate_ratio(10.0, 2.0); // OK, returns 5.0
/// ```
#[macro_export]
macro_rules! debug_assert_not_nan {
    ($value:expr, $name:expr) => {
        if cfg!(debug_assertions) {
            let value: f64 = $value;
            assert!(!value.is_nan(), "{} must not be NaN", $name);
        }
    };
}

/// Report a non-fatal error during development.
///
/// In debug mode, this logs the error. In release mode, it's a no-op.
/// Use this for recoverable errors that indicate bugs but shouldn't crash.
///
/// # Example
///
/// ```rust
/// use flui_foundation::report_error;
///
/// fn try_load_config() -> Option<String> {
///     let result: Result<String, &str> = Err("file not found");
///     match result {
///         Ok(config) => Some(config),
///         Err(e) => {
///             report_error!("Failed to load config: {}", e);
///             None // Return default
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! report_error {
    ($($arg:tt)+) => {
        if cfg!(debug_assertions) {
            tracing::error!($($arg)+);
        }
    };
}

/// Report a non-fatal warning during development.
#[macro_export]
macro_rules! report_warning {
    ($($arg:tt)+) => {
        if cfg!(debug_assertions) {
            tracing::warn!($($arg)+);
        }
    };
}

// Re-export macros at module level
pub use crate::{
    debug_assert_finite, debug_assert_not_nan, debug_assert_range, debug_assert_valid,
    report_error, report_warning,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flui_error_creation() {
        let error = FluiError::new("Test Error", "This is a test message");
        assert_eq!(error.summary(), "Test Error");
        assert_eq!(error.message(), "This is a test message");
        assert!(error.context().is_empty());
        assert!(error.library().is_none());
    }

    #[test]
    fn test_flui_error_with_context() {
        let error = FluiError::new("Layout Error", "Invalid constraints")
            .with_context("During measure phase")
            .with_context("In Container widget")
            .with_library("rendering");

        assert_eq!(error.context().len(), 2);
        assert_eq!(error.library(), Some("rendering"));
    }

    #[test]
    fn test_flui_error_display() {
        let error = FluiError::new("Test", "Message");
        assert_eq!(format!("{}", error), "Test: Message");
    }

    #[test]
    fn test_flui_error_from_message() {
        let error = FluiError::from_message("First line\nSecond line");
        assert_eq!(error.summary(), "First line");
        assert_eq!(error.message(), "First line\nSecond line");
    }

    #[test]
    fn test_debug_assert_valid() {
        debug_assert_valid!(true, "This should not panic");
        debug_assert_valid!(1 + 1 == 2);
    }

    #[test]
    fn test_debug_assert_range() {
        debug_assert_range!(0.5, 0.0..=1.0, "opacity");
        debug_assert_range!(50, 0..100, "percentage");
    }

    #[test]
    fn test_debug_assert_finite() {
        debug_assert_finite!(1.0, "value");
        debug_assert_finite!(0.0, "zero");
        debug_assert_finite!(-100.0, "negative");
    }

    #[test]
    fn test_debug_assert_not_nan() {
        debug_assert_not_nan!(1.0, "value");
        debug_assert_not_nan!(f64::INFINITY, "infinity"); // Infinity is not NaN
    }

    #[test]
    fn test_diagnostic_string() {
        let error = FluiError::new("RenderBox overflow", "A RenderBox overflowed")
            .with_context("During layout")
            .with_library("rendering");

        let diagnostic = error.to_diagnostic_string();
        assert!(diagnostic.contains("RenderBox overflow"));
        assert!(diagnostic.contains("rendering library"));
        assert!(diagnostic.contains("During layout"));
    }
}
