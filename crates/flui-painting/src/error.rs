//! Error types for flui_painting
//!
//! This module defines error types for painting operations including decoration failures,
//! gradient errors, and text/image painting issues.

use std::borrow::Cow;
use thiserror::Error;

/// Painting-specific error type
///
/// All fallible painting operations return `Result<T, PaintingError>`.
///
/// This type is marked as `#[non_exhaustive]` to allow adding new error variants
/// in the future without breaking existing code. Always use a catch-all pattern when matching.
#[non_exhaustive]
#[derive(Error, Debug, Clone)]
pub enum PaintingError {
    /// Decoration painting failed
    #[error("Failed to paint decoration: {reason}")]
    PaintDecorationFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Invalid decoration configuration
    #[error("Invalid decoration: {reason}")]
    InvalidDecoration {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Gradient configuration error
    #[error("Invalid gradient: {reason}")]
    InvalidGradient {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Text painting failed
    #[error("Text painting failed: {reason}")]
    PaintTextFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Image loading/painting failed
    #[error("Image operation failed: {reason}")]
    PaintImageFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },
}

/// Result type for painting operations
pub type Result<T> = std::result::Result<T, PaintingError>;

impl PaintingError {
    /// Create a decoration painting failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn paint_decoration_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::PaintDecorationFailed {
            reason: reason.into(),
        }
    }

    /// Create an invalid decoration error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_decoration(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidDecoration {
            reason: reason.into(),
        }
    }

    /// Create an invalid gradient error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_gradient(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidGradient {
            reason: reason.into(),
        }
    }

    /// Create a text painting failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn paint_text_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::PaintTextFailed {
            reason: reason.into(),
        }
    }

    /// Create an image painting failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn paint_image_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::PaintImageFailed {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paint_decoration_failed() {
        let err = PaintingError::paint_decoration_failed("border rendering failed");
        assert!(err.to_string().contains("border rendering failed"));
    }

    #[test]
    fn test_invalid_decoration() {
        let err = PaintingError::invalid_decoration("border requires border_radius");
        assert!(err.to_string().contains("border requires border_radius"));
    }

    #[test]
    fn test_invalid_gradient() {
        let err = PaintingError::invalid_gradient("gradient must have at least one color");
        assert!(err.to_string().contains("at least one color"));
    }

    #[test]
    fn test_paint_text_failed() {
        let err = PaintingError::paint_text_failed("font not found");
        assert!(err.to_string().contains("font not found"));
    }

    #[test]
    fn test_paint_image_failed() {
        let err = PaintingError::paint_image_failed("image not loaded");
        assert!(err.to_string().contains("image not loaded"));
    }

    #[test]
    fn test_cow_string_static() {
        let err = PaintingError::invalid_gradient("static");
        #[allow(clippy::panic)] // Test assertion
        let PaintingError::InvalidGradient { reason } = err
        else {
            panic!("Expected InvalidGradient variant");
        };
        assert_eq!(reason, "static");
    }

    #[test]
    fn test_cow_string_dynamic() {
        let dynamic = format!("dynamic {}", 42);
        let err = PaintingError::invalid_gradient(dynamic.clone());
        #[allow(clippy::panic)] // Test assertion
        let PaintingError::InvalidGradient { reason } = err
        else {
            panic!("Expected InvalidGradient variant");
        };
        assert_eq!(reason.as_ref(), &dynamic);
    }
}
