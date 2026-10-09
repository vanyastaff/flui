//! Core animation trait and types.

use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use std::fmt;
use std::rc::Rc;

/// Callback for animation status changes.
///
/// Called when an animation's status changes (e.g., from Forward to Completed).
pub type StatusCallback = Rc<dyn Fn(AnimationStatus)>;

/// Internal status relay borrowing the enclosing delivery's recovery context.
#[doc(hidden)]
pub type StatusObserver = Rc<dyn Fn(AnimationStatus, &mut Retirement)>;

/// The direction an animation is running.
///
/// This enum represents whether an animation is progressing forward (from begin to end)
/// or in reverse (from end to begin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationDirection {
    /// Animation is running forward (from begin to end).
    #[default]
    Forward,
    /// Animation is running in reverse (from end to begin).
    Reverse,
}

impl AnimationDirection {
    /// Returns the opposite direction.
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            AnimationDirection::Forward => AnimationDirection::Reverse,
            AnimationDirection::Reverse => AnimationDirection::Forward,
        }
    }
}

/// An animation that progresses from 0.0 to 1.0, or from begin to end.
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
/// fn use_animation<T: Clone + Send + Sync + 'static>(animation: &dyn Animation<T>) {
///     let value = animation.value();
///     let status = animation.status();
///
///     if status.is_running() {
///         // Animation is in progress
///     }
/// }
/// ```
pub trait Animation<T>: Listenable + fmt::Debug
where
    T: Clone + 'static,
{
    /// Returns the current value of the animation.
    fn value(&self) -> T;

    /// Returns the current status of the animation.
    fn status(&self) -> AnimationStatus;

    /// Observe status changes until the returned source-bound guard is dropped.
    /// The guard must not extend the animation owner's lifetime.
    fn subscribe_status(&self, callback: StatusCallback) -> crate::StatusSubscription;

    /// Register a relay borrowing the enclosing delivery's recovery context.
    #[doc(hidden)]
    fn subscribe_status_observer(&self, observer: StatusObserver) -> crate::StatusSubscription {
        self.subscribe_status(Rc::new(move |status| {
            let mut recovery = Retirement::new();
            recovery.run_with(|recovery| observer(status, recovery));
            recovery.finish();
        }))
    }

    /// Whether the animation is currently running.
    #[inline]
    fn is_animating(&self) -> bool {
        self.status().is_running()
    }

    /// Whether the animation is completed.
    #[inline]
    fn is_completed(&self) -> bool {
        self.status().is_completed()
    }

    /// Whether the animation is dismissed (at beginning).
    #[inline]
    fn is_dismissed(&self) -> bool {
        self.status().is_dismissed()
    }

    /// Whether the animation is running forward.
    #[inline]
    fn is_forward(&self) -> bool {
        self.status().is_forward()
    }

    /// Whether the animation is running in reverse.
    #[inline]
    fn is_reverse(&self) -> bool {
        self.status().is_reverse()
    }
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
        self.detach(&mut retirement);
        retirement.finish();
    }
}

impl ParentSubscription {
    pub(crate) fn detach(&self, retirement: &mut Retirement) {
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

/// The last combinator owner withdraws both channels before detaching parents.
pub(crate) struct ParentLinks {
    pub(crate) parent: Terminal<Rc<dyn Animation<f64>>>,
    pub(crate) notifier: Terminal<Rc<ChangeNotifier>>,
    pub(crate) status_notifier: Terminal<Rc<flui_foundation::Notifier<AnimationStatus>>>,
    value_sub: Terminal<Rc<ParentSubscription>>,
    status_sub: Terminal<crate::StatusSubscription>,
}

impl ParentLinks {
    pub(crate) fn new(
        parent: Rc<dyn Animation<f64>>,
        map: impl Fn(AnimationStatus) -> AnimationStatus + 'static,
    ) -> Rc<Self> {
        let parent = Terminal::new(parent);
        let notifier = Rc::new(ChangeNotifier::new());
        let status_notifier = Rc::new(flui_foundation::Notifier::new());
        let value_sub = link_parent(&parent, &notifier);
        let status_sub = link_parent_status(&parent, &status_notifier, map);
        Rc::new(Self {
            parent,
            notifier: Terminal::new(notifier),
            status_notifier: Terminal::new(status_notifier),
            value_sub: Terminal::new(value_sub),
            status_sub: Terminal::new(status_sub),
        })
    }

    pub(crate) fn inherit_failure(&self, recovery: &mut Retirement) {
        self.notifier.inherit_failure(recovery);
        self.status_notifier.inherit_failure(recovery);
    }

    pub(crate) fn subscribe_status(
        self: &Rc<Self>,
        callback: StatusCallback,
    ) -> crate::StatusSubscription {
        let id = self
            .status_notifier
            .add(Rc::new(move |status| callback(*status)));
        crate::StatusSubscription::new(self, id, Self::withdraw_status)
    }

    pub(crate) fn subscribe_status_observer(
        self: &Rc<Self>,
        observer: StatusObserver,
    ) -> crate::StatusSubscription {
        let id = self
            .status_notifier
            .add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)));
        crate::StatusSubscription::new(self, id, Self::withdraw_status)
    }

    fn withdraw_status(
        &self,
        id: ListenerId,
        recovery: &mut Retirement,
    ) -> Option<Rc<flui_foundation::notifier_generic::NotificationCallback<AnimationStatus>>> {
        self.inherit_failure(recovery);
        self.status_notifier.take_callback(id)
    }
}

impl Drop for ParentLinks {
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.inherit_failure(&mut recovery);
        let parent = self.parent.withdraw();
        let notifier = self.notifier.withdraw();
        let statuses = self.status_notifier.withdraw();
        let value_sub = self.value_sub.withdraw();
        let mut status_sub = self.status_sub.withdraw();
        let values = notifier.dispose_and_take_listeners();
        let callbacks = statuses.dispose_and_take_callbacks();
        value_sub.detach(&mut recovery);
        status_sub.get_mut().cancel_with_recovery(&mut recovery);
        for callback in values {
            recovery.retire(Terminal::new(callback));
        }
        for callback in callbacks {
            recovery.retire(Terminal::new(callback));
        }
        recovery.retire(value_sub);
        recovery.retire(status_sub);
        recovery.retire(parent);
        recovery.retire(notifier);
        recovery.retire(statuses);
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
    parent.subscribe_status_observer(Rc::new(move |status, recovery| {
        if let Some(notifier) = weak.upgrade() {
            notifier.notify_with_recovery(&map(status), recovery);
        }
    }))
}
