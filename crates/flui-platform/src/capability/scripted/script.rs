//! [`Script`]: ordered, typed answers to the requests of one capability.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::{Audit, Debt, Scenario, ScenarioFailure, describe};
use crate::capability::Resolver;

/// How many requests a script keeps for [`Script::requests`]; later ones are
/// counted by [`Script::received`] and answered all the same.
pub const MAX_RECORDED_REQUESTS: usize = 4096;

/// A step's test of a request.
type Matcher<R> = Rc<dyn Fn(&R) -> bool>;
/// A script's standing answer.
type Standing<R, A> = Rc<dyn Fn(&R) -> A>;

/// What a double hears of each answer: the request, and the value it was
/// answered with, or `None` when it ended unanswered.
type Observer<R, A> = Rc<dyn Fn(&R, Option<&A>)>;

/// The identity of a step or a waiting request within its scenario, from
/// the scenario's mint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EntryId(NonZeroU64);

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
    /// script or the scenario. If it panics, the request it was testing
    /// stays held and the panic is resumed.
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

/// What a step of a [`Script`] answers the request it takes with: a value,
/// a value computed from the request, or nothing (the resolver dropped), at
/// once or [`after`](Self::after) a delay. A step that keeps its request
/// until the test releases it is [`Script::expect_held`].
pub struct Answer<R, A> {
    reply: Reply<R, A>,
    delay: Duration,
}

enum Reply<R, A> {
    Value(A),
    Computed(Box<dyn FnOnce(&R) -> A>),
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
    /// It runs with no borrow of the script held; if it panics, the request
    /// stays held and the panic is resumed.
    #[must_use]
    pub fn computed(compute: impl FnOnce(&R) -> A + 'static) -> Self {
        Self::of(Reply::Computed(Box::new(compute)))
    }

    /// Answer nothing: the resolver is dropped and the caller's completion
    /// ends [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped),
    /// as a backend torn down mid-call does.
    #[must_use]
    pub fn abandon() -> Self {
        Self::of(Reply::Abandon)
    }

    /// Give this answer `delay` after the request, on the scenario's clock:
    /// when its driver reaches that instant.
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
/// 2. the earliest step ([`expect`](Self::expect),
///    [`expect_held`](Self::expect_held)) whose [`Expect`] accepts it, which
///    it uses up; a step that accepts nothing stays and is
///    [`Unconsumed`](ScenarioFailure::Unconsumed) at the end;
/// 3. the standing answer ([`otherwise`](Self::otherwise)), if one is set;
/// 4. otherwise it is [`Unexpected`](ScenarioFailure::Unexpected): the
///    scenario records it and the caller's completion ends
///    [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
///    No request is answered by a default nobody wrote.
///
/// Every answer settles at the caller's next owner turn (the
/// [`Completion`](crate::capability::Completion) rule), never inside the
/// call that asked. What the script owes outlives its handles: dropped with
/// a held request or an unused step, it hands them to the scenario's ledger.
/// Owner-thread only, cheap to clone; every clone is the same script.
pub struct Script<R: fmt::Debug + 'static, A: 'static> {
    cell: Rc<ScriptCell<R, A>>,
}

struct ScriptCell<R: fmt::Debug + 'static, A: 'static> {
    name: &'static str,
    scenario: Scenario,
    state: RefCell<ScriptState<R, A>>,
}

struct ScriptState<R, A> {
    steps: VecDeque<Step<R, A>>,
    otherwise: Option<Standing<R, A>>,
    /// Heard in the order they were added.
    observers: Vec<Observer<R, A>>,
    holding: bool,
    /// Requests not answered yet, oldest first.
    waiting: VecDeque<Waiting<R, A>>,
    /// The first [`MAX_RECORDED_REQUESTS`] requests, oldest first.
    received: Vec<Rc<R>>,
    /// How many requests reached the script, recorded or not.
    received_count: usize,
}

struct Step<R, A> {
    id: EntryId,
    expect: Expect<R>,
    /// `None` for a step that holds its request.
    answer: Option<Answer<R, A>>,
}

struct Waiting<R, A> {
    id: EntryId,
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
    fn take_released(&mut self) -> VecDeque<Waiting<R, A>> {
        let (released, kept): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|waiting| matches!(waiting.until, Until::Released));
        self.waiting = kept;
        released
    }
}

impl<R: fmt::Debug + 'static, A: 'static> Clone for Script<R, A> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}

impl<R, A> Script<R, A>
where
    R: fmt::Debug + 'static,
    A: 'static,
{
    pub(super) fn on(scenario: &Scenario, name: &'static str) -> Self {
        let cell = Rc::new(ScriptCell {
            name,
            scenario: scenario.clone(),
            state: RefCell::new(ScriptState {
                steps: VecDeque::new(),
                otherwise: None,
                observers: Vec::new(),
                holding: false,
                waiting: VecDeque::new(),
                received: Vec::new(),
                received_count: 0,
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
        self.push_step(expect, Some(answer));
    }

    /// Add a step that keeps the earliest request `expect` accepts
    /// unanswered until [`release_oldest`](Self::release_oldest),
    /// [`release_held`](Self::release_held) or
    /// [`abandon_held`](Self::abandon_held); one still held at the end is
    /// [`Unanswered`](ScenarioFailure::Unanswered).
    pub fn expect_held(&self, expect: Expect<R>) {
        self.push_step(expect, None);
    }

    /// Add a step for whichever request comes next.
    pub fn answer_next(&self, answer: Answer<R, A>) {
        self.expect(Expect::any(), answer);
    }

    fn push_step(&self, expect: Expect<R>, answer: Option<Answer<R, A>>) {
        let id = EntryId(self.cell.scenario.mint());
        self.cell
            .state
            .borrow_mut()
            .steps
            .push_back(Step { id, expect, answer });
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

    /// Hear every answer as it is given, however it is given (a step, a
    /// delayed step, the standing answer, a release), with the request it
    /// answers, before the script resolves the resolver the request came
    /// with, so before its completion settles at a later owner turn:
    /// `Some(value)`,
    /// or `None` for a request that ends unanswered (abandoned, or
    /// unexpected). A double keeps the state its answers imply here, in one
    /// place, whoever wrote the answer. Adds an observer beside those added
    /// before, which keep hearing and run first: a double's own observer
    /// cannot be replaced by a test's. Each runs with no borrow of the
    /// script held, its panic contained like the test's other code.
    pub fn on_answer(&self, observe: impl Fn(&R, Option<&A>) + 'static) {
        self.cell
            .state
            .borrow_mut()
            .observers
            .push(Rc::new(observe));
    }

    fn observers(&self) -> Vec<Observer<R, A>> {
        self.cell.state.borrow().observers.clone()
    }

    /// Answer `request` through `resolver` with `value`, after every
    /// observer heard it.
    fn deliver(&self, request: &R, resolver: Resolver<A>, value: A, first: &mut FirstPanic) {
        for observe in self.observers() {
            first.contain(|| observe(request, Some(&value)));
        }
        // A retired owner has nobody left to tell.
        let _ = resolver.resolve(value);
    }

    /// End `request` unanswered, after every observer heard it.
    fn end_unanswered(&self, request: &R, resolver: Resolver<A>, first: &mut FirstPanic) {
        for observe in self.observers() {
            first.contain(|| observe(request, None));
        }
        drop(resolver);
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
        let Some(waiting) = released else {
            return false;
        };
        let mut first = FirstPanic::new();
        self.deliver(&waiting.request, waiting.resolver, value, &mut first);
        first.resume();
        true
    }

    /// Answer every held request with what `answer` makes of it. Returns
    /// how many it answered.
    ///
    /// `answer` runs with no borrow held. A request whose answer panics
    /// stays held, the others are still answered, and the first panic is
    /// resumed once all of them have been tried.
    pub fn release_held(&self, mut answer: impl FnMut(&R) -> A) -> usize {
        let released = self.cell.state.borrow_mut().take_released();
        let mut answered = 0;
        let mut first = FirstPanic::new();
        for waiting in released {
            match first.contain(|| answer(&waiting.request)) {
                Some(value) => {
                    answered += 1;
                    let Waiting {
                        request, resolver, ..
                    } = waiting;
                    self.deliver(&request, resolver, value, &mut first);
                }
                None => self.keep(waiting),
            }
        }
        first.resume();
        answered
    }

    /// Drop every held request unanswered: their completions end
    /// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
    /// Returns how many it dropped.
    pub fn abandon_held(&self) -> usize {
        let released = self.cell.state.borrow_mut().take_released();
        let count = released.len();
        let mut first = FirstPanic::new();
        for Waiting {
            request, resolver, ..
        } in released
        {
            self.end_unanswered(&request, resolver, &mut first);
        }
        first.resume();
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
        self.cell.state.borrow().received_count
    }
    /// The requests that reached the script, oldest first: the first
    /// [`MAX_RECORDED_REQUESTS`] of them; [`received`](Self::received) counts
    /// every one.
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
    ///
    /// # Panics
    ///
    /// With the first panic of the code the test supplied that ran for this
    /// request (a matcher, a computed or standing answer, the request's
    /// `Debug`), after the request is held or answered.
    pub fn receive(&self, request: R, resolver: Resolver<A>) {
        let request = Rc::new(request);
        let mut first = FirstPanic::new();
        self.dispatch(request, resolver, &mut first);
        first.resume();
    }

    fn dispatch(&self, request: Rc<R>, resolver: Resolver<A>, first: &mut FirstPanic) {
        let candidates = {
            let mut state = self.cell.state.borrow_mut();
            state.received_count = state.received_count.saturating_add(1);
            if state.received.len() < MAX_RECORDED_REQUESTS {
                state.received.push(Rc::clone(&request));
            }
            if state.holding {
                drop(state);
                self.hold_request(request, resolver);
                return;
            }
            state
                .steps
                .iter()
                .map(|step| (step.id, Rc::clone(&step.expect.matches)))
                .collect::<Vec<(EntryId, Matcher<R>)>>()
        };
        // Matched with no borrow held; a matcher that re-entered and used a
        // step up leaves it gone, and the next candidate is tried.
        for (id, matches) in candidates {
            match first.contain(|| matches(&request)) {
                Some(false) => {}
                Some(true) => {
                    let step = {
                        let mut state = self.cell.state.borrow_mut();
                        let position = state.steps.iter().position(|step| step.id == id);
                        position.and_then(|position| state.steps.remove(position))
                    };
                    if let Some(Step { expect, answer, .. }) = step {
                        drop(expect);
                        match answer {
                            Some(answer) => self.answer(request, resolver, answer, first),
                            None => self.hold_request(request, resolver),
                        }
                        return;
                    }
                }
                None => {
                    self.hold_request(request, resolver);
                    return;
                }
            }
        }
        let otherwise = self.cell.state.borrow().otherwise.clone();
        if let Some(otherwise) = otherwise {
            match first.contain(|| otherwise(&request)) {
                Some(value) => self.deliver(&request, resolver, value, first),
                None => self.hold_request(request, resolver),
            }
            return;
        }
        let described = describe(&*request, first);
        self.cell.scenario.fail(ScenarioFailure::Unexpected {
            script: self.cell.name,
            request: described,
        });
        self.end_unanswered(&request, resolver, first);
    }

    fn hold_request(&self, request: Rc<R>, resolver: Resolver<A>) {
        let id = EntryId(self.cell.scenario.mint());
        self.keep(Waiting {
            id,
            request,
            resolver,
            until: Until::Released,
        });
    }

    fn keep(&self, waiting: Waiting<R, A>) {
        self.cell.state.borrow_mut().waiting.push_back(waiting);
    }

    fn answer(
        &self,
        request: Rc<R>,
        resolver: Resolver<A>,
        answer: Answer<R, A>,
        first: &mut FirstPanic,
    ) {
        let Answer { reply, delay } = answer;
        if delay.is_zero() {
            self.reply_now(request, resolver, reply, first);
            return;
        }
        let what = Cow::Owned(format!(
            "{}: the answer to {}",
            self.cell.name,
            describe(&*request, first)
        ));
        let id = EntryId(self.cell.scenario.mint());
        self.keep(Waiting {
            id,
            request,
            resolver,
            until: Until::Due(reply),
        });
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

    fn answer_due(&self, id: EntryId) {
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
            let mut first = FirstPanic::new();
            self.reply_now(request, resolver, reply, &mut first);
            first.resume();
        }
    }

    fn reply_now(
        &self,
        request: Rc<R>,
        resolver: Resolver<A>,
        reply: Reply<R, A>,
        first: &mut FirstPanic,
    ) {
        match reply {
            Reply::Value(value) => self.deliver(&request, resolver, value, first),
            Reply::Computed(compute) => match first.contain(|| compute(&request)) {
                Some(value) => self.deliver(&request, resolver, value, first),
                None => self.hold_request(request, resolver),
            },
            Reply::Abandon => self.end_unanswered(&request, resolver, first),
        }
    }
}

impl<R: fmt::Debug + 'static, A: 'static> Audit for ScriptCell<R, A> {
    fn audit(&self, debts: &mut Vec<Debt>, first: &mut FirstPanic) {
        let (unanswered, unconsumed) = {
            let state = self.state.borrow();
            let unanswered: Vec<(EntryId, Rc<R>)> = state
                .waiting
                .iter()
                .filter(|waiting| matches!(waiting.until, Until::Released))
                .map(|waiting| (waiting.id, Rc::clone(&waiting.request)))
                .collect();
            let unconsumed: Vec<(EntryId, Cow<'static, str>)> = state
                .steps
                .iter()
                .map(|step| (step.id, step.expect.label.clone()))
                .collect();
            (unanswered, unconsumed)
        };
        // Written after the borrow: `Debug` is the request type's code. One
        // that panics is written as such and its panic kept in `first`, so
        // every debt is still written.
        for (id, request) in &unanswered {
            debts.push((
                id.0,
                ScenarioFailure::Unanswered {
                    script: self.name,
                    request: describe(&**request, first),
                },
            ));
        }
        debts.extend(unconsumed.into_iter().map(|(id, step)| {
            (
                id.0,
                ScenarioFailure::Unconsumed {
                    script: self.name,
                    step,
                },
            )
        }));
    }
}

impl<R: fmt::Debug + 'static, A: 'static> Drop for ScriptCell<R, A> {
    /// The last handle is gone: what the script still owes becomes the
    /// scenario's, so a dropped script cannot leave a debt unreported. Every
    /// debt is handed over before a panic of the request type's `Debug` is
    /// resumed.
    fn drop(&mut self) {
        let mut debts = Vec::new();
        let mut first = FirstPanic::new();
        self.audit(&mut debts, &mut first);
        self.scenario.owe(debts);
        first.resume();
    }
}

impl<R: fmt::Debug + 'static, A: 'static> fmt::Debug for Script<R, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (steps, waiting, received, holding) = {
            let state = self.cell.state.borrow();
            (
                state.steps.len(),
                state.waiting.len(),
                state.received_count,
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
