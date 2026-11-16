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
//! - a [`Track`] is a value the platform changes over time: a permission the
//!   user revokes, a service that goes away and comes back
//!   ([`ServiceState`]);
//! - [`Events`] is a stream of events at virtual instants, ended once with a
//!   [`StreamEnd`].
//!
//! Everything here is owner-thread state: no value needs to be `Send`, and
//! none leaves the thread except a [`FailureReport`], through which a double
//! whose contract is `Send + Sync` reports into the ledger.
//!
//! # Time
//!
//! The timeline is a [`ManualClock`]. Exactly one driver moves it
//! ([`Scenario::drive`]): a change scheduled at an instant
//! ([`Track::set_at`], [`Events::emit_at`], [`Answer::after`]) applies when
//! the driver [advances](ScenarioDriver::advance) to or past it, at the
//! owner-turn boundary, never in the middle of a turn.
//! [`Scenario::next_due`] is the driver's next deadline.
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
//! stays held, every other one is still answered, and the first panic is
//! resumed once the rest has run (ADR-0281).
//!
//! # Failures
//!
//! [`Scenario::verify`] returns every failure the run collected, typed. What
//! a script owes outlives its handles: a script dropped with a held request
//! or an unused step hands them to the ledger. A scenario that ends with a
//! failure no `verify` returned panics as it drops, unless the thread is
//! already unwinding, so a test that forgets to verify cannot pass with
//! work left undone.

mod events;
mod script;
mod track;

pub use events::{EventSubscription, Events, StreamEnd, StreamItem};
pub use script::{Answer, Expect, Script};
pub use track::{ServiceState, Track};

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_foundation::{IdMint, ManualClock};

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
    timeline: BTreeMap<(Duration, NonZeroU64), Timed>,
    failures: Vec<ScenarioFailure>,
    audited: Vec<Weak<dyn Audit>>,
    /// What the last `verify` returned.
    acknowledged: Vec<ScenarioFailure>,
}

/// A change scheduled on the timeline.
struct Timed {
    what: Cow<'static, str>,
    apply: Box<dyn FnOnce()>,
}

/// What a script still owes: what [`Scenario::verify`] asks of a live one
/// and what a dropped one hands to the ledger.
trait Audit {
    fn audit(&self, failures: &mut Vec<ScenarioFailure>);
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
        Self::with_clock(ManualClock::new())
    }

    /// A scenario on `clock`: the instants it schedules are `clock`'s
    /// elapsed time.
    #[must_use]
    pub fn with_clock(clock: ManualClock) -> Self {
        Self {
            core: Rc::new(Core {
                clock,
                state: RefCell::new(CoreState::default()),
                reported: FailureReport::default(),
                driven: Cell::new(false),
                ids: IdMint::new(),
            }),
        }
    }

    /// The clock the scenario's instants are on. Read it; move it only
    /// through the [driver](Self::drive), which applies what falls due.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.core.clock
    }

    /// The virtual time now: the clock's elapsed time.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.core.clock.elapsed()
    }

    /// The one driver of the scenario's clock: a headless host takes it, so
    /// time moves only through that host's frames.
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

    /// Apply every change scheduled at or before [`now`](Self::now), in the
    /// order of their instants and, at one instant, the order they were
    /// scheduled. Returns how many it applied.
    ///
    /// A change scheduled while this runs, at an instant already reached,
    /// applies in the same call. Each change runs with no borrow held, so one
    /// that panics leaves the others applied; the first panic is resumed
    /// once all of them have run.
    pub fn run_due(&self) -> usize {
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

    /// Every failure of the run so far, or `Ok` when there is none. What it
    /// returns is acknowledged: the scenario does not panic over it when it
    /// drops.
    ///
    /// # Errors
    ///
    /// [`ScenarioFailures`] listing, in this order: the failures recorded as
    /// they happened (an unexpected request, an event after its stream's
    /// end, a script dropped with debts, a report from another thread), what
    /// each live script still owes (a held request nobody answered, a step no
    /// request used), and every scheduled change not yet applied.
    pub fn verify(&self) -> Result<(), ScenarioFailures> {
        let audited: Vec<Rc<dyn Audit>> = {
            let mut state = self.core.state.borrow_mut();
            state.audited.retain(|script| script.strong_count() > 0);
            state.audited.iter().filter_map(Weak::upgrade).collect()
        };
        let mut failures = self.recorded();
        for script in &audited {
            script.audit(&mut failures);
        }
        drop(audited);
        failures.extend(self.unreached());
        self.core
            .state
            .borrow_mut()
            .acknowledged
            .clone_from(&failures);
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ScenarioFailures(failures))
        }
    }

    /// The failures recorded so far, from this thread and others.
    fn recorded(&self) -> Vec<ScenarioFailure> {
        let mut failures = self.core.state.borrow().failures.clone();
        failures.extend(self.core.reported.take_snapshot());
        failures
    }

    fn unreached(&self) -> Vec<ScenarioFailure> {
        unreached(&self.core.state.borrow())
    }

    /// Schedule `apply` at `at`; `what` names it if the clock never gets
    /// there.
    fn schedule(&self, at: Duration, what: Cow<'static, str>, apply: Box<dyn FnOnce()>) {
        let seq = self.mint();
        self.core
            .state
            .borrow_mut()
            .timeline
            .insert((at, seq), Timed { what, apply });
    }

    /// A fresh id for the timeline or a script.
    fn mint(&self) -> NonZeroU64 {
        self.core
            .ids
            .next()
            .expect("BUG: one scenario mints fewer than u64::MAX ids")
    }

    fn fail(&self, failure: ScenarioFailure) {
        self.core.state.borrow_mut().failures.push(failure);
    }

    fn audit(&self, script: Weak<dyn Audit>) {
        self.core.state.borrow_mut().audited.push(script);
    }
}

fn unreached(state: &CoreState) -> Vec<ScenarioFailure> {
    state
        .timeline
        .iter()
        .map(|(&(at, _), timed)| ScenarioFailure::NotReached {
            what: timed.what.clone(),
            at,
        })
        .collect()
}

impl Drop for Core {
    fn drop(&mut self) {
        // A destructor that panics while the thread unwinds aborts; the
        // unwinding failure is the one to report (ADR-0281).
        if std::thread::panicking() {
            return;
        }
        let state = self.state.get_mut();
        let mut failures = std::mem::take(&mut state.failures);
        failures.extend(self.reported.take_snapshot());
        // Every script is gone by now: each held this scenario alive, and
        // handed its debts to `failures` as it dropped.
        failures.extend(unreached(state));
        let unacknowledged: Vec<ScenarioFailure> = failures
            .into_iter()
            .filter(|failure| !state.acknowledged.contains(failure))
            .collect();
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
            (state.timeline.len(), state.failures.len())
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

    /// Move the clock forward by `dt`, then apply every change due at the
    /// new instant ([`Scenario::run_due`]). Returns how many it applied.
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
#[allow(
    clippy::exhaustive_structs,
    reason = "the refusal is the whole fact: there is nothing more to say about it"
)]
pub struct AlreadyDriven;

/// Where a double whose contract is `Send + Sync` reports a failure into a
/// [`Scenario`]'s ledger, from any thread. Cheap to clone.
#[derive(Clone, Default)]
pub struct FailureReport {
    failures: Arc<Mutex<Vec<ScenarioFailure>>>,
}

impl FailureReport {
    /// Report `failure`; the scenario's [`verify`](Scenario::verify) lists it.
    pub fn report(&self, failure: ScenarioFailure) {
        // A `Vec` push runs no user code; a poisoned lock held a whole list.
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(failure);
    }

    fn take_snapshot(&self) -> Vec<ScenarioFailure> {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl fmt::Debug for FailureReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reported = self
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        f.debug_struct("FailureReport")
            .field("reported", &reported)
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
