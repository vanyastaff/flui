//! Builder pattern for `AnimationController`.
//!
//! This module provides a fluent builder API for constructing [`AnimationController`]
//! instances with various configuration options.

use crate::controller::AnimationController;
use crate::error::AnimationError;
use flui_scheduler::Scheduler;
use std::sync::Arc;
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
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
///
/// let controller = AnimationControllerBuilder::new(
///     Duration::from_millis(300),
///     scheduler,
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
    scheduler: Arc<Scheduler>,
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
    /// * `scheduler` - Scheduler for frame coordination
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// );
    /// ```
    #[must_use]
    pub fn new(duration: Duration, scheduler: Arc<Scheduler>) -> Self {
        Self {
            duration,
            scheduler,
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
    /// Returns [`AnimationError::InvalidBounds`] if `lower >= upper`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::builder::AnimationControllerBuilder;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// )
    /// .bounds(10.0, 20.0)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn bounds(mut self, lower: f32, upper: f32) -> Result<Self, AnimationError> {
        if lower >= upper {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower}) must be less than upper_bound ({upper})"
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
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
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
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let builder = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
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
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = AnimationControllerBuilder::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// )
    /// .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Result<AnimationController, AnimationError> {
        let controller = AnimationController::with_bounds(
            self.duration,
            self.scheduler,
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
    /// * `scheduler` - Scheduler for frame coordination
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::AnimationController;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = AnimationController::builder(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// )
    /// .reverse_duration(Duration::from_millis(500))
    /// .initial_value(0.5)
    /// .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder(duration: Duration, scheduler: Arc<Scheduler>) -> AnimationControllerBuilder {
        AnimationControllerBuilder::new(duration, scheduler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::Animation;

    #[test]
    fn test_builder_default() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), scheduler)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.0);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_bounds() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), scheduler)
            .bounds(10.0, 20.0)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(controller.value(), 10.0);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_initial_value() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), scheduler)
            .initial_value(0.5)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.5);
        controller.dispose();
    }

    #[test]
    fn test_builder_invalid_bounds() {
        let scheduler = Arc::new(Scheduler::new());
        let result = AnimationControllerBuilder::new(Duration::from_millis(100), scheduler)
            .bounds(20.0, 10.0); // Invalid: lower > upper

        assert!(result.is_err());
    }

    #[test]
    fn test_controller_builder_method() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::builder(Duration::from_millis(100), scheduler)
            .initial_value(0.75)
            .build()
            .unwrap();

        assert_eq!(controller.value(), 0.75);
        controller.dispose();
    }

    #[test]
    fn test_builder_with_reverse_duration() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationControllerBuilder::new(Duration::from_millis(100), scheduler)
            .reverse_duration(Duration::from_millis(200))
            .build()
            .unwrap();

        // Can't directly test reverse_duration, but builder should succeed
        controller.dispose();
    }

    #[test]
    fn test_builder_full_configuration() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationControllerBuilder::new(Duration::from_millis(300), scheduler)
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
