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
//! presentation's observers, with the same ticket.
//!
//! # Delivery
//!
//! Resolving a queued request moves `(ticket, outcome)` into the outcome
//! mailbox before any observer runs. A delivery pass hands the head of the
//! mailbox to each observer registered at that moment that has not had it
//! yet, in registration order, with no borrow held; the head leaves the
//! mailbox once every registered observer's call for it returned normally.
//! An observer replaced during its call hands the rest of the batch to its
//! replacement. An observer that panics is contained: the head stays, the
//! observers that already had it do not get it again, and the pass ends. A
//! failed pass makes the next one due at once ([`HapticsHost::next_wake`]),
//! up to three consecutive failed passes per registration; after that the
//! head waits for that observer's replacement or removal, a new registration,
//! or the close. With no observer registered, outcomes wait for the first.
//! The mailbox holds at most [`MAILBOX_CAPACITY`] entries, counting queued
//! requests: a frame-aligned request that finds it full plays immediately
//! instead, before a ticket exists.

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

/// How many outcomes a presentation holds for its observers, counting the
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
/// uses [`FALLBACK_REFRESH_PERIOD`], and says so as a diagnostic: the
/// deadline then fits a 60 Hz display, not necessarily the window's. An
/// overflow is a warning, since a backend reported a period no display has.
/// The host computes the deadline again only when the period changes, so a
/// window reports this once per display, not per request.
#[must_use]
pub fn haptic_frame_deadline(factor: NonZeroU32, period: Option<Duration>) -> Duration {
    let fallback = || {
        FALLBACK_REFRESH_PERIOD
            .checked_mul(factor.get())
            .expect("BUG: a u32 multiple of 16.7 ms fits in a Duration")
    };
    match period.filter(|period| !period.is_zero()) {
        None => {
            tracing::info!(
                ?period,
                factor = factor.get(),
                "the window reports no refresh period; the haptic frame deadline uses the 60 Hz period"
            );
            fallback()
        }
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

/// A resolved request waiting in the mailbox, with the registrations that
/// already accepted it.
struct Entry {
    ticket: HapticTicket,
    outcome: HapticOutcome,
    delivered: Vec<NonZeroU64>,
}

type Observer = Box<dyn FnMut(HapticTicket, HapticOutcome)>;
type Actuator = Box<dyn Fn() -> Option<Arc<dyn PlatformHaptics>>>;

/// One registered observer. `observer` is `None` while a pass is calling it.
struct Slot {
    id: NonZeroU64,
    /// Bumped by every replacement, so a pass can tell its call's observer
    /// was replaced while it ran.
    generation: u64,
    observer: Option<Observer>,
    failed_passes: u8,
}

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
    mailbox: RefCell<VecDeque<Entry>>,
    observers: RefCell<Vec<Slot>>,
    registrations: IdMint<NonZeroU64>,
    delivering: Cell<bool>,
}

/// Drop `outgoing` outside every borrow; a panic in its destructor must not
/// unwind into the widget that replaced or removed it.
fn retire_outside_delivery(outgoing: Option<Observer>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(outgoing)))
    {
        tracing::error!(
            message = flui_foundation::panic::payload_text(payload.as_ref()),
            "a retired haptics observer panicked while it was dropped"
        );
        flui_foundation::panic::retain_opaque_payload(payload);
    }
}

/// What the head of the mailbox needs.
enum Head {
    /// Nothing to deliver, or only to observers past their retry bound.
    Waiting,
    /// The registration `id` has not had it yet.
    Deliver(NonZeroU64),
    /// Every registered observer accepted it.
    Done,
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

    /// Whether nothing may play: the presentation closed, the user turned
    /// haptics off, or the window shows nothing.
    fn suppressed(&self) -> bool {
        self.closed.get() || !self.preference.get() || !self.visible.get()
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
        if self.suppressed() {
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
                Some(outcome) => resolved.push(Entry {
                    ticket: entry.ticket,
                    outcome,
                    delivered: Vec::new(),
                }),
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

    fn head(&self) -> Head {
        let mailbox = self.mailbox.borrow();
        let Some(entry) = mailbox.front() else {
            return Head::Waiting;
        };
        let observers = self.observers.borrow();
        if observers.is_empty() {
            return Head::Waiting;
        }
        let mut pending = observers
            .iter()
            .filter(|slot| !entry.delivered.contains(&slot.id))
            .peekable();
        if pending.peek().is_none() {
            return Head::Done;
        }
        pending
            .find(|slot| slot.observer.is_some() && slot.failed_passes < RETRY_BOUND)
            .map_or(Head::Waiting, |slot| Head::Deliver(slot.id))
    }

    fn delivery_due(&self) -> bool {
        !matches!(self.head(), Head::Waiting)
    }

    fn register(&self, observer: Observer) -> Option<NonZeroU64> {
        let Ok(id) = self.registrations.next() else {
            tracing::warn!("haptics observer identities are exhausted; registration refused");
            return None;
        };
        self.observers.borrow_mut().push(Slot {
            id,
            generation: 0,
            observer: Some(observer),
            failed_passes: 0,
        });
        Some(id)
    }

    fn replace(&self, id: NonZeroU64, observer: Observer) {
        let outgoing = {
            let mut observers = self.observers.borrow_mut();
            let Some(slot) = observers.iter_mut().find(|slot| slot.id == id) else {
                drop(observers);
                retire_outside_delivery(Some(observer));
                return;
            };
            slot.generation = slot.generation.wrapping_add(1);
            slot.failed_passes = 0;
            slot.observer.replace(observer)
        };
        retire_outside_delivery(outgoing);
    }

    fn remove(&self, id: NonZeroU64) {
        let outgoing = {
            let mut observers = self.observers.borrow_mut();
            observers
                .iter()
                .position(|slot| slot.id == id)
                .and_then(|index| observers.remove(index).observer)
        };
        retire_outside_delivery(outgoing);
    }

    /// One delivery pass, under `recovery`'s custody.
    fn deliver(&self, recovery: &mut RecoveryScope<'_>) {
        if self.delivering.replace(true) {
            // The pass already running picks up whatever changed.
            return;
        }
        loop {
            let id = match self.head() {
                Head::Waiting => break,
                Head::Done => {
                    self.mailbox.borrow_mut().pop_front();
                    continue;
                }
                Head::Deliver(id) => id,
            };
            let Some((ticket, outcome)) = self
                .mailbox
                .borrow()
                .front()
                .map(|entry| (entry.ticket, entry.outcome))
            else {
                break;
            };
            let (generation, mut observer) = {
                let mut observers = self.observers.borrow_mut();
                let slot = observers
                    .iter_mut()
                    .find(|slot| slot.id == id)
                    .expect("BUG: the head names a registered observer");
                let observer = slot
                    .observer
                    .take()
                    .expect("BUG: the head names an observer no pass holds");
                (slot.generation, observer)
            };
            let returned = recovery.run_classified(FailureClass::Delivery, |_| {
                observer(ticket, outcome);
            });
            if returned
                && let Some(entry) = self
                    .mailbox
                    .borrow_mut()
                    .front_mut()
                    .filter(|entry| entry.ticket == ticket)
            {
                entry.delivered.push(id);
            }
            let restored = {
                let mut observers = self.observers.borrow_mut();
                match observers.iter_mut().find(|slot| slot.id == id) {
                    Some(slot) if slot.generation == generation => {
                        slot.failed_passes = if returned {
                            0
                        } else {
                            slot.failed_passes.saturating_add(1)
                        };
                        slot.observer = Some(observer);
                        None
                    }
                    // Replaced or removed during its call: the committed
                    // replacement (or removal) wins, and a replacement takes
                    // over the rest of the batch, starting with the head this
                    // call kept if it panicked.
                    _ => Some(observer),
                }
            };
            match restored {
                Some(outgoing) => recovery.retire(outgoing),
                None if !returned => break,
                None => {}
            }
        }
        self.delivering.set(false);
    }

    fn report_undelivered(&self, reason: &'static str) {
        let undelivered = std::mem::take(&mut *self.mailbox.borrow_mut());
        for entry in undelivered {
            tracing::warn!(
                ticket = entry.ticket.get().get(),
                outcome = ?entry.outcome,
                "{reason}"
            );
        }
    }
}

/// A presentation's haptics: its frame queue, outcome mailbox and observers.
///
/// Owner-thread (`!Send`). The presentation that owns it drives it:
/// [`frame_presented`](Self::frame_presented) when a frame is presented,
/// [`service`](Self::service) on each owner turn,
/// [`set_visible`](Self::set_visible) when the window is hidden, occluded or
/// shown, [`set_preference`](Self::set_preference) when the user's haptics
/// setting changes, and [`close`](Self::close) with the presentation.
pub struct HapticsHost {
    state: Rc<State>,
}

impl fmt::Debug for HapticsHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HapticsHost")
            .field("queued", &self.state.queue.borrow().len())
            .field("mailbox", &self.state.mailbox.borrow().len())
            .field("observers", &self.state.observers.borrow().len())
            .field("closed", &self.state.closed.get())
            .finish_non_exhaustive()
    }
}

/// The configuration of a [`HapticsHost`] not yet built.
///
/// No [`HapticsHandle`] can exist before [`build`](Self::build) returns the
/// host, so configuration never runs while a widget holds the host.
#[must_use = "a builder does nothing until it is built"]
pub struct HapticsHostBuilder {
    state: State,
}

impl fmt::Debug for HapticsHostBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HapticsHostBuilder")
            .field("factor", &self.state.factor)
            .finish_non_exhaustive()
    }
}

impl HapticsHostBuilder {
    /// Call `request_frame` when a request joins the frame queue.
    pub fn frame_requester(mut self, request_frame: impl Fn() + 'static) -> Self {
        self.state.request_frame = Box::new(request_frame);
        self
    }

    /// Read the window display's refresh period from `period` when a request
    /// is queued; the deadline is recomputed whenever it changes.
    pub fn refresh_period(mut self, period: impl Fn() -> Option<Duration> + 'static) -> Self {
        self.state.refresh_period = Box::new(period);
        self
    }

    /// The platform's `haptic_frame_deadline_factor`: how many refresh
    /// periods a queued request waits.
    pub fn deadline_factor(mut self, factor: NonZeroU32) -> Self {
        self.state.factor = factor;
        self
    }

    /// Issue tickets after ordinal `last`, so a test reaches the terminal
    /// ticket without issuing every one before it.
    #[cfg(feature = "testing")]
    pub fn tickets_after(mut self, last: u64) -> Self {
        self.state.tickets = IdMint::starting_after(last);
        self
    }

    /// The configured host, ready to hand out handles.
    #[must_use]
    pub fn build(self) -> HapticsHost {
        HapticsHost {
            state: Rc::new(self.state),
        }
    }
}

impl HapticsHost {
    /// Configure a host on `binding`'s clock, tagging requests with the
    /// sequence it is dispatching, playing on the actuator `actuator` returns
    /// (`None` when the window has none or is gone).
    ///
    /// The deadline factor is 2 and the display period unknown until
    /// [`deadline_factor`](HapticsHostBuilder::deadline_factor) and
    /// [`refresh_period`](HapticsHostBuilder::refresh_period) say otherwise.
    pub fn builder(
        binding: &crate::GestureBinding,
        actuator: impl Fn() -> Option<Arc<dyn PlatformHaptics>> + 'static,
    ) -> HapticsHostBuilder {
        HapticsHostBuilder {
            state: State {
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
                observers: RefCell::new(Vec::new()),
                registrations: IdMint::new(),
                delivering: Cell::new(false),
            },
        }
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
    /// before they reach the backend (ADR-0212 §1). Turning it off resolves
    /// every queued request to [`HapticOutcome::Suppressed`], as hiding the
    /// window does, and nothing plays until it is turned on again.
    pub fn set_preference(&self, enabled: bool) {
        self.state.preference.set(enabled);
        if !enabled {
            self.state.suppress_queued();
        }
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
    /// [`HapticOutcome::Suppressed`], as all of them do while haptics are off
    /// or the window is hidden. Runs no observer: their outcomes wait in the
    /// mailbox for the next [`service`](Self::service).
    pub fn frame_presented(&self) {
        let state = &*self.state;
        if state.suppressed() {
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
    /// its ticket, and retire the observers. Later requests are suppressed.
    pub fn close(&self, recovery: &mut RecoveryScope<'_>) {
        let state = &*self.state;
        if state.closed.replace(true) {
            return;
        }
        state.suppress_queued();
        for slot in state.observers.borrow_mut().iter_mut() {
            slot.failed_passes = 0;
        }
        state.deliver(recovery);
        state.report_undelivered(
            "a haptic outcome was not delivered before its presentation closed",
        );
        let observers = std::mem::take(&mut *state.observers.borrow_mut());
        for slot in observers {
            recovery.retire(slot.observer);
        }
    }

    /// Close after a failure that preserves ownership: like
    /// [`close`](Self::close), but runs no observer. Pending outcomes are
    /// reported with their tickets, and the observers are retained, never
    /// dropped, so none of their code runs on the failed path.
    pub fn withdraw(&self) {
        let state = &*self.state;
        state.closed.set(true);
        state.suppress_queued();
        state.report_undelivered("a haptic outcome was withdrawn with its failed presentation");
        let observers = std::mem::take(&mut *state.observers.borrow_mut());
        for slot in observers {
            std::mem::forget(slot.observer);
        }
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

    /// Register an outcome observer on the presentation: it receives each
    /// frame-queued request's final outcome with its ticket, including
    /// outcomes that waited for an observer, beside every other observer
    /// registered. It stays registered while the returned registration lives.
    pub fn observe(
        &self,
        observer: impl FnMut(HapticTicket, HapticOutcome) + 'static,
    ) -> HapticObserver {
        let observer: Observer = Box::new(observer);
        let registered = match self.state.upgrade() {
            Some(state) if !state.closed.get() => state.register(observer),
            _ => {
                retire_outside_delivery(Some(observer));
                None
            }
        };
        HapticObserver {
            state: self.state.clone(),
            id: registered,
        }
    }
}

/// One observer's registration on a presentation's haptics. Dropping it
/// removes the observer, whose outstanding outcomes then no longer wait for
/// it.
#[must_use = "the observer is removed when its registration is dropped"]
pub struct HapticObserver {
    state: Weak<State>,
    /// `None` when the registration was refused: the presentation was gone
    /// or closed, or its observer identities were exhausted.
    id: Option<NonZeroU64>,
}

impl fmt::Debug for HapticObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HapticObserver")
            .field("registered", &self.id.is_some())
            .finish_non_exhaustive()
    }
}

impl HapticObserver {
    /// Replace this registration's observer. Called from the observer's own
    /// call, the replacement takes over the rest of the batch.
    pub fn replace(&self, observer: impl FnMut(HapticTicket, HapticOutcome) + 'static) {
        let observer: Observer = Box::new(observer);
        match (self.id, self.state.upgrade()) {
            (Some(id), Some(state)) => state.replace(id, observer),
            _ => retire_outside_delivery(Some(observer)),
        }
    }
}

impl Drop for HapticObserver {
    fn drop(&mut self) {
        if let (Some(id), Some(state)) = (self.id, self.state.upgrade()) {
            state.remove(id);
        }
    }
}
