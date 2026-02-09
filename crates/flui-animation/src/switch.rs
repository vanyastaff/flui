//! `AnimationSwitch` - switches between animations when values cross.

use crate::animation::{Animation, StatusCallback};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// The mode for determining when to switch animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchMode {
    /// Switch when the next animation's value becomes less than or equal to current.
    Minimize,
    /// Switch when the next animation's value becomes greater than or equal to current.
    Maximize,
}

/// An animation that switches between two animations when their values cross.
///
/// This animation starts by proxying one animation, but when the value of that
/// animation crosses the value of the second (either because the second is going
/// in the opposite direction, or because one overtakes the other), the animation
/// switches to proxying the second animation.
///
/// This is useful for implementing "train hopping" behavior where an animation
/// can seamlessly transition from one train to another.
///
/// Similar to Flutter's `TrainHoppingAnimation`.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationSwitch, AnimationController, Animation};
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
///
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     scheduler.clone(),
/// ));
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     scheduler,
/// ));
///
/// // Set different values
/// controller1.set_value(0.8);
/// controller2.set_value(0.3);
///
/// let switch = AnimationSwitch::new(
///     controller1.clone() as Arc<dyn Animation<f32>>,
///     Some(controller2.clone() as Arc<dyn Animation<f32>>),
/// );
///
/// // Initially uses controller1's value
/// assert_eq!(switch.value(), 0.8);
///
/// // When controller1's value falls below controller2's value,
/// // the switch will automatically switch to controller2
/// ```
pub struct AnimationSwitch {
    inner: Arc<Mutex<AnimationSwitchInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationSwitchInner {
    /// The currently active animation.
    current: Arc<dyn Animation<f32>>,
    /// The next animation to potentially switch to.
    next: Option<Arc<dyn Animation<f32>>>,
    /// The mode for determining when to switch.
    mode: Option<SwitchMode>,
    /// Callback when switched.
    on_switched: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Last reported value (for change detection).
    #[allow(dead_code)]
    last_value: Option<f32>,
    /// Last reported status.
    last_status: Option<AnimationStatus>,
    /// Listener IDs for cleanup.
    current_listener_id: Option<ListenerId>,
    next_listener_id: Option<ListenerId>,
    current_status_listener_id: Option<ListenerId>,
}

impl AnimationSwitch {
    /// Creates a new animation switch.
    ///
    /// If `next` is `None`, this animation will just proxy `current` and never switch.
    /// If both animations have the same initial value, the switch immediately
    /// switches to `next` without calling the `on_switched` callback.
    ///
    /// # Arguments
    ///
    /// * `current` - The initial animation to proxy
    /// * `next` - The animation to switch to when values cross (optional)
    #[must_use]
    pub fn new(current: Arc<dyn Animation<f32>>, next: Option<Arc<dyn Animation<f32>>>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        let mode = if let Some(ref next_anim) = next {
            let current_value = current.value();
            let next_value = next_anim.value();

            if (current_value - next_value).abs() < 1e-6 {
                // Same value - immediately switch to next, no callback
                None // Will be handled specially
            } else if current_value > next_value {
                Some(SwitchMode::Maximize)
            } else {
                Some(SwitchMode::Minimize)
            }
        } else {
            None
        };

        // If values are equal, start with next animation
        let (actual_current, actual_next) = if let Some(ref next_anim) = next {
            let current_value = current.value();
            let next_value = next_anim.value();

            if (current_value - next_value).abs() < 1e-6 {
                (next_anim.clone(), None)
            } else {
                (current, next)
            }
        } else {
            (current, next)
        };

        let inner = AnimationSwitchInner {
            current: actual_current,
            next: actual_next,
            mode,
            on_switched: None,
            last_value: None,
            last_status: None,
            current_listener_id: None,
            next_listener_id: None,
            current_status_listener_id: None,
        };

        let this = Self {
            inner: Arc::new(Mutex::new(inner)),
            notifier,
        };

        // Set up listeners
        this.setup_listeners();

        this
    }

    /// Sets a callback to be called when this animation switches to the next animation.
    ///
    /// This is not called if the two animations have the same initial value.
    #[must_use]
    pub fn on_switched<F>(self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        {
            let mut inner = self.inner.lock();
            inner.on_switched = Some(Arc::new(callback));
        }
        self
    }

    /// Returns the currently active animation.
    #[must_use]
    pub fn current(&self) -> Arc<dyn Animation<f32>> {
        self.inner.lock().current.clone()
    }

    /// Sets up listeners on the current and next animations.
    fn setup_listeners(&self) {
        let inner_weak = Arc::downgrade(&self.inner);
        let notifier = Arc::clone(&self.notifier);

        let value_handler = move || {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let mut inner = inner_arc.lock();

                // Check if we should switch
                let should_switch = if let (Some(mode), Some(ref next)) = (inner.mode, &inner.next)
                {
                    let current_value = inner.current.value();
                    let next_value = next.value();

                    match mode {
                        SwitchMode::Minimize => next_value <= current_value,
                        SwitchMode::Maximize => next_value >= current_value,
                    }
                } else {
                    false
                };

                if should_switch {
                    // Switch animations
                    if let Some(next) = inner.next.take() {
                        inner.current = next;
                        inner.mode = None;

                        // Call on_switched callback
                        if let Some(ref callback) = inner.on_switched {
                            let callback = Arc::clone(callback);
                            // Release lock before calling callback
                            drop(inner);
                            callback();
                        }
                    }
                }

                // Notify listeners of value change
                notifier.notify_listeners();
            }
        };

        let mut inner = self.inner.lock();

        // Add listener to current animation
        let callback: ListenerCallback = Arc::new(value_handler.clone());
        inner.current_listener_id = Some(inner.current.add_listener(callback));

        // Add listener to next animation if present
        if let Some(ref next) = inner.next {
            let callback: ListenerCallback = Arc::new(value_handler);
            inner.next_listener_id = Some(next.add_listener(callback));
        }

        // Add status listener to current
        let inner_weak = Arc::downgrade(&self.inner);
        let notifier = Arc::clone(&self.notifier);
        let status_callback: StatusCallback = Arc::new(move |status| {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let mut inner = inner_arc.lock();
                if inner.last_status != Some(status) {
                    inner.last_status = Some(status);
                    drop(inner);
                    notifier.notify_listeners();
                }
            }
        });
        inner.current_status_listener_id = Some(inner.current.add_status_listener(status_callback));
    }

    /// Disposes of this animation switch, cleaning up listeners.
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();

        // Remove listeners
        if let Some(id) = inner.current_listener_id.take() {
            inner.current.remove_listener(id);
        }
        if let Some(id) = inner.current_status_listener_id.take() {
            inner.current.remove_status_listener(id);
        }
        if let (Some(id), Some(ref next)) = (inner.next_listener_id.take(), &inner.next) {
            next.remove_listener(id);
        }
    }
}

impl Clone for AnimationSwitch {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            notifier: Arc::clone(&self.notifier),
        }
    }
}

impl Animation<f32> for AnimationSwitch {
    #[inline]
    fn value(&self) -> f32 {
        self.inner.lock().current.value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.inner.lock().current.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.inner.lock().current.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.inner.lock().current.remove_status_listener(id);
    }
}

impl Listenable for AnimationSwitch {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

impl fmt::Debug for AnimationSwitch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("AnimationSwitch")
            .field("value", &inner.current.value())
            .field("status", &inner.current.status())
            .field("has_next", &inner.next.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_scheduler::Scheduler;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn create_controller(scheduler: &Arc<Scheduler>, value: f32) -> Arc<AnimationController> {
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        controller.set_value(value);
        controller
    }

    #[test]
    fn test_animation_switch_basic() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = create_controller(&scheduler, 0.5);

        let switch = AnimationSwitch::new(controller.clone() as Arc<dyn Animation<f32>>, None);

        assert_eq!(switch.value(), 0.5);

        controller.dispose();
    }

    #[test]
    fn test_animation_switch_with_next() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = create_controller(&scheduler, 0.8);
        let controller2 = create_controller(&scheduler, 0.3);

        let switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f32>>,
            Some(controller2.clone() as Arc<dyn Animation<f32>>),
        );

        // Initially uses controller1
        assert_eq!(switch.value(), 0.8);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_animation_switch_same_initial_value() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = create_controller(&scheduler, 0.5);
        let controller2 = create_controller(&scheduler, 0.5);

        let switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f32>>,
            Some(controller2.clone() as Arc<dyn Animation<f32>>),
        );

        // Should immediately switch to controller2 since values are equal
        // Both have 0.5, so value should still be 0.5
        assert_eq!(switch.value(), 0.5);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_animation_switch_callback() {
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = create_controller(&scheduler, 0.8);
        let controller2 = create_controller(&scheduler, 0.3);

        let switched = Arc::new(AtomicBool::new(false));
        let switched_clone = Arc::clone(&switched);

        let _switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f32>>,
            Some(controller2.clone() as Arc<dyn Animation<f32>>),
        )
        .on_switched(move || {
            switched_clone.store(true, Ordering::SeqCst);
        });

        // Initially not switched
        assert!(!switched.load(Ordering::SeqCst));

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_animation_switch_status() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = create_controller(&scheduler, 0.5);

        let switch = AnimationSwitch::new(controller.clone() as Arc<dyn Animation<f32>>, None);

        assert_eq!(switch.status(), AnimationStatus::Dismissed);

        controller.forward().unwrap();
        assert_eq!(switch.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn test_animation_switch_current() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = create_controller(&scheduler, 0.5);

        let switch = AnimationSwitch::new(controller.clone() as Arc<dyn Animation<f32>>, None);

        let current = switch.current();
        assert_eq!(current.value(), 0.5);

        controller.dispose();
    }

    #[test]
    fn test_animation_switch_debug() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = create_controller(&scheduler, 0.5);

        let switch = AnimationSwitch::new(controller.clone() as Arc<dyn Animation<f32>>, None);

        let debug_str = format!("{:?}", switch);
        assert!(debug_str.contains("AnimationSwitch"));
        assert!(debug_str.contains("0.5"));

        controller.dispose();
    }
}
