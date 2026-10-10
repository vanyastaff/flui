//! Removal authority bound to the source that admitted a status callback.

use flui_foundation::panic::RecoveryScope;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::animation::{Retirement, Terminal};

trait Cancellation {
    fn cancel(&self, recovery: &mut RecoveryScope<'_>);
}

struct Registration<S: ?Sized, Token, Outgoing> {
    source: Weak<S>,
    token: Token,
    remove: fn(&S, Token, &mut RecoveryScope<'_>) -> Outgoing,
}

impl<S: ?Sized, Token: Copy, Outgoing> Cancellation for Registration<S, Token, Outgoing> {
    fn cancel(&self, recovery: &mut RecoveryScope<'_>) {
        let Some(source) = self.source.upgrade() else {
            return;
        };
        let source = Terminal::new(source);
        let mut outgoing = None;
        recovery.run_with(|recovery| {
            outgoing = Some(Terminal::new((self.remove)(&source, self.token, recovery)));
        });
        // Temporary removal access must end before capture destruction can
        // release the last logical owner and reenter its notification channel.
        recovery.retire(source);
        recovery.retire(outgoing);
    }
}

/// Owns the removal right for one status registration.
///
/// Dropping the guard removes its own callback. The source is held weakly;
/// dropping the guard after source teardown is inert. [`detach`](Self::detach)
/// transfers callback lifetime to the source instead.
#[must_use = "dropping a status subscription removes its callback"]
#[derive(Default)]
pub struct StatusSubscription {
    cancellation: Option<Box<dyn Cancellation>>,
}

impl StatusSubscription {
    /// Construct removal authority for a custom animation's admitted callback.
    ///
    /// `token` must identify that registration for its entire lifetime; the
    /// source must reject stale tokens if it reuses storage. `remove` commits
    /// withdrawal outside state guards and returns outgoing callback custody.
    /// The guard releases temporary source ownership before retiring that
    /// custody through the borrowed recovery context. Deferred delivery custody
    /// may stay in the source's queue; return an empty value in that case.
    /// A function pointer and a `Copy` token keep the guard free of user captures.
    pub fn new<S: ?Sized + 'static, Token: Copy + 'static, Outgoing: 'static>(
        source: &Rc<S>,
        token: Token,
        remove: fn(&S, Token, &mut RecoveryScope<'_>) -> Outgoing,
    ) -> Self {
        Self {
            cancellation: Some(Box::new(Registration {
                source: Rc::downgrade(source),
                token,
                remove,
            })),
        }
    }

    /// Leave the callback registered until its source withdraws or closes it.
    pub fn detach(mut self) {
        // Cancellation contains only Weak, Copy and function-pointer fields.
        // Disabling it cannot invoke or retire the callback it identifies.
        self.cancellation = None;
    }

    /// Withdraw this registration within an enclosing framework cleanup round.
    /// Captures retire through its existing first-failure context.
    #[doc(hidden)]
    pub fn cancel_with_recovery(&mut self, recovery: &mut RecoveryScope<'_>) {
        if let Some(cancellation) = self.cancellation.take() {
            recovery.run_with(|recovery| cancellation.cancel(recovery));
            // The private cancellation envelope owns only Weak, Copy and a
            // function pointer, so it can retire normally after any failure.
        }
    }
}

impl fmt::Debug for StatusSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StatusSubscription")
            .field("active", &self.cancellation.is_some())
            .finish()
    }
}

impl Drop for StatusSubscription {
    fn drop(&mut self) {
        let mut recovery = Retirement::new();
        self.cancel_with_recovery(&mut recovery.scope());
        recovery.finish();
    }
}
