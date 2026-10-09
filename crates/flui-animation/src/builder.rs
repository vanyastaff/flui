//! Configuration and ownership of an animation controller.

use std::time::Duration;

use crate::{AnimationController, AnimationError, DrivenController, Vsync};

/// Finite controller bounds with a finite, positive span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValueRange {
    lower: f64,
    upper: f64,
}

impl ValueRange {
    /// The usual normalized animation range.
    pub const UNIT: Self = Self {
        lower: 0.0,
        upper: 1.0,
    };

    /// Validate endpoints and their difference before constructing a range.
    ///
    /// # Errors
    /// Returns `AnimationError::InvalidBounds` for non-finite endpoints or span,
    /// or when `lower >= upper`.
    pub fn new(lower: f64, upper: f64) -> Result<Self, AnimationError> {
        if !lower.is_finite()
            || !upper.is_finite()
            || lower >= upper
            || !(upper - lower).is_finite()
        {
            return Err(AnimationError::InvalidBounds(format!(
                "bounds ({lower}, {upper}) must have finite endpoints and a finite positive span"
            )));
        }
        Ok(Self { lower, upper })
    }

    /// Lower endpoint.
    #[must_use]
    pub const fn lower(self) -> f64 {
        self.lower
    }

    /// Upper endpoint.
    #[must_use]
    pub const fn upper(self) -> f64 {
        self.upper
    }
}

impl Default for ValueRange {
    fn default() -> Self {
        Self::UNIT
    }
}

/// Configure a manually sampled controller or an owning controller on a Vsync.
///
/// ```
/// use flui_animation::{AnimationController, Animation, ValueRange};
/// use std::time::Duration;
///
/// let controller = AnimationController::builder(Duration::from_millis(300))
///     .bounds(ValueRange::new(10.0, 20.0).expect("finite range"))
///     .initial_value(15.0)
///     .build();
/// assert_eq!(controller.value(), 15.0);
/// ```
#[derive(Clone, Debug)]
pub struct AnimationControllerBuilder {
    duration: Duration,
    bounds: Option<ValueRange>,
    reverse_duration: Option<Duration>,
    initial_value: Option<f64>,
}

impl AnimationControllerBuilder {
    /// Start with normalized bounds and the given forward duration.
    #[must_use]
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            bounds: Some(ValueRange::UNIT),
            reverse_duration: None,
            initial_value: None,
        }
    }

    /// Use a validated finite value range.
    #[must_use]
    pub fn bounds(mut self, bounds: ValueRange) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Admit finite pixel-space values without a finite endpoint.
    #[must_use]
    pub fn unbounded(mut self) -> Self {
        self.bounds = None;
        self
    }

    /// Configure the duration used by reverse runs.
    #[must_use]
    pub fn reverse_duration(mut self, duration: Duration) -> Self {
        self.reverse_duration = Some(duration);
        self
    }

    /// Initial value, clamped to the range. Non-finite values use the lower
    /// endpoint for bounded controllers or zero for unbounded controllers.
    #[must_use]
    pub fn initial_value(mut self, value: f64) -> Self {
        self.initial_value = Some(value);
        self
    }

    /// Build a controller sampled explicitly through `tick_at(Duration)`.
    #[must_use]
    pub fn build(self) -> AnimationController {
        let controller =
            AnimationController::from_config(self.duration, self.bounds, self.initial_value);
        if let Some(duration) = self.reverse_duration {
            controller.set_reverse_duration(duration);
        }
        controller
    }

    /// Build the owner of a controller and its registry seat.
    ///
    /// An exhausted registry leaves the controller unbound and reports the
    /// refusal through diagnostics.
    pub fn build_on(self, vsync: Option<&Vsync>) -> DrivenController {
        DrivenController::new(self.build(), vsync)
    }
}

impl AnimationController {
    /// Configure an animation without acquiring a scheduler or clock.
    #[must_use]
    pub fn builder(duration: Duration) -> AnimationControllerBuilder {
        AnimationControllerBuilder::new(duration)
    }
}
