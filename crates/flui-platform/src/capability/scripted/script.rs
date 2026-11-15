//! [`Script`]: ordered, typed answers to the requests of one capability.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::{Audit, Scenario, ScenarioFailure};
use crate::capability::Resolver;

/// A step's test of a request.
type Matcher<R> = Rc<dyn Fn(&R) -> bool>;
/// A script's standing answer.
type Standing<R, A> = Rc<dyn Fn(&R) -> A>;

/// Which requests a step of a [`Script`] answers, and what the step is called
/// when no request uses it.
pub struct Expect<R> {
    label: Cow<'static, str>,
    matches: Matcher<R>,
}

impl<R: 'static> Expect<R> {
    /// Every request.
    #[must_use]
    pub fn any() -> Self {
        Self {
            label: Cow::Borrowed("any request"),
            matches: Rc::new(|_: &R| true),
        }
    }

    /// The requests `matches` accepts, called `label`.
    ///
    /// `matches` runs with no borrow of the script held: it may read the
    /// script or the scenario.
    #[must_use]
    pub fn when(label: &'static str, matches: impl Fn(&R) -> bool + 'static) -> Self {
        Self {
            label: Cow::Borrowed(label),
            matches: Rc::new(matches),
        }
    }
}

impl<R: PartialEq + fmt::Debug + 'static> Expect<R> {
    /// Exactly the request `expected`, named by its `Debug`.
    #[must_use]
    pub fn exactly(expected: R) -> Self {
        let label = Cow::Owned(format!("{expected:?}"));
        Self {
            label,
            matches: Rc::new(move |request: &R| *request == expected),
        }
    }
}

impl<R> fmt::Debug for Expect<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Expect").field(&self.label).finish()
    }
}

/// What a step of a [`Script`] does with the request it takes.
pub struct Answer<R, A> {
    reply: Reply<R, A>,
    delay: Duration,
}

enum Reply<R, A> {
    Value(A),
    Computed(Box<dyn FnOnce(&R) -> A>),
    /// Kept until the test releases or abandons it.
    Hold,
    /// The resolver dropped: the caller's completion ends
    /// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
    Abandon,
}

impl<R, A> Answer<R, A> {
    /// Answer `value`.
    #[must_use]
    pub fn value(value: A) -> Self {
        Self::of(Reply::Value(value))
    }

    /// Answer what `compute` makes of the request, when the answer is given.
    /// It runs with no borrow of the script held.
    #[must_use]
    pub fn computed(compute: impl FnOnce(&R) -> A + 'static) -> Self {
        Self::of(Reply::Computed(Box::new(compute)))
    }

    /// Keep the request unanswered until [`Script::release_oldest`],
    /// [`Script::release_held`] or [`Script::abandon_held`]; one left held
    /// at [`Scenario::verify`] is
    /// [`Unanswered`](ScenarioFailure::Unanswered). A delay does not apply
    /// to it.
    #[must_use]
    pub fn hold() -> Self {
        Self::of(Reply::Hold)
    }

    /// Answer nothing: the resolver is dropped and the caller's completion
    /// ends [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped),
    /// as a backend torn down mid-call does.
    #[must_use]
    pub fn abandon() -> Self {
        Self::of(Reply::Abandon)
    }

    /// Give this answer `delay` after the request, on the scenario's clock:
    /// at the first [`Scenario::run_due`] on or after that instant.
    #[must_use]
    pub fn after(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn of(reply: Reply<R, A>) -> Self {
        Self {
            reply,
            delay: Duration::ZERO,
        }
    }
}

impl<R, A> fmt::Debug for Answer<R, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reply = match self.reply {
            Reply::Value(_) => "value",
            Reply::Computed(_) => "computed",
            Reply::Hold => "hold",
            Reply::Abandon => "abandon",
        };
        f.debug_struct("Answer")
            .field("reply", &reply)
            .field("delay", &self.delay)
            .finish()
    }
}

/// The requests of one type `R` a double receives, and the ordered answers
/// of type `A` its scenario gives them.
///
/// A request is answered by, in this order:
///
/// 1. nothing yet, while the script [holds](Self::hold): it waits for the
///    test to release it;
/// 2. the earliest step ([`expect`](Self::expect)) whose [`Expect`] accepts
///    it, which it uses up; a step that accepts nothing stays and is
///    [`Unconsumed`](ScenarioFailure::Unconsumed) at the end;
/// 3. the standing answer ([`otherwise`](Self::otherwise)), if one is set;
/// 4. otherwise it is [`Unexpected`](ScenarioFailure::Unexpected): the
///    scenario records it and the caller's completion ends
///    [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
///    No request is answered by a default nobody wrote.
///
/// Every answer settles at the caller's next owner turn (the
/// [`Completion`](crate::capability::Completion) rule), never inside the
/// call that asked. Owner-thread only, cheap to clone; every clone is the
/// same script.
pub struct Script<R, A> {
    cell: Rc<ScriptCell<R, A>>,
}

struct ScriptCell<R, A> {
    name: &'static str,
    scenario: Scenario,
    state: RefCell<ScriptState<R, A>>,
}

struct ScriptState<R, A> {
    steps: VecDeque<Step<R, A>>,
    otherwise: Option<Standing<R, A>>,
    holding: bool,
    /// Requests not answered yet, oldest first.
    waiting: VecDeque<Waiting<R, A>>,
    received: Vec<Rc<R>>,
    next_id: u64,
}

struct Step<R, A> {
    id: u64,
    expect: Expect<R>,
    answer: Answer<R, A>,
}

struct Waiting<R, A> {
    id: u64,
    request: Rc<R>,
    resolver: Resolver<A>,
    until: Until<R, A>,
}

enum Until<R, A> {
    /// Until the test releases or abandons it.
    Released,
    /// Until the scenario's clock reaches the instant scheduled for it.
    Due(Reply<R, A>),
}

impl<R, A> ScriptState<R, A> {
    fn mint(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .expect("BUG: fewer than u64::MAX steps and requests reach one script");
        id
    }

    fn take_released(&mut self) -> Vec<Waiting<R, A>> {
        let (released, kept): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|waiting| matches!(waiting.until, Until::Released));
        self.waiting = kept;
        released.into()
    }
}

impl<R, A> Clone for Script<R, A> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}

impl<R, A> Script<R, A>
where
    R: fmt::Debug + 'static,
    A: Send + 'static,
{
    pub(super) fn on(scenario: &Scenario, name: &'static str) -> Self {
        let cell = Rc::new(ScriptCell {
            name,
            scenario: scenario.clone(),
            state: RefCell::new(ScriptState {
                steps: VecDeque::new(),
                otherwise: None,
                holding: false,
                waiting: VecDeque::new(),
                received: Vec::new(),
                next_id: 0,
            }),
        });
        let audited: Rc<dyn Audit> = Rc::clone(&cell) as Rc<dyn Audit>;
        scenario.audit(Rc::downgrade(&audited));
        Self { cell }
    }

    /// The script's name in the failures it reports.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.cell.name
    }

    /// The scenario the script is on.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        &self.cell.scenario
    }

    /// Add a step: the earliest request `expect` accepts, among those no
    /// earlier step takes, gets `answer`.
    pub fn expect(&self, expect: Expect<R>, answer: Answer<R, A>) {
        let mut state = self.cell.state.borrow_mut();
        let id = state.mint();
        state.steps.push_back(Step { id, expect, answer });
    }

    /// Add a step for whichever request comes next.
    pub fn answer_next(&self, answer: Answer<R, A>) {
        self.expect(Expect::any(), answer);
    }

    /// Answer what no step takes with what `answer` makes of it, from now
    /// on, replacing any earlier standing answer. It runs with no borrow of
    /// the script held, once per request.
    pub fn otherwise(&self, answer: impl Fn(&R) -> A + 'static) {
        let replaced = self
            .cell
            .state
            .borrow_mut()
            .otherwise
            .replace(Rc::new(answer));
        drop(replaced);
    }

    /// Answer what no step takes with a clone of `value`.
    pub fn otherwise_value(&self, value: A)
    where
        A: Clone,
    {
        self.otherwise(move |_| value.clone());
    }

    /// Hold every later request, steps and standing answer aside, until
    /// [`resume`](Self::resume).
    pub fn hold(&self) {
        self.cell.state.borrow_mut().holding = true;
    }

    /// Answer later requests again; the ones held so far stay held.
    pub fn resume(&self) {
        self.cell.state.borrow_mut().holding = false;
    }

    /// Answer the oldest held request with `value`; `false` when none is
    /// held.
    pub fn release_oldest(&self, value: A) -> bool {
        let released = {
            let mut state = self.cell.state.borrow_mut();
            let position = state
                .waiting
                .iter()
                .position(|waiting| matches!(waiting.until, Until::Released));
            position.and_then(|position| state.waiting.remove(position))
        };
        released.is_some_and(|waiting| {
            // A retired owner has nobody left to tell.
            let _ = waiting.resolver.resolve(value);
            true
        })
    }

    /// Answer every held request with what `answer` makes of it. Returns
    /// how many it answered.
    pub fn release_held(&self, mut answer: impl FnMut(&R) -> A) -> usize {
        let released = self.cell.state.borrow_mut().take_released();
        let count = released.len();
        for waiting in released {
            let value = answer(&waiting.request);
            let _ = waiting.resolver.resolve(value);
        }
        count
    }

    /// Drop every held request unanswered: their completions end
    /// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
    /// Returns how many it dropped.
    pub fn abandon_held(&self) -> usize {
        let released = self.cell.state.borrow_mut().take_released();
        let count = released.len();
        drop(released);
        count
    }

    /// How many requests are held.
    #[must_use]
    pub fn held(&self) -> usize {
        self.cell
            .state
            .borrow()
            .waiting
            .iter()
            .filter(|waiting| matches!(waiting.until, Until::Released))
            .count()
    }

    /// How many requests reached the script.
    #[must_use]
    pub fn received(&self) -> usize {
        self.cell.state.borrow().received.len()
    }

    /// The requests that reached the script, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<R>
    where
        R: Clone,
    {
        let received: Vec<Rc<R>> = self.cell.state.borrow().received.clone();
        // Cloned after the borrow: `Clone` is the request type's code.
        received.iter().map(|request| (**request).clone()).collect()
    }

    /// A request reached the double; answer it through `resolver` as the
    /// script says (see the [type docs](Self)).
    pub fn receive(&self, request: R, resolver: Resolver<A>) {
        let request = Rc::new(request);
        let candidates = {
            let mut state = self.cell.state.borrow_mut();
            state.received.push(Rc::clone(&request));
            if state.holding {
                let id = state.mint();
                state.waiting.push_back(Waiting {
                    id,
                    request,
                    resolver,
                    until: Until::Released,
                });
                return;
            }
            state
                .steps
                .iter()
                .map(|step| (step.id, Rc::clone(&step.expect.matches)))
                .collect::<Vec<(u64, Matcher<R>)>>()
        };
        // Matched with no borrow held; a matcher that re-entered and used a
        // step up leaves it gone, and the next candidate is tried.
        for (id, matches) in candidates {
            if !matches(&request) {
                continue;
            }
            let step = {
                let mut state = self.cell.state.borrow_mut();
                let position = state.steps.iter().position(|step| step.id == id);
                position.and_then(|position| state.steps.remove(position))
            };
            if let Some(step) = step {
                let Step { expect, answer, .. } = step;
                drop(expect);
                self.answer(request, resolver, answer);
                return;
            }
        }
        let otherwise = self.cell.state.borrow().otherwise.clone();
        if let Some(otherwise) = otherwise {
            let value = otherwise(&request);
            let _ = resolver.resolve(value);
            return;
        }
        let described = format!("{request:?}");
        self.cell.scenario.fail(ScenarioFailure::Unexpected {
            script: self.cell.name,
            request: described,
        });
        drop(resolver);
    }

    fn answer(&self, request: Rc<R>, resolver: Resolver<A>, answer: Answer<R, A>) {
        let Answer { reply, delay } = answer;
        if matches!(reply, Reply::Hold) {
            let mut state = self.cell.state.borrow_mut();
            let id = state.mint();
            state.waiting.push_back(Waiting {
                id,
                request,
                resolver,
                until: Until::Released,
            });
            return;
        }
        if delay.is_zero() {
            reply_now(request.as_ref(), resolver, reply);
            return;
        }
        let what = Cow::Owned(format!("{}: the answer to {request:?}", self.cell.name));
        let id = {
            let mut state = self.cell.state.borrow_mut();
            let id = state.mint();
            state.waiting.push_back(Waiting {
                id,
                request,
                resolver,
                until: Until::Due(reply),
            });
            id
        };
        let at = self.cell.scenario.now().saturating_add(delay);
        let script = Rc::downgrade(&self.cell);
        self.cell.scenario.schedule(
            at,
            what,
            Box::new(move || {
                if let Some(cell) = script.upgrade() {
                    Script { cell }.answer_due(id);
                }
            }),
        );
    }

    fn answer_due(&self, id: u64) {
        let due = {
            let mut state = self.cell.state.borrow_mut();
            let position = state.waiting.iter().position(|waiting| waiting.id == id);
            position.and_then(|position| state.waiting.remove(position))
        };
        if let Some(Waiting {
            request,
            resolver,
            until: Until::Due(reply),
            ..
        }) = due
        {
            reply_now(request.as_ref(), resolver, reply);
        }
    }
}

fn reply_now<R, A: Send + 'static>(request: &R, resolver: Resolver<A>, reply: Reply<R, A>) {
    // A retired owner has nobody left to tell.
    match reply {
        Reply::Value(value) => {
            let _ = resolver.resolve(value);
        }
        Reply::Computed(compute) => {
            let value = compute(request);
            let _ = resolver.resolve(value);
        }
        Reply::Hold | Reply::Abandon => drop(resolver),
    }
}

impl<R: fmt::Debug, A> Audit for ScriptCell<R, A> {
    fn audit(&self, failures: &mut Vec<ScenarioFailure>) {
        let (unconsumed, unanswered) = {
            let state = self.state.borrow();
            let unconsumed: Vec<Cow<'static, str>> = state
                .steps
                .iter()
                .map(|step| step.expect.label.clone())
                .collect();
            let unanswered: Vec<Rc<R>> = state
                .waiting
                .iter()
                .filter(|waiting| matches!(waiting.until, Until::Released))
                .map(|waiting| Rc::clone(&waiting.request))
                .collect();
            (unconsumed, unanswered)
        };
        // Written after the borrow: `Debug` is the request type's code.
        failures.extend(
            unanswered
                .iter()
                .map(|request| ScenarioFailure::Unanswered {
                    script: self.name,
                    request: format!("{request:?}"),
                }),
        );
        failures.extend(
            unconsumed
                .into_iter()
                .map(|step| ScenarioFailure::Unconsumed {
                    script: self.name,
                    step,
                }),
        );
    }
}

impl<R, A> fmt::Debug for Script<R, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (steps, waiting, received, holding) = {
            let state = self.cell.state.borrow();
            (
                state.steps.len(),
                state.waiting.len(),
                state.received.len(),
                state.holding,
            )
        };
        f.debug_struct("Script")
            .field("name", &self.cell.name)
            .field("steps", &steps)
            .field("waiting", &waiting)
            .field("received", &received)
            .field("holding", &holding)
            .finish_non_exhaustive()
    }
}
