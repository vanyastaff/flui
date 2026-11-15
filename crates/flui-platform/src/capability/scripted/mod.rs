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
//! # Time
//!
//! The timeline is a [`ManualClock`], the one a headless host already frames
//! on; nothing here reads a real timer. A change scheduled at an instant
//! ([`Track::set_at`], [`Events::emit_at`], [`Answer::after`]) applies when
//! [`Scenario::run_due`] runs at or after it. The host calls `run_due` at the
//! owner-turn boundary, after it moves the clock and before the turn's jobs
//! run, so a change takes effect at the first turn on or after its instant
//! and never in the middle of one.
//!
//! # The owner-turn boundary
//!
//! Every answer reaches its caller through a [`Completion`](super::Completion):
//! answering resolves a [`Resolver`](super::Resolver), and the completion
//! settles at the owner's next turn, never inside the call that asked. A
//! caller that re-enters (a widget that asks again from the closure its
//! answer ran) therefore finds the scenario idle: no borrow of a script, a
//! track or a stream is held while code the scenario does not own runs (a
//! matcher, a computed answer, a value's `Clone`, `PartialEq`, `Debug` or
//! `Drop`).
//!
//! # Failures
//!
//! [`Scenario::verify`] returns every failure the run collected, typed:
//! a request no step or standing answer took, a held request never answered,
//! a step never used, a scheduled change the clock never reached, and an
//! event sent on a stream after its end.

mod events;
mod script;
mod track;

pub use events::{EventSubscription, Events, StreamEnd, StreamItem};
pub use script::{Answer, Expect, Script};
pub use track::{ServiceState, Track};

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::ManualClock;
use flui_foundation::panic::FirstPanic;

/// One virtual timeline and the failures of one run, shared by the scripts,
/// tracks and streams made on it.
///
/// Owner-thread only, cheap to clone; every clone is the same scenario. The
/// scripts, tracks and streams keep it alive, and it keeps only weak
/// references back, so dropping the last double ends everything it held.
#[derive(Clone)]
pub struct Scenario {
    core: Rc<Core>,
}

struct Core {
    clock: ManualClock,
    state: RefCell<CoreState>,
}

#[derive(Default)]
struct CoreState {
    /// Scheduled changes by instant, then by the order they were scheduled.
    timeline: BTreeMap<(Duration, u64), Timed>,
    next_seq: u64,
    failures: Vec<ScenarioFailure>,
    audited: Vec<Weak<dyn Audit>>,
}

/// A change scheduled on the timeline.
struct Timed {
    what: Cow<'static, str>,
    apply: Box<dyn FnOnce()>,
}

/// What [`Scenario::verify`] asks of each script: the obligations it still
/// holds.
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
            }),
        }
    }

    /// The clock the scenario's instants are on.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.core.clock
    }

    /// The virtual time now: the clock's elapsed time.
    #[must_use]
    pub fn now(&self) -> Duration {
        self.core.clock.elapsed()
    }

    /// A script that answers requests of type `R` with `A`, named `name` in
    /// the failures it reports.
    #[must_use]
    pub fn script<R, A>(&self, name: &'static str) -> Script<R, A>
    where
        R: fmt::Debug + 'static,
        A: Send + 'static,
    {
        Script::on(self, name)
    }

    /// A value the platform changes over time, starting at `initial`.
    #[must_use]
    pub fn track<T>(&self, name: &'static str, initial: T) -> Track<T>
    where
        T: Clone + PartialEq + fmt::Debug + Send + 'static,
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
        E: Clone + fmt::Debug + Send + 'static,
    {
        Events::on(self, name)
    }

    /// Apply every change scheduled at or before [`now`](Self::now), in the
    /// order of their instants and, at one instant, the order they were
    /// scheduled. Returns how many it applied.
    ///
    /// A change scheduled while this runs, at an instant already reached,
    /// applies in the same call. Each change runs with no borrow held, so one
    /// whose value panics in `Drop` leaves the others applied; the first
    /// panic is resumed once all of them have run.
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

    /// Every failure of the run so far, or `Ok` when there is none.
    ///
    /// # Errors
    ///
    /// [`ScenarioFailures`] listing, in this order: the failures recorded as
    /// they happened (an unexpected request, an event after its stream's
    /// end), what each live script still holds (a held request nobody
    /// answered, a step no request used), and every scheduled change not yet
    /// applied.
    pub fn verify(&self) -> Result<(), ScenarioFailures> {
        let (mut failures, audited, unreached) = {
            let mut state = self.core.state.borrow_mut();
            state.audited.retain(|script| script.strong_count() > 0);
            let audited: Vec<Rc<dyn Audit>> =
                state.audited.iter().filter_map(Weak::upgrade).collect();
            let unreached: Vec<(Duration, Cow<'static, str>)> = state
                .timeline
                .iter()
                .map(|(&(at, _), timed)| (at, timed.what.clone()))
                .collect();
            (state.failures.clone(), audited, unreached)
        };
        for script in audited {
            script.audit(&mut failures);
        }
        failures.extend(
            unreached
                .into_iter()
                .map(|(at, what)| ScenarioFailure::NotReached { what, at }),
        );
        if failures.is_empty() {
            Ok(())
        } else {
            Err(ScenarioFailures(failures))
        }
    }

    /// Schedule `apply` at `at`; `what` names it if the clock never gets
    /// there.
    fn schedule(&self, at: Duration, what: Cow<'static, str>, apply: Box<dyn FnOnce()>) {
        let mut state = self.core.state.borrow_mut();
        let seq = state.next_seq;
        state.next_seq = seq
            .checked_add(1)
            .expect("BUG: fewer than u64::MAX changes are scheduled in one scenario");
        state.timeline.insert((at, seq), Timed { what, apply });
    }

    fn fail(&self, failure: ScenarioFailure) {
        self.core.state.borrow_mut().failures.push(failure);
    }

    fn audit(&self, script: Weak<dyn Audit>) {
        self.core.state.borrow_mut().audited.push(script);
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
            .finish()
    }
}

/// One way a run went other than its scenario said.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScenarioFailure {
    /// A request that no step and no standing answer of its script took. Its
    /// caller's completion ended
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
    /// A change scheduled at an instant the run never reached, or reached
    /// with no owner turn after it.
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
