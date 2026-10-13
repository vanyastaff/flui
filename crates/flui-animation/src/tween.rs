//! `TweenAnimation` - maps f64 animations to any type T.

use crate::animation::{Animation, ParentLinks, Retirement, Terminal};
use crate::status::AnimationStatus;
use crate::tween_types::Animatable;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::rc::Rc;

/// An animation that maps its parent's progress through an [`Animatable`].
///
/// The result is `Animation<A::Value>`; the output needs `Clone` as required by
/// [`Animation`], and need not implement `Animatable` or `Debug`. A parent is
/// typically normalized to 0..1; the mapping receives its progress unchanged.
/// The mapping is moved into the observer and needs `Clone` only to clone it.
///
/// # Type Parameters
///
/// * `A` - The mapping type; `A::Value` determines its output.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, TweenAnimation, Animation};
/// use flui_animation::Tween;
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let controller = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
///
/// let tween = Tween::new(0.0, 100.0);
/// let float_animation = TweenAnimation::new(
///     tween,
///     controller as Rc<dyn Animation<f64>>,
/// );
/// ```
#[derive(Clone)]
pub struct TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    tween: Terminal<A>,
    links: Terminal<Rc<ParentLinks>>,
}

impl<A> Drop for TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.links.inherit_failure(&mut recovery.scope());
        let links = self.links.withdraw();
        let tween = self.tween.withdraw();
        recovery.run(|| drop(links.into_inner()));
        recovery.retire(tween);
        recovery.finish();
    }
}

impl<A> TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    /// Create a new tween animation.
    ///
    /// # Arguments
    ///
    /// * `tween` - The tween that maps f64 → T
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    #[must_use]
    pub fn new(tween: A, parent: Rc<dyn Animation<f64>>) -> Self {
        Self {
            tween: Terminal::new(tween),
            links: Terminal::new(ParentLinks::new(parent, |status| status)),
        }
    }

    /// Get a reference to the tween.
    #[inline]
    #[must_use]
    pub fn tween(&self) -> &A {
        &self.tween
    }

    /// Get a reference to the parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> &Rc<dyn Animation<f64>> {
        &self.links.parent
    }
}

impl<A> Animation<A::Value> for TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    #[inline]
    fn value(&self) -> A::Value {
        let t = self.links.parent.value();
        self.tween.transform(t)
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.links.parent.status()
    }

    fn is_animating(&self) -> bool {
        self.links.parent.is_animating()
    }

    fn observe_status(&self, observer: crate::StatusObserver) -> crate::StatusSubscription {
        self.links.observe_status(observer)
    }
}

impl<A> Listenable for TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.links.channels.add_observer(observer)
    }
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.links.channels.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.links.channels.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.links.channels.remove_all_listeners();
    }
}

impl<A> fmt::Debug for TweenAnimation<A>
where
    A: Animatable + 'static,
    A::Value: Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TweenAnimation")
            .field("parent", &self.links.parent)
            .finish_non_exhaustive()
    }
}
