//! Presentation-local lifecycle observation. Owners commit before notifying.
//!
//! Two observations share one presentation: the app lifecycle state, and the
//! host's one-off system notices ([`HostNotice`]). Both run on the delivery
//! machinery of [`crate::observation`].
pub(crate) use crate::observation::preserve;
use crate::observation::{CloseWindow, Observation, Panic, Phase, Subscription, WeakObservation};
use flui_platform::HostNotice;
use flui_scheduler::AppLifecycleState;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

/// The presentation has closed or no longer accepts subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the presentation lifecycle is closed")]
pub struct LifecycleClosed;

/// Weak, owner-local capability for one presentation's lifecycle history.
///
/// Acquire from `LifecycleContext::lifecycle_handle` in `init_state` or
/// `did_change_dependencies`. This is not the UI runtime scheduler aggregate.
#[derive(Clone)]
pub struct LifecycleHandle {
    states: WeakObservation<AppLifecycleState>,
    notices: WeakObservation<HostNotice>,
}
impl std::fmt::Debug for LifecycleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleHandle").finish_non_exhaustive()
    }
}
impl LifecycleHandle {
    /// Latest committed observation, or `None` before the first observation.
    /// A queued callback can describe an older event than this snapshot.
    ///
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal notification or owner drop.
    pub fn snapshot(&self) -> Result<Option<AppLifecycleState>, LifecycleClosed> {
        self.states.snapshot()
    }
    /// Register future events and atomically return the current observation.
    /// The callback is never invoked synchronously by this method. Events
    /// committed before registration are not replayed. Retain the returned token.
    ///
    /// Notifications run in FIFO order. Reentrant notifications queue behind the
    /// active walk instead of recursively invoking callbacks. The callback argument
    /// describes its event; [`Self::snapshot`] reads the latest committed state,
    /// which may already be newer after a reentrant commit.
    ///
    /// A callback panic does not skip eligible siblings or queued notifications.
    /// The owner resumes the first panic after delivery and cleanup complete.
    /// Cancellation releases callback captures outside source borrows; an active
    /// callback releases its captures after returning if it was cancelled. A
    /// capture-destructor panic cannot undo cancellation or skip siblings, and
    /// does not replace an earlier callback panic. After a caught failure, or
    /// during an independent unwind, retiring callback envelopes are retained.
    /// Otherwise they have ordinary Rust destruction semantics: two panicking
    /// fields in the first retired envelope can abort before containment.
    /// Rejected callbacks follow the same exceptional-retention rule and retire
    /// only after the source borrow has been released.
    ///
    /// # Errors
    /// Returns [`LifecycleClosed`] once terminal close begins or the owner dies.
    pub fn subscribe(
        &self,
        mut callback: impl FnMut(AppLifecycleState) + 'static,
    ) -> Result<(Option<AppLifecycleState>, LifecycleSubscription), LifecycleClosed> {
        let (current, token) = self
            .states
            .subscribe(Box::new(move |state: &AppLifecycleState| callback(*state)))?;
        Ok((current, LifecycleSubscription { _token: token }))
    }
    /// Register for the host's one-off system notices: the user's session
    /// ending, sleep and wake, session lock, memory pressure. Notices are not
    /// replayed, and none is kept: a subscriber learns only what happens after
    /// it registers. Retain the returned token.
    ///
    /// Delivery is the same as [`Self::subscribe`]'s: FIFO, never synchronous
    /// inside this call, reentrant notices queued behind the active walk, and a
    /// callback panic contained until every eligible callback has run. The
    /// owner delivers a notice within the call that reported it, so a callback
    /// that takes a [`SessionEndBlock`](flui_platform::SessionEndBlock)
    /// from [`HostNotice::SessionEnding`] has done so before the host answers
    /// the operating system.
    ///
    /// # Errors
    /// Returns [`LifecycleClosed`] once terminal close begins or the owner dies.
    pub fn subscribe_notices(
        &self,
        callback: impl FnMut(&HostNotice) + 'static,
    ) -> Result<HostNoticeSubscription, LifecycleClosed> {
        let (_, token) = self.notices.subscribe(Box::new(callback))?;
        Ok(HostNoticeSubscription { _token: token })
    }
}

/// Cancels a lifecycle callback on drop, including callbacks not yet started
/// during a notification walk. Does not retain the presentation or callback.
#[must_use = "dropping the subscription cancels lifecycle observation"]
pub struct LifecycleSubscription {
    _token: Subscription<AppLifecycleState>,
}
impl std::fmt::Debug for LifecycleSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSubscription")
            .finish_non_exhaustive()
    }
}

/// Cancels a notice callback on drop, with the same guarantees as
/// [`LifecycleSubscription`].
#[must_use = "dropping the subscription cancels notice observation"]
pub struct HostNoticeSubscription {
    _token: Subscription<HostNotice>,
}
impl std::fmt::Debug for HostNoticeSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostNoticeSubscription")
            .finish_non_exhaustive()
    }
}

/// Composition-root owner for a presentation lifecycle source.
///
/// Keep outside element/binding locks. Commit all local states before running
/// scheduler or lifecycle callbacks, then drain notifications. Build owners
/// receive only [`Self::handle`], never ownership of this source.
#[doc(hidden)]
pub struct LifecycleSource {
    states: Observation<AppLifecycleState>,
    notices: Observation<HostNotice>,
    /// The close-delivery latch, shared with the claimed [`CloseDelivery`].
    close_delivery: Rc<Cell<CloseDeliveryState>>,
}
impl std::fmt::Debug for LifecycleSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSource").finish_non_exhaustive()
    }
}
impl Default for LifecycleSource {
    fn default() -> Self {
        Self::new()
    }
}
impl LifecycleSource {
    /// Create an unobserved presentation source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            states: Observation::keeping_latest(),
            notices: Observation::transient(),
            close_delivery: Rc::new(Cell::new(CloseDeliveryState::NotStarted)),
        }
    }
    /// Weak owner-local consumer capability.
    #[must_use]
    pub fn handle(&self) -> LifecycleHandle {
        LifecycleHandle {
            states: self.states.downgrade(),
            notices: self.notices.downgrade(),
        }
    }
    /// Last committed local observation, including during terminal cleanup.
    #[must_use]
    pub fn current(&self) -> Option<AppLifecycleState> {
        self.states.latest()
    }
    /// Commit an ordinary observation without calling user code.
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal close begins.
    pub fn commit(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_state(state, Phase::Open)
    }
    /// Commit a system notice for delivery by the next [`Self::drain`]. The
    /// notice is queued behind every earlier one and reaches only the
    /// subscribers registered now.
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal close begins.
    pub fn commit_notice(&self, notice: HostNotice) -> Result<(), LifecycleClosed> {
        self.notices.commit(notice, Phase::Open, |_| true)
    }
    /// Hold the in-progress window of a terminal close until the returned
    /// guard drops. While it is held, a preserving close retains callbacks
    /// rejected or cancelled through stale handles; afterwards they retire
    /// normally. The host holds it across the whole presentation close.
    #[must_use = "the close window ends when the guard drops"]
    pub fn close_window(&self) -> LifecycleCloseWindow {
        LifecycleCloseWindow {
            _states: self.states.close_window(),
            _notices: self.notices.close_window(),
        }
    }
    /// Claim this presentation's close delivery: the token for the call that
    /// runs it, `None` once a delivery has run or is running, so a close
    /// requested again from inside a Detached observer, or repeated when the
    /// UI runtime drops, delivers nothing twice. The token records whether the
    /// delivery completed or was interrupted.
    #[must_use = "dropping the token without completing it records an interrupted delivery"]
    pub fn claim_close_delivery(&self) -> Option<CloseDelivery> {
        if self.close_delivery.get() != CloseDeliveryState::NotStarted {
            return None;
        }
        self.close_delivery.set(CloseDeliveryState::Running);
        Some(CloseDelivery {
            state: Rc::clone(&self.close_delivery),
        })
    }

    /// How this presentation's close delivery stands.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read once the host classifies a failed close")
    )]
    pub(crate) fn close_delivery_state(&self) -> CloseDeliveryState {
        self.close_delivery.get()
    }
    /// Fence new subscriptions and ordinary commits before terminal callbacks.
    pub fn begin_close(&self) {
        self.states.begin_close();
        self.notices.begin_close();
    }
    /// Seed terminal recovery without finishing the host's lifecycle ladder.
    pub fn begin_close_with_mode(&self, mode: flui_interaction::__runtime::CloseMode) {
        let preserving = mode == flui_interaction::__runtime::CloseMode::PreservingFailure
            || std::thread::panicking();
        self.states.begin_close_preserving(preserving);
        self.notices.begin_close_preserving(preserving);
    }
    /// Commit an authorized terminal ladder step after `begin_close`.
    /// # Errors
    /// Returns [`LifecycleClosed`] outside the terminal notification phase.
    pub fn commit_terminal(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_state(state, Phase::Closing)
    }
    fn commit_state(&self, state: AppLifecycleState, phase: Phase) -> Result<(), LifecycleClosed> {
        self.states
            .commit(state, phase, |current| current != Some(&state))
    }
    /// Drain committed events in FIFO order. Nested drains defer to this walk.
    /// All eligible callbacks run before the first panic resumes.
    pub fn drain(&self) {
        let mut first = self.states.drain();
        preserve(&mut first, self.notices.drain());
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
    /// Invalidate capabilities and release callbacks before widget disposal.
    /// Cleanup completes before any callback-capture destructor panic resumes.
    pub fn finish_close(&self) {
        self.finish_close_with_mode(flui_interaction::__runtime::CloseMode::Ordinary);
    }

    /// Complete terminal release while preserving a failure caught by the host.
    pub fn finish_close_with_mode(&self, mode: flui_interaction::__runtime::CloseMode) {
        self.begin_close_with_mode(mode);
        self.states.request_finish();
        self.notices.request_finish();
        // A reentrant close is completed by the active FIFO walk after its
        // terminal event. It must not erase an event still awaiting delivery.
        self.drain();
    }
}
/// How a presentation's close delivery stands; see
/// [`LifecycleSource::claim_close_delivery`]. Crate-private until a
/// production reader exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseDeliveryState {
    /// No delivery was claimed.
    NotStarted,
    /// A delivery was claimed and its token is alive.
    Running,
    /// The delivery ran to its end.
    Completed,
    /// The delivery's token was dropped before it completed, as a panic in a
    /// Detached observer does. It is not delivered again.
    Interrupted,
}

/// The claim on one presentation's close delivery. [`Self::complete`]
/// records that it ran to its end; dropping it without completing records
/// an interrupted delivery.
#[doc(hidden)]
#[derive(Debug)]
pub struct CloseDelivery {
    state: Rc<Cell<CloseDeliveryState>>,
}

impl CloseDelivery {
    /// Record that the delivery ran to its end.
    pub fn complete(self) {
        self.state.set(CloseDeliveryState::Completed);
    }
}

impl Drop for CloseDelivery {
    /// A token dropped before [`CloseDelivery::complete`] records an
    /// interrupted delivery; one that completed keeps its record.
    fn drop(&mut self) {
        if self.state.get() == CloseDeliveryState::Running {
            self.state.set(CloseDeliveryState::Interrupted);
        }
    }
}

/// The in-progress window of one terminal close; see
/// [`LifecycleSource::close_window`].
#[doc(hidden)]
pub struct LifecycleCloseWindow {
    _states: CloseWindow<AppLifecycleState>,
    _notices: CloseWindow<HostNotice>,
}
impl std::fmt::Debug for LifecycleCloseWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleCloseWindow")
            .finish_non_exhaustive()
    }
}
impl Drop for LifecycleSource {
    fn drop(&mut self) {
        let mut first: Option<Panic> = None;
        let failure = catch_unwind(AssertUnwindSafe(|| {
            self.states.release(&mut first);
            self.notices.release(&mut first);
        }))
        .err();
        preserve(&mut first, failure);
        if let Some(payload) = first {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AppLifecycleState::{Detached, Hidden, Inactive, Resumed};
    use std::{cell::RefCell, rc::Rc};
    static_assertions::assert_not_impl_any!(LifecycleHandle: Send, Sync);
    static_assertions::assert_not_impl_any!(LifecycleSubscription: Send, Sync);

    fn lifecycle_subscription_reentrant_events_are_fifo_and_new_listeners_do_not_replay() {
        let source = Rc::new(LifecycleSource::new());
        let weak = Rc::downgrade(&source);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let new_tokens = Rc::new(RefCell::new(Vec::new()));
        let keep = Rc::clone(&new_tokens);
        let (_, _first) = source
            .handle()
            .subscribe(move |state| {
                log.borrow_mut().push((1, state));
                if state == Inactive {
                    let owner = weak.upgrade().expect("owner");
                    owner.commit(Hidden).expect("nested commit");
                    let (_, token) = owner
                        .handle()
                        .subscribe(|_| panic!("new subscriber got queued replay"))
                        .expect("late subscribe");
                    keep.borrow_mut().push(token);
                    owner.drain();
                }
            })
            .expect("first");
        let log = Rc::clone(&seen);
        let (_, _second) = source
            .handle()
            .subscribe(move |state| log.borrow_mut().push((2, state)))
            .expect("second");
        source.commit(Inactive).expect("commit");
        source.drain();
        assert_eq!(
            *seen.borrow(),
            [(1, Inactive), (2, Inactive), (1, Hidden), (2, Hidden)]
        );
    }

    fn lifecycle_subscription_terminal_reentry_drains_before_invalidating() {
        let source = Rc::new(LifecycleSource::new());
        let handle = source.handle();
        let weak = Rc::downgrade(&source);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let (_, _first) = handle
            .subscribe(move |state| {
                log.borrow_mut().push((1, state));
                if state == Inactive {
                    let owner = weak.upgrade().expect("owner");
                    owner.begin_close();
                    assert!(owner.handle().subscribe(|_| {}).is_err());
                    assert_eq!(owner.commit(Resumed), Err(LifecycleClosed));
                    owner.commit_terminal(Detached).expect("terminal");
                    owner.finish_close();
                    assert_eq!(owner.handle().snapshot(), Ok(Some(Detached)));
                    panic!("earlier queued callback");
                }
            })
            .expect("first");
        let log = Rc::clone(&seen);
        let (_, _second) = handle
            .subscribe(move |state| log.borrow_mut().push((2, state)))
            .expect("second");
        source.commit(Inactive).expect("commit");
        let failure =
            catch_unwind(AssertUnwindSafe(|| source.drain())).expect_err("panic retained");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"earlier queued callback")
        );
        assert_eq!(
            *seen.borrow(),
            [(1, Inactive), (2, Inactive), (1, Detached), (2, Detached)]
        );
        assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    }

    /// One presentation's close delivery is claimed once; the token records
    /// whether it completed or was interrupted, and neither is claimed again.
    fn close_delivery_is_claimed_once_and_records_how_it_ended() {
        let completed = LifecycleSource::new();
        let delivery = completed.claim_close_delivery();
        let while_running = completed.claim_close_delivery().is_some();
        let running = completed.close_delivery_state();
        if let Some(delivery) = delivery {
            delivery.complete();
        }

        let interrupted = LifecycleSource::new();
        drop(interrupted.claim_close_delivery());

        assert!(!while_running, "a running delivery is not claimed again");
        assert_eq!(running, CloseDeliveryState::Running);
        assert_eq!(
            completed.close_delivery_state(),
            CloseDeliveryState::Completed
        );
        assert!(
            completed.claim_close_delivery().is_none(),
            "a completed delivery is not claimed again"
        );
        assert_eq!(
            interrupted.close_delivery_state(),
            CloseDeliveryState::Interrupted,
            "a token dropped before completing records an interrupted delivery"
        );
        assert!(
            interrupted.claim_close_delivery().is_none(),
            "an interrupted delivery is not delivered again"
        );
    }

    #[test]
    fn lifecycle_subscription_matrix() {
        crate::table_test::run_table(
            "lifecycle_subscription_matrix",
            &[
                (
                    "lifecycle_subscription_reentrant_events_are_fifo_and_new_listeners_do_not_replay",
                    lifecycle_subscription_reentrant_events_are_fifo_and_new_listeners_do_not_replay
                        as fn(),
                ),
                (
                    "lifecycle_subscription_terminal_reentry_drains_before_invalidating",
                    lifecycle_subscription_terminal_reentry_drains_before_invalidating as fn(),
                ),
                (
                    "close_delivery_is_claimed_once_and_records_how_it_ended",
                    close_delivery_is_claimed_once_and_records_how_it_ended as fn(),
                ),
            ],
        );
    }
}
