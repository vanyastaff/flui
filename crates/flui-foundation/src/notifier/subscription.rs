//! The owned handle of one listener registration.

use std::fmt;
use std::rc::{Rc, Weak};

use crate::panic::{PanicRecovery, RecoveryScope};
use crate::sequence::ListenerId;

use super::SubscribedChannel;

/// An owned registration on a notification source: hold it, and its listener
/// fires; drop it, or call [`detach`](Self::detach), and the listener leaves
/// the source.
///
/// A subscription names exactly the registration it was issued for. One from
/// a foundation channel remembers that channel, so a [`ListenerId`]'s value
/// alone never names another channel's registration (ids are minted per
/// channel). One from a custom source ([`for_source`](Self::for_source))
/// holds the source weakly with a token and a removal function, so it never
/// keeps the source alive and holds no user captures. A source that was
/// disposed or is already gone detaches quietly.
///
/// A detach that lands inside its channel's own notify round withdraws the
/// callback through that round's retirement: it destroys outside the
/// channel's borrow, at the round's tail, with the round's containment
/// (ADR-0287). A detach that belongs to an enclosing cleanup round passes
/// that round's scope to [`detach_with_recovery`](Self::detach_with_recovery).
///
/// ```rust
/// use std::{cell::Cell, rc::Rc};
/// use flui_foundation::ChangeNotifier;
///
/// let notifier = ChangeNotifier::new();
/// let hits = Rc::new(Cell::new(0));
/// let subscription = notifier.listen(Rc::new({
///     let hits = Rc::clone(&hits);
///     move || hits.set(hits.get() + 1)
/// }));
/// notifier.notify_listeners();
/// assert_eq!(hits.get(), 1);
///
/// drop(subscription);
/// notifier.notify_listeners();
/// assert_eq!(hits.get(), 1, "dropping the subscription detached the listener");
/// ```
#[must_use = "hold the subscription: dropping it detaches its listener"]
pub struct Subscription {
    registration: Registration,
}

enum Registration {
    /// Nothing to detach: an inert subscription, or one already detached.
    Detached,
    /// A listener on a foundation channel, held weakly so a subscription
    /// never pins a channel its outside has released.
    Channel {
        channel: Weak<dyn SubscribedChannel>,
        id: ListenerId,
    },
    /// A registration on a custom source, removed by the source's own
    /// function.
    Source(Box<dyn SourceRemoval>),
}

/// The removal half of a custom source's registration, with its user
/// captures left in the source.
trait SourceRemoval {
    fn remove(&self, recovery: &mut RecoveryScope<'_>);
}

struct BoundRemoval<S: ?Sized, Token, Outgoing> {
    source: Weak<S>,
    token: Token,
    remove: fn(&S, Token, &mut RecoveryScope<'_>) -> Outgoing,
}

impl<S: ?Sized, Token: Copy, Outgoing> SourceRemoval for BoundRemoval<S, Token, Outgoing> {
    fn remove(&self, recovery: &mut RecoveryScope<'_>) {
        let Some(source) = self.source.upgrade() else {
            return;
        };
        let mut outgoing = None;
        recovery.run_with(|recovery| {
            outgoing = Some((self.remove)(&source, self.token, recovery));
        });
        // The temporary strong reference ends before the outgoing captures
        // are destroyed: a capture that releases the source's last owner then
        // closes its channels before anything re-enters them.
        recovery.retire(source);
        recovery.retire(outgoing);
    }
}

impl Subscription {
    /// A subscription bound to `channel`, cancelling on drop.
    pub(crate) fn for_channel(channel: Weak<dyn SubscribedChannel>, id: ListenerId) -> Self {
        Self {
            registration: Registration::Channel { channel, id },
        }
    }

    /// A subscription to a custom source's registration `token`.
    ///
    /// Detaching upgrades `source` and calls `remove`, which commits the
    /// withdrawal with the source's state borrows released and returns the
    /// withdrawn callback. The subscription releases its temporary reference
    /// to the source before it destroys that callback, inside the detaching
    /// recovery scope. A removal that leaves the callback in the source's own
    /// deferred queue (a delivery in progress) returns an empty value.
    ///
    /// `token` names the registration for as long as it lives: a source that
    /// reuses storage refuses a stale token. A function pointer and a `Copy`
    /// token keep the subscription free of user captures.
    pub fn for_source<S, Token, Outgoing>(
        source: &Rc<S>,
        token: Token,
        remove: fn(&S, Token, &mut RecoveryScope<'_>) -> Outgoing,
    ) -> Self
    where
        S: ?Sized + 'static,
        Token: Copy + 'static,
        Outgoing: 'static,
    {
        Self {
            registration: Registration::Source(Box::new(BoundRemoval {
                source: Rc::downgrade(source),
                token,
                remove,
            })),
        }
    }

    /// An inert subscription: nothing to detach, ever. For a source that
    /// admits no registration (a constant animation, a disposed owner) and
    /// hands this form out instead of an id that names nothing.
    pub fn inert() -> Self {
        Self {
            registration: Registration::Detached,
        }
    }

    /// Detach now rather than at drop. A panic in the withdrawn callback's
    /// destructor propagates after the withdrawal has committed.
    pub fn detach(mut self) {
        self.detach_in_place();
    }

    /// Leave the listener registered for as long as its source lives: the
    /// source's disposal or retirement removes it. Nothing leaks; the source
    /// keeps the callback it already owns, and this handle gives up its right
    /// to remove it.
    pub fn forget(mut self) {
        // The registration holds only a weak reference, a `Copy` token and a
        // function pointer: dropping it runs no user code.
        self.registration = Registration::Detached;
    }

    /// Detach inside an enclosing cleanup round: the withdrawn callback is
    /// destroyed under `recovery`, so the round's first failure stays the one
    /// it resumes.
    pub fn detach_with_recovery(mut self, recovery: &mut RecoveryScope<'_>) {
        match std::mem::replace(&mut self.registration, Registration::Detached) {
            Registration::Detached => {}
            Registration::Channel { channel, id } => {
                if let Some(channel) = channel.upgrade() {
                    recovery.run(|| channel.detach_listener(id));
                }
            }
            Registration::Source(removal) => {
                recovery.run_with(|recovery| removal.remove(recovery));
            }
        }
    }

    fn detach_in_place(&mut self) {
        match std::mem::replace(&mut self.registration, Registration::Detached) {
            Registration::Detached => {}
            Registration::Channel { channel, id } => {
                if let Some(channel) = channel.upgrade() {
                    channel.detach_listener(id);
                }
            }
            Registration::Source(removal) => {
                let mut recovery = PanicRecovery::new();
                removal.remove(&mut recovery.scope());
                recovery.finish();
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.detach_in_place();
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.registration {
            Registration::Detached => f.write_str("Subscription(detached)"),
            Registration::Channel { id, .. } => write!(f, "Subscription({id:?})"),
            Registration::Source(_) => f.write_str("Subscription(source)"),
        }
    }
}
