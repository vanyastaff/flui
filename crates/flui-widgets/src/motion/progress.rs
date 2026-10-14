//! [`Progress`] — what an `Animate`'s stages read: one parent animation and
//! the side of a transition, either of which changes the pose.

use std::cell::Cell;
use std::fmt;
use std::rc::{Rc, Weak};

use flui_animation::{
    Animation, AnimationStatus, ProxyAnimation, StatusObserver, StatusSubscription,
};
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId, ListenerObserver};

use super::transition::Phase;

/// The stages' parent animation. Its listeners hear every value change of the
/// current parent and every change of side, so a side switch at an unchanged
/// value still repaints the stages.
pub(super) struct Progress {
    proxy: ProxyAnimation<f64>,
    phase: Cell<Phase>,
    changes: ChangeNotifier,
    relay: Cell<Option<ListenerId>>,
}

impl Progress {
    pub(super) fn new(parent: Rc<dyn Animation<f64>>) -> Rc<Self> {
        let progress = Rc::new(Self {
            proxy: ProxyAnimation::new(parent),
            phase: Cell::new(Phase::Enter),
            changes: ChangeNotifier::new(),
            relay: Cell::new(None),
        });
        let weak: Weak<Self> = Rc::downgrade(&progress);
        let relay =
            progress
                .proxy
                .add_observer(Rc::new(move |recovery: &mut RecoveryScope<'_>| {
                    if let Some(progress) = weak.upgrade() {
                        progress.changes.notify_listeners_with_recovery(recovery);
                    }
                }));
        progress.relay.set(Some(relay));
        progress
    }

    pub(super) fn phase(&self) -> Phase {
        self.phase.get()
    }

    /// Switch sides, notifying the stages if the side changed.
    pub(super) fn set_phase(&self, phase: Phase) {
        if self.phase.replace(phase) != phase {
            self.changes.notify_listeners();
        }
    }

    /// [`set_phase`](Self::set_phase) inside a delivery this switch answers.
    pub(super) fn set_phase_with_recovery(&self, phase: Phase, recovery: &mut RecoveryScope<'_>) {
        if self.phase.replace(phase) != phase {
            self.changes.notify_listeners_with_recovery(recovery);
        }
    }

    /// Follow `parent` from now on. Commits before the previous parent's
    /// subscriptions are removed; a panicking removal propagates afterwards.
    pub(super) fn set_parent(&self, parent: Rc<dyn Animation<f64>>) {
        self.proxy.set_parent(parent);
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(relay) = self.relay.take() {
            self.proxy.remove_listener(relay);
        }
    }
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Progress")
            .field("value", &self.proxy.value())
            .field("phase", &self.phase.get())
            .finish_non_exhaustive()
    }
}

impl Listenable for Progress {
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        self.changes.add_observer(observer)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.changes.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.changes.remove_all_listeners();
    }
}

impl Animation<f64> for Progress {
    fn value(&self) -> f64 {
        self.proxy.value()
    }

    fn status(&self) -> AnimationStatus {
        self.proxy.status()
    }

    fn is_animating(&self) -> bool {
        self.proxy.is_animating()
    }

    fn observe_status(&self, observer: StatusObserver) -> StatusSubscription {
        self.proxy.observe_status(observer)
    }
}
