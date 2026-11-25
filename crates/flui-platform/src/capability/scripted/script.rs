//! [`Script`]: ordered, typed answers to the requests of one capability.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::queue::{Effect, Event, Queue, Waits};
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
/// 1. nothing yet, while it is [held](#held-requests);
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
///
/// # Held requests
///
/// A held request waits either for a hold or for the test.
///
/// - **For a hold.** While a [`ScriptHold`] of the script lives
///   ([`hold`](Self::hold)), every request that reaches it is kept. The
///   script counts its holds, so whoever took one (a test, a double's own
///   method) lets go of its own and never of another's.
/// - **The last hold dropped or [released](ScriptHold::release)** starts a
///   drain: the kept requests go on, oldest first and one at a time, to the
///   steps and the standing answer, as if they arrived then. Each stays in
///   the script until its turn, so the code a drain runs finds the rest: a
///   ticket dropped meanwhile withdraws one, a request that arrives meanwhile
///   queues behind them, a hold taken meanwhile stops the drain with the rest
///   kept in order, and a hold taken and let go meanwhile, either way,
///   leaves the rest to that drain.
/// - **The last hold let go with [`leave_held`](ScriptHold::leave_held)**,
///   with no drain running, sends nothing on: the kept requests wait for the
///   test from then on.
/// - **For the test** waits a request a step held
///   ([`expect_held`](Self::expect_held)), one whose matcher or answer
///   panicked ([`release_held`](Self::release_held)'s included), one the
///   last hold left, and every held request a
///   [`verify`](Scenario::verify) reported: what was reported stays
///   reported, and no later drain answers it. Only
///   [`release_oldest`](Self::release_oldest),
///   [`release_held`](Self::release_held) and
///   [`abandon_held`](Self::abandon_held) answer it (they answer a request
///   kept for a hold too), and the run reports one still waiting
///   [`Unanswered`](ScenarioFailure::Unanswered).
/// - **A ticket** ([`receive_with_ticket`](Self::receive_with_ticket))
///   withdraws its request in any of these states, or while its answer is
///   scheduled.
/// - **Counts.** [`ScriptHold::release`] and [`release_held`](Self::release_held)
///   count the requests they answered then: a request a step holds or
///   answers later, one that ends unanswered or whose answer panicked, one
///   withdrawn meanwhile and one a hold taken meanwhile keeps are not
///   counted.
/// - **Panics.** The test's code (a matcher, an answer, an observer, the
///   drop of a value it supplied) runs contained, one request at a time; a
///   request whose code panicked waits for the test, the others are still
///   answered, and the first panic is resumed once all of them have been
///   tried.
pub struct Script<R: fmt::Debug + 'static, A: 'static> {
    cell: Rc<ScriptCell<R, A>>,
}

struct ScriptCell<R: fmt::Debug + 'static, A: 'static> {
    name: &'static str,
    scenario: Scenario,
    state: RefCell<ScriptState<R, A>>,
}

/// A request the script has not answered yet: the request, and the resolver
/// it is answered through.
struct Held<R, A> {
    request: Rc<R>,
    resolver: Resolver<A>,
}

/// An answer waiting for the scenario's clock.
struct Due<R, A> {
    reply: Reply<R, A>,
    /// Where it sits on the scenario's timeline.
    scheduled: TimelineKey,
}

/// The one owner of what the script holds (see [`Queue`]).
type Requests<R, A> = Queue<EntryId, Held<R, A>, Due<R, A>>;
type RequestEvent<R, A> = Event<EntryId, Held<R, A>, Due<R, A>>;
type RequestEffect<R, A> = Effect<EntryId, Held<R, A>, Due<R, A>>;

struct ScriptState<R, A> {
    steps: VecDeque<Step<R, A>>,
    otherwise: Option<Standing<R, A>>,
    /// Heard in the order they were added.
    observers: Vec<Observer<R, A>>,
    /// The requests not answered yet and the holds.
    requests: Requests<R, A>,
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

/// Run the test's code for one request, the one place a script does:
/// `run` contains the panics it can go on after, and one that escapes it
/// (the drop of a value the test supplied) is contained here. Every panic
/// lands in `first`, which keeps the earliest: those `run` contained came
/// before one that escaped it.
fn contained<T>(first: &mut FirstPanic, run: impl FnOnce(&mut FirstPanic) -> T) -> Option<T> {
    let mut inside = FirstPanic::new();
    let mut escaped = FirstPanic::new();
    let outcome = escaped.contain(|| run(&mut inside));
    first.absorb(inside);
    first.absorb(escaped);
    outcome
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
                requests: Requests::default(),
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

    /// Add a step that keeps the earliest request `expect` accepts for the
    /// test ([held requests](Self#held-requests)); one still held at the end
    /// is [`Unanswered`](ScenarioFailure::Unanswered).
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

    /// Answer what no step takes with a clone of `value`.
    pub fn otherwise_value(&self, value: A)
    where
        A: Clone,
    {
        self.otherwise(move |_| value.clone());
    }

    /// Hold every later request while the returned hold, or another hold of
    /// this script, lives ([held requests](Self#held-requests)).
    #[must_use = "requests are held only while the hold is alive"]
    pub fn hold(&self) -> ScriptHold {
        let _ = self.step(Event::HoldTaken);
        let script: Rc<dyn Lift> = Rc::clone(&self.cell) as Rc<dyn Lift>;
        ScriptHold {
            script: Some(Rc::downgrade(&script)),
        }
    }

    /// Answer the oldest held request with `value`; `false` when none is
    /// held.
    pub fn release_oldest(&self, value: A) -> bool {
        let mut first = FirstPanic::new();
        let released = if let RequestEffect::Taken(_, Held { request, resolver }, _) =
            self.step(Event::ReleaseOldest)
        {
            let _ = contained(&mut first, |first| {
                self.deliver(&request, resolver, value, first);
            });
            true
        } else {
            first.retire(value);
            false
        };
        first.resume();
        released
    }

    /// Answer every held request with what `answer` makes of it, oldest
    /// first. Returns how many it answered ([held
    /// requests](Self#held-requests)).
    ///
    /// `answer` runs with no borrow held, and each request stays in the
    /// script until its own answer is given, so code that runs meanwhile
    /// still finds the requests after it and can withdraw one by its ticket.
    pub fn release_held(&self, mut answer: impl FnMut(&R) -> A) -> usize {
        let keys = self.cell.state.borrow().requests.held_keys();
        let mut answered = 0;
        let mut first = FirstPanic::new();
        for key in keys {
            let delivered = contained(&mut first, |first| {
                let request = self
                    .cell
                    .state
                    .borrow()
                    .requests
                    .held_payload(key)
                    .map(|held| Rc::clone(&held.request));
                let Some(request) = request else {
                    return false;
                };
                let Some(value) = first.contain(|| answer(&request)) else {
                    let _ = self.step(Event::AnswerPanicked(key));
                    return false;
                };
                let RequestEffect::Taken(_, Held { request, resolver }, _) =
                    self.step(Event::Release(key))
                else {
                    // Withdrawn while its answer was made; the answer's
                    // `Drop` is the test's code, contained by `contained`.
                    drop(value);
                    return false;
                };
                self.deliver(&request, resolver, value, first);
                true
            });
            if delivered == Some(true) {
                answered += 1;
            }
        }
        first.resume();
        answered
    }

    /// Drop every held request unanswered, oldest first: their completions
    /// end [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
    /// Returns how many it dropped.
    pub fn abandon_held(&self) -> usize {
        let keys = self.cell.state.borrow().requests.held_keys();
        let mut count = 0;
        let mut first = FirstPanic::new();
        for key in keys {
            let dropped = contained(&mut first, |first| {
                let RequestEffect::Taken(_, Held { request, resolver }, _) =
                    self.step(Event::Release(key))
                else {
                    return false;
                };
                self.end_unanswered(&request, resolver, first);
                true
            });
            if dropped == Some(true) {
                count += 1;
            }
        }
        first.resume();
        count
    }

    /// How many requests are held.
    #[must_use]
    pub fn held(&self) -> usize {
        self.cell.state.borrow().requests.held().count()
    }

    /// How many requests reached the script.
    #[must_use]
    pub fn received(&self) -> usize {
        self.cell.state.borrow().received_count
    }

    /// The requests that reached the script, oldest first: the first
    /// [`MAX_RECORDED_REQUESTS`] of them; [`received`](Self::received) counts
    /// every one.
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
    /// request (a matcher, a computed or standing answer, an observer, the
    /// request's `Debug`, the drop of a value it supplied), after the request
    /// is held or answered.
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

    /// How many requests were withdrawn by dropping their
    /// [`RequestTicket`] before they were answered.
    #[must_use]
    pub fn withdrawn(&self) -> usize {
        self.cell.state.borrow().withdrawn
    }

    /// Carry `event` out on the script's requests.
    fn step(&self, event: RequestEvent<R, A>) -> RequestEffect<R, A> {
        self.cell.state.borrow_mut().requests.step(event)
    }

    fn receive_as(&self, id: EntryId, request: R, resolver: Resolver<A>) {
        let request = Rc::new(request);
        {
            let mut state = self.cell.state.borrow_mut();
            state.received_count = state.received_count.saturating_add(1);
            if state.received.len() < MAX_RECORDED_REQUESTS {
                state.received.push(Rc::clone(&request));
            }
        }
        let mut first = FirstPanic::new();
        if let RequestEffect::SendOn(id, held) =
            self.step(Event::Arrived(id, Held { request, resolver }))
        {
            let _ = contained(&mut first, |first| self.send_on(id, held, first));
        }
        first.resume();
    }

    /// The last hold went: send the kept requests on, one at a time, until
    /// the queue says the drain is over. Returns how many were answered.
    fn drain(&self, first: &mut FirstPanic) -> usize {
        let mut answered = 0;
        while let RequestEffect::SendOn(id, held) = self.step(Event::DrainNext) {
            if contained(first, |first| self.send_on(id, held, first)) == Some(true) {
                answered += 1;
            }
        }
        answered
    }

    /// Give the request to the steps, then the standing answer. Returns
    /// whether it was answered now: `false` for one a step holds or answers
    /// later, one whose matcher or answer panicked (kept for the test), and
    /// one that ends unanswered. Runs inside [`contained`].
    fn send_on(&self, id: EntryId, held: Held<R, A>, first: &mut FirstPanic) -> bool {
        let Held { request, resolver } = held;
        let candidates: Vec<(EntryId, Matcher<R>)> = self
            .cell
            .state
            .borrow()
            .steps
            .iter()
            .map(|step| (step.id, Rc::clone(&step.expect.matches)))
            .collect();
        // Matched with no borrow held; a matcher that re-entered and used a
        // step up leaves it gone, and the next candidate is tried. Each
        // clone is let go under `first`: once its step is gone it is the
        // matcher's last, and the matcher's captures are the test's.
        let mut candidates = candidates.into_iter();
        while let Some((step_id, matches)) = candidates.next() {
            let accepted = first.contain(|| matches(&request));
            first.retire(matches);
            match accepted {
                Some(false) => {}
                Some(true) => {
                    let step = {
                        let mut state = self.cell.state.borrow_mut();
                        let position = state.steps.iter().position(|step| step.id == step_id);
                        position.and_then(|position| state.steps.remove(position))
                    };
                    if let Some(Step { expect, answer, .. }) = step {
                        let answered = if let Some(answer) = answer {
                            self.answer(id, request, resolver, answer, first)
                        } else {
                            self.keep_for_test(id, request, resolver);
                            false
                        };
                        // The used step is let go once its answer is given.
                        first.retire(candidates);
                        first.retire(expect);
                        return answered;
                    }
                }
                None => {
                    first.retire(candidates);
                    self.keep_for_test(id, request, resolver);
                    return false;
                }
            }
        }
        let otherwise = self.cell.state.borrow().otherwise.clone();
        if let Some(otherwise) = otherwise {
            let value = first.contain(|| otherwise(&request));
            first.retire(otherwise);
            return self.deliver_or_keep(id, request, resolver, value, first);
        }
        let described = describe(&*request, first);
        self.cell.scenario.fail(ScenarioFailure::Unexpected {
            script: self.cell.name,
            request: described,
        });
        self.end_unanswered(&request, resolver, first);
        false
    }

    /// Answer the request with the answer the test's code made, or keep it
    /// for the test when that code panicked (`None`). Returns whether it was
    /// answered.
    fn deliver_or_keep(
        &self,
        id: EntryId,
        request: Rc<R>,
        resolver: Resolver<A>,
        value: Option<A>,
        first: &mut FirstPanic,
    ) -> bool {
        let Some(value) = value else {
            self.keep_for_test(id, request, resolver);
            return false;
        };
        self.deliver(&request, resolver, value, first);
        true
    }

    fn keep_for_test(&self, id: EntryId, request: Rc<R>, resolver: Resolver<A>) {
        let _ = self.step(Event::KeepForTest(id, Held { request, resolver }));
    }

    /// Answer `request` through `resolver` with `value`, after every
    /// observer heard it. The resolver drops the value when its completion
    /// is gone, and that `Drop` is the test's code: the caller runs this
    /// inside [`contained`].
    fn deliver(&self, request: &R, resolver: Resolver<A>, value: A, first: &mut FirstPanic) {
        let observers = self.cell.state.borrow().observers.clone();
        for observe in observers {
            first.contain(|| observe(request, Some(&value)));
        }
        // A retired owner has nobody left to tell.
        let _ = resolver.resolve(value);
    }

    /// End `request` unanswered, after every observer heard it.
    fn end_unanswered(&self, request: &R, resolver: Resolver<A>, first: &mut FirstPanic) {
        let observers = self.cell.state.borrow().observers.clone();
        for observe in observers {
            first.contain(|| observe(request, None));
        }
        drop(resolver);
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
        let _ = self.step(Event::Schedule(
            id,
            Held { request, resolver },
            Due { reply, scheduled },
        ));
        false
    }

    fn answer_due(&self, id: EntryId) {
        if let RequestEffect::Taken(id, Held { request, resolver }, Waits::Clock(due)) =
            self.step(Event::ClockReached(id))
        {
            let mut first = FirstPanic::new();
            let _ = contained(&mut first, |first| {
                self.reply_now(id, request, resolver, due.reply, first)
            });
            first.resume();
        }
    }

    /// Answer the request with `reply`. Returns whether it was answered: a
    /// computed answer that panicked keeps it for the test, and an abandon
    /// ends it unanswered.
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
                self.deliver_or_keep(id, request, resolver, value, first)
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

/// One hold of a [`Script`] ([held requests](Script#held-requests)). A
/// hold does not keep the script alive: one let go after its script does
/// nothing. Owner-thread only.
#[must_use = "requests are held only while the hold is alive"]
pub struct ScriptHold {
    /// `None` once let go.
    script: Option<std::rc::Weak<dyn Lift>>,
}

impl ScriptHold {
    /// Let go of this hold now, as dropping it does. Returns how many of
    /// the kept requests the drain this starts answered ([held
    /// requests](Script#held-requests)): none while another hold lives.
    ///
    /// # Panics
    ///
    /// With the first panic of the code the test supplied that ran for those
    /// requests, once every one of them has been answered or kept.
    pub fn release(mut self) -> usize {
        let mut first = FirstPanic::new();
        let answered = self.lift(&mut first);
        first.resume();
        answered
    }

    /// Let go of this hold without starting a drain ([held
    /// requests](Script#held-requests)). A double that keeps a hold in its
    /// own state lets go of it this way when the state is torn down, so its
    /// teardown answers nothing.
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
    /// One hold dropped or released; returns how many requests the drain it
    /// started answered.
    fn lift(self: Rc<Self>, first: &mut FirstPanic) -> usize;

    /// One hold let go with `leave_held`.
    fn leave(&self);
}

impl<R: fmt::Debug + 'static, A: 'static> Lift for ScriptCell<R, A> {
    fn lift(self: Rc<Self>, first: &mut FirstPanic) -> usize {
        let script = Script { cell: self };
        if let RequestEffect::Drain = script.step(Event::HoldDropped) {
            script.drain(first)
        } else {
            0
        }
    }

    fn leave(&self) {
        let _ = self.state.borrow_mut().requests.step(Event::HoldLeft);
    }
}

/// A script as a [`RequestTicket`] reaches it, whatever its types.
trait Withdraw {
    /// Withdraw the request `id` if it still waits.
    fn withdraw(&self, id: EntryId);
}

impl<R: fmt::Debug + 'static, A: 'static> Withdraw for ScriptCell<R, A> {
    fn withdraw(&self, id: EntryId) {
        let taken = self.state.borrow_mut().requests.step(Event::Withdraw(id));
        let RequestEffect::Taken(_, held, waits) = taken else {
            return;
        };
        {
            let mut state = self.state.borrow_mut();
            state.withdrawn = state.withdrawn.saturating_add(1);
        }
        // A scheduled answer is taken off the timeline, so the run does not
        // owe it.
        if let Waits::Clock(Due { scheduled, .. }) = &waits {
            self.scenario.unschedule(*scheduled);
        }
        // The request ends unanswered: every observer hears it, as of an
        // abandoned one, and the request, its resolver and any scheduled
        // answer are dropped, all as the test's code.
        let mut first = FirstPanic::new();
        let _ = contained(&mut first, |first| {
            let observers = self.state.borrow().observers.clone();
            for observe in observers {
                first.contain(|| observe(&held.request, None));
            }
            drop((held, waits));
        });
        // A ticket dropped while its thread unwinds (the panic of the code
        // that received the request): `resume` discards this panic instead
        // of raising a second one from a destructor (ADR-0281 §4).
        first.resume();
    }
}

impl<R: fmt::Debug + 'static, A: 'static> Audit for ScriptCell<R, A> {
    /// Report every held request and unused step. What is reported waits
    /// for the test from now on ([held requests](Script#held-requests)).
    fn audit(&self, debts: &mut Vec<Debt>, first: &mut FirstPanic) {
        let (unanswered, unconsumed) = {
            let mut state = self.state.borrow_mut();
            let _ = state.requests.step(Event::Reported);
            let unanswered: Vec<(EntryId, Rc<R>)> = state
                .requests
                .held()
                .map(|(id, held)| (id, Rc::clone(&held.request)))
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
                state.requests.len(),
                state.received_count,
                state.requests.holds(),
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
