//! What the platform delivers to the application's handler (ADR-0254 §4).
//!
//! A loop keeps one [`AppDelivery`]. [`Platform::run`] binds the
//! application's [`AppHandler`] to it on the owner thread, in an owner-thread
//! slot, so the handler is reached, called and dropped only there; the loop's
//! sources report to it from then on, and before then too.
//!
//! What the platform *reports* (a quit, a reopen, a host notice) goes through
//! one causal FIFO. A report accepted while the handler cannot hear it yet —
//! before [`AppHandler::ready`] has returned, while another method runs, or on
//! another thread — waits in order and is delivered on the owner thread as
//! soon as it can be: after the running method, or in an owner turn the
//! delivery asks for. Nothing accepted is dropped for want of a handler. What
//! the platform *asks* (a session end's answer, the exit policy, the wake
//! deadline) cannot wait: it is asked at once, re-entrantly if a method is
//! running, and the host answers it itself when no handler is bound; a report
//! raised while it answers is delivered right after the answer.
//!
//! Every method is called through the call point (`call_point::call`) on a
//! clone of the handler, with no lock of the delivery or the host held, so a
//! method may report, ask, quit or open a window. A panicking method is
//! contained: the reports queued behind it are still delivered, each in its
//! own containment, and the first panic is returned to the caller to resume or
//! contain (ADR-0207 §7); a later one never replaces it.
//!
//! The owner capability each method borrows is minted for the call from the
//! platform and the owner hooks of the run, which the delivery holds weakly:
//! the platform owns the loop that owns this delivery, so a strong hold would
//! keep both alive. Once the platform is gone nothing can be delivered.
//!
//! [`Platform::run`]: crate::Platform::run

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};
use parking_lot::Mutex;

use super::call_point::{Called, Failure, HandlerSlot, call, retire};
use super::signal::{OwnerSignal, WindowWake};
use super::slot::OwnerSlot;
use super::{OwnerHooks, OwnerPlatform};
use crate::error::BootstrapError;
use crate::platform::{AppHandler, Platform};

/// What the platform reports to the application, accepted in order.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Report {
    Quit,
    Reopen,
    Notice(SystemNotice),
}

impl Report {
    fn deliver(self, handler: &dyn AppHandler, owner: &OwnerPlatform) {
        match self {
            Self::Quit => handler.quit_requested(owner),
            Self::Reopen => handler.reopen(owner),
            Self::Notice(notice) => handler.host_notice(owner, notice),
        }
    }
}

/// The handler of one run, and what mints the owner capability its methods
/// borrow.
struct Binding {
    handler: OwnerSlot<Rc<dyn AppHandler>>,
    platform: Weak<dyn Platform>,
    hooks: Weak<dyn OwnerHooks>,
}

#[derive(Default)]
struct State {
    binding: Option<Binding>,
    /// Accepted, in order, and not yet heard.
    pending: VecDeque<Report>,
    /// A method runs on the owner thread (`ready`, or a report being
    /// delivered): a report accepted now waits behind it.
    delivering: bool,
    /// The quit was accepted: a later one is the same quit.
    quit_accepted: bool,
}

impl State {
    fn on_owner(&self) -> bool {
        self.binding
            .as_ref()
            .is_some_and(|binding| binding.handler.is_owner_thread())
    }
}

/// What a loop delivers to the application's handler. See the module docs.
#[derive(Default)]
pub(crate) struct AppDelivery {
    state: Mutex<State>,
    /// Asks the owner for a turn that delivers what another thread reported
    /// ([`Self::resume_through`]).
    owner_turn: Mutex<Option<WindowWake>>,
}

/// The bound handler as a call-point slot: a call takes a clone, so the
/// handler stays bound while one of its methods runs and may be called again
/// from inside it. Only a clone that turns out to be the last, because the
/// run released its handler meanwhile, goes back to the call point, which
/// drops it outside every lock, or retains it when the method failed.
struct Resident<'a>(&'a AppDelivery);

impl HandlerSlot for Resident<'_> {
    type Handler = Rc<dyn AppHandler>;

    fn take(&self) -> Option<Self::Handler> {
        self.0.state.lock().binding.as_ref()?.handler.cloned()
    }

    fn restore(&self, handler: Self::Handler) -> Option<Self::Handler> {
        (Rc::strong_count(&handler) == 1).then_some(handler)
    }

    fn replace(&self, handler: Self::Handler) -> Option<Self::Handler> {
        // A run binds its handler once (`AppDelivery::bind`); nothing installs
        // one through the call point.
        Some(handler)
    }
}

/// Ends a delivery that unwinds out of a method, so the next report delivers
/// again. One that runs out of reports has already ended under the lock and
/// disarms this.
struct Walk<'a>(&'a AppDelivery);

impl Walk<'_> {
    fn disarm(self) {
        std::mem::forget(self);
    }
}

impl Drop for Walk<'_> {
    fn drop(&mut self) {
        self.0.state.lock().delivering = false;
    }
}

impl AppDelivery {
    /// A delivery with no handler bound: what is reported waits for one.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Bind the run's `handler` on the calling thread, which becomes the one
    /// it is called on, over the run's `platform` and owner `hooks`. Reports
    /// accepted before stay queued for it; they are delivered once
    /// [`Self::ready`] returns.
    pub(crate) fn bind(
        &self,
        handler: Rc<dyn AppHandler>,
        platform: &Arc<dyn Platform>,
        hooks: &Arc<dyn OwnerHooks>,
    ) {
        let slot = OwnerSlot::new();
        let refused = slot.replace(handler).err();
        if refused.is_some() {
            tracing::error!("the application handler could not be kept: owner slots are exhausted");
        }
        let previous = self.state.lock().binding.replace(Binding {
            handler: slot,
            platform: Arc::downgrade(platform),
            hooks: Arc::downgrade(hooks),
        });
        retire(previous);
        retire(refused);
    }

    /// Release the run's handler: its loop ended. It is dropped here, outside
    /// every lock; a report still queued has no handler left to hear it.
    #[cfg_attr(
        not(any(
            windows,
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            target_arch = "wasm32",
            all(target_os = "linux", feature = "winit-backend"),
            test
        )),
        expect(
            dead_code,
            reason = "a headless platform keeps its handler for its life: its run never ends"
        )
    )]
    pub(crate) fn release(&self) {
        let (binding, unheard) = {
            let mut state = self.state.lock();
            (state.binding.take(), std::mem::take(&mut state.pending))
        };
        if !unheard.is_empty() {
            tracing::debug!(
                ?unheard,
                "the run ended with reports its handler never heard"
            );
        }
        if let Some(binding) = binding {
            // The handler leaves its slot here: while an unwind is under way
            // `retire` keeps it for good rather than leave it in the owner
            // thread's table, which would drop it, and its captures, when the
            // thread exits.
            retire(binding.handler.take());
            drop(binding);
        }
    }

    /// Call [`AppHandler::ready`] with `owner`, then deliver what was
    /// reported before and during it. A report raised from inside `ready`
    /// waits for it to return. A panic in `ready` or in a report after it
    /// resumes here, the first one.
    ///
    /// # Errors
    /// What `ready` returned. The reports wait for a later delivery then.
    pub(crate) fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
        self.state.lock().delivering = true;
        let walk = Walk(self);
        match call(&Resident(self), |handler| handler.ready(owner)) {
            Called::Returned(result) => result?,
            Called::Panicked(failure) => failure.resume(),
            Called::Vacant => {
                return Err(
                    "the run has no application handler on this thread to bootstrap".into(),
                );
            }
        }
        walk.disarm();
        self.drain().or_resume(());
        Ok(())
    }

    /// The platform quits ([`AppHandler::quit_requested`]); once per run.
    pub(crate) fn quit_requested(&self) -> Called<()> {
        self.report(Report::Quit)
    }

    /// The user reopened the application ([`AppHandler::reopen`]).
    pub(crate) fn reopen(&self) -> Called<()> {
        self.report(Report::Reopen)
    }

    /// A system notice ([`AppHandler::host_notice`]). The lifecycle feature's
    /// notice channel marks what a wake makes stale before it reports one.
    pub(crate) fn host_notice(&self, notice: SystemNotice) -> Called<()> {
        self.report(Report::Notice(notice))
    }

    /// Ask the handler about a phase of the session ending
    /// ([`AppHandler::session_end`]); vacant with no handler to ask.
    pub(crate) fn session_end(&self, phase: SessionEndPhase) -> Called<SessionEndAnswer> {
        self.question(|handler, owner| handler.session_end(owner, phase))
    }

    /// Ask the handler whether the loop may end ([`AppHandler::exit_policy`]);
    /// vacant with no handler to ask.
    pub(crate) fn exit_policy(&self) -> Called<bool> {
        self.question(<dyn AppHandler>::exit_policy)
    }

    /// Ask the handler for its wake deadline ([`AppHandler::wake_deadline`]);
    /// vacant with no handler to ask.
    #[cfg_attr(
        not(any(
            windows,
            target_os = "macos",
            target_os = "ios",
            target_os = "android",
            all(target_os = "linux", feature = "winit-backend"),
            test
        )),
        expect(dead_code, reason = "the web and headless hosts have no idle wait")
    )]
    pub(crate) fn wake_deadline(&self) -> Called<Option<web_time::Instant>> {
        self.question(<dyn AppHandler>::wake_deadline)
    }

    /// Whether the run's handler is bound, on this thread.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "only the Win32 loop arms a deadline it validates")
    )]
    pub(crate) fn is_bound_here(&self) -> bool {
        self.state.lock().on_owner()
    }

    /// Ask for the owner turns that deliver what other threads report; `wake`
    /// runs [`Self::deliver_queued`] on the owner thread.
    pub(crate) fn resume_through(&self, wake: WindowWake) {
        *self.owner_turn.lock() = Some(wake);
    }

    /// [`Self::resume_through`] an owner turn of `signal`, the loop's own
    /// wake. The turn holds this delivery weakly, so a queued turn keeps
    /// nothing alive.
    pub(crate) fn resume_through_signal(self: &Arc<Self>, signal: &Arc<OwnerSignal>) {
        let delivery = Arc::downgrade(self);
        self.resume_through(OwnerSignal::window_wake(signal, move || {
            if let Some(delivery) = delivery.upgrade() {
                delivery.deliver_queued().or_resume(());
            }
        }));
    }

    /// Deliver, on the owner thread, what was reported while nothing could
    /// hear it; nothing when a delivery already runs there, which reaches it
    /// in order.
    pub(crate) fn deliver_queued(&self) -> Called<()> {
        {
            let mut state = self.state.lock();
            if !state.on_owner() || state.delivering || state.pending.is_empty() {
                return Called::Vacant;
            }
            state.delivering = true;
        }
        self.drain()
    }

    /// Accept `report` and deliver it unless it has to wait.
    fn report(&self, report: Report) -> Called<()> {
        {
            let mut state = self.state.lock();
            if report == Report::Quit {
                if state.quit_accepted {
                    return Called::Vacant;
                }
                state.quit_accepted = true;
            }
            state.pending.push_back(report);
            if !state.on_owner() {
                // Before the run binds a handler, the queue waits for its
                // `ready`; on another thread, for an owner turn.
                let bound = state.binding.is_some();
                drop(state);
                if bound {
                    self.ask_owner_turn();
                }
                return Called::Vacant;
            }
            if state.delivering {
                return Called::Vacant;
            }
            state.delivering = true;
        }
        self.drain()
    }

    /// Ask the owner for a turn that delivers what another thread reported.
    fn ask_owner_turn(&self) {
        let wake = self.owner_turn.lock().clone();
        if let Some(wake) = wake {
            wake();
        } else {
            // Still queued: the owner's next report or turn delivers it.
            tracing::warn!(
                "an application report raised off the owner thread waits for the owner's next one"
            );
        }
    }

    /// Deliver the queue in order, with `delivering` set by the caller, until
    /// it is empty; the walk ends under the lock that finds it so.
    fn drain(&self) -> Called<()> {
        let walk = Walk(self);
        let mut first: Option<Failure> = None;
        let mut heard = false;
        loop {
            let next = {
                let mut state = self.state.lock();
                let next = state.pending.pop_front();
                if next.is_none() {
                    state.delivering = false;
                }
                next
            };
            let Some(report) = next else { break };
            match self.ask(|handler, owner| report.deliver(handler, owner)) {
                Called::Returned(()) => heard = true,
                Called::Panicked(failure) => match first {
                    None => first = Some(failure),
                    Some(_) => failure.contain("the application handler"),
                },
                Called::Vacant => tracing::debug!(
                    ?report,
                    "a report has no handler to hear it: the platform is gone"
                ),
            }
        }
        walk.disarm();
        match first {
            Some(failure) => Called::Panicked(failure),
            None if heard => Called::Returned(()),
            None => Called::Vacant,
        }
    }

    /// Ask the handler with `body` at once. A report raised while it answers
    /// waits for the answer, as one raised inside any method does, and is
    /// delivered right after it. When the answer came but a report delivered
    /// after it panicked, that panic is the question's failure: the host
    /// falls back as it does for a handler that panicked.
    fn question<R>(&self, body: impl FnOnce(&dyn AppHandler, &OwnerPlatform) -> R) -> Called<R> {
        let begun = {
            let mut state = self.state.lock();
            let begun = state.on_owner() && !state.delivering;
            if begun {
                state.delivering = true;
            }
            begun
        };
        if !begun {
            // Off the owner, or nested in a method that delivers what this
            // one raises after it.
            return self.ask(body);
        }
        let walk = Walk(self);
        let answer = self.ask(body);
        walk.disarm();
        match (answer, self.drain()) {
            (Called::Panicked(first), Called::Panicked(later)) => {
                later.contain("the application handler");
                Called::Panicked(first)
            }
            (_, Called::Panicked(failure)) => Called::Panicked(failure),
            (answer, _) => answer,
        }
    }

    /// Call `body` on a clone of the bound handler with the run's owner
    /// capability; vacant off the owner thread, before the run binds a
    /// handler, after it released it, and once the platform is gone.
    fn ask<R>(&self, body: impl FnOnce(&dyn AppHandler, &OwnerPlatform) -> R) -> Called<R> {
        let Some(owner) = self.owner() else {
            return Called::Vacant;
        };
        let called = call(&Resident(self), |handler| body(&**handler, &owner));
        // The capability may hold the last of the platform: its teardown runs
        // with no lock held, contained.
        retire(owner);
        called
    }

    /// The run's owner capability, minted for one call on the owner thread.
    fn owner(&self) -> Option<OwnerPlatform> {
        let (platform, hooks) = {
            let state = self.state.lock();
            if !state.on_owner() {
                return None;
            }
            let binding = state.binding.as_ref()?;
            (binding.platform.clone(), binding.hooks.clone())
        };
        let platform = platform.upgrade()?;
        let Some(hooks) = hooks.upgrade() else {
            retire(platform);
            return None;
        };
        Some(OwnerPlatform::new(platform, hooks))
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
