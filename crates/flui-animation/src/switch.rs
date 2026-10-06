//! `AnimationSwitch` - switches between animations when values cross.

use crate::animation::{Animation, Retirement, StatusCallback, Terminal};
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
/// # Examples
///
/// ```
/// use flui_animation::{AnimationSwitch, AnimationController, Animation};
/// use flui_scheduler::UpdateScheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
///
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
///
/// // Set different values
/// controller1.set_value(0.8);
/// controller2.set_value(0.3);
///
/// let switch = AnimationSwitch::new(
///     controller1.clone() as Arc<dyn Animation<f64>>,
///     Some(controller2.clone() as Arc<dyn Animation<f64>>),
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
    current: Terminal<Arc<dyn Animation<f64>>>,
    /// The next animation to potentially switch to.
    next: Option<Terminal<Arc<dyn Animation<f64>>>>,
    /// The mode for determining when to switch.
    mode: Option<SwitchMode>,
    /// Callback when switched.
    on_switched: Option<Terminal<Arc<dyn Fn() + Send + Sync>>>,
    /// Last reported value (for change detection).
    #[expect(dead_code)]
    last_value: Option<f64>,
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
    disposed: bool,
}

fn detach_parents(
    current: &Arc<dyn Animation<f64>>,
    next: Option<&Terminal<Arc<dyn Animation<f64>>>>,
    value_id: Option<ListenerId>,
    status_id: Option<ListenerId>,
    next_id: Option<ListenerId>,
    retirement: &mut Retirement,
) {
    if let Some(id) = value_id {
        retirement.run(|| current.remove_listener(id));
    }
    if let Some(id) = status_id {
        retirement.run(|| current.remove_status_listener(id));
    }
    if let (Some(id), Some(next)) = (next_id, next) {
        retirement.run(|| next.remove_listener(id));
    }
}

impl Drop for AnimationSwitchInner {
    fn drop(&mut self) {
        self.disposed = true;
        self.mode = None;
        let current = self.current.withdraw();
        let next = self.next.take();
        let callback = self.on_switched.take();
        let value_id = self.current_listener_id.take();
        let status_id = self.current_status_listener_id.take();
        let next_id = self.next_listener_id.take();
        let listeners: Vec<_> = std::mem::take(&mut self.status_listeners)
            .into_iter()
            .map(|(_, callback)| Terminal::new(callback))
            .collect();
        let mut retirement = Retirement::new();
        detach_parents(
            &current,
            next.as_ref(),
            value_id,
            status_id,
            next_id,
            &mut retirement,
        );
        retirement.retire(callback);
        for listener in listeners {
            retirement.retire(listener);
        }
        retirement.retire(current);
        retirement.retire(next);
        retirement.finish();
    }
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
    pub fn new(current: Arc<dyn Animation<f64>>, next: Option<Arc<dyn Animation<f64>>>) -> Self {
        let current = Terminal::new(current);
        let next = next.map(Terminal::new);
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
                (Terminal::new(next_anim.get().clone()), None)
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
            disposed: false,
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
        let old = {
            let mut inner = self.inner.lock();
            inner.on_switched.replace(Terminal::new(Arc::new(callback)))
        };
        drop(old);
        self
    }

    /// Returns the currently active animation.
    #[must_use]
    pub fn current(&self) -> Arc<dyn Animation<f64>> {
        self.inner.lock().current.get().clone()
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
                if inner.disposed {
                    return;
                }
                if inner.last_status != Some(status) {
                    inner.last_status = Some(status);
                    // Snapshot-then-fire: user callbacks run without the
                    // inner lock held (they may re-enter the switch).
                    let callbacks: Vec<Terminal<StatusCallback>> = inner
                        .status_listeners
                        .iter()
                        .map(|(_, cb)| Terminal::new(Arc::clone(cb)))
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
                if inner.disposed {
                    return;
                }

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
                    let old_current =
                        std::mem::replace(&mut inner.current, Terminal::new(Arc::clone(&next)));
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

        let (current, next) = {
            let inner = self.inner.lock();
            (
                Terminal::new(inner.current.get().clone()),
                inner.next.clone(),
            )
        };
        let current_id = current.add_listener(Arc::new(value_handler.clone()));
        self.admit_value_subscription(&current, current_id);
        if let Some(next) = next.as_ref() {
            let next_id = next.add_listener(Arc::new(value_handler));
            self.admit_value_subscription(next, next_id);
        }
        let status_parent = Terminal::new(self.current());
        let status_id = status_parent.add_status_listener(status_callback);
        let admitted = {
            let mut inner = self.inner.lock();
            if !inner.disposed && Arc::ptr_eq(inner.current.get(), &status_parent) {
                Some(inner.current_status_listener_id.replace(status_id))
            } else {
                None
            }
        };
        match admitted {
            Some(Some(old)) => status_parent.remove_status_listener(old),
            Some(None) => {}
            None => status_parent.remove_status_listener(status_id),
        }
    }

    fn admit_value_subscription(&self, parent: &Arc<dyn Animation<f64>>, id: ListenerId) {
        let admitted = {
            let mut inner = self.inner.lock();
            if inner.disposed {
                None
            } else if Arc::ptr_eq(inner.current.get(), parent) {
                Some(inner.current_listener_id.replace(id))
            } else if inner
                .next
                .as_ref()
                .is_some_and(|next| Arc::ptr_eq(next.get(), parent))
            {
                Some(inner.next_listener_id.replace(id))
            } else {
                None
            }
        };
        match admitted {
            Some(Some(old)) => parent.remove_listener(old),
            Some(None) => {}
            None => parent.remove_listener(id),
        }
    }

    /// Disposes of this animation switch, cleaning up listeners.
    pub fn dispose(&self) {
        let (current, next, value_id, status_id, next_id) = {
            let mut inner = self.inner.lock();
            if inner.disposed {
                return;
            }
            inner.disposed = true;
            inner.mode = None;
            (
                Terminal::new(inner.current.get().clone()),
                inner.next.clone(),
                inner.current_listener_id.take(),
                inner.current_status_listener_id.take(),
                inner.next_listener_id.take(),
            )
        };
        let mut retirement = Retirement::new();
        detach_parents(
            &current,
            next.as_ref(),
            value_id,
            status_id,
            next_id,
            &mut retirement,
        );
        retirement.retire(current);
        retirement.retire(next);
        retirement.finish();
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

impl Animation<f64> for AnimationSwitch {
    #[inline]
    fn value(&self) -> f64 {
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
        let removed = {
            let mut inner = self.inner.lock();
            inner
                .status_listeners
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|index| inner.status_listeners.remove(index).1)
        };
        drop(Terminal::new(removed));
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
    use flui_scheduler::UpdateScheduler;

    use std::time::Duration;

    fn create_controller(scheduler: &UpdateScheduler, value: f64) -> Arc<AnimationController> {
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        controller.set_value(value);
        controller
    }

    /// Disposing an `AnimationSwitch` detaches from **both** trains: status +
    /// value listeners from the current train, and the value listener from the
    /// next one. The route layer disposes a hopper that never hopped, so a leak
    /// here would keep a disposed route's controller alive and notifying.
    ///
    /// This pins the *pre-hop* case.
    ///
    /// Red-check: delete the `next_listener_id` branch of `AnimationSwitch::dispose`.
    #[test]
    fn dispose_before_a_hop_detaches_from_both_trains() {
        let scheduler = UpdateScheduler::new();
        let controller1 = create_controller(&scheduler, 0.8);
        let controller2 = create_controller(&scheduler, 0.3);

        let value1 = controller1.debug_value_listener_count();
        let value2 = controller2.debug_value_listener_count();
        let status1 = controller1.debug_status_listener_count();

        let switch = AnimationSwitch::new(
            controller1.clone() as Arc<dyn Animation<f64>>,
            Some(controller2.clone() as Arc<dyn Animation<f64>>),
        );
        assert_eq!(controller1.debug_value_listener_count(), value1 + 1);
        assert_eq!(controller2.debug_value_listener_count(), value2 + 1);
        assert_eq!(controller1.debug_status_listener_count(), status1 + 1);

        switch.dispose();

        assert_eq!(
            controller1.debug_value_listener_count(),
            value1,
            "the current train's value listener must be removed"
        );
        assert_eq!(
            controller1.debug_status_listener_count(),
            status1,
            "the current train's status listener must be removed"
        );
        assert_eq!(
            controller2.debug_value_listener_count(),
            value2,
            "the NEXT train's value listener must be removed too"
        );

        // And a disposed switch never hops.
        controller1.set_value(0.2);
        assert!(
            Arc::ptr_eq(
                &switch.current(),
                &(controller1.clone() as Arc<dyn Animation<f64>>)
            ),
            "a disposed switch must not hop"
        );

        controller1.dispose();
        controller2.dispose();
    }
}
