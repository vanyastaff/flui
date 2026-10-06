//! Core animation trait and types.

use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use std::fmt;
use std::sync::Arc;

/// Callback for animation status changes.
///
/// Called when an animation's status changes (e.g., from Forward to Completed).
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

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
/// # Thread Safety
///
/// All animations must be thread-safe (`Send + Sync`).
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
pub trait Animation<T>: Listenable + Send + Sync + fmt::Debug
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns the current value of the animation.
    fn value(&self) -> T;

    /// Returns the current status of the animation.
    fn status(&self) -> AnimationStatus;

    /// Add a status listener (called when animation starts, completes, etc.).
    ///
    /// Returns a listener ID that can be used to remove the listener later.
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;

    /// Remove a status listener.
    fn remove_status_listener(&self, id: ListenerId);

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
    // `Mutex<Option<…>>` makes the `Send`-only teardown closure `Sync` so the
    // enclosing combinator stays `Send + Sync`; the closure runs once, on drop.
    teardown: parking_lot::Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl ParentSubscription {
    pub(crate) fn new(teardown: impl FnMut() + Send + 'static) -> Arc<Self> {
        Arc::new(Self {
            teardown: parking_lot::Mutex::new(Some(Box::new(teardown))),
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
        let teardown = self.teardown.lock().take();
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

    fn get_mut(&mut self) -> &mut T {
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
pub(crate) struct Retirement {
    incoming: bool,
    first: Option<Box<dyn std::any::Any + Send>>,
}

impl Retirement {
    pub(crate) fn new() -> Self {
        Self {
            incoming: std::thread::panicking(),
            first: None,
        }
    }

    pub(crate) fn run(&mut self, action: impl FnOnce()) {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
            if self.incoming || self.first.is_some() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                self.first = Some(payload);
            }
        }
    }

    pub(crate) fn retire<T>(&mut self, value: T) {
        if self.incoming || self.first.is_some() {
            std::mem::forget(value);
        } else {
            self.run(|| drop(value));
        }
    }

    pub(crate) fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            std::panic::resume_unwind(payload);
        }
    }
}

impl Drop for Retirement {
    fn drop(&mut self) {
        if let Some(payload) = self.first.take() {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

/// Subscribe `notifier` to re-emit whenever `parent`'s value changes, returning
/// a shared [`ParentSubscription`] that removes the subscription on the last
/// combinator clone's drop.
///
/// The parent's callback holds only a `Weak` reference to `notifier`, so the
/// subscription never keeps the combinator's notifier alive on its own.
pub(crate) fn link_parent<T>(
    parent: &Arc<dyn Animation<T>>,
    notifier: &Arc<ChangeNotifier>,
) -> Arc<ParentSubscription>
where
    T: Clone + Send + Sync + 'static,
{
    let weak = Arc::downgrade(notifier);
    let id = parent.add_listener(Arc::new(move || {
        if let Some(notifier) = weak.upgrade() {
            notifier.notify_listeners();
        }
    }));
    let parent = Arc::clone(parent);
    ParentSubscription::new(move || parent.remove_listener(id))
}
