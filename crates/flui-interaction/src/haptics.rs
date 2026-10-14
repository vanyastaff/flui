//! A presentation's haptics: the widget handle, the frame queue and the
//! outcome mailbox (ADR-0210 §3–§6, §10).
//!
//! [`HapticsHost`] is owned by one presentation and runs on its owner thread.
//! Widgets reach it through a [`HapticsHandle`] acquired from
//! `LifecycleContext::haptics`. An immediate request plays at once and returns
//! its final outcome. A [`HapticTiming::FramePresented`] request joins the
//! frame queue, requests a frame and returns
//! [`HapticOutcome::Queued`] with a [`HapticTicket`]; it plays when the
//! presentation presents a frame by its deadline, and otherwise resolves to
//! [`HapticOutcome::Suppressed`], never late. Its final outcome goes to the
//! presentation's observer, with the same ticket.
//!
//! # Delivery
//!
//! Resolving a queued request moves `(ticket, outcome)` into the outcome
//! mailbox before any observer runs. A delivery pass hands the head of the
//! mailbox to the observer registered at that moment, with no borrow held;
//! an entry leaves the mailbox only when the call for it returns normally.
//! An observer replaced during its call hands the rest of the batch to its
//! replacement. An observer that panics is contained: the entry it panicked
//! on stays at the head, and the pass ends. A failed pass makes the next one
//! due at once ([`HapticsHost::next_wake`]), up to three consecutive failed
//! passes per registration; after that the head waits for a new registration
//! or the close. The mailbox holds at most [`MAILBOX_CAPACITY`] entries,
//! counting queued requests: a frame-aligned request that finds it full plays
//! immediately instead, before a ticket exists.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::panic::{FailureClass, RecoveryScope};
use flui_foundation::{IdMint, InputSequenceId, MonotonicClock};
use flui_platform_api::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTicket, HapticTiming,
};
use flui_platform_api::{DeviceId, EventTime, HapticFeedback, PlatformHaptics};

/// How many outcomes a presentation holds for its observer, counting the
/// requests still queued for a frame (ADR-0210 §4).
pub const MAILBOX_CAPACITY: usize = 64;

/// Consecutive failed delivery passes one registration gets before the head
/// waits for a new registration (ADR-0210 §4).
const RETRY_BOUND: u8 = 3;

/// The display period assumed until the window reports one: 60 Hz.
pub const FALLBACK_REFRESH_PERIOD: Duration = Duration::from_nanos(16_666_667);

/// How long a frame-aligned haptic waits for its frame: `factor` refresh
/// periods of the window's display (ADR-0212 §4).
///
/// An unknown or zero period, or one whose product with `factor` overflows,
/// uses [`FALLBACK_REFRESH_PERIOD`]; the overflow is reported as a
/// diagnostic, since a backend reported a period no display has.
#[must_use]
pub fn haptic_frame_deadline(factor: NonZeroU32, period: Option<Duration>) -> Duration {
    let fallback = || {
        FALLBACK_REFRESH_PERIOD
            .checked_mul(factor.get())
            .expect("BUG: a u32 multiple of 16.7 ms fits in a Duration")
    };
    match period.filter(|period| !period.is_zero()) {
        None => fallback(),
        Some(period) => period.checked_mul(factor.get()).unwrap_or_else(|| {
            tracing::warn!(
                ?period,
                factor = factor.get(),
                "the haptic frame deadline overflows; using the 60 Hz period"
            );
            fallback()
        }),
    }
}

/// What a request plays.
#[derive(Clone)]
enum Play {
    Feedback(HapticFeedback),
    Pattern {
        pattern: HapticPattern,
        fallback: HapticFeedback,
    },
}

struct Queued {
    ticket: HapticTicket,
    deadline: EventTime,
    request: HapticRequest,
    play: Play,
}

type Observer = Box<dyn FnMut(HapticTicket, HapticOutcome)>;
type Actuator = Box<dyn Fn() -> Option<Arc<dyn PlatformHaptics>>>;

struct State {
    clock: Arc<dyn MonotonicClock>,
    sequence: Rc<Cell<Option<InputSequenceId>>>,
    actuator: Actuator,
    request_frame: Box<dyn Fn()>,
    refresh_period: Box<dyn Fn() -> Option<Duration>>,
    factor: NonZeroU32,
    /// The last period read and the deadline it gave, so the deadline is
    /// recomputed (and an overflow reported) only when the display changes.
    deadline: Cell<Option<(Option<Duration>, Duration)>>,
    preference: Cell<bool>,
    visible: Cell<bool>,
    closed: Cell<bool>,
    tickets: IdMint<NonZeroU64>,
    queue: RefCell<VecDeque<Queued>>,
    mailbox: RefCell<VecDeque<(HapticTicket, HapticOutcome)>>,
    observer: RefCell<Option<Observer>>,
    /// Bumped by every registration change, so a pass can tell its observer
    /// was replaced or removed while it ran.
    registration: Cell<u64>,
    delivering: Cell<bool>,
    failed_passes: Cell<u8>,
}

impl State {
    fn now(&self) -> EventTime {
        crate::events::event_time_of(self.clock.as_ref(), self.clock.now())
    }

    fn frame_deadline(&self) -> Duration {
        let period = (self.refresh_period)();
        match self.deadline.get() {
            Some((cached, deadline)) if cached == period => deadline,
            _ => {
                let deadline = haptic_frame_deadline(self.factor, period);
                self.deadline.set(Some((period, deadline)));
                deadline
            }
        }
    }

    fn play_now(&self, play: &Play, request: &HapticRequest) -> HapticOutcome {
        let Some(actuator) = (self.actuator)() else {
            return HapticOutcome::Unavailable;
        };
        match play {
            Play::Feedback(kind) => actuator.perform_with(*kind, request),
            Play::Pattern { pattern, fallback } => actuator.play(pattern, *fallback, request),
        }
    }

    fn submit(&self, play: Play, mut request: HapticRequest) -> HapticOutcome {
        if self.closed.get() || !self.preference.get() || !self.visible.get() {
            return HapticOutcome::Suppressed;
        }
        if request.sequence.is_none() {
            request.sequence = self.sequence.get();
        }
        if request.timing == HapticTiming::Immediate {
            return self.play_now(&play, &request);
        }
        let held = self.queue.borrow().len() + self.mailbox.borrow().len();
        if held >= MAILBOX_CAPACITY {
            return self.play_now(&play, &request);
        }
        // The terminal time has no later time to expire at.
        let Some(deadline) = self
            .now()
            .checked_add(self.frame_deadline())
            .filter(|deadline| *deadline != EventTime::from_nanos(u64::MAX))
        else {
            return HapticOutcome::Suppressed;
        };
        let Ok(ordinal) = self.tickets.next() else {
            return self.play_now(&play, &request);
        };
        let ticket = HapticTicket::new(ordinal);
        self.queue.borrow_mut().push_back(Queued {
            ticket,
            deadline,
            request,
            play,
        });
        (self.request_frame)();
        HapticOutcome::Queued(ticket)
    }

    /// Resolve every queued request `resolve` decides, in ticket order, into
    /// the mailbox; requests it returns `None` for stay queued.
    fn resolve(&self, mut resolve: impl FnMut(&Queued) -> Option<HapticOutcome>) {
        let queued = std::mem::take(&mut *self.queue.borrow_mut());
        let mut kept = VecDeque::new();
        let mut resolved = Vec::new();
        for entry in queued {
            match resolve(&entry) {
                Some(outcome) => resolved.push((entry.ticket, outcome)),
                None => kept.push_back(entry),
            }
        }
        // A backend call above may have queued more; they stay behind.
        let mut queue = self.queue.borrow_mut();
        kept.append(&mut queue);
        *queue = kept;
        drop(queue);
        self.mailbox.borrow_mut().extend(resolved);
    }

    fn suppress_queued(&self) {
        self.resolve(|_| Some(HapticOutcome::Suppressed));
    }

    fn delivery_due(&self) -> bool {
        !self.mailbox.borrow().is_empty()
            && self.observer.borrow().is_some()
            && self.failed_passes.get() < RETRY_BOUND
    }

    fn set_observer(&self, observer: Option<Observer>) {
        self.registration
            .set(self.registration.get().wrapping_add(1));
        self.failed_passes.set(0);
        let outgoing = std::mem::replace(&mut *self.observer.borrow_mut(), observer);
        // Retired after the slot is committed: its destructor may reenter,
        // and a panic in it must not unwind into the widget that replaced it.
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(outgoing)))
        {
            tracing::error!(
                message = flui_foundation::panic::payload_text(payload.as_ref()),
                "a replaced haptics observer panicked while it was dropped"
            );
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    /// One delivery pass, under `recovery`'s custody.
    fn deliver(&self, recovery: &mut RecoveryScope<'_>) {
        if self.delivering.replace(true) {
            // The pass already running picks up whatever changed.
            return;
        }
        loop {
            let Some(&(ticket, outcome)) = self.mailbox.borrow().front() else {
                break;
            };
            let Some(mut observer) = self.observer.borrow_mut().take() else {
                break;
            };
            let registration = self.registration.get();
            let returned = recovery.run_classified(FailureClass::Delivery, |_| {
                observer(ticket, outcome);
            });
            let replaced = self.registration.get() != registration;
            if returned {
                self.mailbox.borrow_mut().pop_front();
            }
            if replaced {
                // The committed replacement (or removal) wins; the outgoing
                // observer is retired under the same custody.
                // A replacement takes over the rest of the batch, starting
                // with the head this call kept if it panicked.
                recovery.retire(observer);
                continue;
            }
            *self.observer.borrow_mut() = Some(observer);
            if returned {
                self.failed_passes.set(0);
            } else {
                self.failed_passes
                    .set(self.failed_passes.get().saturating_add(1));
                break;
            }
        }
        self.delivering.set(false);
    }
}

/// A presentation's haptics: its frame queue, outcome mailbox and observer.
///
/// Owner-thread (`!Send`). The presentation that owns it drives it:
/// [`frame_presented`](Self::frame_presented) when a frame is presented,
/// [`service`](Self::service) on each owner turn,
/// [`set_visible`](Self::set_visible) when the window is hidden, occluded or
/// shown, and [`close`](Self::close) with the presentation.
pub struct HapticsHost {
    state: Rc<State>,
}

impl fmt::Debug for HapticsHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HapticsHost")
            .field("queued", &self.state.queue.borrow().len())
            .field("mailbox", &self.state.mailbox.borrow().len())
            .field("closed", &self.state.closed.get())
            .finish_non_exhaustive()
    }
}

impl HapticsHost {
    /// A host on `binding`'s clock, tagging requests with the sequence it is
    /// dispatching, playing on the actuator `actuator` returns (`None` when
    /// the window has none or is gone).
    ///
    /// The deadline factor is 2 and the display period unknown until
    /// [`with_deadline_factor`](Self::with_deadline_factor) and
    /// [`with_refresh_period`](Self::with_refresh_period) say otherwise.
    #[must_use]
    pub fn new(
        binding: &crate::GestureBinding,
        actuator: impl Fn() -> Option<Arc<dyn PlatformHaptics>> + 'static,
    ) -> Self {
        Self {
            state: Rc::new(State {
                clock: Arc::clone(binding.clock()),
                sequence: binding.dispatched_sequence_cell(),
                actuator: Box::new(actuator),
                request_frame: Box::new(|| {}),
                refresh_period: Box::new(|| None),
                factor: NonZeroU32::new(2).expect("BUG: literal is nonzero"),
                deadline: Cell::new(None),
                preference: Cell::new(true),
                visible: Cell::new(true),
                closed: Cell::new(false),
                tickets: IdMint::new(),
                queue: RefCell::new(VecDeque::new()),
                mailbox: RefCell::new(VecDeque::new()),
                observer: RefCell::new(None),
                registration: Cell::new(0),
                delivering: Cell::new(false),
                failed_passes: Cell::new(0),
            }),
        }
    }

    fn configured(self, configure: impl FnOnce(&mut State)) -> Self {
        let mut state =
            Rc::into_inner(self.state).expect("BUG: a host is configured before any handle exists");
        configure(&mut state);
        Self {
            state: Rc::new(state),
        }
    }

    /// Call `request_frame` when a request joins the frame queue.
    #[must_use]
    pub fn with_frame_requester(self, request_frame: impl Fn() + 'static) -> Self {
        self.configured(|state| state.request_frame = Box::new(request_frame))
    }

    /// Read the window display's refresh period from `period` when a request
    /// is queued; the deadline is recomputed whenever it changes.
    #[must_use]
    pub fn with_refresh_period(self, period: impl Fn() -> Option<Duration> + 'static) -> Self {
        self.configured(|state| state.refresh_period = Box::new(period))
    }

    /// The platform's `haptic_frame_deadline_factor`: how many refresh
    /// periods a queued request waits.
    #[must_use]
    pub fn with_deadline_factor(self, factor: NonZeroU32) -> Self {
        self.configured(|state| state.factor = factor)
    }

    /// Issue tickets after ordinal `last`, so a test reaches the terminal
    /// ticket without issuing every one before it.
    #[must_use]
    pub fn with_tickets_after(self, last: u64) -> Self {
        self.configured(|state| state.tickets = IdMint::starting_after(last))
    }

    /// A widget-facing handle on this host.
    #[must_use]
    pub fn handle(&self) -> HapticsHandle {
        HapticsHandle {
            state: Rc::downgrade(&self.state),
            switched_off: false,
        }
    }

    /// Record the user's haptics setting: `false` suppresses both tiers
    /// before they reach the backend (ADR-0212 §1).
    pub fn set_preference(&self, enabled: bool) {
        self.state.preference.set(enabled);
    }

    /// Record whether the window can show anything. Hiding, occluding or
    /// suspending it resolves every queued request to
    /// [`HapticOutcome::Suppressed`], and nothing plays until it is shown.
    pub fn set_visible(&self, visible: bool) {
        self.state.visible.set(visible);
        if !visible {
            self.state.suppress_queued();
        }
    }

    /// The deadline a request queued now gets: the factor times the window's
    /// current refresh period.
    #[must_use]
    pub fn frame_deadline(&self) -> Duration {
        self.state.frame_deadline()
    }

    /// A frame was presented: every queued request whose deadline has not
    /// passed plays now, and the rest resolve to
    /// [`HapticOutcome::Suppressed`]. Runs no observer: their outcomes wait
    /// in the mailbox for the next [`service`](Self::service).
    pub fn frame_presented(&self) {
        let state = &*self.state;
        if !state.visible.get() {
            state.suppress_queued();
            return;
        }
        let now = state.now();
        state.resolve(|entry| {
            Some(if now <= entry.deadline {
                state.play_now(&entry.play, &entry.request)
            } else {
                HapticOutcome::Suppressed
            })
        });
    }

    /// The owner turn's work: resolve queued requests whose deadline has
    /// passed to [`HapticOutcome::Suppressed`], then run a delivery pass if
    /// one is due, under `recovery`'s custody.
    pub fn service(&self, recovery: &mut RecoveryScope<'_>) {
        let state = &*self.state;
        let now = state.now();
        state.resolve(|entry| (now > entry.deadline).then_some(HapticOutcome::Suppressed));
        if state.delivery_due() {
            state.deliver(recovery);
        }
    }

    /// When the owner must next run [`service`](Self::service): the first
    /// time strictly after the earliest queued deadline, or now when a
    /// delivery pass is due.
    #[must_use]
    pub fn next_wake(&self) -> Option<web_time::Instant> {
        let state = &*self.state;
        if state.delivery_due() {
            return Some(state.clock.now());
        }
        state
            .queue
            .borrow()
            .iter()
            .map(|entry| entry.deadline)
            .min()
            .map(|deadline| {
                let after = EventTime::from_nanos(deadline.as_nanos().saturating_add(1));
                crate::events::instant_at(state.clock.as_ref(), after)
            })
    }

    /// Whether a delivery pass is due now.
    #[must_use]
    pub fn has_pending_delivery(&self) -> bool {
        self.state.delivery_due()
    }

    /// Close: resolve every queued request to [`HapticOutcome::Suppressed`],
    /// run one final delivery pass, report what it could not deliver with
    /// its ticket, and retire the observer. Later requests are suppressed.
    pub fn close(&self, recovery: &mut RecoveryScope<'_>) {
        let state = &*self.state;
        if state.closed.replace(true) {
            return;
        }
        state.suppress_queued();
        state.failed_passes.set(0);
        state.deliver(recovery);
        let undelivered = std::mem::take(&mut *state.mailbox.borrow_mut());
        for (ticket, outcome) in undelivered {
            tracing::warn!(
                ticket = ticket.get().get(),
                ?outcome,
                "a haptic outcome was not delivered before its presentation closed"
            );
        }
        let observer = state.observer.borrow_mut().take();
        recovery.retire(observer);
    }

    /// Close after a failure that preserves ownership: like
    /// [`close`](Self::close), but runs no observer. Pending outcomes are
    /// reported with their tickets, and the observer is retained, never
    /// dropped, so none of its code runs on the failed path.
    pub fn withdraw(&self) {
        let state = &*self.state;
        state.closed.set(true);
        state.suppress_queued();
        let undelivered = std::mem::take(&mut *state.mailbox.borrow_mut());
        for (ticket, outcome) in undelivered {
            tracing::warn!(
                ticket = ticket.get().get(),
                ?outcome,
                "a haptic outcome was withdrawn with its failed presentation"
            );
        }
        std::mem::forget(state.observer.borrow_mut().take());
    }
}

/// A widget's haptics capability on one presentation (ADR-0210 §10).
///
/// Acquired from `LifecycleContext::haptics` in `init_state` or
/// `did_change_dependencies`; owner-thread (`!Send`), cheap to clone. Under a
/// subtree that switched haptics off, every request resolves to
/// [`HapticOutcome::Suppressed`]. A request outside dispatch carries no
/// sequence; inside one, the handle tags it with the sequence being
/// dispatched.
#[derive(Clone)]
pub struct HapticsHandle {
    state: Weak<State>,
    switched_off: bool,
}

impl fmt::Debug for HapticsHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HapticsHandle")
            .field("switched_off", &self.switched_off)
            .finish_non_exhaustive()
    }
}

impl HapticsHandle {
    /// This handle with haptics switched off, for a subtree that turns them
    /// off.
    #[must_use]
    pub fn switched_off(self) -> Self {
        Self {
            switched_off: true,
            ..self
        }
    }

    fn submit(&self, play: Play, request: HapticRequest) -> HapticOutcome {
        if self.switched_off {
            return HapticOutcome::Suppressed;
        }
        match self.state.upgrade() {
            Some(state) => state.submit(play, request),
            None => HapticOutcome::Unavailable,
        }
    }

    /// Perform semantic feedback `kind` for `request` (tier 1).
    pub fn perform(&self, kind: HapticFeedback, request: HapticRequest) -> HapticOutcome {
        self.submit(Play::Feedback(kind), request)
    }

    /// Play `pattern` for `request`, or `fallback` where it cannot play
    /// (tier 2).
    pub fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: HapticRequest,
    ) -> HapticOutcome {
        self.submit(
            Play::Pattern {
                pattern: pattern.clone(),
                fallback,
            },
            request,
        )
    }

    /// What `source`'s actuator (the window's for `None`) can play; read it
    /// when the device set changes, not per request.
    #[must_use]
    pub fn capabilities(&self, source: Option<DeviceId>) -> HapticCapabilities {
        self.state
            .upgrade()
            .and_then(|state| (state.actuator)())
            .map_or(HapticCapabilities::NONE, |actuator| {
                actuator.capabilities(source)
            })
    }

    /// Register the presentation's outcome observer, replacing the one
    /// before: it receives each frame-queued request's final outcome with
    /// its ticket, including outcomes that waited for an observer. One
    /// observer serves the presentation.
    pub fn set_observer(&self, observer: impl FnMut(HapticTicket, HapticOutcome) + 'static) {
        if let Some(state) = self.state.upgrade() {
            state.set_observer(Some(Box::new(observer)));
        }
    }

    /// Remove the presentation's outcome observer; outcomes wait in the
    /// mailbox for the next one.
    pub fn clear_observer(&self) {
        if let Some(state) = self.state.upgrade() {
            state.set_observer(None);
        }
    }
}
