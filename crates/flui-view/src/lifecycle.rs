//! Presentation-local lifecycle observation. Owners commit before notifying.
use flui_scheduler::AppLifecycleState;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

type Callback = Box<dyn FnMut(AppLifecycleState)>;
type Panic = Box<dyn std::any::Any + Send>;

/// The presentation has closed or no longer accepts subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the presentation lifecycle is closed")]
pub struct LifecycleClosed;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Open,
    Closing,
    Closed,
}
struct Listener {
    active: Cell<bool>,
    callback: RefCell<Option<Callback>>,
}
struct Event {
    state: AppLifecycleState,
    listeners: Vec<Weak<Listener>>,
}
struct State {
    current: Option<AppLifecycleState>,
    phase: Phase,
    listeners: Vec<Rc<Listener>>,
    pending: VecDeque<Event>,
    draining: bool,
    // Caught drain failures also fence retirement from nested callbacks.
    retirement_failed: bool,
    preserving_close: bool,
    finish_requested: bool,
    // Open close windows: a terminal close or its recovery is in progress.
    closing: usize,
}
impl State {
    /// Whether a rejected or cancelled callback is retained rather than
    /// dropped: after a caught drain failure, or while a preserving close is
    /// still in progress. Once that close returns, stale handles retire their
    /// callbacks normally again (ADR-0123).
    fn retains_retired(&self) -> bool {
        self.retirement_failed || (self.preserving_close && (self.closing > 0 || self.draining))
    }
}
struct Inner(RefCell<State>);

/// Weak, owner-local capability for one presentation's lifecycle history.
///
/// Acquire from `LifecycleContext::lifecycle_handle` in `init_state` or
/// `did_change_dependencies`. This is not the UI runtime scheduler aggregate.
#[derive(Clone)]
pub struct LifecycleHandle {
    inner: Weak<Inner>,
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
        let inner = self.inner.upgrade().ok_or(LifecycleClosed)?;
        let state = inner.0.borrow();
        if state.phase == Phase::Closed {
            Err(LifecycleClosed)
        } else {
            Ok(state.current)
        }
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
        callback: impl FnMut(AppLifecycleState) + 'static,
    ) -> Result<(Option<AppLifecycleState>, LifecycleSubscription), LifecycleClosed> {
        let Some(inner) = self.inner.upgrade() else {
            retire_rejected_callback(callback, false);
            return Err(LifecycleClosed);
        };
        let mut state = inner.0.borrow_mut();
        if state.phase != Phase::Open {
            let prior_failure = state.retains_retired();
            drop(state);
            retire_rejected_callback(callback, prior_failure);
            return Err(LifecycleClosed);
        }
        let listener = Rc::new(Listener {
            active: Cell::new(true),
            callback: RefCell::new(Some(Box::new(callback))),
        });
        let token = LifecycleSubscription {
            source: Rc::downgrade(&inner),
            listener: Rc::downgrade(&listener),
        };
        state.listeners.push(listener);
        Ok((state.current, token))
    }
}

/// Cancels a lifecycle callback on drop, including callbacks not yet started
/// during a notification walk. Does not retain the presentation or callback.
#[must_use = "dropping the subscription cancels lifecycle observation"]
pub struct LifecycleSubscription {
    source: Weak<Inner>,
    listener: Weak<Listener>,
}
impl std::fmt::Debug for LifecycleSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSubscription")
            .finish_non_exhaustive()
    }
}
impl Drop for LifecycleSubscription {
    fn drop(&mut self) {
        let Some(listener) = self.listener.upgrade() else {
            return;
        };
        listener.active.set(false);
        let (removed, prior_failure) = self.source.upgrade().map_or((None, false), |source| {
            let mut state = source.0.borrow_mut();
            let prior_failure = state.retains_retired();
            let removed = state
                .listeners
                .iter()
                .position(|entry| Rc::ptr_eq(entry, &listener))
                .map(|index| state.listeners.remove(index));
            (removed, prior_failure)
        });
        // A leased callback is dropped by the dispatcher after invocation.
        let callback = listener.callback.borrow_mut().take();
        drop(removed);
        let mut first = None;
        if prior_failure {
            std::mem::forget(callback);
        } else {
            retire_callback(callback, &mut first);
        }
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
}

pub(crate) fn preserve(first: &mut Option<Panic>, next: Option<Panic>) {
    if let Some(payload) = next {
        if first.is_none() {
            *first = Some(payload);
        } else {
            std::mem::forget(payload);
        }
    }
}

// Rejected admission owns the generic callback too. It must release source
// borrows before ordinary retirement, and cannot compete with an earlier failure.
fn retire_rejected_callback(callback: impl FnMut(AppLifecycleState), prior_failure: bool) {
    if prior_failure || std::thread::panicking() {
        std::mem::forget(callback);
    } else {
        drop(callback);
    }
}

/// Retire one envelope only while no failure already owns the unwind.
fn retire_callback(callback: Option<Callback>, first: &mut Option<Panic>) {
    if first.is_some() || std::thread::panicking() {
        std::mem::forget(callback);
    } else {
        preserve(
            first,
            catch_unwind(AssertUnwindSafe(|| drop(callback))).err(),
        );
    }
}

/// Composition-root owner for a presentation lifecycle source.
///
/// Keep outside element/binding locks. Commit all local states before running
/// scheduler or lifecycle callbacks, then drain notifications. Build owners
/// receive only [`Self::handle`], never ownership of this source.
#[doc(hidden)]
pub struct LifecycleSource {
    inner: Rc<Inner>,
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
            inner: Rc::new(Inner(RefCell::new(State {
                current: None,
                phase: Phase::Open,
                listeners: Vec::new(),
                pending: VecDeque::new(),
                draining: false,
                retirement_failed: false,
                preserving_close: false,
                finish_requested: false,
                closing: 0,
            }))),
        }
    }
    /// Weak owner-local consumer capability.
    #[must_use]
    pub fn handle(&self) -> LifecycleHandle {
        LifecycleHandle {
            inner: Rc::downgrade(&self.inner),
        }
    }
    /// Last committed local observation, including during terminal cleanup.
    #[must_use]
    pub fn current(&self) -> Option<AppLifecycleState> {
        self.inner.0.borrow().current
    }
    /// Commit an ordinary observation without calling user code.
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal close begins.
    pub fn commit(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_for_phase(state, Phase::Open)
    }
    /// Hold the in-progress window of a terminal close until the returned
    /// guard drops. While it is held, a preserving close retains callbacks
    /// rejected or cancelled through stale handles; afterwards they retire
    /// normally. The host holds it across the whole presentation close.
    #[must_use = "the close window ends when the guard drops"]
    pub fn close_window(&self) -> LifecycleCloseWindow {
        self.inner.0.borrow_mut().closing += 1;
        LifecycleCloseWindow {
            inner: Rc::clone(&self.inner),
        }
    }
    /// Claim this presentation's close delivery: the token for the call that
    /// runs it, `None` once a delivery has run or is running, so a close
    /// requested again from inside a Detached observer, or repeated when the
    /// UI runtime drops, delivers nothing twice. The token records whether the
    /// delivery completed or was interrupted.
    ///
    /// Not yet latched: every call gets a token.
    #[must_use = "dropping the token without completing it records an interrupted delivery"]
    pub fn claim_close_delivery(&self) -> Option<CloseDelivery> {
        Some(CloseDelivery { _private: () })
    }

    /// How this presentation's close delivery stands.
    ///
    /// Not yet recorded: always [`CloseDeliveryState::NotStarted`].
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read once the host classifies a failed close")
    )]
    #[expect(
        clippy::unused_self,
        reason = "read from the source once the latch records"
    )]
    pub(crate) fn close_delivery_state(&self) -> CloseDeliveryState {
        CloseDeliveryState::NotStarted
    }
    /// Fence new subscriptions and ordinary commits before terminal callbacks.
    pub fn begin_close(&self) {
        let mut state = self.inner.0.borrow_mut();
        if state.phase == Phase::Open {
            state.phase = Phase::Closing;
        }
    }
    /// Seed terminal recovery without finishing the host's lifecycle ladder.
    pub fn begin_close_with_mode(&self, mode: flui_interaction::__runtime::CloseMode) {
        self.begin_close();
        if mode == flui_interaction::__runtime::CloseMode::PreservingFailure
            || std::thread::panicking()
        {
            let mut state = self.inner.0.borrow_mut();
            state.preserving_close = true;
            state.pending.clear();
        }
    }
    /// Commit an authorized terminal ladder step after `begin_close`.
    /// # Errors
    /// Returns [`LifecycleClosed`] outside the terminal notification phase.
    pub fn commit_terminal(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_for_phase(state, Phase::Closing)
    }
    fn commit_for_phase(
        &self,
        current: AppLifecycleState,
        phase: Phase,
    ) -> Result<(), LifecycleClosed> {
        let mut state = self.inner.0.borrow_mut();
        if state.phase != phase {
            return Err(LifecycleClosed);
        }
        if state.current != Some(current) {
            state.current = Some(current);
            if !state.preserving_close {
                let listeners = state.listeners.iter().map(Rc::downgrade).collect();
                state.pending.push_back(Event {
                    state: current,
                    listeners,
                });
            }
        }
        Ok(())
    }
    /// Drain committed events in FIFO order. Nested drains defer to this walk.
    /// All eligible callbacks run before the first panic resumes.
    pub fn drain(&self) {
        {
            let mut state = self.inner.0.borrow_mut();
            if state.draining {
                return;
            }
            state.draining = true;
            state.retirement_failed = false;
        }
        let mut first = None;
        loop {
            let event = self.inner.0.borrow_mut().pending.pop_front();
            let Some(event) = event else {
                break;
            };
            for listener in event.listeners {
                let Some(listener) = listener.upgrade() else {
                    continue;
                };
                if !listener.active.get() {
                    continue;
                }
                let callback = listener.callback.borrow_mut().take();
                let Some(mut callback) = callback else {
                    continue;
                };
                preserve(
                    &mut first,
                    catch_unwind(AssertUnwindSafe(|| callback(event.state))).err(),
                );
                if first.is_some() {
                    self.inner.0.borrow_mut().retirement_failed = true;
                }
                if listener.active.get() && self.inner.0.borrow().phase != Phase::Closed {
                    let previous = listener.callback.replace(Some(callback));
                    drop(previous);
                } else {
                    retire_callback(Some(callback), &mut first);
                    if first.is_some() {
                        self.inner.0.borrow_mut().retirement_failed = true;
                    }
                }
            }
        }
        if self.inner.0.borrow().finish_requested {
            self.release(&mut first);
        }
        {
            let mut state = self.inner.0.borrow_mut();
            state.draining = false;
            state.retirement_failed = false;
        }
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
        self.inner.0.borrow_mut().finish_requested = true;
        // A reentrant close is completed by the active FIFO walk after its
        // terminal event. It must not erase an event still awaiting delivery.
        self.drain();
    }
    fn release(&self, first: &mut Option<Panic>) {
        let listeners = {
            let mut state = self.inner.0.borrow_mut();
            state.phase = Phase::Closed;
            state.pending.clear();
            std::mem::take(&mut state.listeners)
        };
        for listener in listeners {
            listener.active.set(false);
            let callback = listener.callback.borrow_mut().take();
            if self.inner.0.borrow().retirement_failed || self.inner.0.borrow().preserving_close {
                std::mem::forget(callback);
            } else {
                retire_callback(callback, first);
            }
            if first.is_some() {
                self.inner.0.borrow_mut().retirement_failed = true;
                self.inner.0.borrow_mut().preserving_close = true;
            }
        }
    }
}
/// How a presentation's close delivery stands; see
/// [`LifecycleSource::claim_close_delivery`]. Crate-private until a
/// production reader exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "recorded once the close delivery latches")
)]
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
    _private: (),
}

impl CloseDelivery {
    /// Record that the delivery ran to its end.
    pub fn complete(self) {}
}

/// The in-progress window of one terminal close; see
/// [`LifecycleSource::close_window`].
#[doc(hidden)]
pub struct LifecycleCloseWindow {
    inner: Rc<Inner>,
}
impl std::fmt::Debug for LifecycleCloseWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleCloseWindow")
            .finish_non_exhaustive()
    }
}
impl Drop for LifecycleCloseWindow {
    fn drop(&mut self) {
        let mut state = self.inner.0.borrow_mut();
        state.closing = state.closing.saturating_sub(1);
    }
}
impl Drop for LifecycleSource {
    fn drop(&mut self) {
        let mut first = None;
        let failure = catch_unwind(AssertUnwindSafe(|| self.release(&mut first))).err();
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
    #[test]
    #[ignore = "contract: close delivery is claimed once and records how it ended"]
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
            ],
        );
    }
}
