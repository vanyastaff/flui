//! Debug assertions and error handling utilities.
//!
//! This module provides assertion macros for development-time checks that are
//! stripped in release builds.
//!
//! # Example
//!
//! ```rust
//! use flui_foundation::debug_assert_valid;
//!
//! fn layout_child(width: f64) {
//!     debug_assert_valid!(width >= 0.0, "Width must be non-negative, got {}", width);
//!     // ... layout logic
//! }
//! ```

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
}
