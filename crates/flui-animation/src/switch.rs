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
    /// Switch-owned status listeners.
    ///
    /// Owned here (not delegated to `current`) because `current` changes on
    /// a train-hop: an id minted by the old current would be removed against
    /// the new one and orphan the callback on the retired animation. The
    /// internal per-current forwarder fans out to this registry instead.
    status_listeners: Vec<(ListenerId, StatusCallback)>,
    /// Next id for `status_listeners` (starts at 1 so 0 never collides).
    next_status_listener_id: usize,
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
            status_listeners: Vec::new(),
            next_status_listener_id: 1,
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

    /// Builds the status forwarder that re-emits the current animation's
    /// status transitions through our notifier.
    fn make_status_callback(
        inner_weak: &std::sync::Weak<Mutex<AnimationSwitchInner>>,
        notifier: &Arc<ChangeNotifier>,
    ) -> StatusCallback {
        let inner_weak = inner_weak.clone();
        let notifier = Arc::clone(notifier);
        Arc::new(move |status| {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let mut inner = inner_arc.lock();
                if inner.last_status != Some(status) {
                    inner.last_status = Some(status);
                    // Snapshot-then-fire: user callbacks run without the
                    // inner lock held (they may re-enter the switch).
                    let callbacks: Vec<StatusCallback> = inner
                        .status_listeners
                        .iter()
                        .map(|(_, cb)| Arc::clone(cb))
                        .collect();
                    drop(inner);
                    notifier.notify_listeners();
                    for callback in callbacks {
                        callback(status);
                    }
                }
            }
        })
    }

    /// Sets up listeners on the current and next animations.
    fn setup_listeners(&self) {
        let inner_weak = Arc::downgrade(&self.inner);
        let notifier = Arc::clone(&self.notifier);
        let status_callback = Self::make_status_callback(&inner_weak, &notifier);
        let status_callback_for_handler = Arc::clone(&status_callback);

        let value_handler = move || {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let mut inner = inner_arc.lock();

                // Check if we should switch
                let should_switch = if let (Some(mode), Some(next)) = (inner.mode, &inner.next) {
                    let current_value = inner.current.value();
                    let next_value = next.value();

                    match mode {
                        SwitchMode::Minimize => next_value <= current_value,
                        SwitchMode::Maximize => next_value >= current_value,
                    }
                } else {
                    false
                };

                // On switch, rebind all listener bookkeeping so the ids stored
                // in `inner` always describe live registrations on `current`:
                //  - the old current's value + status listeners are removed
                //    (it may be externally alive long after the hop);
                //  - the promoted next's value listener becomes the current one;
                //  - a fresh status listener is attached to the new current.
                let mut callback = None;
                let mut rebind = None;
                if should_switch && let Some(next) = inner.next.take() {
                    let old_current = std::mem::replace(&mut inner.current, Arc::clone(&next));
                    inner.mode = None;
                    let old_value_id = inner.current_listener_id.take();
                    let old_status_id = inner.current_status_listener_id.take();
                    inner.current_listener_id = inner.next_listener_id.take();
                    callback.clone_from(&inner.on_switched);
                    rebind = Some((old_current, old_value_id, old_status_id, next));
                }
                // Release the lock before touching other animations' listener
                // registries or running user callbacks.
                drop(inner);

                if let Some((old_current, old_value_id, old_status_id, new_current)) = rebind {
                    if let Some(id) = old_value_id {
                        old_current.remove_listener(id);
                    }
                    if let Some(id) = old_status_id {
                        old_current.remove_status_listener(id);
                    }
                    let status_id =
                        new_current.add_status_listener(Arc::clone(&status_callback_for_handler));
                    inner_arc.lock().current_status_listener_id = Some(status_id);
                }
                if let Some(callback) = callback {
                    callback();
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
        if let (Some(id), Some(next)) = (inner.next_listener_id.take(), &inner.next) {
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

    /// Registers on the switch's own registry (NOT the current animation):
    /// `current` changes on a train-hop, so a delegated id would later be
    /// removed against the wrong animation. The internal per-current
    /// forwarder re-emits the active animation's transitions here.
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut inner = self.inner.lock();
        let id = ListenerId::new(inner.next_status_listener_id);
        inner.next_status_listener_id += 1;
        inner.status_listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.inner
            .lock()
            .status_listeners
            .retain(|(listener_id, _)| *listener_id != id);
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
    fn switch_rebinds_listeners_to_new_current() {
        use std::sync::atomic::AtomicUsize;

        let scheduler = Arc::new(Scheduler::new());
        let controller1 = create_controller(&scheduler, 0.8);
        let controller2 = create_controller(&scheduler, 0.3);

        let before1 = controller1.debug_value_listener_count();
        let before2 = controller2.debug_value_listener_count();

        let switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f32>>,
            Some(controller2.clone() as Arc<dyn Animation<f32>>),
        );
        assert_eq!(controller1.debug_value_listener_count(), before1 + 1);
        assert_eq!(controller2.debug_value_listener_count(), before2 + 1);

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let _id = switch.add_listener(Arc::new(move || {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        // Drive controller1 below controller2's value -> hop to controller2.
        controller1.set_value(0.2);
        assert_eq!(switch.value(), 0.3, "switch must hop to next");

        // The retired animation's listeners must be removed: further changes
        // on controller1 must not re-notify the switch.
        assert_eq!(
            controller1.debug_value_listener_count(),
            before1,
            "old current's value listener must be removed after the hop"
        );
        let after_hop = hits.load(Ordering::SeqCst);
        controller1.set_value(0.9);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            after_hop,
            "retired animation must not notify the switch"
        );

        // The new current still drives the switch.
        controller2.set_value(0.6);
        assert!(hits.load(Ordering::SeqCst) > after_hop);

        // dispose() removes the (rebound) listeners from the new current.
        switch.dispose();
        assert_eq!(
            controller2.debug_value_listener_count(),
            before2,
            "dispose must remove the rebound listener from the new current"
        );

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn status_listeners_survive_the_hop() {
        use std::sync::atomic::AtomicUsize;

        // Status listener ids are minted by the switch itself; they must
        // keep firing after a train-hop and stay removable by the same id
        // (previously they were delegated to the pre-hop `current`).
        let scheduler = Arc::new(Scheduler::new());
        let controller1 = create_controller(&scheduler, 0.8);
        let controller2 = create_controller(&scheduler, 0.3);

        let switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f32>>,
            Some(controller2.clone() as Arc<dyn Animation<f32>>),
        );

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let id = switch.add_status_listener(Arc::new(move |_status| {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        // Hop to controller2.
        controller1.set_value(0.2);
        assert_eq!(switch.value(), 0.3);

        // A status transition on the NEW current must reach the listener.
        // (set_value(1.0) -> Completed is guaranteed to differ from the
        // interior Forward status, so the duplicate filter cannot eat it.)
        let before = hits.load(Ordering::SeqCst);
        controller2.set_value(1.0);
        assert!(
            hits.load(Ordering::SeqCst) > before,
            "status listener must follow the switch to the new current"
        );

        // Removal by the switch-minted id must work after the hop.
        switch.remove_status_listener(id);
        let after_remove = hits.load(Ordering::SeqCst);
        controller2.set_value(0.0); // Completed -> Dismissed transition
        assert_eq!(
            hits.load(Ordering::SeqCst),
            after_remove,
            "removed status listener must not fire"
        );

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
        // Start at the lower bound so status is genuinely Dismissed: a mid-range
        // set_value now reports Forward per Flutter's _internalSetValue.
        let controller = create_controller(&scheduler, 0.0);

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

        let debug_str = format!("{switch:?}");
        assert!(debug_str.contains("AnimationSwitch"));
        assert!(debug_str.contains("0.5"));

        controller.dispose();
    }
}
