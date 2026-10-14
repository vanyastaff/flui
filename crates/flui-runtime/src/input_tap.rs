//! Delivery of a runtime's [`InputRecord`] stream to its tap (ADR-0215 §1).
//!
//! The runtime admits each record after the dispatch it describes has
//! committed, then a delivery pass hands the owned rows to the tap one by one
//! with no runtime borrow held. A pass delivers only the rows present when it
//! started: rows a tap callback admits reentrantly wait for the next pass,
//! which an owner wake schedules, so a tap that dispatches on every row cannot
//! keep the owner loop from returning.
//!
//! Every tap call is contained. The first failure is authoritative and
//! reported; the tap stays registered, the row it panicked on is not offered
//! again, and the loss travels as a pending count delivered as an
//! [`InputRecord::Gap`] row in a second contained call, retried before the
//! next row while it keeps failing. Finishing delivers the pending gap and an
//! [`InputRecord::End`] row; a recording that cannot get both out reports
//! [`RecordingIncomplete`] instead of claiming to be whole.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::panic::{payload_text, retain_opaque_payload};
use flui_foundation::{IdExhausted, IdMint};

use crate::input_record::InputRecord;

/// The tracing target every contained tap failure is reported on.
pub const INPUT_TAP_TARGET: &str = "flui_runtime::input_tap";

/// Observes the records a runtime emits, on its owner thread, in the order
/// the runtime applied them.
///
/// A tap may dispatch input, replace or remove itself, or panic: dispatched
/// input is recorded in a later pass, a replacement receives the next row, and
/// a panic is contained (see the `input_tap` module docs).
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
/// [`InputRecord::Gap`] or on the [`InputRecord::End`] row, or its
/// registration was replaced before it finished.
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
    /// Destroying a replaced or removed tap.
    Retire,
    /// The owner wake that schedules the next delivery pass.
    Wake,
}

/// The first failure a tap registry contained: where it happened and the
/// panic's message. The payload itself is retained, never dropped, because
/// its destructor is user code.
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
/// Clones share one registry. [`install`](Self::install) registers a tap;
/// the runtime admits and delivers records only while one is registered.
#[derive(Clone)]
pub struct InputTaps {
    state: Rc<TapState>,
}

impl std::fmt::Debug for InputTaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputTaps")
            .field("recording", &self.is_recording())
            .field("queued", &self.state.queue.borrow().len())
            .finish_non_exhaustive()
    }
}

struct TapState {
    queue: RefCell<VecDeque<InputRecord>>,
    current: RefCell<Option<Registered>>,
    /// Issues registration generations; never reuses one.
    generations: IdMint<NonZeroU64>,
    /// Lost rows not yet delivered as a `Gap`.
    pending_gap: Cell<u64>,
    lost_total: Cell<u64>,
    delivering: Cell<bool>,
    first_failure: RefCell<Option<TapFailure>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

struct Registered {
    generation: NonZeroU64,
    tap: Rc<dyn InputTap>,
}

impl InputTaps {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            state: Rc::new(TapState {
                queue: RefCell::new(VecDeque::new()),
                current: RefCell::new(None),
                generations: IdMint::new(),
                pending_gap: Cell::new(0),
                lost_total: Cell::new(0),
                delivering: Cell::new(false),
                first_failure: RefCell::new(None),
                wake,
            }),
        }
    }

    /// Register `tap` in place of the current one, which is retired.
    ///
    /// Inside a tap callback the replacement takes effect from the next row.
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
        let outgoing = self.state.current.replace(Some(Registered {
            generation,
            tap: Rc::from(tap),
        }));
        self.state.retire(outgoing.map(|registered| registered.tap));
        Ok(InputTapHandle {
            state: Rc::downgrade(&self.state),
            generation,
        })
    }

    /// Whether a tap is registered, so the runtime records.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.state.current.borrow().is_some()
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
            self.state.queue.borrow_mut().push_back(record);
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
        for record in batch {
            self.deliver_pending_gap();
            self.offer(&record);
        }
        self.delivering.set(false);
        if !self.queue.borrow().is_empty() {
            let wake = Arc::clone(&self.wake);
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| wake())) {
                self.report(payload, TapFailureSite::Wake);
            }
        }
    }

    /// Hand `record` to the current tap; a panic loses the row and sends the
    /// loss as a `Gap` right away.
    fn offer(&self, record: &InputRecord) {
        if self.call(record, TapFailureSite::Row) == Some(false) {
            self.lost_total.set(self.lost_total.get().saturating_add(1));
            self.pending_gap
                .set(self.pending_gap.get().saturating_add(1));
            self.deliver_pending_gap();
        }
    }

    /// Deliver the pending lost-row count as a `Gap`; it stays pending when
    /// that call panics too. Returns whether nothing is pending afterwards.
    fn deliver_pending_gap(&self) -> bool {
        let rows = self.pending_gap.get();
        if rows == 0 {
            return true;
        }
        let delivered = self.call(&InputRecord::Gap { rows }, TapFailureSite::Gap) == Some(true);
        if delivered {
            self.pending_gap.set(self.pending_gap.get() - rows);
        }
        delivered
    }

    /// One contained call of the current tap: `None` with no tap, else
    /// whether it returned. The failure is reported before the clone is
    /// retired, because a tap replaced or removed inside the call may be
    /// retired by it, and its destructor's failure must not come first.
    fn call(&self, record: &InputRecord, site: TapFailureSite) -> Option<bool> {
        let tap = self.current_tap()?;
        let delivered = catch_unwind(AssertUnwindSafe(|| tap.record(record)));
        let returned = delivered.is_ok();
        if let Err(payload) = delivered {
            self.report(payload, site);
        }
        self.retire(Some(tap));
        Some(returned)
    }

    fn current_tap(&self) -> Option<Rc<dyn InputTap>> {
        self.current
            .borrow()
            .as_ref()
            .map(|registered| Rc::clone(&registered.tap))
    }

    /// Drop an outgoing tap's captures under containment: a panic there is
    /// reported but never replaces the first failure or escapes.
    fn retire(&self, tap: Option<Rc<dyn InputTap>>) {
        let Some(tap) = tap else {
            return;
        };
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(tap))) {
            self.report(payload, TapFailureSite::Retire);
        }
    }

    /// Keep the first failure's record, committed before diagnostics run,
    /// then report it; the payload is retained, never dropped.
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
        retain_opaque_payload(payload);
        let failure = message
            .as_deref()
            .unwrap_or("a panic without a text payload");
        let traced = catch_unwind(AssertUnwindSafe(|| {
            if first {
                tracing::error!(target: INPUT_TAP_TARGET, ?site, failure, "contained the first input tap failure");
            } else {
                tracing::warn!(target: INPUT_TAP_TARGET, ?site, failure, "contained a later input tap failure");
            }
        }));
        if let Err(payload) = traced {
            retain_opaque_payload(payload);
        }
    }

    fn remove(&self, generation: NonZeroU64) -> bool {
        let outgoing = {
            let mut current = self.current.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|registered| registered.generation == generation)
            {
                current.take()
            } else {
                None
            }
        };
        let removed = outgoing.is_some();
        self.retire(outgoing.map(|registered| registered.tap));
        if self.current.borrow().is_none() {
            self.queue.borrow_mut().clear();
        }
        removed
    }

    fn is_current(&self, generation: NonZeroU64) -> bool {
        self.current
            .borrow()
            .as_ref()
            .is_some_and(|registered| registered.generation == generation)
    }
}

impl Drop for TapState {
    /// A tap still registered when its runtime goes away (one installed for
    /// the runtime's life) is finished as [`InputTapHandle::finish`] would:
    /// its queued rows, the pending gap and the `End` row go out.
    fn drop(&mut self) {
        if self.current.get_mut().is_none() {
            return;
        }
        self.deliver();
        if self.deliver_pending_gap() {
            let lost_rows = self.lost_total.get();
            let _ = self.call(&InputRecord::End { lost_rows }, TapFailureSite::End);
        }
        let outgoing = self.current.get_mut().take();
        self.retire(outgoing.map(|registered| registered.tap));
    }
}

/// One tap registration. Dropping it unregisters that registration only: a
/// handle whose tap was replaced leaves the replacement registered.
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
    /// Whether this registration is still the registered one.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|state| state.is_current(self.generation))
    }

    /// Deliver every queued row, the pending [`InputRecord::Gap`] and the
    /// [`InputRecord::End`] row, then unregister.
    ///
    /// # Errors
    ///
    /// [`RecordingIncomplete`] when the tap panicked on the final gap or the
    /// end row (neither is retried), or this registration is no longer the
    /// registered one.
    pub fn finish(self) -> Result<(), RecordingIncomplete> {
        let Some(state) = self.state.upgrade() else {
            return Err(RecordingIncomplete { lost_rows: 0 });
        };
        if !state.is_current(self.generation) {
            return Err(RecordingIncomplete {
                lost_rows: state.lost_total.get(),
            });
        }
        state.deliver();
        let complete = state.is_current(self.generation)
            && state.deliver_pending_gap()
            && state.is_current(self.generation)
            && state.call(
                &InputRecord::End {
                    lost_rows: state.lost_total.get(),
                },
                TapFailureSite::End,
            ) == Some(true);
        let lost_rows = state.lost_total.get();
        // `self` drops here and unregisters this generation.
        drop(self);
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

/// Makes one tap per runtime: the application's recording hook, which
/// `flui-app` installs on every runtime it builds (ADR-0215 §1). A tap lives
/// on its runtime's owner thread, so the factory, not the tap, crosses
/// threads.
#[derive(Clone)]
pub struct InputTapFactory(Arc<dyn Fn() -> Box<dyn InputTap> + Send + Sync>);

impl InputTapFactory {
    /// A factory calling `make` once per runtime.
    pub fn new(make: impl Fn() -> Box<dyn InputTap> + Send + Sync + 'static) -> Self {
        Self(Arc::new(make))
    }

    /// Register a fresh tap on `taps` for the rest of its runtime's life.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] when `taps` issued every registration generation.
    pub fn install_on(&self, taps: &InputTaps) -> Result<(), IdExhausted> {
        taps.install_boxed((self.0)())?.detach();
        Ok(())
    }
}

impl std::fmt::Debug for InputTapFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InputTapFactory").finish()
    }
}
