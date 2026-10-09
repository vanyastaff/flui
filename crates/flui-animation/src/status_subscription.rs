//! Removal authority bound to the source that admitted a status callback.

use std::fmt;
use std::rc::{Rc, Weak};

use crate::animation::{Retirement, Terminal};

trait Cancellation {
    fn cancel(&self, recovery: &mut Retirement);
}

struct Registration<S: ?Sized, Token> {
    source: Weak<S>,
    token: Token,
    remove: fn(&S, Token, &mut Retirement),
}

impl<S: ?Sized, Token: Copy> Cancellation for Registration<S, Token> {
    fn cancel(&self, recovery: &mut Retirement) {
        let Some(source) = self.source.upgrade() else {
            return;
        };
        let source = Terminal::new(source);
        recovery.run_with(|recovery| (self.remove)(&source, self.token, recovery));
        recovery.retire(source);
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
    /// source must reject stale tokens if it reuses storage. `remove` withdraws
    /// the callback outside state guards and retires its captures through the
    /// borrowed recovery context, preserving any earlier delivery failure.
    /// A function pointer and a `Copy` token keep the guard free of user captures.
    pub fn new<S: ?Sized + 'static, Token: Copy + 'static>(
        source: &Rc<S>,
        token: Token,
        remove: fn(&S, Token, &mut flui_foundation::panic::PanicRecovery),
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
    pub fn cancel_with_recovery(&mut self, recovery: &mut Retirement) {
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
        self.cancel_with_recovery(&mut recovery);
        recovery.finish();
    }
}
