//! Error types for flui_painting
//!
//! This module defines error types for painting operations including decoration failures,
//! gradient errors, and text/image painting issues.

use std::borrow::Cow;
use thiserror::Error;

/// Painting-specific error type
///
/// All fallible painting operations return `Result<T, PaintingError>`.
#[derive(Error, Debug, Clone)]
pub enum PaintingError {
    /// Decoration painting failed
    #[error("Failed to paint decoration: {reason}")]
    DecorationFailed {
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
    TextPaintingFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Image loading/painting failed
    #[error("Image operation failed: {reason}")]
    ImageFailed {
        /// Failure reason
        reason: Cow<'static, str>,
    },
}

/// Result type for painting operations
pub type Result<T> = std::result::Result<T, PaintingError>;

impl PaintingError {
    /// Create a decoration failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn decoration_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::DecorationFailed {
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
    pub fn text_painting_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::TextPaintingFailed {
            reason: reason.into(),
        }
    }

    /// Create an image failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn image_failed(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::ImageFailed {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoration_failed() {
        let err = PaintingError::decoration_failed("border rendering failed");
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
    fn test_text_painting_failed() {
        let err = PaintingError::text_painting_failed("font not found");
        assert!(err.to_string().contains("font not found"));
    }

    #[test]
    fn test_image_failed() {
        let err = PaintingError::image_failed("image not loaded");
        assert!(err.to_string().contains("image not loaded"));
    }

    #[test]
    fn test_cow_string_static() {
        let err = PaintingError::invalid_gradient("static");
        match err {
            PaintingError::InvalidGradient { reason } => {
                assert_eq!(reason, "static");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_cow_string_dynamic() {
        let dynamic = format!("dynamic {}", 42);
        let err = PaintingError::invalid_gradient(dynamic.clone());
        match err {
            PaintingError::InvalidGradient { reason } => {
                assert_eq!(reason.as_ref(), &dynamic);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
