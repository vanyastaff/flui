//! `AnimationSwitch` - switches between animations when values cross.

use crate::animation::{Animation, Retirement, Terminal};
use crate::channels::{Channels, Withdrawn};
use crate::status::AnimationStatus;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId, Notifier};
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
/// use std::rc::Rc;
/// use std::time::Duration;
///
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

/// The switch's own listener channels live beside, not inside, its parent
/// state: `current` changes on a train-hop, so registrations cannot be
/// delegated to it.
struct SwitchOwner {
    inner: Terminal<Rc<RefCell<AnimationSwitchInner>>>,
    channels: Channels,
}

impl SwitchOwner {
    fn reject_status<T>(&self, callback: T) {
        let mut recovery = Retirement::new();
        self.channels.inherit_failure(&mut recovery.scope());
        recovery.retire(Terminal::new(callback));
        recovery.finish();
    }

    fn withdraw_status(
        &self,
        id: ListenerId,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<Withdrawn<AnimationStatus>> {
        self.channels.take_status(id, recovery)
    }
}

impl Drop for SwitchOwner {
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.channels.inherit_failure(&mut recovery.scope());
        let inner = self.inner.withdraw();
        let channels = self.channels.withdraw();
        dispose_switch(&inner, &channels, &mut recovery.scope());
        recovery.retire(inner);
        recovery.retire(channels);
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
    /// Last reported status.
    last_status: Option<AnimationStatus>,
    /// Listener IDs for cleanup.
    current_listener_id: Option<ListenerId>,
    next_listener_id: Option<ListenerId>,
    current_status_subscription: Option<Terminal<crate::StatusSubscription>>,
    disposed: bool,
}

fn detach_parents(
    current: &Rc<dyn Animation<f64>>,
    next: Option<&Terminal<Rc<dyn Animation<f64>>>>,
    value_id: Option<ListenerId>,
    status_subscription: Option<Terminal<crate::StatusSubscription>>,
    next_id: Option<ListenerId>,
    retirement: &mut RecoveryScope<'_>,
) {
    if let Some(id) = value_id {
        retirement.run(|| current.remove_listener(id));
    }
    cancel_status_subscription(status_subscription, retirement);
    if let (Some(id), Some(next)) = (next_id, next) {
        retirement.run(|| next.remove_listener(id));
    }
}

fn cancel_status_subscription(
    subscription: Option<Terminal<crate::StatusSubscription>>,
    recovery: &mut RecoveryScope<'_>,
) {
    if let Some(mut subscription) = subscription {
        subscription.get_mut().cancel_with_recovery(recovery);
        recovery.retire(subscription);
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
        let status_subscription = self.current_status_subscription.take();
        let next_id = self.next_listener_id.take();
        let mut retirement = Retirement::new();
        detach_parents(
            &current,
            next.as_ref(),
            value_id,
            status_subscription,
            next_id,
            &mut retirement.scope(),
        );
        retirement.retire(callback);
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

        let (actual_current, actual_next, mode) = if let Some(ref next_anim) = next {
            let current_value = current.value();
            let next_value = next_anim.value();

            if current_value == next_value {
                (Terminal::new(next_anim.get().clone()), None, None)
            } else {
                let mode = if current_value > next_value {
                    SwitchMode::Maximize
                } else {
                    SwitchMode::Minimize
                };
                (current, next, Some(mode))
            }
        } else {
            (current, None, None)
        };

        let inner = AnimationSwitchInner {
            current: actual_current,
            next: actual_next,
            mode,
            on_switched: None,
            last_status: None,
            current_listener_id: None,
            next_listener_id: None,
            current_status_subscription: None,
            disposed: false,
        };

        let this = Self {
            owner: Rc::new(SwitchOwner {
                inner: Terminal::new(Rc::new(RefCell::new(inner))),
                channels: Channels::new(),
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
        self.owner.inner.borrow().current.get().clone()
    }

    /// Builds the status forwarder that re-emits the current animation's
    /// status transitions through the switch's own status channel.
    fn make_status_callback(
        inner_weak: &std::rc::Weak<RefCell<AnimationSwitchInner>>,
        statuses: &Rc<Notifier<AnimationStatus>>,
    ) -> crate::StatusObserver {
        let inner_weak = inner_weak.clone();
        let statuses = Rc::downgrade(statuses);
        Rc::new(move |status, recovery| {
            if let Some(state) = inner_weak.upgrade() {
                let mut inner = state.borrow_mut();
                if inner.disposed {
                    return;
                }
                if inner.last_status != Some(status) {
                    inner.last_status = Some(status);
                    drop(inner);
                    if let Some(statuses) = statuses.upgrade() {
                        statuses.notify_with_recovery(&status, recovery);
                    }
                }
            }
        })
    }

    /// Sets up listeners on the current and next animations.
    fn setup_listeners(&self) {
        let inner_weak = Rc::downgrade(&self.owner.inner);
        let notifier = Rc::clone(self.owner.channels.values());
        let status_callback =
            Self::make_status_callback(&inner_weak, self.owner.channels.statuses());
        let status_callback_for_handler = Rc::clone(&status_callback);

        let value_handler = move |recovery: &mut RecoveryScope<'_>| {
            if let Some(state) = inner_weak.upgrade() {
                let (current, next, mode) = {
                    let inner = state.borrow();
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
                let mut inner = state.borrow_mut();
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
                    let old_status_subscription = inner.current_status_subscription.take();
                    inner.current_listener_id = inner.next_listener_id.take();
                    callback.clone_from(&inner.on_switched);
                    rebind = Some((old_current, old_value_id, old_status_subscription, next));
                }
                // Release the lock before touching other animations' listener
                // registries or running user callbacks.
                drop(inner);

                if let Some((old_current, old_value_id, old_status_subscription, new_current)) =
                    rebind
                {
                    if let Some(id) = old_value_id {
                        recovery.run(|| old_current.remove_listener(id));
                    }
                    cancel_status_subscription(old_status_subscription, recovery);
                    let mut subscription = Some(Terminal::new(
                        new_current.observe_status(Rc::clone(&status_callback_for_handler)),
                    ));
                    let admitted = {
                        let mut inner = state.borrow_mut();
                        if !inner.disposed && Rc::ptr_eq(inner.current.get(), &new_current) {
                            inner.current_status_subscription = subscription.take();
                            true
                        } else {
                            false
                        }
                    };
                    if admitted {
                        status_callback_for_handler(new_current.status(), recovery);
                    }
                    cancel_status_subscription(subscription, recovery);
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
        let mut subscription = Some(Terminal::new(status_parent.observe_status(status_callback)));
        let previous = {
            let mut inner = self.owner.inner.borrow_mut();
            if !inner.disposed && Rc::ptr_eq(inner.current.get(), &status_parent) {
                std::mem::replace(&mut inner.current_status_subscription, subscription.take())
            } else {
                None
            }
        };
        let mut recovery = Retirement::new();
        cancel_status_subscription(previous, &mut recovery.scope());
        cancel_status_subscription(subscription, &mut recovery.scope());
        recovery.finish();
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
        self.owner.channels.inherit_failure(&mut retirement.scope());
        dispose_switch(
            &self.owner.inner,
            &self.owner.channels,
            &mut retirement.scope(),
        );
        retirement.finish();
    }
}

fn dispose_switch(
    owner_inner: &Rc<RefCell<AnimationSwitchInner>>,
    channels: &Channels,
    retirement: &mut RecoveryScope<'_>,
) {
    let (current, next, value_id, status_subscription, next_id, callback) = {
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
            inner.current_status_subscription.take(),
            inner.next_listener_id.take(),
            inner.on_switched.take(),
        )
    };
    detach_parents(
        &current,
        next.as_ref(),
        value_id,
        status_subscription,
        next_id,
        retirement,
    );
    channels.dispose().retire_statuses_first(retirement);
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
    fn observe_status(&self, observer: crate::StatusObserver) -> crate::StatusSubscription {
        if self.owner.inner.borrow().disposed {
            self.owner.reject_status(observer);
            return crate::StatusSubscription::default();
        }
        let id = self.owner.channels.add_status(observer);
        crate::StatusSubscription::new(&self.owner, id, SwitchOwner::withdraw_status)
    }
}

impl Listenable for AnimationSwitch {
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.owner.channels.add_observer(observer)
    }

    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.owner.channels.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.owner.channels.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.owner.channels.remove_all_listeners();
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
