//! [`Script`]: ordered, typed answers to the requests of one capability.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::{Audit, Debt, Scenario, ScenarioFailure, TimelineKey, describe};
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
/// 1. nothing yet, while a [hold](Self::hold) of the script lives: it waits
///    until the last hold is dropped, then goes down this list, or until the
///    test releases it;
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
    /// How many [`ScriptHold`]s are alive: requests are held while any is.
    holds: usize,
    /// Whether the last hold's lift is sending the requests the holds kept
    /// on: a request that arrives meanwhile queues behind them.
    lifting: bool,
    /// Requests not answered yet, oldest first.
    waiting: VecDeque<Waiting<R, A>>,
    /// The first [`MAX_RECORDED_REQUESTS`] requests, oldest first.
    received: Vec<Rc<R>>,
    /// How many requests reached the script, recorded or not.
    received_count: usize,
    /// How many requests their tickets withdrew before an answer.
    withdrawn: usize,
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
    /// Until the test releases or abandons it: a step held it, or the code
    /// that was to answer it panicked.
    Released,
    /// Until the last [`ScriptHold`] is dropped, which sends it on to the
    /// steps and the standing answer, or until the test releases or abandons
    /// it.
    Lifted,
    /// Until the scenario's clock reaches the instant scheduled for it.
    Due {
        reply: Reply<R, A>,
        /// Where its answer sits on the scenario's timeline.
        scheduled: TimelineKey,
    },
}

impl<R, A> Until<R, A> {
    /// Whether the request waits for the test (or a hold), not the clock.
    fn is_held(&self) -> bool {
        matches!(self, Self::Released | Self::Lifted)
    }
}

impl<R, A> ScriptState<R, A> {
    /// The held requests, oldest first.
    fn held_ids(&self) -> Vec<EntryId> {
        self.waiting
            .iter()
            .filter(|waiting| waiting.until.is_held())
            .map(|waiting| waiting.id)
            .collect()
    }

    /// The request `id`, while it is still held.
    fn held_request(&self, id: EntryId) -> Option<Rc<R>> {
        self.waiting
            .iter()
            .find(|waiting| waiting.id == id && waiting.until.is_held())
            .map(|waiting| Rc::clone(&waiting.request))
    }

    /// Take the request `id` out, while it is still held.
    fn take_held(&mut self, id: EntryId) -> Option<Waiting<R, A>> {
        let position = self
            .waiting
            .iter()
            .position(|waiting| waiting.id == id && waiting.until.is_held())?;
        self.waiting.remove(position)
    }

    /// Take out the oldest request a hold kept.
    fn take_oldest_lifted(&mut self) -> Option<Waiting<R, A>> {
        let position = self
            .waiting
            .iter()
            .position(|waiting| matches!(waiting.until, Until::Lifted))?;
        self.waiting.remove(position)
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
                holds: 0,
                lifting: false,
                waiting: VecDeque::new(),
                received: Vec::new(),
                received_count: 0,
                withdrawn: 0,
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

    /// Hold every later request, steps and standing answer aside, while the
    /// returned hold, or another hold of this script, lives.
    ///
    /// The script counts its holds, so whoever took one (a test, a double's
    /// own method) lets go of its own and never of another's. When the last
    /// is dropped or [released](ScriptHold::release), the requests the holds
    /// kept go on, oldest first and one at a time, to the steps and the
    /// standing answer, as if they arrived then; a request that arrives
    /// meanwhile queues behind them. A request a step holds is not one of
    /// them: it
    /// waits for [`release_oldest`](Self::release_oldest),
    /// [`release_held`](Self::release_held) or
    /// [`abandon_held`](Self::abandon_held), which also answer requests a
    /// hold keeps.
    #[must_use = "requests are held only while the hold is alive"]
    pub fn hold(&self) -> ScriptHold {
        {
            let mut state = self.cell.state.borrow_mut();
            state.holds = state.holds.saturating_add(1);
        }
        let script: Rc<dyn Lift> = Rc::clone(&self.cell) as Rc<dyn Lift>;
        ScriptHold {
            script: Some(Rc::downgrade(&script)),
        }
    }

    /// Answer the oldest held request with `value`; `false` when none is
    /// held.
    pub fn release_oldest(&self, value: A) -> bool {
        let released = {
            let mut state = self.cell.state.borrow_mut();
            let position = state
                .waiting
                .iter()
                .position(|waiting| waiting.until.is_held());
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
    /// `answer` runs with no borrow held, and each request stays in the
    /// script until its own answer is given, so code that runs meanwhile
    /// (an answer, an observer) still finds the requests after it and can
    /// withdraw one by its ticket. A request whose answer panics stays held
    /// where it was, the others are still answered, and the first panic is
    /// resumed once all of them have been tried.
    pub fn release_held(&self, mut answer: impl FnMut(&R) -> A) -> usize {
        let held = self.cell.state.borrow().held_ids();
        let mut answered = 0;
        let mut first = FirstPanic::new();
        for id in held {
            let request = self.cell.state.borrow().held_request(id);
            let Some(request) = request else {
                continue;
            };
            let Some(value) = first.contain(|| answer(&request)) else {
                continue;
            };
            let taken = self.cell.state.borrow_mut().take_held(id);
            match taken {
                Some(Waiting {
                    request, resolver, ..
                }) => {
                    answered += 1;
                    self.deliver(&request, resolver, value, &mut first);
                }
                // Withdrawn while its answer was made: the answer's `Drop`
                // is the test's code.
                None => {
                    let _ = first.contain(|| drop(value));
                }
            }
        }
        first.resume();
        answered
    }

    /// Drop every held request unanswered: their completions end
    /// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
    /// Returns how many it dropped. Each stays in the script until it is
    /// dropped, as in [`release_held`](Self::release_held).
    pub fn abandon_held(&self) -> usize {
        let held = self.cell.state.borrow().held_ids();
        let mut count = 0;
        let mut first = FirstPanic::new();
        for id in held {
            let taken = self.cell.state.borrow_mut().take_held(id);
            if let Some(Waiting {
                request, resolver, ..
            }) = taken
            {
                count += 1;
                self.end_unanswered(&request, resolver, &mut first);
            }
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
            .filter(|waiting| waiting.until.is_held())
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
        self.receive_as(EntryId(self.cell.scenario.mint()), request, resolver);
    }

    /// A request its caller can withdraw reached the double: answer it as
    /// [`receive`](Self::receive) does, and hand back the ticket it waits
    /// by.
    ///
    /// Dropping the ticket while the request is still held or scheduled
    /// withdraws it, as a system dismisses what its caller stopped waiting
    /// for (a dialog closed with its window, a started activity whose
    /// requester left): its resolver is dropped, no step or release answers
    /// it, it is no [`Unanswered`](ScenarioFailure::Unanswered) failure, and
    /// [`withdrawn`](Self::withdrawn) counts it. A request answered already
    /// is not affected.
    ///
    /// # Panics
    ///
    /// As [`receive`](Self::receive); the ticket is dropped on the way out,
    /// so a request the panic left held is withdrawn. An observer that
    /// panics during that withdrawal has its panic discarded: the first one
    /// goes on.
    pub fn receive_with_ticket(&self, request: R, resolver: Resolver<A>) -> RequestTicket {
        let id = EntryId(self.cell.scenario.mint());
        let script: Rc<dyn Withdraw> = Rc::clone(&self.cell) as Rc<dyn Withdraw>;
        let ticket = RequestTicket {
            script: Rc::downgrade(&script),
            id,
        };
        self.receive_as(id, request, resolver);
        ticket
    }

    fn receive_as(&self, id: EntryId, request: R, resolver: Resolver<A>) {
        let request = Rc::new(request);
        let mut first = FirstPanic::new();
        self.dispatch(id, request, resolver, &mut first);
        first.resume();
    }

    /// How many requests were withdrawn by dropping their
    /// [`RequestTicket`] before they were answered.
    #[must_use]
    pub fn withdrawn(&self) -> usize {
        self.cell.state.borrow().withdrawn
    }

    fn dispatch(&self, id: EntryId, request: Rc<R>, resolver: Resolver<A>, first: &mut FirstPanic) {
        {
            let mut state = self.cell.state.borrow_mut();
            state.received_count = state.received_count.saturating_add(1);
            if state.received.len() < MAX_RECORDED_REQUESTS {
                state.received.push(Rc::clone(&request));
            }
        }
        self.route(id, request, resolver, first);
    }

    /// Answer a request that reached the script, or keep it while a hold
    /// lives or the last hold's lift is sending the kept requests on, behind
    /// them (see the [type docs](Self)).
    fn route(&self, id: EntryId, request: Rc<R>, resolver: Resolver<A>, first: &mut FirstPanic) {
        let kept = {
            let state = self.cell.state.borrow();
            state.holds > 0 || state.lifting
        };
        if kept {
            self.keep(Waiting {
                id,
                request,
                resolver,
                until: Until::Lifted,
            });
            return;
        }
        let _ = self.answer_by_script(id, request, resolver, first);
    }

    /// Give the request to the steps, then the standing answer. Returns
    /// whether it was answered now: `false` for one a step holds or answers
    /// later, one whose matcher or answer panicked (held), and one that ends
    /// unanswered.
    fn answer_by_script(
        &self,
        id: EntryId,
        request: Rc<R>,
        resolver: Resolver<A>,
        first: &mut FirstPanic,
    ) -> bool {
        let candidates = {
            let state = self.cell.state.borrow();
            state
                .steps
                .iter()
                .map(|step| (step.id, Rc::clone(&step.expect.matches)))
                .collect::<Vec<(EntryId, Matcher<R>)>>()
        };
        // Matched with no borrow held; a matcher that re-entered and used a
        // step up leaves it gone, and the next candidate is tried.
        for (step_id, matches) in candidates {
            match first.contain(|| matches(&request)) {
                Some(false) => {}
                Some(true) => {
                    let step = {
                        let mut state = self.cell.state.borrow_mut();
                        let position = state.steps.iter().position(|step| step.id == step_id);
                        position.and_then(|position| state.steps.remove(position))
                    };
                    if let Some(Step { expect, answer, .. }) = step {
                        drop(expect);
                        if let Some(answer) = answer {
                            return self.answer(id, request, resolver, answer, first);
                        }
                        self.hold_request(id, request, resolver);
                        return false;
                    }
                }
                None => {
                    self.hold_request(id, request, resolver);
                    return false;
                }
            }
        }
        let otherwise = self.cell.state.borrow().otherwise.clone();
        if let Some(otherwise) = otherwise {
            let value = first.contain(|| otherwise(&request));
            return self.deliver_or_hold(id, request, resolver, value, first);
        }
        let described = describe(&*request, first);
        self.cell.scenario.fail(ScenarioFailure::Unexpected {
            script: self.cell.name,
            request: described,
        });
        self.end_unanswered(&request, resolver, first);
        false
    }

    /// Answer the request with the answer test code made, or hold it when
    /// that code panicked (`None`). Returns whether it was answered.
    fn deliver_or_hold(
        &self,
        id: EntryId,
        request: Rc<R>,
        resolver: Resolver<A>,
        value: Option<A>,
        first: &mut FirstPanic,
    ) -> bool {
        let Some(value) = value else {
            self.hold_request(id, request, resolver);
            return false;
        };
        self.deliver(&request, resolver, value, first);
        true
    }

    fn hold_request(&self, id: EntryId, request: Rc<R>, resolver: Resolver<A>) {
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

    /// Give `answer` to the request. Returns whether it was answered now.
    fn answer(
        &self,
        id: EntryId,
        request: Rc<R>,
        resolver: Resolver<A>,
        answer: Answer<R, A>,
        first: &mut FirstPanic,
    ) -> bool {
        let Answer { reply, delay } = answer;
        if delay.is_zero() {
            return self.reply_now(id, request, resolver, reply, first);
        }
        let what = Cow::Owned(format!(
            "{}: the answer to {}",
            self.cell.name,
            describe(&*request, first)
        ));
        let at = self.cell.scenario.now().saturating_add(delay);
        let script = Rc::downgrade(&self.cell);
        let scheduled = self.cell.scenario.schedule(
            at,
            what,
            Box::new(move || {
                if let Some(cell) = script.upgrade() {
                    Script { cell }.answer_due(id);
                }
            }),
        );
        self.keep(Waiting {
            id,
            request,
            resolver,
            until: Until::Due { reply, scheduled },
        });
        false
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
            until: Until::Due { reply, .. },
            ..
        }) = due
        {
            let mut first = FirstPanic::new();
            let _ = self.reply_now(id, request, resolver, reply, &mut first);
            first.resume();
        }
    }

    /// Answer the request with `reply`. Returns whether it was answered: a
    /// computed answer that panicked holds it, and an abandon ends it
    /// unanswered.
    fn reply_now(
        &self,
        id: EntryId,
        request: Rc<R>,
        resolver: Resolver<A>,
        reply: Reply<R, A>,
        first: &mut FirstPanic,
    ) -> bool {
        match reply {
            Reply::Value(value) => {
                self.deliver(&request, resolver, value, first);
                true
            }
            Reply::Computed(compute) => {
                let value = first.contain(|| compute(&request));
                self.deliver_or_hold(id, request, resolver, value, first)
            }
            Reply::Abandon => {
                self.end_unanswered(&request, resolver, first);
                false
            }
        }
    }
}

/// What a double keeps for a request it received through
/// [`Script::receive_with_ticket`]: the request waits for as long as the
/// ticket is held, and dropping the ticket withdraws it if nothing answered
/// it yet.
///
/// The scenario's form of a system request that lives while its requester
/// holds it (a portal's `Request` object, a started activity's ticket).
/// Owner-thread only.
#[must_use = "dropping the ticket withdraws the request"]
pub struct RequestTicket {
    script: std::rc::Weak<dyn Withdraw>,
    id: EntryId,
}

impl Drop for RequestTicket {
    fn drop(&mut self) {
        if let Some(script) = self.script.upgrade() {
            script.withdraw(self.id);
        }
    }
}

impl fmt::Debug for RequestTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestTicket")
            .field("id", &self.id.0)
            .finish_non_exhaustive()
    }
}

/// One hold of a [`Script`]: the script holds every request that reaches it
/// while this, or another of its holds, lives ([`Script::hold`]).
///
/// Dropping or [releasing](Self::release) the last hold sends the requests
/// the holds kept on to the script's steps and standing answer, oldest
/// first, one at a time: each stays in the script until it is sent on, and
/// a request that arrives meanwhile queues behind them. A hold does not
/// keep the script alive: one dropped after its script does nothing.
/// Owner-thread only.
#[must_use = "requests are held only while the hold is alive"]
pub struct ScriptHold {
    /// `None` once let go.
    script: Option<std::rc::Weak<dyn Lift>>,
}

impl ScriptHold {
    /// Let go of this hold now, as dropping it does. Returns how many of
    /// the held requests were answered by the steps and the standing answer
    /// as they went on: none while another hold of the script lives. A
    /// request a step holds or answers later, one whose matcher or answer
    /// panics (it stays held), one that ends unanswered, and one a hold
    /// taken meanwhile keeps are not counted.
    ///
    /// # Panics
    ///
    /// With the first panic of the code the test supplied that ran for those
    /// requests (a matcher, a computed or standing answer, an observer),
    /// once every one of them has been answered or held again.
    pub fn release(mut self) -> usize {
        let mut first = FirstPanic::new();
        let answered = self.lift(&mut first);
        first.resume();
        answered
    }

    /// Let go of this hold without sending on what the holds kept: when it
    /// was the last, later requests are answered again, and the kept ones
    /// stay held, for the test to release or the run to report
    /// [`Unanswered`](ScenarioFailure::Unanswered). A double that keeps a
    /// hold in its own state lets go of it this way when the state is torn
    /// down, so its teardown answers nothing.
    pub fn leave_held(mut self) {
        if let Some(script) = self.script.take().and_then(|script| script.upgrade()) {
            script.leave();
        }
    }

    fn lift(&mut self, first: &mut FirstPanic) -> usize {
        self.script
            .take()
            .and_then(|script| script.upgrade())
            .map_or(0, |script| script.lift(first))
    }
}

impl Drop for ScriptHold {
    fn drop(&mut self) {
        let mut first = FirstPanic::new();
        let _ = self.lift(&mut first);
        // While the thread unwinds, `resume` discards the panic instead of
        // raising a second one from a destructor (ADR-0281 §4).
        first.resume();
    }
}

impl fmt::Debug for ScriptHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptHold")
            .field("released", &self.script.is_none())
            .finish_non_exhaustive()
    }
}

/// A script as a [`ScriptHold`] reaches it, whatever its types.
trait Lift {
    /// One hold let go: when it was the last, send the requests the holds
    /// kept on to the steps and the standing answer, and return how many
    /// of them were answered.
    fn lift(self: Rc<Self>, first: &mut FirstPanic) -> usize;

    /// One hold let go, sending nothing on.
    fn leave(&self);
}

impl<R: fmt::Debug + 'static, A: 'static> Lift for ScriptCell<R, A> {
    fn lift(self: Rc<Self>, first: &mut FirstPanic) -> usize {
        {
            let mut state = self.state.borrow_mut();
            state.holds = state.holds.saturating_sub(1);
            // Another hold lives, or this hold was let go from inside a
            // lift's own code, and that lift goes on once this returns.
            if state.holds > 0 || state.lifting {
                return 0;
            }
            state.lifting = true;
        }
        let script = Script { cell: self };
        let mut answered = 0;
        // One request at a time, the oldest first, taken out only as it is
        // sent on: the code it runs (a step, a standing answer, an observer,
        // the test's) finds the rest in the script, where a ticket can
        // withdraw one and a new request queues behind them. That code is
        // contained, so the loop always reaches its end. A hold taken
        // meanwhile stops it and keeps the rest, in order.
        loop {
            let next = {
                let mut state = script.cell.state.borrow_mut();
                if state.holds > 0 {
                    None
                } else {
                    state.take_oldest_lifted()
                }
            };
            let Some(Waiting {
                id,
                request,
                resolver,
                ..
            }) = next
            else {
                break;
            };
            if script.answer_by_script(id, request, resolver, first) {
                answered += 1;
            }
        }
        script.cell.state.borrow_mut().lifting = false;
        answered
    }

    fn leave(&self) {
        let mut state = self.state.borrow_mut();
        state.holds = state.holds.saturating_sub(1);
    }
}

/// A script as a [`RequestTicket`] reaches it, whatever its types.
trait Withdraw {
    /// Withdraw the request `id` if it still waits.
    fn withdraw(&self, id: EntryId);
}

impl<R: fmt::Debug + 'static, A: 'static> Withdraw for ScriptCell<R, A> {
    fn withdraw(&self, id: EntryId) {
        let withdrawn = {
            let mut state = self.state.borrow_mut();
            let position = state.waiting.iter().position(|waiting| waiting.id == id);
            let withdrawn = position.and_then(|position| state.waiting.remove(position));
            if withdrawn.is_some() {
                state.withdrawn = state.withdrawn.saturating_add(1);
            }
            withdrawn
        };
        // A scheduled answer is taken off the timeline, so the run does not
        // owe it; the request and its resolver are dropped after the borrow,
        // since the request's `Drop` is the test's code.
        if let Some(Waiting {
            until: Until::Due { scheduled, .. },
            ..
        }) = &withdrawn
        {
            self.scenario.unschedule(*scheduled);
        }
        // The request ends unanswered: every observer hears it, as of an
        // abandoned one, with no borrow held and each panic contained.
        let mut first = FirstPanic::new();
        if let Some(waiting) = &withdrawn {
            let observers = self.state.borrow().observers.clone();
            for observe in observers {
                first.contain(|| observe(&waiting.request, None));
            }
        }
        // The request's and the resolver's drops run contained too, so an
        // observer's earlier panic stays the first.
        first.contain(|| drop(withdrawn));
        // A ticket dropped while its thread unwinds (the panic of the code
        // that received the request): `resume` discards this panic instead
        // of raising a second one from a destructor (ADR-0281 §4).
        first.resume();
    }
}

impl<R: fmt::Debug + 'static, A: 'static> Audit for ScriptCell<R, A> {
    fn audit(&self, debts: &mut Vec<Debt>, first: &mut FirstPanic) {
        let (unanswered, unconsumed) = {
            let state = self.state.borrow();
            let unanswered: Vec<(EntryId, Rc<R>)> = state
                .waiting
                .iter()
                .filter(|waiting| waiting.until.is_held())
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
        let (steps, waiting, received, holds) = {
            let state = self.cell.state.borrow();
            (
                state.steps.len(),
                state.waiting.len(),
                state.received_count,
                state.holds,
            )
        };
        f.debug_struct("Script")
            .field("name", &self.cell.name)
            .field("steps", &steps)
            .field("waiting", &waiting)
            .field("received", &received)
            .field("holds", &holds)
            .finish_non_exhaustive()
    }
}
