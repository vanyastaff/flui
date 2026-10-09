//! `AnimationSwitch` - switches between animations when values cross.

use crate::animation::{Animation, Retirement, StatusCallback, Terminal};
use crate::status::AnimationStatus;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

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
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
///
/// let controller1 = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
/// let controller2 = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
///
/// // Set different values
/// controller1.set_value(0.8);
/// controller2.set_value(0.3);
///
/// let switch = AnimationSwitch::new(
///     controller1.clone() as Rc<dyn Animation<f64>>,
///     Some(controller2.clone() as Rc<dyn Animation<f64>>),
/// );
///
/// // Initially uses controller1's value
/// assert_eq!(switch.value(), 0.8);
///
/// // When controller1's value falls below controller2's value,
/// // the switch will automatically switch to controller2
/// ```
pub struct AnimationSwitch {
    owner: Rc<SwitchOwner>,
}

struct SwitchOwner {
    inner: Terminal<Rc<RefCell<AnimationSwitchInner>>>,
    notifier: Terminal<Rc<ChangeNotifier>>,
}

impl Drop for SwitchOwner {
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.notifier.inherit_failure(&mut recovery.scope());
        self.inner
            .borrow()
            .status_listeners
            .inherit_failure(&mut recovery.scope());
        let inner = self.inner.withdraw();
        let notifier = self.notifier.withdraw();
        dispose_switch(&inner, &notifier, &mut recovery.scope());
        recovery.retire(inner);
        recovery.retire(notifier);
        recovery.finish();
    }
}

struct AnimationSwitchInner {
    /// The currently active animation.
    current: Terminal<Rc<dyn Animation<f64>>>,
    /// The next animation to potentially switch to.
    next: Option<Terminal<Rc<dyn Animation<f64>>>>,
    /// The mode for determining when to switch.
    mode: Option<SwitchMode>,
    /// Callback when switched.
    on_switched: Option<Terminal<Rc<dyn Fn()>>>,
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
    status_listeners: Rc<flui_foundation::Notifier<AnimationStatus>>,
    disposed: bool,
}

fn detach_parents(
    current: &Rc<dyn Animation<f64>>,
    next: Option<&Terminal<Rc<dyn Animation<f64>>>>,
    value_id: Option<ListenerId>,
    status_id: Option<ListenerId>,
    next_id: Option<ListenerId>,
    retirement: &mut RecoveryScope<'_>,
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
        let listeners = self.status_listeners.dispose_and_take_callbacks();
        let mut retirement = Retirement::new();
        detach_parents(
            &current,
            next.as_ref(),
            value_id,
            status_id,
            next_id,
            &mut retirement.scope(),
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
    pub fn new(current: Rc<dyn Animation<f64>>, next: Option<Rc<dyn Animation<f64>>>) -> Self {
        let current = Terminal::new(current);
        let next = next.map(Terminal::new);
        let notifier = Rc::new(ChangeNotifier::new());

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
            status_listeners: Rc::new(flui_foundation::Notifier::new()),
            disposed: false,
        };

        let this = Self {
            owner: Rc::new(SwitchOwner {
                inner: Terminal::new(Rc::new(RefCell::new(inner))),
                notifier: Terminal::new(notifier),
            }),
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
        F: Fn() + 'static,
    {
        let old = {
            let mut inner = self.owner.inner.borrow_mut();
            inner.on_switched.replace(Terminal::new(Rc::new(callback)))
        };
        drop(old);
        self
    }

    /// Returns the currently active animation.
    #[must_use]
    pub fn current(&self) -> Rc<dyn Animation<f64>> {
        self.owner.inner.borrow_mut().current.get().clone()
    }

    /// Builds the status forwarder that re-emits the current animation's
    /// status transitions through our notifier.
    fn make_status_callback(
        inner_weak: &std::rc::Weak<RefCell<AnimationSwitchInner>>,
    ) -> crate::animation::StatusObserver {
        let inner_weak = inner_weak.clone();
        Rc::new(move |status, recovery| {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let mut inner = inner_arc.borrow_mut();
                if inner.disposed {
                    return;
                }
                if inner.last_status != Some(status) {
                    inner.last_status = Some(status);
                    let listeners = Rc::clone(&inner.status_listeners);
                    drop(inner);
                    listeners.notify_with_recovery(&status, recovery);
                }
            }
        })
    }

    /// Sets up listeners on the current and next animations.
    fn setup_listeners(&self) {
        let inner_weak = Rc::downgrade(&self.owner.inner);
        let notifier = Rc::clone(&self.owner.notifier);
        let status_callback = Self::make_status_callback(&inner_weak);
        let status_callback_for_handler = Rc::clone(&status_callback);

        let value_handler = move |recovery: &mut RecoveryScope<'_>| {
            if let Some(inner_arc) = inner_weak.upgrade() {
                let (current, next, mode) = {
                    let inner = inner_arc.borrow();
                    if inner.disposed {
                        return;
                    }
                    (Rc::clone(&inner.current), inner.next.clone(), inner.mode)
                };
                // Parent methods are user code and may re-enter this switch.
                let should_switch = if let (Some(mode), Some(next)) = (mode, &next) {
                    let current_value = current.value();
                    let next_value = next.value();

                    match mode {
                        SwitchMode::Minimize => next_value <= current_value,
                        SwitchMode::Maximize => next_value >= current_value,
                    }
                } else {
                    false
                };
                let mut inner = inner_arc.borrow_mut();
                if inner.disposed || !Rc::ptr_eq(inner.current.get(), &current) {
                    return;
                }

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
                        std::mem::replace(&mut inner.current, Terminal::new(Rc::clone(&next)));
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
                        new_current.add_status_observer(Rc::clone(&status_callback_for_handler));
                    let admitted = {
                        let mut inner = inner_arc.borrow_mut();
                        if !inner.disposed && Rc::ptr_eq(inner.current.get(), &new_current) {
                            inner.current_status_listener_id = Some(status_id);
                            true
                        } else {
                            false
                        }
                    };
                    if admitted {
                        status_callback_for_handler(new_current.status(), recovery);
                    } else {
                        new_current.remove_status_listener(status_id);
                    }
                }
                if let Some(callback) = callback {
                    recovery.run(&**callback);
                    recovery.retire(callback);
                }

                // Notify listeners of value change
                notifier.notify_listeners_with_recovery(recovery);
            }
        };

        let (current, next) = {
            let inner = self.owner.inner.borrow_mut();
            (
                Terminal::new(inner.current.get().clone()),
                inner.next.clone(),
            )
        };
        let current_id = current.add_observer(Rc::new(value_handler.clone()));
        self.admit_value_subscription(&current, current_id);
        if let Some(next) = next.as_ref() {
            let next_id = next.add_observer(Rc::new(value_handler));
            self.admit_value_subscription(next, next_id);
        }
        let status_parent = Terminal::new(self.current());
        let status_id = status_parent.add_status_observer(status_callback);
        let admitted = {
            let mut inner = self.owner.inner.borrow_mut();
            if !inner.disposed && Rc::ptr_eq(inner.current.get(), &status_parent) {
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

    fn admit_value_subscription(&self, parent: &Rc<dyn Animation<f64>>, id: ListenerId) {
        let admitted = {
            let mut inner = self.owner.inner.borrow_mut();
            if inner.disposed {
                None
            } else if Rc::ptr_eq(inner.current.get(), parent) {
                Some(inner.current_listener_id.replace(id))
            } else if inner
                .next
                .as_ref()
                .is_some_and(|next| Rc::ptr_eq(next.get(), parent))
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
        let mut retirement = Retirement::new();
        self.owner.notifier.inherit_failure(&mut retirement.scope());
        self.owner
            .inner
            .borrow()
            .status_listeners
            .inherit_failure(&mut retirement.scope());
        dispose_switch(
            &self.owner.inner,
            &self.owner.notifier,
            &mut retirement.scope(),
        );
        retirement.finish();
    }
}

fn dispose_switch(
    owner_inner: &Rc<RefCell<AnimationSwitchInner>>,
    owner_notifier: &Rc<ChangeNotifier>,
    retirement: &mut RecoveryScope<'_>,
) {
    let (current, next, value_id, status_id, next_id, callback, listeners) = {
        let mut inner = owner_inner.borrow_mut();
        if inner.disposed {
            return;
        }
        inner.disposed = true;
        inner.mode = None;
        (
            Terminal::new(inner.current.get().clone()),
            inner.next.take(),
            inner.current_listener_id.take(),
            inner.current_status_listener_id.take(),
            inner.next_listener_id.take(),
            inner.on_switched.take(),
            Rc::clone(&inner.status_listeners),
        )
    };
    detach_parents(
        &current,
        next.as_ref(),
        value_id,
        status_id,
        next_id,
        retirement,
    );
    let status_callbacks = listeners.dispose_and_take_callbacks();
    let value_callbacks = owner_notifier.dispose_and_take_listeners();
    for callback in status_callbacks {
        retirement.retire(Terminal::new(callback));
    }
    for callback in value_callbacks {
        retirement.retire(Terminal::new(callback));
    }
    retirement.retire(callback);
    retirement.retire(current);
    retirement.retire(next);
}

impl Clone for AnimationSwitch {
    fn clone(&self) -> Self {
        Self {
            owner: Rc::clone(&self.owner),
        }
    }
}

impl Animation<f64> for AnimationSwitch {
    #[inline]
    fn value(&self) -> f64 {
        self.current().value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.current().status()
    }

    fn is_animating(&self) -> bool {
        let parent = {
            let inner = self.owner.inner.borrow();
            if inner.disposed {
                return false;
            }
            Rc::clone(&inner.current)
        };
        parent.is_animating()
    }

    /// Registers on the switch's own registry (NOT the current animation):
    /// `current` changes on a train-hop, so a delegated id would later be
    /// removed against the wrong animation. The internal per-current
    /// forwarder re-emits the active animation's transitions here.
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let listeners = Rc::clone(&self.owner.inner.borrow().status_listeners);
        listeners.add(Rc::new(move |status| callback(*status)))
    }

    fn add_status_observer(&self, observer: crate::animation::StatusObserver) -> ListenerId {
        let listeners = Rc::clone(&self.owner.inner.borrow().status_listeners);
        listeners.add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)))
    }

    fn remove_status_listener(&self, id: ListenerId) {
        let listeners = Rc::clone(&self.owner.inner.borrow().status_listeners);
        let callback = listeners.take_callback(id);
        let mut recovery = Retirement::new();
        listeners.inherit_failure(&mut recovery.scope());
        self.owner.notifier.inherit_failure(&mut recovery.scope());
        recovery.retire(callback);
        recovery.finish();
    }
}

impl Listenable for AnimationSwitch {
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.owner.notifier.add_observer(observer)
    }

    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.owner.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        let callback = self.owner.notifier.take_listener(id);
        let mut recovery = Retirement::new();
        self.owner.notifier.inherit_failure(&mut recovery.scope());
        self.owner
            .inner
            .borrow()
            .status_listeners
            .inherit_failure(&mut recovery.scope());
        recovery.retire(callback);
        recovery.finish();
    }

    fn remove_all_listeners(&self) {
        let callbacks = self.owner.notifier.take_listeners();
        let mut recovery = Retirement::new();
        self.owner.notifier.inherit_failure(&mut recovery.scope());
        self.owner
            .inner
            .borrow()
            .status_listeners
            .inherit_failure(&mut recovery.scope());
        for callback in callbacks {
            recovery.retire(callback);
        }
        recovery.finish();
    }
}

impl fmt::Debug for AnimationSwitch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (current, has_next) = {
            let inner = self.owner.inner.borrow();
            (
                Terminal::new(inner.current.get().clone()),
                inner.next.is_some(),
            )
        };
        f.debug_struct("AnimationSwitch")
            .field("value", &current.value())
            .field("status", &current.status())
            .field("has_next", &has_next)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;

    use std::time::Duration;

    fn create_controller(value: f64) -> Rc<AnimationController> {
        let controller = Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
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
        let controller1 = create_controller(0.8);
        let controller2 = create_controller(0.3);

        let value1 = controller1.debug_value_listener_count();
        let value2 = controller2.debug_value_listener_count();
        let status1 = controller1.debug_status_listener_count();

        let switch = AnimationSwitch::new(
            controller1.clone() as Rc<dyn Animation<f64>>,
            Some(controller2.clone() as Rc<dyn Animation<f64>>),
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
            Rc::ptr_eq(
                &switch.current(),
                &(controller1.clone() as Rc<dyn Animation<f64>>)
            ),
            "a disposed switch must not hop"
        );

        controller1.dispose();
        controller2.dispose();
    }
}
