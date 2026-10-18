//! The animation slot a transition's state keeps: the proxy its render object
//! listens to for life, retargeted when the view hands it another animation.

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};

/// A stable [`ProxyAnimation`] retargeted in place when the view hands the
/// transition a different animation.
///
/// The proxy's own parent is the record of what is installed: a replacement
/// that unwinds before the swap leaves the old parent in place, so the next
/// rebuild sees the difference and retries.
#[derive(Debug)]
pub(super) struct ProxySlot<T: Clone + 'static> {
    proxy: ProxyAnimation<T>,
}

impl<T: Clone + 'static> ProxySlot<T> {
    pub(super) fn new(source: &Rc<dyn Animation<T>>) -> Self {
        Self {
            proxy: ProxyAnimation::new(Rc::clone(source)),
        }
    }

    /// The proxy the render object subscribes to.
    pub(super) fn proxy(&self) -> ProxyAnimation<T> {
        self.proxy.clone()
    }

    /// Points the proxy at `source` unless it already wraps that animation.
    pub(super) fn retarget(&mut self, source: &Rc<dyn Animation<T>>) {
        if !Rc::ptr_eq(&self.proxy.parent(), source) {
            self.proxy.set_parent(Rc::clone(source));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use flui_animation::{
        AnimationController, AnimationStatus, StatusObserver, StatusSubscription,
    };
    use flui_foundation::notifier::ListenerObserver;
    use flui_foundation::{Listenable, ListenerId};

    use super::*;

    /// An animation whose first `status` read panics, as user code may while
    /// the proxy subscribes to it.
    #[derive(Debug)]
    struct StatusPanicsOnce {
        inner: AnimationController,
        armed: Cell<bool>,
    }

    impl Listenable for StatusPanicsOnce {
        fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
            self.inner.add_observer(observer)
        }

        fn remove_listener(&self, id: ListenerId) {
            self.inner.remove_listener(id);
        }

        fn remove_all_listeners(&self) {
            self.inner.remove_all_listeners();
        }
    }

    impl Animation<f64> for StatusPanicsOnce {
        fn value(&self) -> f64 {
            self.inner.value()
        }

        fn status(&self) -> AnimationStatus {
            assert!(!self.armed.replace(false), "the first status read fails");
            self.inner.status()
        }

        fn is_animating(&self) -> bool {
            self.inner.is_animating()
        }

        fn observe_status(&self, observer: StatusObserver) -> StatusSubscription {
            self.inner.observe_status(observer)
        }
    }

    /// A replacement that unwinds before the swap is retried by the next
    /// rebuild that hands over the same animation.
    #[test]
    fn a_failed_retarget_is_retried() {
        let first: Rc<dyn Animation<f64>> =
            Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
        let second: Rc<dyn Animation<f64>> = Rc::new(StatusPanicsOnce {
            inner: AnimationController::builder(Duration::from_millis(100)).build(),
            armed: Cell::new(true),
        });
        let mut slot = ProxySlot::new(&first);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            slot.retarget(&second);
        }));
        assert!(failed.is_err(), "the authored failure propagates");
        assert!(Rc::ptr_eq(&slot.proxy().parent(), &first));

        slot.retarget(&second);
        assert!(
            Rc::ptr_eq(&slot.proxy().parent(), &second),
            "the next rebuild installs the animation the failure refused"
        );
    }
}
