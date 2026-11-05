//! The notification channels a combinator owns.

use std::rc::Rc;

use flui_foundation::notifier::ListenerObserver;
use flui_foundation::notifier_generic::NotificationCallback;
use flui_foundation::panic::RecoveryScope;
use flui_foundation::{ChangeNotifier, ListenerCallback, ListenerId, Notifier, Subscription};

use crate::animation::{StatusObserver, Terminal};
use crate::status::AnimationStatus;

/// A withdrawn callback envelope, retired by its owner (ADR-0178).
pub(crate) type Withdrawn<Arg> = Rc<NotificationCallback<Arg>>;

/// The value listeners and status observers registered on one combinator.
///
/// Every combinator owns exactly these two channels, whatever it does with
/// its parents; relays from parents hold them only weakly, so they die with the
/// combinator's last owner. Withdrawal and disposal hand captures to the
/// caller's recovery scope instead of destroying them in place.
pub(crate) struct Channels {
    values: Terminal<Rc<ChangeNotifier>>,
    statuses: Terminal<Rc<Notifier<AnimationStatus>>>,
}

/// Every callback a disposed [`Channels`] held, for its owner to retire after
/// detaching from parents.
#[must_use = "disposed callbacks must be retired"]
pub(crate) struct Disposed {
    statuses: Vec<Withdrawn<AnimationStatus>>,
    values: Vec<Withdrawn<()>>,
}

impl Channels {
    pub(crate) fn new() -> Self {
        Self {
            values: Terminal::new(Rc::new(ChangeNotifier::new())),
            statuses: Terminal::new(Rc::new(Notifier::new())),
        }
    }

    pub(crate) fn values(&self) -> &Rc<ChangeNotifier> {
        &self.values
    }

    pub(crate) fn statuses(&self) -> &Rc<Notifier<AnimationStatus>> {
        &self.statuses
    }

    /// Join a failure either channel's current delivery already holds.
    pub(crate) fn inherit_failure(&self, recovery: &mut RecoveryScope<'_>) {
        self.values.inherit_failure(recovery);
        self.statuses.inherit_failure(recovery);
    }

    pub(crate) fn listen(&self, listener: ListenerCallback) -> Subscription {
        self.values.listen(listener)
    }

    /// Subscribe an owned relay on the value channel, inside the value
    /// delivery's recovery scope; the subscription detaches on drop.
    pub(crate) fn listen_with_recovery(&self, relay: ListenerObserver) -> Subscription {
        self.values.listen_with_recovery(relay)
    }

    pub(crate) fn add_status(&self, observer: StatusObserver) -> ListenerId {
        self.statuses
            .add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)))
    }

    /// Withdraw a status observer for its subscription guard to retire.
    pub(crate) fn take_status(
        &self,
        id: ListenerId,
        recovery: &mut RecoveryScope<'_>,
    ) -> Option<Withdrawn<AnimationStatus>> {
        self.inherit_failure(recovery);
        self.statuses.take_callback(id)
    }

    /// Close both channels and take every registered callback. Later
    /// registrations and notifications are inert.
    pub(crate) fn dispose(&self) -> Disposed {
        Disposed {
            statuses: self.statuses.dispose_and_take_callbacks(),
            values: self.values.dispose_and_take_listeners(),
        }
    }

    /// Move both channel allocations out for retirement by a dropping owner.
    pub(crate) fn withdraw(&mut self) -> Self {
        Self {
            values: self.values.withdraw(),
            statuses: self.statuses.withdraw(),
        }
    }
}

impl Disposed {
    /// Retire status observers, then value listeners, each channel in its own
    /// registration order: the order of owners whose parent can change
    /// (proxy, switch).
    pub(crate) fn retire_statuses_first(self, recovery: &mut RecoveryScope<'_>) {
        for callback in self.statuses {
            recovery.retire(Terminal::new(callback));
        }
        for callback in self.values {
            recovery.retire(Terminal::new(callback));
        }
    }

    /// Retire value listeners, then status observers: the order of the
    /// fixed-parent wrappers (curved, tween, reverse), which decides the
    /// authoritative failure when captures in both channels fail.
    pub(crate) fn retire_values_first(self, recovery: &mut RecoveryScope<'_>) {
        for callback in self.values {
            recovery.retire(Terminal::new(callback));
        }
        for callback in self.statuses {
            recovery.retire(Terminal::new(callback));
        }
    }
}
