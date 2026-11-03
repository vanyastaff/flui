//! Delivery of a runtime's [`InputRecord`] stream to its taps (ADR-0215 §1).
//!
//! The runtime admits each record after the dispatch it describes has
//! committed, then a delivery pass hands the owned rows to the taps one by
//! one with no runtime borrow held. A pass delivers only the rows present when
//! it started: rows a tap callback admits reentrantly wait for the next pass,
//! which an owner wake schedules, so a tap that dispatches on every row cannot
//! keep the owner loop from returning.
//!
//! Several taps may be registered at once, an application's recording beside
//! a driver's. Each registration is offered the rows admitted after it was
//! installed, in the order they were admitted, and keeps its own account of
//! what it lost: one tap's failure never reaches another's recording.
//!
//! Every tap call is contained. The first failure is authoritative and
//! reported; the tap stays registered, the row it panicked on is not offered
//! to it again, and the loss travels as its pending count delivered as an
//! [`InputRecord::Gap`] row in a second contained call, retried before its
//! next row while it keeps failing. Finishing delivers its pending gap and an
//! [`InputRecord::End`] row; a recording that cannot get both out reports
//! [`RecordingIncomplete`] instead of claiming to be whole.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::panic::payload_text;
use flui_foundation::{IdExhausted, IdMint, InputSequenceId};
use flui_interaction::PointerEvent;
use flui_platform::PlatformInput;

use crate::HostInput;
use crate::input_record::{InputRecord, InputRow, RecordedSequence, RecordingStart};
use crate::owner::InputOutcome;

/// The tracing target every contained tap failure is reported on.
pub const INPUT_TAP_TARGET: &str = "flui_runtime::input_tap";

/// Observes the records a runtime emits, on its owner thread, in the order
/// the runtime applied them.
///
/// A tap may dispatch input, install or remove taps, or panic: dispatched
/// input is recorded in a later pass, a tap installed meanwhile is offered the
/// rows admitted after it, and a panic is contained (see the `input_tap`
/// module docs).
pub trait InputTap {
    /// Observe one record.
    fn record(&self, record: &InputRecord);
}

impl<F: Fn(&InputRecord)> InputTap for F {
    fn record(&self, record: &InputRecord) {
        self(record);
    }
}

/// A recording ended without its final rows: the tap panicked on the pending
/// [`InputRecord::Gap`] or on the [`InputRecord::End`] row, or its runtime's
/// registry was gone when it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the recording is incomplete: {lost_rows} rows were lost and its end was not delivered")]
pub struct RecordingIncomplete {
    /// Every row the tap lost.
    pub lost_rows: u64,
}

/// Where a contained tap failure happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TapFailureSite {
    /// The tap's call for a row.
    Row,
    /// The tap's call for a pending [`InputRecord::Gap`] row.
    Gap,
    /// The tap's call for the [`InputRecord::End`] row.
    End,
    /// Destroying a removed tap.
    Retire,
    /// The owner wake that schedules the next delivery pass.
    Wake,
}

/// The first failure a tap registry contained: where it happened and the
/// panic's message. The payload itself is discarded under containment, because
/// its destructor is user code (ADR-0281 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TapFailure {
    /// Where the panic was contained.
    pub site: TapFailureSite,
    /// The panic's message; `None` for a payload that is not text.
    pub message: Option<String>,
}

/// A runtime's tap registry and its pending records.
///
/// Clones share one registry. [`install`](Self::install) adds a registration
/// beside the others: each is offered the rows admitted after it was
/// installed and keeps its own lost-row count, pending gap and `End` row, so
/// an application's recording and a driver's run side by side (ADR-0215 §1).
/// The runtime admits and delivers records only while one is registered.
#[derive(Clone)]
pub struct InputTaps {
    state: Rc<TapState>,
}

impl std::fmt::Debug for InputTaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputTaps")
            .field("registrations", &self.state.registered.borrow().len())
            .field("queued", &self.state.queue.borrow().len())
            .finish_non_exhaustive()
    }
}

struct TapState {
    /// Rows admitted for the next pass.
    queue: RefCell<VecDeque<Admitted>>,
    /// The rows of the running delivery pass not yet offered.
    in_flight: RefCell<VecDeque<Admitted>>,
    /// The row the running pass is offering, and to which registration.
    offering: Cell<Option<Offering>>,
    /// The registrations in the order they were installed, so by generation.
    registered: RefCell<Vec<Rc<Registration>>>,
    /// Issues registration generations; never reuses one.
    generations: IdMint<NonZeroU64>,
    /// The number the next admitted row takes.
    next_row: Cell<u64>,
    delivering: Cell<bool>,
    first_failure: RefCell<Option<TapFailure>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// A row admitted for delivery, numbered in the order it was admitted.
struct Admitted {
    number: u64,
    record: InputRecord,
}

/// Where the running pass is with its row.
#[derive(Clone, Copy)]
struct Offering {
    row: u64,
    /// The registration whose call is running.
    generation: NonZeroU64,
    /// Whether that call is the row's own, rather than the gap ahead of it.
    row_call: bool,
}

/// One registration: its tap and its own account of the rows it lost.
struct Registration {
    generation: NonZeroU64,
    /// The number of the first row it is offered: the rows admitted before it
    /// was installed belong to the recordings already running.
    first_row: u64,
    tap: Box<dyn InputTap>,
    /// Lost rows not yet delivered as a `Gap`.
    pending_gap: Cell<u64>,
    lost_total: Cell<u64>,
}

impl Registration {
    fn wants(&self, row: u64) -> bool {
        row >= self.first_row
    }

    fn lose(&self, rows: u64) {
        self.lost_total
            .set(self.lost_total.get().saturating_add(rows));
        self.pending_gap
            .set(self.pending_gap.get().saturating_add(rows));
    }
}

impl InputTaps {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            state: Rc::new(TapState {
                queue: RefCell::new(VecDeque::new()),
                in_flight: RefCell::new(VecDeque::new()),
                offering: Cell::new(None),
                registered: RefCell::new(Vec::new()),
                generations: IdMint::new(),
                next_row: Cell::new(0),
                delivering: Cell::new(false),
                first_failure: RefCell::new(None),
                wake,
            }),
        }
    }

    /// Register `tap` beside the registered taps. It is offered every row
    /// admitted from now on: inside a tap call, from the next row admitted,
    /// not the rest of the running pass.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`], permanently, once every registration generation was
    /// issued.
    pub fn install(&self, tap: impl InputTap + 'static) -> Result<InputTapHandle, IdExhausted> {
        self.install_boxed(Box::new(tap))
    }

    /// [`install`](Self::install) for a tap a factory produced as a trait
    /// object.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`], as [`install`](Self::install).
    pub fn install_boxed(&self, tap: Box<dyn InputTap>) -> Result<InputTapHandle, IdExhausted> {
        let generation = self.state.generations.next()?;
        self.state
            .registered
            .borrow_mut()
            .push(Rc::new(Registration {
                generation,
                first_row: self.state.next_row.get(),
                tap,
                pending_gap: Cell::new(0),
                lost_total: Cell::new(0),
            }));
        Ok(InputTapHandle {
            state: Rc::downgrade(&self.state),
            generation,
        })
    }

    /// Whether any tap is registered, so the runtime records.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        !self.state.registered.borrow().is_empty()
    }

    /// The first tap failure this registry contained, if any. Later
    /// failures never replace it.
    #[must_use]
    pub fn first_failure(&self) -> Option<TapFailure> {
        self.state.first_failure.borrow().clone()
    }

    /// Issue no registration generation at or below `last` from now on, so
    /// a test can reach the terminal generation.
    #[cfg(feature = "testing")]
    pub fn issue_generations_after(&self, last: u64) {
        self.state.generations.reserve_through(last);
    }

    /// Queue `record` for the next pass, while a tap is registered.
    pub(crate) fn admit(&self, record: InputRecord) {
        if self.is_recording() {
            let number = self.state.next_row.get();
            // Past 2^64 rows the number stays put: a registration installed
            // then is offered a row admitted just before it, and loses none.
            self.state.next_row.set(number.saturating_add(1));
            self.state
                .queue
                .borrow_mut()
                .push_back(Admitted { number, record });
        }
    }

    /// Deliver the records present now. Reentrant calls return at once;
    /// records admitted meanwhile go to a pass an owner wake schedules.
    pub(crate) fn deliver(&self) {
        self.state.deliver();
    }
}

impl TapState {
    fn deliver(&self) {
        if self.delivering.replace(true) {
            return;
        }
        let batch = std::mem::take(&mut *self.queue.borrow_mut());
        *self.in_flight.borrow_mut() = batch;
        while let Some(admitted) = self.next_in_flight() {
            let mut after = None;
            while let Some(registration) = self.registration_after(after) {
                after = Some(registration.generation);
                if registration.wants(admitted.number) {
                    self.offer(&registration, &admitted);
                }
                self.retire(registration);
            }
        }
        self.offering.set(None);
        self.delivering.set(false);
        if !self.queue.borrow().is_empty() {
            let wake = Arc::clone(&self.wake);
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a tap's wake runs contained and is reported"
            )]
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| wake())) {
                self.report(payload, TapFailureSite::Wake);
            }
        }
    }

    fn next_in_flight(&self) -> Option<Admitted> {
        self.in_flight.borrow_mut().pop_front()
    }

    /// The first registration installed after `generation` (the first of
    /// all for `None`). A pass walks them this way, so a tap call may install
    /// or remove registrations.
    fn registration_after(&self, generation: Option<NonZeroU64>) -> Option<Rc<Registration>> {
        self.registered
            .borrow()
            .iter()
            .find(|registration| generation.is_none_or(|after| registration.generation > after))
            .map(Rc::clone)
    }

    fn registration(&self, generation: NonZeroU64) -> Option<Rc<Registration>> {
        self.registered
            .borrow()
            .iter()
            .find(|registration| registration.generation == generation)
            .map(Rc::clone)
    }

    fn is_registered(&self, generation: NonZeroU64) -> bool {
        self.registered
            .borrow()
            .iter()
            .any(|registration| registration.generation == generation)
    }

    /// Count as lost, for `registration` alone, the rows a finishing
    /// registration will never be offered: the rest of a pass it finished
    /// inside (the running row too, when this registration's call for it has
    /// not started) and rows admitted for a later pass. The other
    /// registrations are still offered them.
    fn skip_undelivered(&self, registration: &Registration) {
        let wanted = |admitted: &&Admitted| registration.wants(admitted.number);
        let ahead = self.in_flight.borrow().iter().filter(wanted).count()
            + self.queue.borrow().iter().filter(wanted).count();
        let running = self.offering.get().is_some_and(|offering| {
            registration.wants(offering.row)
                && (registration.generation > offering.generation
                    || (registration.generation == offering.generation && !offering.row_call))
        });
        registration.lose(
            u64::try_from(ahead)
                .unwrap_or(u64::MAX)
                .saturating_add(u64::from(running)),
        );
    }

    /// Offer `admitted` to `registration`, after the gap it still owes; a
    /// panic loses the row for that registration alone and sends the loss as
    /// a `Gap` right away.
    fn offer(&self, registration: &Registration, admitted: &Admitted) {
        let mut offering = Offering {
            row: admitted.number,
            generation: registration.generation,
            row_call: false,
        };
        self.offering.set(Some(offering));
        self.deliver_pending_gap(registration);
        offering.row_call = true;
        self.offering.set(Some(offering));
        if self.call(registration, &admitted.record, TapFailureSite::Row) == Some(false) {
            registration.lose(1);
            self.deliver_pending_gap(registration);
        }
    }

    /// Deliver `registration`'s pending lost-row count as a `Gap`; it stays
    /// pending when that call panics too. Returns whether nothing is pending
    /// afterwards.
    fn deliver_pending_gap(&self, registration: &Registration) -> bool {
        let rows = registration.pending_gap.get();
        if rows == 0 {
            return true;
        }
        let delivered = self.call(
            registration,
            &InputRecord::Gap { rows },
            TapFailureSite::Gap,
        ) == Some(true);
        if delivered {
            registration
                .pending_gap
                .set(registration.pending_gap.get().saturating_sub(rows));
        }
        delivered
    }

    /// One contained call of `registration`'s tap: `None` once it is no
    /// longer registered, else whether the call returned. The caller holds
    /// the registration and retires it after, because a tap removed inside
    /// the call is destroyed by that, and its destructor's failure must not
    /// come first.
    fn call(
        &self,
        registration: &Registration,
        record: &InputRecord,
        site: TapFailureSite,
    ) -> Option<bool> {
        if !self.is_registered(registration.generation) {
            return None;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a tap's record call runs contained so delivery continues"
        )]
        let delivered = catch_unwind(AssertUnwindSafe(|| registration.tap.record(record)));
        let returned = delivered.is_ok();
        if let Err(payload) = delivered {
            self.report(payload, site);
        }
        Some(returned)
    }

    /// Let go of a registration under containment: when it was the last
    /// hold, the tap's captures are destroyed, and a panic there is reported
    /// but never replaces the first failure or escapes.
    fn retire(&self, registration: Rc<Registration>) {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a retired tap's drop runs contained and is reported"
        )]
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(registration))) {
            self.report(payload, TapFailureSite::Retire);
        }
    }

    /// Keep the first failure's record, committed before diagnostics run,
    /// then report it; the payload is discarded under containment.
    fn report(&self, payload: Box<dyn Any + Send>, site: TapFailureSite) {
        let message = payload_text(payload.as_ref()).map(str::to_owned);
        let first = {
            let mut first_failure = self.first_failure.borrow_mut();
            let first = first_failure.is_none();
            if first {
                *first_failure = Some(TapFailure {
                    site,
                    message: message.clone(),
                });
            }
            first
        };
        flui_foundation::panic::discard_payload(payload);
        let failure = message
            .as_deref()
            .unwrap_or("a panic without a text payload");
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a tracing subscriber's panic must not replace the first tap failure"
        )]
        let traced = catch_unwind(AssertUnwindSafe(|| {
            if first {
                tracing::error!(target: INPUT_TAP_TARGET, ?site, failure, "contained the first input tap failure");
            } else {
                tracing::warn!(target: INPUT_TAP_TARGET, ?site, failure, "contained a later input tap failure");
            }
        }));
        if let Err(payload) = traced {
            flui_foundation::panic::discard_payload(payload);
        }
    }

    /// Unregister `generation`; the last registration's going clears the
    /// rows queued for it.
    fn remove(&self, generation: NonZeroU64) {
        let outgoing = {
            let mut registered = self.registered.borrow_mut();
            registered
                .iter()
                .position(|registration| registration.generation == generation)
                .map(|index| registered.remove(index))
        };
        if let Some(outgoing) = outgoing {
            self.retire(outgoing);
        }
        if self.registered.borrow().is_empty() {
            self.queue.borrow_mut().clear();
        }
    }
}

impl Drop for TapState {
    /// The taps still registered when the runtime goes away (ones installed
    /// for the runtime's life) are finished as [`InputTapHandle::finish`]
    /// would: the queued rows go out, then each one's pending gap and `End`
    /// row, which counts the rows that last pass admitted as lost.
    fn drop(&mut self) {
        if self.registered.get_mut().is_empty() {
            return;
        }
        self.deliver();
        let mut after = None;
        while let Some(registration) = self.registration_after(after) {
            after = Some(registration.generation);
            self.skip_undelivered(&registration);
            if self.deliver_pending_gap(&registration) {
                let lost_rows = registration.lost_total.get();
                let _ = self.call(
                    &registration,
                    &InputRecord::End { lost_rows },
                    TapFailureSite::End,
                );
            }
            self.retire(registration);
        }
        for outgoing in std::mem::take(self.registered.get_mut()) {
            self.retire(outgoing);
        }
    }
}

/// One tap registration. Dropping it unregisters this registration alone:
/// the other taps keep recording.
#[must_use = "dropping the handle unregisters the tap"]
pub struct InputTapHandle {
    state: Weak<TapState>,
    generation: NonZeroU64,
}

impl std::fmt::Debug for InputTapHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputTapHandle")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl InputTapHandle {
    /// Whether this registration is registered: while its runtime's
    /// registry lives, until the handle goes.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|state| state.is_registered(self.generation))
    }

    /// Deliver every queued row, then this registration's pending
    /// [`InputRecord::Gap`] and its [`InputRecord::End`] row, and unregister.
    ///
    /// Called inside a tap call, the running pass keeps its rows: the ones it
    /// has not offered this registration yet, and rows admitted for a later
    /// pass, are lost to it and counted in its final `Gap` and `End` rows.
    ///
    /// # Errors
    ///
    /// [`RecordingIncomplete`] when the tap panicked on the final gap or the
    /// end row (neither is retried), or the runtime's registry is gone.
    pub fn finish(self) -> Result<(), RecordingIncomplete> {
        let Some(state) = self.state.upgrade() else {
            return Err(RecordingIncomplete { lost_rows: 0 });
        };
        // Inside a tap call this delivers nothing: the running pass owns the
        // rows, and the ones it has not offered this registration are lost.
        state.deliver();
        // Only its handle unregisters a registration, so this one is here.
        let Some(registration) = state.registration(self.generation) else {
            return Err(RecordingIncomplete { lost_rows: 0 });
        };
        state.skip_undelivered(&registration);
        let complete = state.deliver_pending_gap(&registration)
            && state.call(
                &registration,
                &InputRecord::End {
                    lost_rows: registration.lost_total.get(),
                },
                TapFailureSite::End,
            ) == Some(true);
        let lost_rows = registration.lost_total.get();
        // `self` drops here and unregisters this generation; the tap is
        // destroyed when the registration is retired after it.
        drop(self);
        state.retire(registration);
        if complete {
            Ok(())
        } else {
            Err(RecordingIncomplete { lost_rows })
        }
    }

    /// Keep the tap registered for the rest of the runtime's life.
    pub fn detach(mut self) {
        self.state = Weak::new();
    }
}

impl Drop for InputTapHandle {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            state.remove(self.generation);
        }
    }
}

/// The row of pointer input a presentation holds until its tree is committed
/// (ADR-0048), kept back until the runtime dispatches or discards that input,
/// so the row names the sequence its dispatch placed it in (ADR-0215 §1).
///
/// [`dispatched`](Self::dispatched) admits it as the event dispatch delivered.
/// Dropped undispatched — the input refused, folded into a later move,
/// superseded, evicted, or dropped with the queue — it admits the event as it
/// arrived, with [`InputOutcome::Queued`] and no sequence: accepted for a
/// delivery that never happened, which a replay does not apply. Either way the
/// row keeps the time the input arrived; admitting it runs no tap.
pub(crate) struct HeldInputRow {
    taps: InputTaps,
    row: Option<InputRow>,
}

impl std::fmt::Debug for HeldInputRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeldInputRow")
            .field("pending", &self.row.is_some())
            .finish_non_exhaustive()
    }
}

impl HeldInputRow {
    /// Hold `row`, the input as it arrived, for `taps`; until it is
    /// dispatched it is undelivered, in no sequence.
    pub(crate) fn new(taps: InputTaps, mut row: InputRow) -> Self {
        row.outcome = InputOutcome::Queued;
        row.sequence = None;
        Self {
            taps,
            row: Some(row),
        }
    }

    /// Admit the row for `event`, the input as dispatch delivered it (a held
    /// move carries the moves folded into it), in `sequence`.
    pub(crate) fn dispatched(mut self, event: &PointerEvent, sequence: Option<InputSequenceId>) {
        if let Some(mut row) = self.row.take() {
            row.input = HostInput::Platform(PlatformInput::Pointer(event.clone()));
            row.outcome = InputOutcome::Handled;
            row.sequence = sequence.map(RecordedSequence::from);
            self.taps.admit(InputRecord::Input(row));
        }
    }
}

impl Drop for HeldInputRow {
    fn drop(&mut self) {
        if let Some(row) = self.row.take() {
            self.taps.admit(InputRecord::Input(row));
        }
    }
}

/// What an [`InputTapFactory`] calls once per runtime.
type MakeTap = dyn Fn(&RecordingStart) -> Box<dyn InputTap> + Send + Sync;

/// Makes one tap per runtime: the application's recording hook, which
/// `flui-app` installs on every runtime it builds (ADR-0215 §1). A tap lives
/// on its runtime's owner thread, so the factory, not the tap, crosses
/// threads.
#[derive(Clone)]
pub struct InputTapFactory(Arc<MakeTap>);

impl InputTapFactory {
    /// A factory calling `make` once per runtime, with what that runtime's
    /// recording starts from, for the recording's header.
    pub fn new(
        make: impl Fn(&RecordingStart) -> Box<dyn InputTap> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(make))
    }

    /// Register a fresh tap on `runtime` for the rest of its life, made from
    /// [`UiRuntime::recording_start`](crate::ui_runtime::UiRuntime::recording_start).
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] when the runtime's taps issued every registration
    /// generation.
    pub fn install_on(&self, runtime: &crate::ui_runtime::UiRuntime) -> Result<(), IdExhausted> {
        let tap = (self.0)(&runtime.recording_start());
        runtime.input_taps().install_boxed(tap)?.detach();
        Ok(())
    }
}

impl std::fmt::Debug for InputTapFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InputTapFactory").finish()
    }
}
