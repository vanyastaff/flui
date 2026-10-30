//! [`CarouselAutoplay`]: a carousel that moves on by itself, within the limits that keep it
//! usable (WCAG 2.2.2 Pause, Stop, Hide).

use std::time::Duration;

/// The shortest autoplay period: slower than five seconds is the floor a reader of each item
/// needs, and WCAG's limit for motion that starts on its own.
pub const MIN_AUTOPLAY_PERIOD: Duration = Duration::from_secs(5);

/// Why [`CarouselAutoplay::every`] refused its period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CarouselAutoplayError {
    /// The period is shorter than [`MIN_AUTOPLAY_PERIOD`].
    #[error("a carousel's autoplay period must be at least five seconds")]
    TooFast,
}

/// A carousel that moves to the next item in reading order every period, and from the last
/// item back to the first.
///
/// An autoplaying carousel draws its own pause control, the first focus stop inside it, a
/// button that pauses and resumes; its controller's [`pause`](super::CarouselController::pause)
/// and [`resume`](super::CarouselController::resume) do the same. Autoplay also stops for good,
/// until that control or `resume`, when keyboard focus enters the carousel, a screen reader
/// reads one of its items (where the platform reports its cursor), a pointer hovers or presses
/// on it, or the app stops being in front; a step is a programmatic scroll the user's
/// own input takes over from. Reduced motion turns autoplay off, a carousel whose items all fit
/// takes no steps, and no step is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarouselAutoplay {
    period: Duration,
}

impl CarouselAutoplay {
    /// A step every `period`.
    ///
    /// # Errors
    ///
    /// [`CarouselAutoplayError::TooFast`] for a period shorter than [`MIN_AUTOPLAY_PERIOD`].
    pub fn every(period: Duration) -> Result<Self, CarouselAutoplayError> {
        if period < MIN_AUTOPLAY_PERIOD {
            return Err(CarouselAutoplayError::TooFast);
        }
        Ok(Self { period })
    }

    /// The time between steps.
    #[must_use]
    pub fn period(&self) -> Duration {
        self.period
    }
}
