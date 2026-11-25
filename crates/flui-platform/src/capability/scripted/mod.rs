//! The scenario primitive every capability's test double is built from
//! (ADR-0154 §10), under the `scripted` feature.
//!
//! A [`Scenario`] is one virtual timeline and one ledger of failures. On it a
//! double builds what its platform service does:
//!
//! - a [`Script`] answers the requests of one type `R` with answers of one
//!   type `A`: each [`Expect`] step matches a request by its value, the
//!   earliest step that accepts a request answers it, a standing answer covers
//!   what no step takes, and a request nothing answers is
//!   [`Unexpected`](ScenarioFailure::Unexpected), never a silent default;
//! - a [`Track`] is a value the platform changes over time: a service that
//!   goes away and comes back ([`ServiceState`]);
//! - [`Events`] is a stream of events at virtual instants, ended once with a
//!   [`StreamEnd`].
//!
//! Everything here is owner-thread state: no value needs to be `Send`, and
//! none leaves the thread except a [`FailureReport`], through which a double
//! whose contract is `Send + Sync` reports into the ledger.
//!
//! # Time
//!
//! The timeline is a `ManualClock` that only the scenario's one driver
//! holds ([`Scenario::drive`]): the scenario itself shows the time
//! ([`Scenario::now`]) and nothing else can move it. A change scheduled at an
//! instant ([`Track::set_at`], [`Events::emit_at`], [`Answer::after`]) applies
//! when the driver [advances](ScenarioDriver::advance) to or past it, in the
//! same call that moves the clock. [`Scenario::next_due`] is the driver's next
//! deadline.
//!
//! # The owner-turn boundary
//!
//! Every answer reaches its caller through a
//! [`Completion`](super::Completion): answering resolves a
//! [`Resolver`](super::Resolver), and the completion settles at the owner's
//! next turn, never inside the call that asked. A caller that re-enters (a
//! widget that asks again from the closure its answer ran) therefore finds
//! the scenario idle: no borrow of a script, a track or a stream is held
//! while code the scenario does not own runs (a matcher, a computed answer,
//! a value's `Clone`, `PartialEq`, `Debug` or `Drop`). When that code
//! panics, the state is already committed: the request it was answering
//! stays held, every other one is still answered and every debt recorded,
//! and the first panic is resumed once the rest has run (ADR-0281).
//!
//! # Failures
//!
//! [`Scenario::verify`] returns every failure the run collected, typed. What
//! a script owes outlives its handles: a script dropped with a held request
//! or an unused step hands them to the ledger. What `verify` returned is
//! acknowledged by position and identity, not by value: a failure recorded
//! after it, even one equal to an acknowledged one, is new. A scenario that
//! ends with a failure no `verify` returned panics as it drops, unless the
//! thread is already unwinding, so a test that forgets to verify cannot pass
//! with work left undone.

mod events;
mod script;
mod track;

pub use events::{EventSubscription, Events, StreamEnd, StreamItem};
pub use script::{Answer, Expect, MAX_RECORDED_REQUESTS, RequestTicket, Script, ScriptHold};
pub use track::{ServiceState, Track};

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_foundation::{IdMint, ManualClock};

/// How many reports from other threads a scenario keeps; one past it is
/// counted in [`ScenarioFailure::ReportsDropped`] instead.
pub const MAX_REPORTS: usize = 256;

/// One virtual timeline and the failures of one run, shared by the scripts,
/// tracks and streams made on it.
///
/// Owner-thread only, cheap to clone; every clone is the same scenario. The
/// scripts, tracks and streams keep it alive and it keeps only weak
/// references back, so the last of them to drop ends it.
///
/// # Panics
///
/// When the last clone drops with a failure that no [`verify`](Self::verify)
/// returned, and the thread is not already unwinding.
#[derive(Clone)]
pub struct Scenario {
    core: Rc<Core>,
}

struct Core {
    clock: ManualClock,
    state: RefCell<CoreState>,
    /// Failures reported from other threads.
    reported: FailureReport,
    /// Whether a driver holds the clock.
    driven: Cell<bool>,
    /// Issues the timeline's sequence and every script's step and request
    /// ids: one mint, so no two ever collide.
    ids: IdMint<NonZeroU64>,
}

#[derive(Default)]
struct CoreState {
    /// Scheduled changes by instant, then by the order they were scheduled.
    timeline: BTreeMap<TimelineKey, Timed>,
    /// Failures recorded on the owner thread as they happened.
    events: Vec<ScenarioFailure>,
    /// What dropped scripts still owed, each under its step's or request's
    /// id.
    debts: Vec<(NonZeroU64, ScenarioFailure)>,
    audited: Vec<Weak<dyn Audit>>,
    /// What the last `verify` returned.
    acknowledged: Acknowledged,
}

/// What [`Scenario::verify`] returned: how far each append-only list had
/// been read, and the ids of the debts and scheduled changes it listed.
#[derive(Default)]
struct Acknowledged {
    events: usize,
    reports: usize,
    dropped_reports: u64,
    debts: BTreeSet<NonZeroU64>,
    scheduled: BTreeSet<NonZeroU64>,
}

/// Where a change sits on the timeline: its instant, then the order it was
/// scheduled in.
type TimelineKey = (Duration, NonZeroU64);

/// A change scheduled on the timeline.
struct Timed {
    what: Cow<'static, str>,
    apply: Box<dyn FnOnce()>,
}

/// A failure a script still owes, under the id of its step or request.
type Debt = (NonZeroU64, ScenarioFailure);

/// What a script still owes: what [`Scenario::verify`] asks of a live one
/// and what a dropped one hands to the ledger. A panic of the request
/// type's `Debug` is kept in `first`, after every debt is written.
trait Audit {
    fn audit(&self, debts: &mut Vec<Debt>, first: &mut FirstPanic);
}

impl Default for Scenario {
    fn default() -> Self {
        Self::new()
    }
}

impl Scenario {
    /// A scenario on a clock of its own, at zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            core: Rc::new(Core {
                clock: ManualClock::new(),
                state: RefCell::new(CoreState::default()),
                reported: FailureReport::default(),
                driven: Cell::new(false),
                ids: IdMint::new(),
            }),
        }
    }

    /// The virtual time now: how far the driver has moved the clock.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.core.clock.elapsed()
    }

    /// The one driver of the scenario's clock: a headless host takes it, so
    /// time moves only through that host.
    ///
    /// # Errors
    ///
    /// [`AlreadyDriven`] while another driver holds it: two hosts on one
    /// scenario would each move the other's time.
    pub fn drive(&self) -> Result<ScenarioDriver, AlreadyDriven> {
        if self.core.driven.replace(true) {
            return Err(AlreadyDriven);
        }
        Ok(ScenarioDriver {
            scenario: self.clone(),
        })
    }

    /// A script that answers requests of type `R` with `A`, named `name` in
    /// the failures it reports.
    #[must_use]
    pub fn script<R, A>(&self, name: &'static str) -> Script<R, A>
    where
        R: fmt::Debug + 'static,
        A: 'static,
    {
        Script::on(self, name)
    }

    /// A value the platform changes over time, starting at `initial`.
    #[must_use]
    pub fn track<T>(&self, name: &'static str, initial: T) -> Track<T>
    where
        T: Clone + PartialEq + fmt::Debug + 'static,
    {
        Track::on(self, name, initial)
    }

    /// A service that starts [on](ServiceState::On).
    #[must_use]
    pub fn service(&self, name: &'static str) -> Track<ServiceState> {
        self.track(name, ServiceState::On)
    }

    /// A stream of events of type `E`.
    #[must_use]
    pub fn events<E>(&self, name: &'static str) -> Events<E>
    where
        E: Clone + fmt::Debug + 'static,
    {
        Events::on(self, name)
    }

    /// Where a double whose contract is `Send + Sync` reports a failure from
    /// any thread; [`verify`](Self::verify) lists it with the rest.
    #[must_use]
    pub fn failure_report(&self) -> FailureReport {
        self.core.reported.clone()
    }

    /// The earliest instant a change is scheduled at, if any: a deadline of
    /// whoever drives the clock.
    #[must_use]
    pub fn next_due(&self) -> Option<Duration> {
        let state = self.core.state.borrow();
        state.timeline.keys().next().map(|&(at, _)| at)
    }

    /// Every failure of the run so far, or `Ok` when there is none. What it
    /// returns is acknowledged: the scenario does not panic over it when it
    /// drops. A failure recorded after this call is not, even when it equals
    /// one this call returned.
    ///
    /// # Errors
    ///
    /// [`ScenarioFailures`] listing, in this order: the failures recorded on
    /// the owner thread, in the order they happened (an unexpected request,
    /// an event after its stream's end); the reports from other threads, in
    /// the order they arrived, and how many were dropped past
    /// [`MAX_REPORTS`]; what each live script owes (a held request nobody
    /// answered, a step no request used); what dropped scripts owed; and
    /// every scheduled change not yet applied, by instant.
    ///
    /// # Panics
    ///
    /// With the first panic of a request type's `Debug`, once everything is
    /// listed and acknowledged.
    pub fn verify(&self) -> Result<(), ScenarioFailures> {
        let audited: Vec<Rc<dyn Audit>> = {
            let mut state = self.core.state.borrow_mut();
            state.audited.retain(|script| script.strong_count() > 0);
            state.audited.iter().filter_map(Weak::upgrade).collect()
        };
        let mut first = FirstPanic::new();
        let mut live = Vec::new();
        for script in &audited {
            script.audit(&mut live, &mut first);
        }
        drop(audited);
        let (reports, dropped_reports) = self.core.reported.snapshot();
        let mut state = self.core.state.borrow_mut();
        let mut failures = state.events.clone();
        failures.extend(reports.iter().cloned());
        if dropped_reports > 0 {
            failures.push(ScenarioFailure::ReportsDropped {
                dropped: dropped_reports,
            });
        }
        let mut debts: BTreeSet<NonZeroU64> = BTreeSet::new();
        for (id, failure) in live.into_iter().chain(state.debts.iter().cloned()) {
            debts.insert(id);
            failures.push(failure);
        }
        let mut scheduled = BTreeSet::new();
        for (&(at, seq), timed) in &state.timeline {
            scheduled.insert(seq);
            failures.push(ScenarioFailure::NotReached {
                what: timed.what.clone(),
                at,
            });
        }
        state.acknowledged = Acknowledged {
            events: state.events.len(),
            reports: reports.len(),
            dropped_reports,
            debts,
            scheduled,
        };
        drop(state);
        first.resume();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ScenarioFailures(failures))
        }
    }

    /// Apply every change scheduled at or before [`now`](Self::now), in the
    /// order of their instants and, at one instant, the order they were
    /// scheduled. Reached only through the driver, as it moves the clock.
    ///
    /// A change scheduled while this runs, at an instant already reached,
    /// applies in the same call. Each change runs with no borrow held, so one
    /// that panics leaves the others applied; the first panic is resumed
    /// once all of them have run.
    fn run_due(&self) -> usize {
        let now = self.now();
        let mut applied = 0;
        let mut first = FirstPanic::new();
        loop {
            let due = {
                let mut state = self.core.state.borrow_mut();
                match state.timeline.first_key_value() {
                    Some((&(at, _), _)) if at <= now => state.timeline.pop_first(),
                    _ => None,
                }
            };
            let Some((_, Timed { what, apply })) = due else {
                break;
            };
            applied += 1;
            first.contain(apply);
            drop(what);
        }
        first.resume();
        applied
    }

    /// Schedule `apply` at `at`; `what` names it if the clock never gets
    /// there.
    fn schedule(
        &self,
        at: Duration,
        what: Cow<'static, str>,
        apply: Box<dyn FnOnce()>,
    ) -> TimelineKey {
        let key = (at, self.mint());
        self.core
            .state
            .borrow_mut()
            .timeline
            .insert(key, Timed { what, apply });
        key
    }

    /// Take the change scheduled under `key` off the timeline, unapplied, if
    /// it is still there: what it would have changed is no longer owed.
    fn unschedule(&self, key: TimelineKey) {
        let removed = self.core.state.borrow_mut().timeline.remove(&key);
        // Its closure's captures are dropped after the borrow.
        drop(removed);
    }

    /// A fresh id for the timeline or a script.
    fn mint(&self) -> NonZeroU64 {
        self.core
            .ids
            .next()
            .expect("BUG: one scenario mints fewer than u64::MAX ids")
    }

    fn fail(&self, failure: ScenarioFailure) {
        self.core.state.borrow_mut().events.push(failure);
    }

    /// Whether the last [`verify`](Self::verify) reported the step or
    /// request `id` as a debt.
    fn acknowledged_debt(&self, id: NonZeroU64) -> bool {
        self.core.state.borrow().acknowledged.debts.contains(&id)
    }

    fn owe(&self, debts: Vec<Debt>) {
        self.core.state.borrow_mut().debts.extend(debts);
    }

    fn audit(&self, script: Weak<dyn Audit>) {
        let mut state = self.core.state.borrow_mut();
        state.audited.retain(|script| script.strong_count() > 0);
        state.audited.push(script);
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let (reports, dropped_reports) = self.reported.end();
        // A destructor that panics while the thread unwinds aborts; the
        // unwinding failure is the one to report (ADR-0281).
        if std::thread::panicking() {
            return;
        }
        let state = self.state.get_mut();
        let acknowledged = &state.acknowledged;
        // Every script is gone by now: each held this scenario alive, and
        // handed its debts to `debts` as it dropped.
        let mut unacknowledged: Vec<ScenarioFailure> = state
            .events
            .get(acknowledged.events..)
            .unwrap_or_default()
            .to_vec();
        unacknowledged.extend(
            reports
                .get(acknowledged.reports..)
                .unwrap_or_default()
                .iter()
                .cloned(),
        );
        if dropped_reports > acknowledged.dropped_reports {
            unacknowledged.push(ScenarioFailure::ReportsDropped {
                dropped: dropped_reports - acknowledged.dropped_reports,
            });
        }
        unacknowledged.extend(
            state
                .debts
                .iter()
                .filter(|(id, _)| !acknowledged.debts.contains(id))
                .map(|(_, failure)| failure.clone()),
        );
        unacknowledged.extend(
            state
                .timeline
                .iter()
                .filter(|((_, seq), _)| !acknowledged.scheduled.contains(seq))
                .map(|(&(at, _), timed)| ScenarioFailure::NotReached {
                    what: timed.what.clone(),
                    at,
                }),
        );
        assert!(
            unacknowledged.is_empty(),
            "a scenario ended unverified: {}",
            ScenarioFailures(unacknowledged)
        );
    }
}

impl fmt::Debug for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counted under the borrow and written after it: the formatter's
        // writer is the caller's code.
        let (scheduled, failures) = {
            let state = self.core.state.borrow();
            (state.timeline.len(), state.events.len() + state.debts.len())
        };
        f.debug_struct("Scenario")
            .field("now", &self.now())
            .field("scheduled", &scheduled)
            .field("failures", &failures)
            .field("driven", &self.core.driven.get())
            .finish()
    }
}

/// The one handle that moves a [`Scenario`]'s clock, so no path advances
/// time without applying what falls due. Dropping it lets another driver
/// take the scenario.
pub struct ScenarioDriver {
    scenario: Scenario,
}

impl ScenarioDriver {
    /// The scenario this drives.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// The clock itself, for the driver's own reader (a UI runtime that
    /// frames on it). Move it only with [`advance`](Self::advance).
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.scenario.core.clock
    }

    /// Move the clock forward by `dt`, then apply every change due at the
    /// new instant. Returns how many it applied.
    ///
    /// # Panics
    ///
    /// With the first panic of a change it applied, once every due change
    /// has run.
    pub fn advance(&self, dt: Duration) -> usize {
        self.scenario.core.clock.advance(dt);
        self.scenario.run_due()
    }
}

impl Drop for ScenarioDriver {
    fn drop(&mut self) {
        self.scenario.core.driven.set(false);
    }
}

impl fmt::Debug for ScenarioDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScenarioDriver")
            .field("now", &self.scenario.now())
            .finish()
    }
}

/// [`Scenario::drive`] refused: another driver holds the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("another driver already moves this scenario's clock")]
pub struct AlreadyDriven;

/// Where a double whose contract is `Send + Sync` reports a failure into a
/// [`Scenario`]'s ledger, from any thread. Cheap to clone.
///
/// It keeps at most [`MAX_REPORTS`]; the ones past it are counted. Once the
/// scenario has ended, a report is refused and nothing accumulates.
#[derive(Clone, Default)]
pub struct FailureReport {
    shared: Arc<Mutex<Reports>>,
}

#[derive(Default)]
struct Reports {
    failures: Vec<ScenarioFailure>,
    dropped: u64,
    ended: bool,
}

impl FailureReport {
    /// Report `failure`; the scenario's [`verify`](Scenario::verify) lists
    /// it. `false` when the scenario has ended and nobody will read it.
    pub fn report(&self, failure: ScenarioFailure) -> bool {
        let mut reports = self.lock();
        if reports.ended {
            return false;
        }
        if reports.failures.len() < MAX_REPORTS {
            reports.failures.push(failure);
        } else {
            reports.dropped = reports.dropped.saturating_add(1);
        }
        true
    }

    fn lock(&self) -> MutexGuard<'_, Reports> {
        // A `Vec` push runs no user code; a poisoned lock held a whole list.
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn snapshot(&self) -> (Vec<ScenarioFailure>, u64) {
        let reports = self.lock();
        (reports.failures.clone(), reports.dropped)
    }

    /// The scenario ends: what was reported, and no report from now on.
    fn end(&self) -> (Vec<ScenarioFailure>, u64) {
        let mut reports = self.lock();
        reports.ended = true;
        (std::mem::take(&mut reports.failures), reports.dropped)
    }
}

impl fmt::Debug for FailureReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (reported, ended) = {
            let reports = self.lock();
            (reports.failures.len(), reports.ended)
        };
        f.debug_struct("FailureReport")
            .field("reported", &reported)
            .field("ended", &ended)
            .finish()
    }
}

/// One way a run went other than its scenario said.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScenarioFailure {
    /// A request that nothing in the scenario answers. A script's caller sees
    /// its completion end
    /// [`ProviderDropped`](super::Abandoned::ProviderDropped).
    Unexpected {
        /// The script's name.
        script: &'static str,
        /// The request, as its `Debug` writes it.
        request: String,
    },
    /// A held request that was neither answered nor abandoned.
    Unanswered {
        /// The script's name.
        script: &'static str,
        /// The request, as its `Debug` writes it.
        request: String,
    },
    /// A step that no request used.
    Unconsumed {
        /// The script's name.
        script: &'static str,
        /// What the step expected.
        step: Cow<'static, str>,
    },
    /// A change scheduled at an instant the run never reached.
    NotReached {
        /// What would have changed.
        what: Cow<'static, str>,
        /// When.
        at: Duration,
    },
    /// An event sent, or a second end, on a stream that had ended.
    AfterEnd {
        /// The stream's name.
        stream: &'static str,
        /// What was sent, as its `Debug` writes it.
        sent: String,
    },
    /// Reports from other threads past [`MAX_REPORTS`], counted instead of
    /// kept.
    ReportsDropped {
        /// How many.
        dropped: u64,
    },
}

impl fmt::Display for ScenarioFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unexpected { script, request } => {
                write!(f, "{script}: nothing in the scenario answers {request}")
            }
            Self::Unanswered { script, request } => {
                write!(f, "{script}: {request} is held and was never answered")
            }
            Self::Unconsumed { script, step } => {
                write!(f, "{script}: no request used the step expecting {step}")
            }
            Self::NotReached { what, at } => {
                write!(f, "{what} at {at:?} was never reached")
            }
            Self::AfterEnd { stream, sent } => {
                write!(f, "{stream}: {sent} was sent after the stream ended")
            }
            Self::ReportsDropped { dropped } => {
                write!(
                    f,
                    "{dropped} report(s) past the first {MAX_REPORTS} were dropped"
                )
            }
        }
    }
}

/// The failures [`Scenario::verify`] found, in the order it lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioFailures(Vec<ScenarioFailure>);

impl ScenarioFailures {
    /// Each failure.
    #[must_use]
    pub fn failures(&self) -> &[ScenarioFailure] {
        &self.0
    }
}

impl fmt::Display for ScenarioFailures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the run left the scenario in {} way(s):", self.0.len())?;
        for failure in &self.0 {
            write!(f, "\n- {failure}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ScenarioFailures {}

/// `value`'s `Debug`, contained: a `Debug` that panics is written as such,
/// and its panic is kept in `first` to resume once the state is committed.
fn describe<T: fmt::Debug + ?Sized>(value: &T, first: &mut FirstPanic) -> String {
    first
        .contain(|| format!("{value:?}"))
        .unwrap_or_else(|| "<a value whose Debug panicked>".to_owned())
}
