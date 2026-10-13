//! Core animation trait and types.

use crate::channels::{Channels, Withdrawn};
use crate::status::AnimationStatus;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use std::fmt;
use std::rc::Rc;

/// Callback for animation status changes.
///
/// Called when an animation's status changes (e.g., from Forward to ForwardSettled).
pub type StatusCallback = Rc<dyn Fn(AnimationStatus)>;

/// A status observer that runs inside the notifying delivery's [`RecoveryScope`].
///
/// Relays that forward a status (a wrapper re-emitting its parent's
/// transitions) register this form so the first failure of the whole delivery
/// stays authoritative; see [`Animation::observe_status`].
pub type StatusObserver = Rc<dyn Fn(AnimationStatus, &mut RecoveryScope<'_>)>;

/// The direction an animation is running.
///
/// This enum represents whether an animation is progressing forward (from begin to end)
/// or in reverse (from end to begin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub(crate) enum AnimationDirection {
    /// Animation is running forward (from begin to end).
    #[default]
    Forward,
    /// Animation is running in reverse (from end to begin).
    Reverse,
}

/// An observable value with a directional phase and explicit run activity.
///
/// This is the foundation of FLUI's animation system. All animations implement
/// this trait and the `Listenable` trait, allowing widgets to rebuild when the
/// animation value changes.
///
/// # Type Parameter
///
/// * `T` - The type of value this animation produces (e.g., `f64`, `Color`, `Size`)
///
/// # Ownership
///
/// Animation values and callbacks belong to the UI owner. Implementations may
/// capture `Rc` state; cross-thread producers use the owner's wake or IO edge.
///
/// # Examples
///
/// ```
/// use flui_animation::Animation;
/// use flui_animation::AnimationStatus;
///
/// fn use_animation<T: Clone + 'static>(animation: &dyn Animation<T>) {
///     let value = animation.value();
///     let status = animation.status();
///
///     if animation.is_animating() {
///         // The source owns a run, including a paused run.
///     }
/// }
/// ```
pub trait Animation<T>: Listenable + fmt::Debug
where
    T: Clone + 'static,
{
    /// Returns the current value of the animation.
    fn value(&self) -> T;

    /// Returns the directional phase, independent of run ownership and value.
    fn status(&self) -> AnimationStatus;

    /// Observe status changes, inside the notifying delivery's
    /// [`RecoveryScope`], until the returned source-bound guard is dropped.
    ///
    /// Implementations forward the observer to the channel that notifies, so a
    /// wrapper's observers share its parent's failure custody. The guard must
    /// not extend the animation owner's lifetime.
    fn observe_status(&self, observer: StatusObserver) -> crate::StatusSubscription;

    /// Call `callback` on status changes until the returned guard is dropped.
    ///
    /// An observer that ignores the scope; a panic in it is still contained
    /// by the notifying delivery.
    fn subscribe_status(&self, callback: StatusCallback) -> crate::StatusSubscription {
        self.observe_status(Rc::new(move |status, _| callback(status)))
    }

    /// Whether the source owns an active run, including a paused run.
    ///
    /// Directional status is independent: a constant or a manually positioned
    /// animation may report [`AnimationStatus::Forward`] without owning a run.
    /// Owners report run ownership, combinators forward their source's answer,
    /// and fixed values return `false`.
    fn is_animating(&self) -> bool;
}

/// A shared handle that removes a combinator's value-subscription from its
/// parent animation when the **last** clone of the combinator is dropped.
///
/// Combinators (`CurvedAnimation`, `ReverseAnimation`, ...) re-emit their
/// parent's value changes to their own listeners by subscribing to the parent.
/// Because combinators derive `Clone` and share one `notifier` across clones,
/// the subscription is reference-counted here and torn down exactly once, on the
/// final drop — never while a sibling clone is still alive.
pub(crate) struct ParentSubscription {
    teardown: std::cell::RefCell<Option<Box<dyn FnMut()>>>,
}

impl ParentSubscription {
    pub(crate) fn new(teardown: impl FnMut() + 'static) -> Rc<Self> {
        Rc::new(Self {
            teardown: std::cell::RefCell::new(Some(Box::new(teardown))),
        })
    }
}

impl fmt::Debug for ParentSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParentSubscription").finish_non_exhaustive()
    }
}

impl Drop for ParentSubscription {
    fn drop(&mut self) {
        let mut retirement = Retirement::new();
        self.detach(&mut retirement.scope());
        retirement.finish();
    }
}

impl ParentSubscription {
    pub(crate) fn detach(&self, retirement: &mut RecoveryScope<'_>) {
        let teardown = self.teardown.borrow_mut().take();
        if let Some(teardown) = teardown {
            let mut teardown = Terminal::new(teardown);
            retirement.run(|| (teardown.get_mut())());
            retirement.retire(teardown);
        }
    }
}

/// Private custody for independently owned animation resources. Owners withdraw
/// every field before retirement; unwind retains the remaining opaque envelopes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Terminal<T>(Option<T>);

impl<T> Terminal<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Some(value))
    }

    pub(crate) fn get(&self) -> &T {
        self.0
            .as_ref()
            .expect("BUG: a live animation owns its field")
    }

    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.0
            .as_mut()
            .expect("BUG: a live animation owns its field")
    }

    pub(crate) fn into_inner(mut self) -> T {
        self.0
            .take()
            .expect("BUG: a withdrawn owner owns its field")
    }

    pub(crate) fn withdraw(&mut self) -> Self {
        Self(self.0.take())
    }
}

impl<T> std::ops::Deref for Terminal<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.get()
    }
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Terminal<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Terminal<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::new)
    }
}

impl<T> Drop for Terminal<T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            std::mem::forget(self.0.take());
        }
    }
}

/// Keeps failure priority explicit while subscription detachments continue.
pub(crate) use flui_foundation::panic::PanicRecovery as Retirement;

/// The last combinator owner closes its channels before detaching parents.
pub(crate) struct ParentLinks {
    pub(crate) parent: Terminal<Rc<dyn Animation<f64>>>,
    pub(crate) channels: Channels,
    value_sub: Terminal<Rc<ParentSubscription>>,
    status_sub: Terminal<crate::StatusSubscription>,
}

impl ParentLinks {
    pub(crate) fn new(
        parent: Rc<dyn Animation<f64>>,
        map: impl Fn(AnimationStatus) -> AnimationStatus + 'static,
    ) -> Rc<Self> {
        let parent = Terminal::new(parent);
        let channels = Channels::new();
        let value_sub = link_parent(&parent, channels.values());
        let status_sub = link_parent_status(&parent, channels.statuses(), map);
        Rc::new(Self {
            parent,
            channels,
            value_sub: Terminal::new(value_sub),
            status_sub: Terminal::new(status_sub),
        })
    }

    pub(crate) fn inherit_failure(&self, recovery: &mut RecoveryScope<'_>) {
        self.channels.inherit_failure(recovery);
    }

    pub(crate) fn observe_status(
        self: &Rc<Self>,
        observer: StatusObserver,
    ) -> crate::StatusSubscription {
        let id = self.channels.add_status(observer);
        crate::StatusSubscription::new(self, id, Self::withdraw_status)
    }

    fn withdraw_status(
        &self,
        id: ListenerId,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<Withdrawn<AnimationStatus>> {
        self.channels.take_status(id, recovery)
    }
}

impl Drop for ParentLinks {
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.inherit_failure(&mut recovery.scope());
        let parent = self.parent.withdraw();
        let channels = self.channels.withdraw();
        let value_sub = self.value_sub.withdraw();
        let mut status_sub = self.status_sub.withdraw();
        let disposed = channels.dispose();
        value_sub.detach(&mut recovery.scope());
        status_sub
            .get_mut()
            .cancel_with_recovery(&mut recovery.scope());
        disposed.retire_values_first(&mut recovery.scope());
        recovery.retire(value_sub);
        recovery.retire(status_sub);
        recovery.retire(parent);
        recovery.retire(channels);
        recovery.finish();
    }
}

/// Subscribe `notifier` to re-emit whenever `parent`'s value changes, returning
/// a shared [`ParentSubscription`] that removes the subscription on the last
/// combinator clone's drop.
///
/// The parent's callback holds only a `Weak` reference to `notifier`, so the
/// subscription never keeps the combinator's notifier alive on its own.
pub(crate) fn link_parent<T>(
    parent: &Rc<dyn Animation<T>>,
    notifier: &Rc<ChangeNotifier>,
) -> Rc<ParentSubscription>
where
    T: Clone + 'static,
{
    let weak = Rc::downgrade(notifier);
    let id = parent.add_observer(Rc::new(move |recovery| {
        if let Some(notifier) = weak.upgrade() {
            notifier.notify_listeners_with_recovery(recovery);
        }
    }));
    let parent = Rc::clone(parent);
    ParentSubscription::new(move || parent.remove_listener(id))
}

/// Status listeners belong to the wrapper; its parent holds only a weak relay.
pub(crate) fn link_parent_status<T: Clone + 'static>(
    parent: &Rc<dyn Animation<T>>,
    notifier: &Rc<flui_foundation::Notifier<AnimationStatus>>,
    map: impl Fn(AnimationStatus) -> AnimationStatus + 'static,
) -> crate::StatusSubscription {
    let weak = Rc::downgrade(notifier);
    parent.observe_status(Rc::new(move |status, recovery| {
        if let Some(notifier) = weak.upgrade() {
            notifier.notify_with_recovery(&map(status), recovery);
        }
    }))
}
