//! [`Script`]: ordered, typed answers to the requests of one capability.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::requests::{Effect, Event, Pending, Waits};
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
///    it uses up when it is still asked for once that matcher returns
///    ([held requests](#held-requests)); a step that accepts nothing stays
///    and is [`Unconsumed`](ScenarioFailure::Unconsumed) at the end;
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
/// - **While its answer is made** (by a matcher, a computed or standing
///   answer) a request stays in the script, where it was: a ticket dropped
///   meanwhile withdraws it, and one then kept for the test, or scheduled,
///   keeps its place ahead of the requests that came after it.
/// - **A ticket** ([`receive_with_ticket`](Self::receive_with_ticket))
///   withdraws its request in any of these states, while its answer is
///   made, or while its answer is scheduled. No more of an answer is made
///   for a withdrawn request: no step is tried after the one that ran, no
///   standing answer runs, nothing is recorded as unexpected and nothing
///   is scheduled.
/// - **Which step a withdrawn request uses up.** A step is used up when its
///   matcher accepted a request that is still asked for once the matcher
///   returns. A ticket dropped from then on (by the used step's own drop,
///   say) leaves the step used and its answer unmade; one dropped by the
///   matcher itself leaves the step for a later request.
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

/// The one owner of what the script holds (see [`Pending`]).
type Requests<R, A> = Pending<EntryId, Held<R, A>, Due<R, A>>;
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

/// Run the test's code for one request, the one place a script does. `run`
/// contains each piece of the test's code where it runs (a matcher, an
/// answer, an observer, the drop of a value the test supplied), so none is
/// left alive across another; a panic that escapes `run` anyway is
/// contained here, so the state `run` moves is never left half done. Every
/// panic lands in `first`, which keeps the earliest: those `run` contained
/// came before one that escaped it.
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
        let _ = self.apply(Event::HoldTaken);
        let script: Rc<dyn HoldOwner> = Rc::clone(&self.cell) as Rc<dyn HoldOwner>;
        ScriptHold {
            script: Some(Rc::downgrade(&script)),
        }
    }

    /// Answer the oldest held request with `value`; `false` when none is
    /// held.
    pub fn release_oldest(&self, value: A) -> bool {
        let mut first = FirstPanic::new();
        let released = if let RequestEffect::Taken(_, held, _) = self.apply(Event::ReleaseOldest) {
            let _ = contained(&mut first, |first| {
                self.deliver(held, value, first);
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
                    let _ = self.apply(Event::AnswerPanicked(key));
                    return false;
                };
                let RequestEffect::Taken(_, held, _) = self.apply(Event::Release(key)) else {
                    // Withdrawn while its answer was made; the answer's
                    // `Drop` is the test's code.
                    first.retire(value);
                    return false;
                };
                self.deliver(held, value, first);
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
                let RequestEffect::Taken(_, held, _) = self.apply(Event::Release(key)) else {
                    return false;
                };
                self.end_unanswered(held, first);
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
    /// Dropping the ticket while the request is still held, scheduled or
    /// while its answer is made withdraws it, as a system dismisses what its
    /// caller stopped waiting for (a dialog closed with its window, a started
    /// activity whose requester left): its resolver is dropped, no more of
    /// its answer is made and no step or release answers it, it is no
    /// [`Unanswered`](ScenarioFailure::Unanswered) failure, and
    /// [`withdrawn`](Self::withdrawn) counts it ([held
    /// requests](Self#held-requests)). A request answered already is not
    /// affected.
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
    fn apply(&self, event: RequestEvent<R, A>) -> RequestEffect<R, A> {
        self.cell.state.borrow_mut().requests.apply(event)
    }

    /// Carry out an event that may start making a request's answer, and
    /// hand out that request with its id, read in the same borrow that
    /// marked it `Answering`.
    fn start_answering(&self, event: RequestEvent<R, A>) -> Option<(EntryId, Rc<R>)> {
        let mut state = self.cell.state.borrow_mut();
        let RequestEffect::SendOn(id) = state.requests.apply(event) else {
            return None;
        };
        let request = state
            .requests
            .payload(id)
            .map(|held| Rc::clone(&held.request))
            .expect("BUG: a request marked answering is in the pending requests");
        Some((id, request))
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
        if let Some((id, request)) =
            self.start_answering(Event::Arrived(id, Held { request, resolver }))
        {
            let _ = contained(&mut first, |first| self.answer_request(id, request, first));
        }
        first.resume();
    }

    /// The last hold went: send the kept requests on, one at a time, until
    /// the pending requests say the drain is over. Returns how many were
    /// answered.
    fn drain(&self, first: &mut FirstPanic) -> usize {
        let mut answered = 0;
        while let Some((id, request)) = self.start_answering(Event::DrainNext) {
            if contained(first, |first| self.answer_request(id, request, first)) == Some(true) {
                answered += 1;
            }
        }
        answered
    }

    /// Make the answer of the request `id` ([`send_on`](Self::send_on)),
    /// then let go of this handle of it, whose `Drop` is the test's code
    /// when it is the last. Runs inside [`contained`].
    fn answer_request(&self, id: EntryId, request: Rc<R>, first: &mut FirstPanic) -> bool {
        let answered = self.send_on(id, &request, first);
        first.retire(request);
        answered
    }

    /// Run one piece of the test's code while the answer of the request
    /// `id` is made, contained, and say what became of it: its value, its
    /// panic, or that a ticket withdrew the request meanwhile, in which case
    /// the value is dropped as the test's code and the caller makes no more
    /// of the answer. Every piece of the test's code that runs while an
    /// answer is made goes through here.
    fn while_answering<T>(
        &self,
        id: EntryId,
        first: &mut FirstPanic,
        run: impl FnOnce() -> T,
    ) -> Ran<T> {
        let outcome = first.contain(run);
        match (self.is_answering(id), outcome) {
            (true, Some(value)) => Ran::Done(value),
            (true, None) => Ran::Panicked,
            (false, outcome) => {
                first.retire(outcome);
                Ran::Withdrawn
            }
        }
    }

    /// Whether the request `id` is still being answered: no ticket withdrew
    /// it since its answer began.
    fn is_answering(&self, id: EntryId) -> bool {
        self.cell.state.borrow().requests.is_answering(id)
    }

    /// Let go of `value`, whose drop is the test's code, while the answer of
    /// the request `id` is made. Returns whether the request is still being
    /// answered.
    fn drop_while_answering<T>(&self, id: EntryId, first: &mut FirstPanic, value: T) -> bool {
        !matches!(
            self.while_answering(id, first, || drop(value)),
            Ran::Withdrawn
        )
    }

    /// Make the answer of the request `id` by the steps, then the standing
    /// answer. Returns whether it was answered now: `false` for one a step
    /// holds or answers later, one whose matcher or answer panicked (kept
    /// for the test), one that ends unanswered, and one a ticket withdrew
    /// meanwhile, for which no more of the answer is made. Runs inside
    /// [`contained`].
    fn send_on(&self, id: EntryId, request: &R, first: &mut FirstPanic) -> bool {
        let candidates: Vec<(EntryId, Matcher<R>)> = self
            .cell
            .state
            .borrow()
            .steps
            .iter()
            .map(|step| (step.id, Rc::clone(&step.expect.matches)))
            .collect::<Vec<(EntryId, Matcher<R>)>>();
        // Matched with no borrow held; a matcher that re-entered and used a
        // step up leaves it gone, and the next candidate is tried. Each
        // clone is let go as the test's code: once its step is gone it is the
        // matcher's last, and the matcher's captures are the test's.
        let mut candidates = candidates.into_iter();
        while let Some((step_id, matches)) = candidates.next() {
            let accepted = self.while_answering(id, first, || matches(request));
            let still_asked = self.drop_while_answering(id, first, matches);
            match (still_asked, accepted) {
                // Withdrawn by the matcher or by its clone's drop: the
                // matcher's step is not used up.
                (false, _) | (_, Ran::Withdrawn) => {
                    let _ = self.drop_while_answering(id, first, candidates);
                    return false;
                }
                (true, Ran::Done(false)) => {}
                (true, Ran::Done(true)) => {
                    let step = {
                        let mut state = self.cell.state.borrow_mut();
                        let position = state.steps.iter().position(|step| step.id == step_id);
                        position.and_then(|position| state.steps.remove(position))
                    };
                    if let Some(Step { expect, answer, .. }) = step {
                        // The other matchers and the used step are let go
                        // before the answer is given, so none of the test's
                        // values is still held while its answer runs.
                        // Both are let go, each contained on its own,
                        // whatever the first one's drop did: a skipped
                        // `expect` would drop as the matcher's last handle
                        // outside the containment.
                        let asked = self.drop_while_answering(id, first, candidates);
                        let asked = self.drop_while_answering(id, first, expect) && asked;
                        if !asked {
                            let _ = self.drop_while_answering(id, first, answer);
                            return false;
                        }
                        let Some(answer) = answer else {
                            self.keep_for_test(id);
                            return false;
                        };
                        return self.answer(id, request, answer, first);
                    }
                }
                (true, Ran::Panicked) => {
                    if self.drop_while_answering(id, first, candidates) {
                        self.keep_for_test(id);
                    }
                    return false;
                }
            }
        }
        let otherwise = self.cell.state.borrow().otherwise.clone();
        if let Some(otherwise) = otherwise {
            let value = self.while_answering(id, first, || otherwise(request));
            let still_asked = self.drop_while_answering(id, first, otherwise);
            return match (still_asked, value) {
                (true, Ran::Done(value)) => self.finish(id, value, first),
                (true, Ran::Panicked) => {
                    self.keep_for_test(id);
                    false
                }
                // Withdrawn by the standing answer or by its clone's drop.
                (false, value) | (_, value @ Ran::Withdrawn) => {
                    let _ = self.drop_while_answering(id, first, value);
                    false
                }
            };
        }
        // The request's `Debug` is the test's code too: one that panics is
        // written as such and its panic kept, and one that withdrew the
        // request leaves nothing to record.
        let described = describe(request, first);
        if !self.is_answering(id) {
            return false;
        }
        self.cell.scenario.fail(ScenarioFailure::Unexpected {
            script: self.cell.name,
            request: described,
        });
        self.end_answering_unanswered(id, first);
        false
    }

    /// Answer the request `id` with `value`. Returns whether it was
    /// answered: not when a ticket withdrew it while its answer was made,
    /// and then the answer is dropped as the test's code.
    fn finish(&self, id: EntryId, value: A, first: &mut FirstPanic) -> bool {
        let RequestEffect::Taken(_, held, _) = self.apply(Event::Answered(id)) else {
            first.retire(value);
            return false;
        };
        self.deliver(held, value, first);
        true
    }

    /// The request `id`, whose answer was being made, waits for the test.
    fn keep_for_test(&self, id: EntryId) {
        let _ = self.apply(Event::KeepForTest(id));
    }

    /// End the request `id`, whose answer was being made, unanswered.
    fn end_answering_unanswered(&self, id: EntryId, first: &mut FirstPanic) {
        if let RequestEffect::Taken(_, held, _) = self.apply(Event::Answered(id)) {
            self.end_unanswered(held, first);
        }
    }

    /// Answer the request with `value`, after every observer heard it. Each
    /// piece of the test's code runs contained: the observers, the value's
    /// `Drop` (the resolver drops it when its completion is gone) and the
    /// request's.
    fn deliver(&self, held: Held<R, A>, value: A, first: &mut FirstPanic) {
        let Held { request, resolver } = held;
        let observers = self.cell.state.borrow().observers.clone();
        for observe in observers {
            first.contain(|| observe(&request, Some(&value)));
        }
        // A retired owner has nobody left to tell.
        first.contain(|| {
            let _ = resolver.resolve(value);
        });
        first.retire(request);
    }

    /// End the request unanswered, after every observer heard it.
    fn end_unanswered(&self, held: Held<R, A>, first: &mut FirstPanic) {
        let observers = self.cell.state.borrow().observers.clone();
        for observe in observers {
            first.contain(|| observe(&held.request, None));
        }
        first.retire(held);
    }

    /// Give `answer` to the request `id`. Returns whether it was answered
    /// now.
    fn answer(
        &self,
        id: EntryId,
        request: &R,
        answer: Answer<R, A>,
        first: &mut FirstPanic,
    ) -> bool {
        let Answer { reply, delay } = answer;
        if delay.is_zero() {
            return self.reply_now(id, request, reply, first);
        }
        // The request's `Debug` is the test's code: one that withdraws the
        // request leaves the scheduled answer orphaned, which the
        // `Schedule` event hands back to be taken off the timeline.
        let described = describe(request, first);
        let what = Cow::Owned(format!("{}: the answer to {described}", self.cell.name));
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
        if let RequestEffect::Orphaned(_, due) =
            self.apply(Event::Schedule(id, Due { reply, scheduled }))
        {
            // Withdrawn meanwhile: nothing serves the scheduled answer.
            self.cell.scenario.unschedule(due.scheduled);
            first.retire(due);
        }
        false
    }

    fn answer_due(&self, id: EntryId) {
        let (request, due) = {
            let mut state = self.cell.state.borrow_mut();
            let RequestEffect::Due(_, due) = state.requests.apply(Event::ClockReached(id)) else {
                return;
            };
            let request = state
                .requests
                .payload(id)
                .map(|held| Rc::clone(&held.request))
                .expect("BUG: a request marked answering is in the pending requests");
            (request, due)
        };
        let mut first = FirstPanic::new();
        let _ = contained(&mut first, |first| {
            let answered = self.reply_now(id, &request, due.reply, first);
            // The request's last handle may be this one: its `Drop` is the
            // test's code.
            first.retire(request);
            answered
        });
        first.resume();
    }

    /// Answer the request `id` with `reply`. Returns whether it was
    /// answered: a computed answer that panicked keeps it for the test, an
    /// abandon ends it unanswered, and one a ticket withdrew while the
    /// computed answer ran gets nothing more.
    fn reply_now(
        &self,
        id: EntryId,
        request: &R,
        reply: Reply<R, A>,
        first: &mut FirstPanic,
    ) -> bool {
        match reply {
            Reply::Value(value) => self.finish(id, value, first),
            Reply::Computed(compute) => {
                match self.while_answering(id, first, || compute(request)) {
                    Ran::Done(value) => self.finish(id, value, first),
                    Ran::Panicked => {
                        self.keep_for_test(id);
                        false
                    }
                    Ran::Withdrawn => false,
                }
            }
            Reply::Abandon => {
                self.end_answering_unanswered(id, first);
                false
            }
        }
    }
}

/// What became of one piece of the test's code run while a request's answer
/// was made ([`Script::while_answering`]).
enum Ran<T> {
    /// It returned this, and the request is still being answered.
    Done(T),
    /// It panicked (the panic is kept), and the request is still being
    /// answered.
    Panicked,
    /// A ticket withdrew the request meanwhile.
    Withdrawn,
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
    script: Option<std::rc::Weak<dyn HoldOwner>>,
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
        let answered = self.drop_hold(&mut first);
        first.resume();
        answered
    }

    /// Let go of this hold without starting a drain ([held
    /// requests](Script#held-requests)). A double that keeps a hold in its
    /// own state lets go of it this way when the state is torn down, so its
    /// teardown answers nothing.
    pub fn leave_held(mut self) {
        if let Some(script) = self.script.take().and_then(|script| script.upgrade()) {
            script.leave_hold();
        }
    }

    fn drop_hold(&mut self, first: &mut FirstPanic) -> usize {
        self.script
            .take()
            .and_then(|script| script.upgrade())
            .map_or(0, |script| script.drop_hold(first))
    }
}

impl Drop for ScriptHold {
    fn drop(&mut self) {
        let mut first = FirstPanic::new();
        let _ = self.drop_hold(&mut first);
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
trait HoldOwner {
    /// One hold dropped or released; returns how many requests the drain it
    /// started answered.
    fn drop_hold(self: Rc<Self>, first: &mut FirstPanic) -> usize;

    /// One hold let go with `leave_held`.
    fn leave_hold(&self);
}

impl<R: fmt::Debug + 'static, A: 'static> HoldOwner for ScriptCell<R, A> {
    fn drop_hold(self: Rc<Self>, first: &mut FirstPanic) -> usize {
        let script = Script { cell: self };
        if let RequestEffect::Drain = script.apply(Event::HoldDropped) {
            script.drain(first)
        } else {
            0
        }
    }

    fn leave_hold(&self) {
        let _ = self.state.borrow_mut().requests.apply(Event::HoldLeft);
    }
}

/// A script as a [`RequestTicket`] reaches it, whatever its types.
trait Withdraw {
    /// Withdraw the request `id` if it still waits.
    fn withdraw(&self, id: EntryId);
}

impl<R: fmt::Debug + 'static, A: 'static> Withdraw for ScriptCell<R, A> {
    fn withdraw(&self, id: EntryId) {
        let taken = self.state.borrow_mut().requests.apply(Event::Withdraw(id));
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
            let _ = state.requests.apply(Event::Reported);
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
