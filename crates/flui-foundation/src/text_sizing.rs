//! Portable authored text sizes and numeric growth profiles.

/// A numeric growth profile, independent of a font or a theme preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TextScaleProfile {
    /// Large title growth.
    LargeTitle,
    /// First title growth.
    Title1,
    /// Second title growth.
    Title2,
    /// Third title growth.
    Title3,
    /// Headline growth.
    Headline,
    /// Ordinary body growth, used when no profile is inherited.
    #[default]
    Body,
    /// Callout growth.
    Callout,
    /// Subheadline growth.
    Subheadline,
    /// Footnote growth.
    Footnote,
    /// First caption growth.
    Caption1,
    /// Second caption growth.
    Caption2,
}

/// Authored size behavior. Absence on a style inherits its parent's intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum TextSizingIntent {
    /// Resolve through the selected numeric growth profile.
    Profile(TextScaleProfile),
    /// Keep the authored logical size.
    Fixed,
}

/// A positive finite logical size representable by the shaping backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "f64", into = "f64"))]
pub struct TextSize(u64);

/// A size that cannot produce a positive finite shaping size.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("invalid text size: {0}")]
pub struct TextSizeError(pub f64);

impl TextSize {
    /// Validate the authored value and its shaping representation.
    ///
    /// # Errors
    /// Returns [`TextSizeError`] for non-positive or non-finite values, including
    /// values that overflow or underflow the shaper's positive finite `f32` size.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "validate the shaping representation"
    )]
    pub fn new(value: f64) -> Result<Self, TextSizeError> {
        let shaped = value as f32;
        if value.is_finite() && value > 0.0 && shaped.is_finite() && shaped > 0.0 {
            Ok(Self(value.to_bits()))
        } else {
            Err(TextSizeError(value))
        }
    }

    /// The admitted logical size.
    #[must_use]
    pub const fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl TryFrom<f64> for TextSize {
    type Error = TextSizeError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TextSize> for f64 {
    fn from(value: TextSize) -> Self {
        value.value()
    }
}

/// One exact numeric answer requested from a captured sizing source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextSizeRequest {
    /// The authored logical size, before growth.
    pub size: TextSize,
    /// The inherited numeric growth profile.
    pub profile: TextScaleProfile,
}
