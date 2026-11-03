//! [`BlurSigma`], the blur a text shadow is drawn with (ADR-0309 §4).

/// The standard deviation of a Gaussian blur, in logical pixels: finite,
/// non-negative and at most [`BlurSigma::MAX`]. Zero is sharp.
///
/// The bound is a type, so a style value can neither fail a frame's
/// recording nor ask the renderer for an unbounded kernel.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct BlurSigma(f32);

impl BlurSigma {
    /// No blur.
    pub const ZERO: Self = Self(0.0);

    /// The widest blur a text shadow takes, in logical pixels.
    pub const MAX: Self = Self(128.0);

    /// `sigma` logical pixels, or `None` unless it is finite, non-negative
    /// and at most [`Self::MAX`].
    #[must_use]
    pub fn try_new(sigma: f64) -> Option<Self> {
        // In range, the narrowing only rounds.
        (sigma.is_finite() && (0.0..=f64::from(Self::MAX.0)).contains(&sigma))
            .then_some(Self(sigma as f32))
    }

    /// The sigma of a shadow's blur radius, converted as Flutter and Skia
    /// do: `radius · 0.57735 + 0.5` for a positive radius, zero otherwise.
    /// A finite radius wider than [`Self::MAX`] allows is clamped to it; a
    /// non-finite one is `None`.
    #[must_use]
    pub fn from_blur_radius(radius: f64) -> Option<Self> {
        if !radius.is_finite() {
            return None;
        }
        if radius <= 0.0 {
            return Some(Self::ZERO);
        }
        Self::try_new((radius * 0.577_35 + 0.5).min(f64::from(Self::MAX.0)))
    }

    /// The sigma in logical pixels.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}
