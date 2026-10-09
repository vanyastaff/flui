//! Ordinary failures while preparing or shaping text.

/// A request that cannot produce valid text geometry.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TextLayoutError {
    /// Authored scaling must be positive and finite.
    #[error("invalid text scale factor: {factor}")]
    InvalidScale {
        /// The authored scale factor.
        factor: f64,
    },
    /// The resolved size is not positive and finite in the shaping backend.
    #[error("font size cannot be represented as a positive finite shaping size: {size}")]
    InvalidFontSize {
        /// The requested logical size, before narrowing.
        size: f64,
    },
    /// The requested line height cannot be represented by the backend.
    #[error("invalid text line height: {height}")]
    InvalidLineHeight {
        /// The requested line height or relative multiplier.
        height: f64,
    },
    /// A finite width constraint cannot be represented by the backend.
    #[error("invalid text width: {width}")]
    InvalidWidth {
        /// The requested width.
        width: f64,
    },
    /// Spacing cannot be represented by the backend.
    #[error("invalid text spacing: {spacing}")]
    InvalidSpacing {
        /// The requested letter or word spacing.
        spacing: f64,
    },
    /// Shaping or subsequent metric arithmetic produced nonfinite geometry.
    #[error("text layout produced nonfinite geometry")]
    NonFiniteGeometry,
    /// Layout requires text configuration.
    #[error("text must be configured before layout")]
    TextNotSet,
    /// Layout requires a text direction.
    #[error("text direction must be configured before layout")]
    TextDirectionNotSet,
}

/// Check the representation that the backend actually consumes.
#[expect(
    clippy::cast_possible_truncation,
    reason = "validate the shaping conversion"
)]
pub(crate) fn font_size(size: f64) -> Result<f32, TextLayoutError> {
    let narrowed = size as f32;
    if narrowed > 0.0 && narrowed.is_finite() {
        Ok(narrowed)
    } else {
        Err(TextLayoutError::InvalidFontSize { size })
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "validate the shaping conversion"
)]
pub(crate) fn width(width: f64) -> Result<f32, TextLayoutError> {
    let narrowed = width as f32;
    if width >= 0.0 && narrowed >= 0.0 && narrowed.is_finite() {
        Ok(narrowed)
    } else {
        Err(TextLayoutError::InvalidWidth { width })
    }
}
