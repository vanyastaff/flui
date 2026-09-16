//! Builder pattern for `AnimationController`.
//!
//! This module provides a fluent builder API for constructing [`AnimationController`]
//! instances with various configuration options.

use crate::controller::AnimationController;
use crate::error::AnimationError;
use flui_scheduler::UpdateScheduler;
use std::time::Duration;

/// Builder for creating [`AnimationController`] instances.
///
/// Provides a fluent API for configuring animation controllers with
/// custom bounds, durations, and initial values.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), flui_animation::AnimationError> {
/// use flui_animation::builder::AnimationControllerBuilder;
/// use flui_animation::Animation;
/// use flui_scheduler::UpdateScheduler;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
///
/// let controller = AnimationControllerBuilder::new(
///     Duration::from_millis(300),
///     &scheduler,
/// )
/// .bounds(0.0, 100.0)?
/// .reverse_duration(Duration::from_millis(500))
/// .initial_value(50.0)
/// .build()?;
///
/// assert_eq!(controller.value(), 50.0);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct AnimationControllerBuilder {
    duration: Duration,
    scheduler: UpdateScheduler,
    lower_bound: f32,
    upper_bound: f32,
    reverse_duration: Option<Duration>,
    initial_value: Option<f32>,
}

impl std::fmt::Debug for AnimationControllerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationControllerBuilder")
            .field("duration", &self.duration)
            .field("lower_bound", &self.lower_bound)
            .field("upper_bound", &self.upper_bound)
            .field("reverse_duration", &self.reverse_duration)
            .field("initial_value", &self.initial_value)
            .finish_non_exhaustive()
    }
}

impl AnimationControllerBuilder {
    /// Create a new builder with required parameters.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - UpdateScheduler for frame coordination
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// );
    /// ```
    #[must_use]
    pub fn new(duration: Duration, scheduler: &UpdateScheduler) -> Self {
        Self {
            duration,
            scheduler: scheduler.clone(),
            lower_bound: 0.0,
            upper_bound: 1.0,
            reverse_duration: None,
            initial_value: None,
        }
    }

    /// Set custom bounds for the animation.
    ///
    /// # Arguments
    ///
    /// * `lower` - Minimum value (default 0.0)
    /// * `upper` - Maximum value (default 1.0)
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] unless both bounds are
    /// finite, `lower < upper`, AND `upper - lower` itself fits in `f32` —
    /// bounded means finite endpoints AND a finite span
    /// (`(-f32::MAX, f32::MAX)` has finite endpoints but a span of
    /// `f32::INFINITY`). An [`AnimationController::unbounded`] controller
    /// is not reachable through this builder (it has no bound to
    /// configure).
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// )
    /// .bounds(10.0, 20.0)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn bounds(mut self, lower: f32, upper: f32) -> Result<Self, AnimationError> {
        // `lower >= upper` (not the negated `!(lower < upper)`, which
        // clippy's `neg_cmp_op_on_partial_ord` flags on a `PartialOrd`-only
        // type): NaN makes the two diverge, but NaN is caught by the
        // `is_finite` clauses below regardless of which form this takes.
        // The span check mirrors `AnimationController::with_bounds_inner`'s
        // own rule: two finite endpoints do not make a finite range.
        if lower >= upper
            || !lower.is_finite()
            || !upper.is_finite()
            || !(upper - lower).is_finite()
        {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower}) and upper_bound ({upper}) must both be finite, with \
                 lower_bound < upper_bound, and the range (upper_bound - lower_bound) must fit \
                 in f32"
            )));
        }
        self.lower_bound = lower;
        self.upper_bound = upper;
        Ok(self)
    }

    /// Set a different duration for reverse animation.
    ///
    /// If not set, the reverse animation uses the same duration as forward.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the reverse animation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// )
    /// .reverse_duration(Duration::from_millis(500));
    /// ```
    #[must_use]
    pub fn reverse_duration(mut self, duration: Duration) -> Self {
        self.reverse_duration = Some(duration);
        self
    }

    /// Set the initial value of the animation.
    ///
    /// The value will be clamped to the configured bounds.
    /// If not set, defaults to `lower_bound`.
    ///
    /// # Arguments
    ///
    /// * `value` - Initial value (will be clamped to bounds)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// )
    /// .initial_value(0.5);
    /// ```
    #[must_use]
    pub fn initial_value(mut self, value: f32) -> Self {
        self.initial_value = Some(value);
        self
    }

    /// Build the [`AnimationController`].
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] if bounds are invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let controller = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// )
    /// .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Result<AnimationController, AnimationError> {
        let controller = AnimationController::with_bounds(
            self.duration,
            &self.scheduler,
            self.lower_bound,
            self.upper_bound,
        )?;

        if let Some(rev_dur) = self.reverse_duration {
            controller.set_reverse_duration(rev_dur);
        }

        if let Some(value) = self.initial_value {
            controller.set_value(value);
        }

        Ok(controller)
    }
}

// Add builder() method to AnimationController
impl AnimationController {
    /// Create a builder for configuring an [`AnimationController`].
    ///
    /// This provides a fluent API for creating controllers with custom settings.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - UpdateScheduler for frame coordination
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::AnimationController;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let controller = AnimationController::builder(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    /// )
    /// .reverse_duration(Duration::from_millis(500))
    /// .initial_value(0.5)
    /// .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder(duration: Duration, scheduler: &UpdateScheduler) -> AnimationControllerBuilder {
        AnimationControllerBuilder::new(duration, scheduler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::Animation;

    #[test]
    fn test_builder_default() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.0);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_bounds() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .bounds(10.0, 20.0)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(controller.value(), 10.0);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_initial_value() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .initial_value(0.5)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.5);
        controller.dispose();
    }

    #[test]
    fn test_builder_invalid_bounds() {
        let scheduler = UpdateScheduler::new();
        let result = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .bounds(20.0, 10.0); // Invalid: lower > upper

        assert!(result.is_err());
    }

    /// #1183: `.bounds()` duplicates `with_bounds_inner`'s validation, so it
    /// must adopt the same "bounded means finite" rule -- red before the
    /// fix, since the old `lower >= upper` check accepts `NaN` (`NaN >=
    /// upper` is always `false`) and any infinite pair with `lower < upper`.
    #[test]
    fn bounds_rejects_non_finite_endpoints() {
        let scheduler = UpdateScheduler::new();
        // Same list `controller::tests::bounds_constructors_reject_non_finite_bounds`
        // uses, plus the finite-endpoints-infinite-range case.
        let cases: &[(f32, f32)] = &[
            (f32::NAN, 1.0),
            (0.0, f32::NAN),
            (f32::NEG_INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, 5.0),
            (5.0, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
            (-f32::MAX, f32::MAX),
        ];
        for &(lower, upper) in cases {
            let result = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
                .bounds(lower, upper);
            assert!(
                matches!(result, Err(AnimationError::InvalidBounds(_))),
                "bounds({lower}, {upper}) must be rejected"
            );
        }
    }

    #[test]
    fn test_controller_builder_method() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationController::builder(Duration::from_millis(100), &scheduler)
            .initial_value(0.75)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.75);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_reverse_duration() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .reverse_duration(Duration::from_millis(200))
            .build()
            .unwrap();

        // Can't directly test reverse_duration, but builder should succeed
        controller.dispose();
    }

    #[test]
    fn test_builder_full_configuration() {
        let scheduler = UpdateScheduler::new();
        let controller = AnimationControllerBuilder::new(Duration::from_millis(300), &scheduler)
            .bounds(0.0, 100.0)
            .unwrap()
            .reverse_duration(Duration::from_millis(500))
            .initial_value(50.0)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 50.0);
        controller.dispose();
    }
}
