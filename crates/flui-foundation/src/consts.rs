//! Compile-time constants for build modes and platform detection.
//!
//! These constants are resolved at compile time, enabling dead code elimination.
//!
//! # Example
//!
//! ```rust
//! use flui_foundation::consts::{DEBUG_MODE, RELEASE_MODE};
//!
//! if DEBUG_MODE {
//!     println!("Debug assertions enabled");
//! }
//!
//! // This branch is eliminated in release builds
//! if !RELEASE_MODE {
//!     // expensive debug checks
//! }
//! ```

/// True when compiling in debug mode (with debug assertions).
///
/// This is the inverse of [`RELEASE_MODE`]. Use this to gate
/// expensive debugging code that should be stripped in production.
///
/// # Example
///
/// ```rust
/// use flui_foundation::consts::DEBUG_MODE;
///
/// fn validate_expensive(data: &[u8]) {
///     if DEBUG_MODE {
///         // O(n²) validation only in debug builds
///         assert!(data.windows(2).all(|w| w[0] <= w[1]), "Data must be sorted");
///     }
/// }
/// ```
pub const DEBUG_MODE: bool = cfg!(debug_assertions);

/// True when compiling in release mode (without debug assertions).
///
/// This is the inverse of [`DEBUG_MODE`]. Use this to enable
/// production-only optimizations.
pub const RELEASE_MODE: bool = !cfg!(debug_assertions);

/// True when compiling for WebAssembly targets.
///
/// Use this to gate web-specific code paths.
///
/// # Example
///
/// ```rust
/// use flui_foundation::consts::IS_WEB;
///
/// fn get_storage_path() -> &'static str {
///     if IS_WEB {
///         "/indexeddb"
///     } else {
///         "/local/storage"
///     }
/// }
/// ```
pub const IS_WEB: bool = cfg!(target_family = "wasm");

/// True when compiling for mobile platforms (Android or iOS).
pub const IS_MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// True when compiling for desktop platforms (Windows, macOS, Linux).
pub const IS_DESKTOP: bool = cfg!(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux"
));

/// Precision tolerance for floating-point comparisons (f64).
///
/// When comparing floating-point numbers for equality, use this tolerance
/// to account for precision errors. Two values `a` and `b` are considered
/// equal if `(a - b).abs() < EPSILON`.
pub const EPSILON: f64 = 1e-10;

/// Single-precision tolerance for f32 comparisons.
pub const EPSILON_F32: f32 = 1e-6;

/// Check if two f64 values are approximately equal within [`EPSILON`].
///
/// # Example
///
/// ```rust
/// use flui_foundation::consts::approx_equal;
///
/// let a = 0.1 + 0.2;
/// let b = 0.3;
/// assert!(approx_equal(a, b)); // true despite floating-point errors
/// ```
#[inline]
#[must_use]
pub const fn approx_equal(a: f64, b: f64) -> bool {
    let diff = a - b;
    // Manual abs since f64::abs is not const
    let abs_diff = if diff < 0.0 { -diff } else { diff };
    abs_diff < EPSILON
}

/// Check if two f32 values are approximately equal within [`EPSILON_F32`].
#[inline]
#[must_use]
pub const fn approx_equal_f32(a: f32, b: f32) -> bool {
    let diff = a - b;
    let abs_diff = if diff < 0.0 { -diff } else { diff };
    abs_diff < EPSILON_F32
}

/// Check if a f64 value is approximately zero.
#[inline]
#[must_use]
pub const fn is_near_zero(value: f64) -> bool {
    let abs_value = if value < 0.0 { -value } else { value };
    abs_value < EPSILON
}

/// Check if a f32 value is approximately zero.
#[inline]
#[must_use]
pub const fn is_near_zero_f32(value: f32) -> bool {
    let abs_value = if value < 0.0 { -value } else { value };
    abs_value < EPSILON_F32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_mode_constants() {
        // One of these must be true
        assert!(DEBUG_MODE || RELEASE_MODE);
        // They are mutually exclusive
        assert_ne!(DEBUG_MODE, RELEASE_MODE);
    }

    #[test]
    fn test_platform_constants() {
        // At most one platform category
        let platform_count = [IS_WEB, IS_MOBILE, IS_DESKTOP]
            .iter()
            .filter(|&&x| x)
            .count();
        assert!(
            platform_count <= 1,
            "Only one platform category should be true"
        );
    }

    #[test]
    fn test_approx_equal() {
        assert!(approx_equal(0.1 + 0.2, 0.3));
        assert!(approx_equal(1.0, 1.0));
        assert!(!approx_equal(1.0, 2.0));
        assert!(approx_equal(0.0, 0.0));
    }

    #[test]
    fn test_approx_equal_f32() {
        assert!(approx_equal_f32(0.1_f32 + 0.2_f32, 0.3_f32));
        assert!(approx_equal_f32(1.0_f32, 1.0_f32));
        assert!(!approx_equal_f32(1.0_f32, 2.0_f32));
    }

    #[test]
    fn test_is_near_zero() {
        assert!(is_near_zero(0.0));
        assert!(is_near_zero(1e-11));
        assert!(is_near_zero(-1e-11));
        assert!(!is_near_zero(1.0));
        assert!(!is_near_zero(-1.0));
    }

    #[test]
    fn test_is_near_zero_f32() {
        assert!(is_near_zero_f32(0.0_f32));
        assert!(is_near_zero_f32(1e-7_f32));
        assert!(!is_near_zero_f32(1.0_f32));
    }
}
