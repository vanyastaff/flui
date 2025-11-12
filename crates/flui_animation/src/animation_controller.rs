//! AnimationController - The primary animation driver.

use crate::animation::{Animation, AnimationDirection, StatusCallback};
use flui_core::foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_scheduler::{Scheduler, Ticker};
use flui_types::animation::AnimationStatus;
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Errors that can occur when using an AnimationController.
#[derive(Debug, thiserror::Error)]
pub enum AnimationError {
    /// The AnimationController has been disposed.
    #[error("AnimationController has been disposed")]
    Disposed,

    /// Invalid animation bounds.
    #[error("Invalid animation bounds: {0}")]
    InvalidBounds(String),

    /// Ticker not available.
    #[error("Ticker not available")]
    TickerNotAvailable,
}

/// Controls an animation, driving it forward/backward.
///
/// AnimationController is a **PERSISTENT OBJECT** that survives widget rebuilds.
/// It must be disposed when no longer needed to prevent resource leaks.
///
/// The controller generates values from `lower_bound` to `upper_bound` (typically 0.0 to 1.0)
/// over the specified duration. It implements `Animation<f32>` and can be used directly,
/// or transformed using `Tween` or `CurvedAnimation`.
///
/// # Thread Safety
///
/// AnimationController is fully thread-safe using `Arc` and `Mutex`.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, Animation};
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller = AnimationController::new(
///     Duration::from_millis(300),
///     scheduler,
/// );
///
/// // Start animation
/// controller.forward().unwrap();
///
/// // Get current value (0.0 to 1.0)
/// let value = controller.value();
///
/// // Cleanup when done
/// controller.dispose();
/// ```
#[derive(Clone)]
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationControllerInner {
    /// Current value (typically 0.0 to 1.0)
    value: f32,

    /// Animation status
    status: AnimationStatus,

    /// Duration of forward animation
    duration: Duration,

    /// Duration of reverse animation (defaults to duration)
    reverse_duration: Option<Duration>,

    /// Lower bound (default 0.0)
    lower_bound: f32,

    /// Upper bound (default 1.0)
    upper_bound: f32,

    /// Ticker for frame callbacks
    ticker: Option<Ticker>,

    /// Scheduler reference for ticker coordination
    #[allow(dead_code)]
    scheduler: Option<Arc<Scheduler>>,

    /// Status listeners
    status_listeners: Vec<(ListenerId, StatusCallback)>,

    /// Animation direction
    direction: AnimationDirection,

    /// Start time of current animation
    animation_start_time: Option<Instant>,

    /// Value when animation started (for partial animations)
    start_value: f32,

    /// Target value for current animation
    target_value: f32,

    /// Is disposed?
    disposed: bool,

    /// Next listener ID
    next_listener_id: usize,
}

impl AnimationController {
    /// Create a new animation controller.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - Scheduler for frame coordination
    pub fn new(duration: Duration, scheduler: Arc<Scheduler>) -> Self {
        Self::with_bounds(duration, scheduler, 0.0, 1.0)
    }

    /// Create an animation controller with custom bounds.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - Scheduler for frame coordination
    /// * `lower_bound` - Minimum value (default 0.0)
    /// * `upper_bound` - Maximum value (default 1.0)
    pub fn with_bounds(
        duration: Duration,
        scheduler: Arc<Scheduler>,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Self {
        assert!(
            lower_bound < upper_bound,
            "lower_bound must be less than upper_bound"
        );

        let notifier = Arc::new(ChangeNotifier::new());

        // Create ticker (no callback needed - we'll handle ticking manually)
        let ticker = Ticker::new();

        let inner = AnimationControllerInner {
            value: lower_bound,
            status: AnimationStatus::Dismissed,
            duration,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            ticker: Some(ticker),
            scheduler: Some(scheduler),
            status_listeners: Vec::new(),
            direction: AnimationDirection::Forward,
            animation_start_time: None,
            start_value: lower_bound,
            target_value: upper_bound,
            disposed: false,
            next_listener_id: 0,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
            notifier,
        }
    }

    /// Set the duration for reverse animation.
    pub fn set_reverse_duration(&self, duration: Duration) {
        let mut inner = self.inner.lock();
        inner.reverse_duration = Some(duration);
    }

    /// Start animation forward from current value to upper bound.
    pub fn forward(&self) -> Result<(), AnimationError> {
        self.forward_from(None)
    }

    /// Start animation forward from a specific value.
    ///
    /// If `from` is None, starts from current value.
    pub fn forward_from(&self, from: Option<f32>) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.direction = AnimationDirection::Forward;
        inner.status = AnimationStatus::Forward;
        inner.animation_start_time = Some(Instant::now());
        inner.start_value = inner.value;
        inner.target_value = inner.upper_bound;

        if let Some(ticker) = &mut inner.ticker {
            let notifier = Arc::clone(&self.notifier);
            ticker.start(move |_elapsed| {
                notifier.notify_listeners();
            });
        }

        self.notify_status_listeners(AnimationStatus::Forward, &inner);
        Ok(())
    }

    /// Start animation in reverse from current value to lower bound.
    pub fn reverse(&self) -> Result<(), AnimationError> {
        self.reverse_from(None)
    }

    /// Start animation in reverse from a specific value.
    ///
    /// If `from` is None, starts from current value.
    pub fn reverse_from(&self, from: Option<f32>) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.direction = AnimationDirection::Reverse;
        inner.status = AnimationStatus::Reverse;
        inner.animation_start_time = Some(Instant::now());
        inner.start_value = inner.value;
        inner.target_value = inner.lower_bound;

        if let Some(ticker) = &mut inner.ticker {
            let notifier = Arc::clone(&self.notifier);
            ticker.start(move |_elapsed| {
                notifier.notify_listeners();
            });
        }

        self.notify_status_listeners(AnimationStatus::Reverse, &inner);
        Ok(())
    }

    /// Stop the animation at its current value.
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }

        // Update status based on current value
        inner.status = if (inner.value - inner.upper_bound).abs() < 1e-6 {
            AnimationStatus::Completed
        } else if (inner.value - inner.lower_bound).abs() < 1e-6 {
            AnimationStatus::Dismissed
        } else {
            // Stopped in middle, keep status based on direction
            match inner.direction {
                AnimationDirection::Forward => AnimationStatus::Forward,
                AnimationDirection::Reverse => AnimationStatus::Reverse,
            }
        };

        Ok(())
    }

    /// Reset to beginning (lower bound).
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        inner.value = inner.lower_bound;
        inner.status = AnimationStatus::Dismissed;

        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }

        drop(inner);
        self.notifier.notify_listeners();

        let inner = self.inner.lock();
        self.notify_status_listeners(AnimationStatus::Dismissed, &inner);

        Ok(())
    }

    /// Animate to a specific value.
    pub fn animate_to(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        self.check_disposed(&inner)?;

        let target = target.clamp(inner.lower_bound, inner.upper_bound);
        inner.animation_start_time = Some(Instant::now());
        inner.start_value = inner.value;
        inner.target_value = target;

        // Determine direction
        inner.direction = if target > inner.value {
            AnimationDirection::Forward
        } else {
            AnimationDirection::Reverse
        };

        inner.status = match inner.direction {
            AnimationDirection::Forward => AnimationStatus::Forward,
            AnimationDirection::Reverse => AnimationStatus::Reverse,
        };

        // Override duration if provided
        if let Some(dur) = duration {
            inner.duration = dur;
        }

        if let Some(ticker) = &mut inner.ticker {
            let notifier = Arc::clone(&self.notifier);
            ticker.start(move |_elapsed| {
                notifier.notify_listeners();
            });
        }

        self.notify_status_listeners(inner.status, &inner);
        Ok(())
    }

    /// Repeat the animation indefinitely.
    ///
    /// If `reverse` is true, the animation will bounce back and forth.
    pub fn repeat(&self, _reverse: bool) -> Result<(), AnimationError> {
        // TODO: Implement repeat logic with state tracking for reverse mode
        // For now, just start forward
        self.forward()
    }

    /// Update the animation value based on elapsed time.
    ///
    /// This is typically called by the ticker.
    pub fn tick(&self) {
        let mut inner = self.inner.lock();

        if inner.status != AnimationStatus::Forward && inner.status != AnimationStatus::Reverse {
            return;
        }

        let Some(start_time) = inner.animation_start_time else {
            return;
        };

        let elapsed = Instant::now().duration_since(start_time);
        let duration = match inner.direction {
            AnimationDirection::Forward => inner.duration,
            AnimationDirection::Reverse => inner.reverse_duration.unwrap_or(inner.duration),
        };

        let t = if duration.is_zero() {
            1.0
        } else {
            (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
        };

        // Linear interpolation from start_value to target_value
        let range = inner.target_value - inner.start_value;
        inner.value = inner.start_value + range * t;

        // Check if animation is complete
        if t >= 1.0 {
            inner.value = inner.target_value;

            if let Some(ticker) = &mut inner.ticker {
                ticker.stop();
            }

            let new_status = if (inner.value - inner.upper_bound).abs() < 1e-6 {
                AnimationStatus::Completed
            } else if (inner.value - inner.lower_bound).abs() < 1e-6 {
                AnimationStatus::Dismissed
            } else {
                inner.status
            };

            inner.status = new_status;
            drop(inner);

            let inner = self.inner.lock();
            self.notify_status_listeners(new_status, &inner);
        }
    }

    /// Get the current value.
    #[must_use]
    pub fn current_value(&self) -> f32 {
        self.inner.lock().value
    }

    /// Set the value directly without animating.
    pub fn set_value(&self, value: f32) {
        let mut inner = self.inner.lock();
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        drop(inner);
        self.notifier.notify_listeners();
    }

    /// **CRITICAL:** Dispose when done to prevent leaks.
    ///
    /// This stops the animation and cleans up resources.
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();

        if inner.disposed {
            return;
        }

        if let Some(mut ticker) = inner.ticker.take() {
            ticker.stop();
        }

        inner.status_listeners.clear();
        inner.disposed = true;
    }

    fn check_disposed(&self, inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        if inner.disposed {
            Err(AnimationError::Disposed)
        } else {
            Ok(())
        }
    }

    fn notify_status_listeners(&self, status: AnimationStatus, inner: &AnimationControllerInner) {
        for (_, callback) in &inner.status_listeners {
            callback(status);
        }
    }
}

impl Animation<f32> for AnimationController {
    fn value(&self) -> f32 {
        self.inner.lock().value
    }

    fn status(&self) -> AnimationStatus {
        self.inner.lock().status
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut inner = self.inner.lock();
        let id = ListenerId::new(inner.next_listener_id);
        inner.next_listener_id += 1;
        inner.status_listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        let mut inner = self.inner.lock();
        inner
            .status_listeners
            .retain(|(listener_id, _)| *listener_id != id);
    }
}

impl Listenable for AnimationController {
    fn add_listener(&mut self, callback: ListenerCallback) -> ListenerId {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .add_listener(callback)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_listener(id)
    }

    fn remove_all_listeners(&mut self) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_all_listeners()
    }
}

impl fmt::Debug for AnimationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("AnimationController")
            .field("value", &inner.value)
            .field("status", &inner.status)
            .field("direction", &inner.direction)
            .field("lower_bound", &inner.lower_bound)
            .field("upper_bound", &inner.upper_bound)
            .field("disposed", &inner.disposed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_scheduler::Scheduler;

    #[test]
    fn test_animation_controller_creation() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(100), scheduler);

        assert_eq!(controller.value(), 0.0);
        assert_eq!(controller.status(), AnimationStatus::Dismissed);

        controller.dispose();
    }

    #[test]
    fn test_animation_controller_forward() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(100), scheduler);

        controller.forward().unwrap();
        assert_eq!(controller.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn test_animation_controller_reset() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(100), scheduler);

        controller.set_value(0.5);
        assert_eq!(controller.value(), 0.5);

        controller.reset().unwrap();
        assert_eq!(controller.value(), 0.0);
        assert_eq!(controller.status(), AnimationStatus::Dismissed);

        controller.dispose();
    }

    #[test]
    fn test_animation_controller_bounds() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::with_bounds(
            Duration::from_millis(100),
            scheduler,
            10.0,
            20.0,
        );

        assert_eq!(controller.value(), 10.0);

        controller.set_value(15.0);
        assert_eq!(controller.value(), 15.0);

        // Test clamping
        controller.set_value(100.0);
        assert_eq!(controller.value(), 20.0);

        controller.dispose();
    }

    #[test]
    fn test_animation_controller_dispose() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(100), scheduler);

        controller.dispose();

        // Should fail after dispose
        assert!(matches!(
            controller.forward(),
            Err(AnimationError::Disposed)
        ));
    }
}
