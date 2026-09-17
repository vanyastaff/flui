//! Tests for the route stack.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/navigator_test.dart` —
//! `'Can push, pop, and replace in sequence'`, `'Push and pop should trigger the
//! observers'`, `'initial route trigger observer in the right order'`,
//! `'pushReplacement correctly reports didReplace to the observer'`,
//! `'removeRoute'`, `'remove a route whose value is awaited'`.
//! Expected values are read from `navigator.dart`, not from running this code.
//!
//! Every test here constructs a `RouteHistory` and nothing else. No element tree,
//! no build owner, no render pipeline, no overlay — `route_stack_flush_is_pure_data`
//! checks that claim against the sources rather than asserting it in prose.

use std::{cell::Cell, rc::Rc, sync::Arc};

use flui_scheduler::TickerFuture;
use parking_lot::Mutex;

use super::binding::{CompletedSignal, RouteBinding, RouteCommand};
use super::history::RouteHistory;
use super::lifecycle::RouteLifecycle;
use super::observer::{NavigatorObserver, Notification, Observation};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};

// ============================================================================
// PROBES
// ============================================================================

/// Every lifecycle callback a route received, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Install,
    DidPush,
    DidAdd,
    DidReplace(Option<RouteId>),
    DidPop,
    DidComplete(Option<i32>),
    DidPopNext(RouteId),
    DidChangeNext(Option<RouteId>),
    DidChangePrevious(Option<RouteId>),
    OnPopInvoked(bool),
    Dispose,
}

type Log = Arc<Mutex<Vec<Event>>>;

/// A route with an `i32` result and a full callback trace.
struct Probe {
    settings: RouteSettings,
    log: Log,
    /// Flutter's `currentResult` fallback.
    current_result: Option<i32>,
    /// Whether `did_pop` consents. `false` models `LocalHistoryRoute`.
    consents_to_pop: bool,
    push: PushCompletion,
    finished_when_popped: bool,
}

impl Probe {
    fn new(log: &Log) -> Self {
        Self {
            settings: RouteSettings::default(),
            log: Arc::clone(log),
            current_result: None,
            consents_to_pop: true,
            push: PushCompletion::Immediate,
            finished_when_popped: true,
        }
    }

    fn record(&self, event: Event) {
        self.log.lock().push(event);
    }
}

impl Route for Probe {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<i32> {
        self.current_result
    }

    fn finished_when_popped(&self) -> bool {
        self.finished_when_popped
    }

    fn install(&mut self) {
        self.record(Event::Install);
    }

    fn did_push(&mut self) -> PushCompletion {
        self.record(Event::DidPush);
        self.push.clone()
    }

    fn did_add(&mut self) {
        self.record(Event::DidAdd);
    }

    fn did_replace(&mut self, previous: Option<RouteId>) {
        self.record(Event::DidReplace(previous));
    }

    fn did_pop(&mut self) -> bool {
        self.record(Event::DidPop);
        self.consents_to_pop
    }

    fn did_complete(&mut self, result: Option<&i32>) {
        self.record(Event::DidComplete(result.copied()));
    }

    fn did_pop_next(&mut self, popped: RouteId) {
        self.record(Event::DidPopNext(popped));
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.record(Event::DidChangeNext(next));
    }

    fn did_change_previous(&mut self, previous: Option<RouteId>) {
        self.record(Event::DidChangePrevious(previous));
    }

    fn on_pop_invoked(&mut self, did_pop: bool) {
        self.record(Event::OnPopInvoked(did_pop));
    }

    fn dispose(&mut self) {
        self.record(Event::Dispose);
    }
}

/// A route whose result is a `String`, to exercise the erasure boundary.
struct StringRoute {
    settings: RouteSettings,
}

impl Route for StringRoute {
    type Output = String;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }
}

/// Records every observer notification, in the order delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Note {
    Push(RouteId, Option<RouteId>),
    Pop(RouteId, Option<RouteId>),
    Remove(RouteId, Option<RouteId>),
    Replace(Option<RouteId>, Option<RouteId>),
    ChangeTop(RouteId, Option<RouteId>),
}

#[derive(Default)]
struct Spy {
    notes: Mutex<Vec<Note>>,
}

impl Spy {
    fn notes(&self) -> Vec<Note> {
        self.notes.lock().clone()
    }
    fn kinds(&self) -> Vec<&'static str> {
        self.notes
            .lock()
            .iter()
            .map(|note| match note {
                Note::Push(..) => "push",
                Note::Pop(..) => "pop",
                Note::Remove(..) => "remove",
                Note::Replace(..) => "replace",
                Note::ChangeTop(..) => "changeTop",
            })
            .collect()
    }
}

impl NavigatorObserver for Spy {
    fn did_push(&self, route: RouteId, previous: Option<RouteId>) {
        self.notes.lock().push(Note::Push(route, previous));
    }
    fn did_pop(&self, route: RouteId, previous: Option<RouteId>) {
        self.notes.lock().push(Note::Pop(route, previous));
    }
    fn did_remove(&self, route: RouteId, previous: Option<RouteId>) {
        self.notes.lock().push(Note::Remove(route, previous));
    }
    fn did_replace(&self, new_route: Option<RouteId>, old_route: Option<RouteId>) {
        self.notes.lock().push(Note::Replace(new_route, old_route));
    }
    fn did_change_top(&self, top: RouteId, previous_top: Option<RouteId>) {
        self.notes.lock().push(Note::ChangeTop(top, previous_top));
    }
}

/// An erased pop result, as `RouteHistory::pop` takes one.
fn boxed(value: i32) -> super::route::AnyResult {
    super::route::AnyResult::new(value)
}

fn spy() -> Arc<Spy> {
    Arc::new(Spy::default())
}

/// The observer list a `NavigatorShared` would hold.
fn watching(spy: &Arc<Spy>) -> Vec<Arc<dyn NavigatorObserver>> {
    vec![Arc::clone(spy) as Arc<dyn NavigatorObserver>]
}

/// The other half of a flush: `NavigatorShared::apply`, minus the overlay.
///
/// `RouteHistory` walks the stack and *decides*; it no longer notifies observers or
/// disposes routes, because both run arbitrary code that reaches back through a
/// `NavigatorHandle` and would deadlock under the history's mutex.
/// A test that drives the history directly must therefore settle it, and settling
/// with **no** observers is how the production path drops the observations of a
/// flush nobody was listening to.
fn settle(history: &mut RouteHistory, observers: &[Arc<dyn NavigatorObserver>]) {
    let Some(mut outcome) = history.take_outcome() else {
        return;
    };
    super::observer::deliver(&outcome.notifications, observers);
    outcome.dispose_routes();
}

/// Settle a flush nobody is observing — the routes still die.
fn settle_unobserved(history: &mut RouteHistory) {
    settle(history, &[]);
}

// ============================================================================
// 1. LIFECYCLE ORDER
// ============================================================================

/// The four flush predicates are index ranges over Flutter's declaration order
/// (`navigator.dart:3519-3539`). Pin every membership, because reordering a
/// single variant silently changes all four.
///
/// Red-check: swap any two variants in `RouteLifecycle`.
#[test]
fn lifecycle_order_matches_flush_ranges() {
    use RouteLifecycle::{
        Add, Adding, Complete, Dispose, Disposed, Idle, Pop, Popping, Push, PushReplace, Pushing,
        Remove, Removing, Replace,
    };

    // Declaration order, minus `staging` and `disposing` (see lifecycle.rs).
    let all = [
        Add,
        Adding,
        Push,
        PushReplace,
        Pushing,
        Replace,
        Idle,
        Pop,
        Complete,
        Remove,
        Popping,
        Removing,
        Dispose,
        Disposed,
    ];
    let mut sorted = all;
    sorted.sort_unstable();
    assert_eq!(all, sorted, "variants must be declared in Flutter's order");

    let members = |predicate: fn(RouteLifecycle) -> bool| -> Vec<RouteLifecycle> {
        all.iter().copied().filter(|s| predicate(*s)).collect()
    };

    // add ..= idle
    assert_eq!(
        members(RouteLifecycle::will_be_present),
        vec![Add, Adding, Push, PushReplace, Pushing, Replace, Idle]
    );
    // add ..= remove
    assert_eq!(
        members(RouteLifecycle::is_present),
        vec![
            Add,
            Adding,
            Push,
            PushReplace,
            Pushing,
            Replace,
            Idle,
            Pop,
            Complete,
            Remove
        ]
    );
    // push ..= removing
    assert_eq!(
        members(RouteLifecycle::suitable_for_announcement),
        vec![
            Push,
            PushReplace,
            Pushing,
            Replace,
            Idle,
            Pop,
            Complete,
            Remove,
            Popping,
            Removing
        ]
    );
    // push ..= remove
    assert_eq!(
        members(RouteLifecycle::suitable_for_transition_animation),
        vec![
            Push,
            PushReplace,
            Pushing,
            Replace,
            Idle,
            Pop,
            Complete,
            Remove
        ]
    );
}

// ============================================================================
// 2-3. PUSH / OBSERVER ADDITIONS
// ============================================================================

/// `handlePush` installs, pushes, then **enqueues** the observation; observers
/// never fire inline (`navigator.dart:3271-3308`, `:4585`).
///
/// Red-check: notify the observer inside `handle_push` instead of returning an
/// `Observation` — `Install`/`DidPush` would no longer precede the notification.
#[test]
fn push_installs_then_pushes_then_notifies_observer() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let spy = spy();

    let (id, _result) = history.push(Probe::new(&log));
    settle(&mut history, &watching(&spy));

    assert_eq!(
        *log.lock(),
        vec![
            Event::Install,
            Event::DidPush,
            Event::DidChangeNext(None),
            Event::DidChangePrevious(None),
        ],
        "install → did_push → did_change_next(None) from handle_push, then \
         did_change_previous(None) from the announcement (see \
         `bottom_route_receives_its_initial_did_change_previous`)"
    );
    assert_eq!(
        spy.notes(),
        vec![Note::Push(id, None), Note::ChangeTop(id, None)]
    );
}

/// Additions drain **LIFO** (`_observedRouteAdditions.removeLast()`,
/// `navigator.dart:4628`), and the flush enqueues them **top-down**. Net effect:
/// the observer hears about a batch of seeded routes **bottom-up**.
///
/// This is Flutter's `'initial route trigger observer in the right order'`:
/// `defaultGenerateInitialRoutes` seeds `/`, `/a`, `/a/b` in one flush
/// (`restoreState`, `:3900-3934`) and the observer sees `/` first.
///
/// The LIFO drain is only observable on a flush carrying **two or more**
/// additions, which is why `seed_initial` does not flush.
///
/// Red-check: change `pop_back()` to `pop_front()` in `ObservationQueues::flush`;
/// the three pushes arrive top-down.
#[test]
fn push_adds_route_and_notifies_observer_lifo() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let spy = spy();

    // Three `Add` entries, one flush — the deep-link back-stack.
    let (bottom, _r0) = history.seed_initial(Probe::new(&log));
    let (middle, _r1) = history.seed_initial(Probe::new(&log));
    let (top, _r2) = history.seed_initial(Probe::new(&log));
    history.flush(true);
    settle(&mut history, &watching(&spy));

    assert_eq!(
        spy.notes(),
        vec![
            Note::Push(bottom, None),
            Note::Push(middle, Some(bottom)),
            Note::Push(top, Some(middle)),
            Note::ChangeTop(top, None),
        ],
        "additions enqueue top-down and drain LIFO, so they arrive bottom-up"
    );
}

// ============================================================================
// 3-5. RESULT CHANNEL
// ============================================================================

/// `pop(result)` → `didPop` → `didComplete(result)` → the future resolves.
///
/// Red-check: make `RouteRecord::did_pop` return `true` without calling
/// `did_complete`.
#[test]
fn pop_completes_route_with_explicit_result() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));
    let (_top, result) = history.push(Probe::new(&log));

    assert!(
        !result.is_completed(),
        "the future exists before it resolves"
    );
    assert!(history.pop(Some(boxed(42))));

    assert_eq!(result.try_take(), Some(Some(42)));
    assert!(log.lock().contains(&Event::OnPopInvoked(true)));
}

/// `_popCompleter.complete(result ?? currentResult)` (`navigator.dart:481`).
///
/// Red-check: drop the `None => self.route.current_result()` arm in
/// `RouteRecord::did_complete`; the result becomes `None`.
#[test]
fn pop_uses_current_result_fallback() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));

    let mut probe = Probe::new(&log);
    probe.current_result = Some(7);
    let (_top, result) = history.push(probe);

    assert!(history.pop(None), "popped with no explicit result");
    assert_eq!(result.try_take(), Some(Some(7)), "currentResult fallback");
}

/// **A removed route still completes its future.** `removeRoute` →
/// `_RouteEntry.complete` → `handleComplete` → `didComplete`
/// (`navigator.dart:3381-3386`). Oracle: `'remove a route whose value is awaited'`.
///
/// Red-check: delete the `Complete` arm's `handle_complete()` call in the flush,
/// or make `handle_complete` skip `did_complete`. The future never resolves —
/// which in a real app hangs every `await`.
#[test]
fn remove_route_still_completes_its_future() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));
    let (top, result) = history.push(Probe::new(&log));

    assert!(history.remove_route(top, Some(boxed(9))));
    // `Route::dispose` runs at settle, not inside the flush — the history hands the
    // dying route to its caller so teardown never runs under the mutex.
    settle_unobserved(&mut history);

    assert_eq!(result.try_take(), Some(Some(9)));
    assert!(log.lock().contains(&Event::Dispose));
    assert_eq!(history.len(), 1);
}

/// A removed route with no result still completes, with `currentResult`.
#[test]
fn remove_route_without_result_uses_current_result_fallback() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));
    let mut probe = Probe::new(&log);
    probe.current_result = Some(-1);
    let (top, result) = history.push(probe);

    history.remove_route(top, None);
    assert_eq!(result.try_take(), Some(Some(-1)));
}

/// The completer fires exactly once. Dart's `Completer.complete` throws on the
/// second call; `_RouteEntry.complete`'s `>= remove` early-return
/// (`navigator.dart:3431`) is what stops it being reached.
///
/// Red-check: delete that early-return in `RouteEntry::arm_complete`; the second
/// `remove_route` re-arms a disposed-or-removing entry.
#[test]
fn double_pop_or_double_remove_does_not_double_complete() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));
    let (top, result) = history.push(Probe::new(&log));

    history.remove_route(top, Some(boxed(1)));
    assert_eq!(result.try_take(), Some(Some(1)));

    // The entry is gone; a second removal finds nothing.
    assert!(!history.remove_route(top, Some(boxed(2))));
    assert_eq!(result.try_take(), None, "the future resolved exactly once");

    let completes = log
        .lock()
        .iter()
        .filter(|event| matches!(event, Event::DidComplete(_)))
        .count();
    assert_eq!(completes, 1);
}

/// Pop a route that is **not** `finished_when_popped`: it completes immediately
/// (`didPop` → `didComplete`) but parks in `Popping`. Removing it afterwards must
/// not complete it a second time with a different result.
///
/// The guard that stops it is *not* the completer's. It is `arm_complete`'s
/// `>= Remove` early-return (`navigator.dart:3431`) — because in Flutter's
/// declaration order **`popping` (index 11) sits after `remove` (index 10)**,
/// which is exactly why a popping route is not `isPresent`. Asserted below so the
/// surprise is recorded rather than rediscovered.
///
/// Red-check: delete the `>= Remove` guard in `RouteEntry::arm_complete`. The
/// entry re-arms to `Complete`, `handle_complete` runs, and — because
/// `RouteRecord::did_complete` and `Completer::complete` are *also* guarded — the
/// value stays `4` but `did_complete` fires twice and the route is disposed while
/// its exit transition is still in flight.
#[test]
fn pop_then_remove_of_an_animating_route_completes_exactly_once() {
    assert!(
        RouteLifecycle::Popping > RouteLifecycle::Remove,
        "Flutter orders popping after remove; is_present and arm_complete both rely on it"
    );

    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let mut probe = Probe::new(&log);
    probe.finished_when_popped = false;
    let (top, result) = history.push(probe);

    history.pop(Some(boxed(4)));
    assert_eq!(history.state_of(top), Some(RouteLifecycle::Popping));

    history.remove_route(top, Some(boxed(99)));

    assert_eq!(
        history.state_of(top),
        Some(RouteLifecycle::Popping),
        "the popping route was not re-armed"
    );
    assert_eq!(result.try_take(), Some(Some(4)), "the first result wins");
    let completes = log
        .lock()
        .iter()
        .filter(|event| matches!(event, Event::DidComplete(_)))
        .count();
    assert_eq!(completes, 1, "did_complete ran exactly once");
}

/// [`Completer`] is a one-shot: the second completion is dropped, not applied and
/// not panicked on. Dart's `Completer.complete` throws instead.
///
/// Tested here, directly, because through `RouteHistory` it is unreachable —
/// `arm_complete`'s `>= Remove` guard fires first (see the test above). Rather
/// than ship an untestable correctness primitive or delete one, test it at the
/// layer whose contract it is. Same posture as ADR-0018's `apply_fold`.
///
/// Red-check: drop the `if shared.value.is_some() { return false; }` guard in
/// `Completer::complete`.
#[test]
fn completer_completes_exactly_once() {
    let (completer, result) = super::result::Completer::<i32>::new();

    assert!(!completer.is_completed());
    assert!(completer.complete(Some(1)), "first completion is accepted");
    assert!(completer.is_completed());
    assert!(!completer.complete(Some(2)), "second is rejected");

    assert_eq!(result.try_take(), Some(Some(1)));
}

/// `ErasedRoute::did_complete` is idempotent, and applies the
/// `result ?? current_result` fallback exactly once.
///
/// Also unreachable through `RouteHistory` (see above), and likewise tested at
/// its own layer — it is the contract the `Navigator` view will call against.
///
/// Red-check: delete `if self.completer.is_completed() { return; }` in
/// `RouteRecord::did_complete`; `did_complete` fires on the route twice.
#[test]
fn erased_did_complete_is_idempotent() {
    let log: Log = Log::default();
    let mut probe = Probe::new(&log);
    probe.current_result = Some(5);

    let (mut erased, result) = super::route::RouteRecord::erase(probe);

    erased.did_complete(None);
    erased.did_complete(Some(boxed(77)));

    assert_eq!(
        result.try_take(),
        Some(Some(5)),
        "currentResult, applied once"
    );
    assert_eq!(
        log.lock()
            .iter()
            .filter(|event| matches!(event, Event::DidComplete(_)))
            .count(),
        1
    );
}

/// `_RouteEntry.complete`'s `>= remove` early-return (`navigator.dart:3431`).
///
/// A route that was **replaced** reports no removal (`_reportRemovalToObserver ==
/// false`) and sits in `Removing` while the incoming route animates. Removing it
/// again must not resurrect it into `Complete`, reset that flag, and emit a
/// spurious `didRemove`.
///
/// Red-check: delete the `if self.state >= RouteLifecycle::Remove { return; }`
/// guard in `RouteEntry::arm_complete`; the observer sees a `remove` note and the
/// route is disposed while the push is still in flight.
#[test]
fn remove_route_on_an_already_removing_route_is_a_noop() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));
    let (old, _r1) = history.push(Probe::new(&log));
    settle_unobserved(&mut history);

    let spy = spy();

    let mut incoming = Probe::new(&log);
    incoming.push = PushCompletion::Animating(TickerFuture::complete());
    let (new_top, _r2) = history.push_replacement(incoming, None);

    assert_eq!(history.state_of(old), Some(RouteLifecycle::Removing));
    assert_eq!(history.state_of(new_top), Some(RouteLifecycle::Pushing));

    history.remove_route(old, Some(boxed(1)));
    settle(&mut history, &watching(&spy));

    assert_eq!(
        history.state_of(old),
        Some(RouteLifecycle::Removing),
        "the entry was not re-armed"
    );
    assert!(
        !spy.kinds().contains(&"remove"),
        "a replaced route must never emit didRemove: {:?}",
        spy.notes()
    );
}

/// A pop whose route refuses (`did_pop → false`) returns the entry to `Idle`,
/// completes nothing, and leaves the stack intact
/// (`navigator.dart:3369-3371`, `:4531`).
///
/// Red-check: ignore `did_pop`'s return value in `RouteRecord::did_pop`.
#[test]
fn refused_pop_returns_the_entry_to_idle_and_completes_nothing() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&log));

    let mut probe = Probe::new(&log);
    probe.consents_to_pop = false;
    let (top, result) = history.push(probe);

    history.pop(Some(boxed(5)));

    assert_eq!(history.state_of(top), Some(RouteLifecycle::Idle));
    assert_eq!(history.len(), 2);
    assert!(!result.is_completed(), "a refused pop completes nothing");
    assert!(!log.lock().contains(&Event::OnPopInvoked(true)));
}

/// The private `dyn Any` boundary. A wrong result type logs and
/// completes with `None`, where Flutter throws a cast error.
///
/// Red-check: `unwrap()` the downcast instead — the test panics.
#[test]
fn pop_with_mismatched_result_type_yields_none() {
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(StringRoute {
        settings: RouteSettings::default(),
    });
    let (_top, result) = history.push(StringRoute {
        settings: RouteSettings::named("second"),
    });

    // An `i32` for a `String` route.
    history.pop(Some(boxed(3)));

    assert_eq!(
        result.try_take(),
        Some(None),
        "the future resolves with None rather than hanging or panicking"
    );
}

// ============================================================================
// 6. DELETIONS ARE FIFO
// ============================================================================

/// Deletions drain **FIFO** (`_observedRouteDeletions.removeFirst()`,
/// `navigator.dart:4633`), and every addition precedes every deletion
/// (`:4627-4635`).
///
/// The flush walks **top-down**, so with two routes removed in one flush the
/// deletion enqueued first is the *upper* one — and it is announced first.
///
/// Red-check: change `pop_front()` to `pop_back()` in `ObservationQueues::flush`;
/// the two removals invert. Move the deletions loop above the additions loop and
/// `additions_precede_deletions` fails too.
#[test]
fn delete_notifications_are_fifo() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (bottom, _r0) = history.add_initial(Probe::new(&log));
    let (middle, _r1) = history.push(Probe::new(&log));
    let (top, _r2) = history.push(Probe::new(&log));
    settle_unobserved(&mut history);

    let spy = spy();

    // `pushAndRemoveUntil(keep: bottom)` arms two removals and one push, then
    // flushes exactly once — the only Flutter API that batches deletions.
    let (pushed, _r3) = history.push_and_remove_until(Probe::new(&log), |id| id == bottom);
    settle(&mut history, &watching(&spy));

    let removed: Vec<RouteId> = spy
        .notes()
        .iter()
        .filter_map(|note| match note {
            Note::Remove(route, _) => Some(*route),
            _ => None,
        })
        .collect();

    assert_eq!(
        removed,
        vec![top, middle],
        "deletions enqueue top-down and drain FIFO, so the upper route is announced first"
    );
    assert_eq!(history.ids(), vec![bottom, pushed]);
}

/// Every addition is announced before every deletion, within one flush
/// (`navigator.dart:4627-4635`) — even though the flush's reverse walk enqueues
/// the deletions *before* it reaches the pushed route at the top.
///
/// Red-check: swap the two `while` loops in `ObservationQueues::flush`.
#[test]
fn additions_precede_deletions_within_one_flush() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (bottom, _r0) = history.add_initial(Probe::new(&log));
    let (_middle, _r1) = history.push(Probe::new(&log));
    let (_top, _r2) = history.push(Probe::new(&log));
    settle_unobserved(&mut history);

    let spy = spy();
    history.push_and_remove_until(Probe::new(&log), |id| id == bottom);
    settle(&mut history, &watching(&spy));

    let kinds = spy.kinds();
    let last_addition = kinds
        .iter()
        .rposition(|kind| *kind == "push" || *kind == "replace")
        .expect("the pushed route is an addition");
    let first_deletion = kinds
        .iter()
        .position(|kind| *kind == "remove")
        .expect("two routes were removed");

    assert!(
        last_addition < first_deletion,
        "additions must all precede deletions: {kinds:?}"
    );
}

/// A flush nobody observed still **drains** its queues, so its observations cannot
/// resurface in the next one (`navigator.dart:4623-4626`). Route lifecycle is
/// unchanged either way: registering an observer never changes what the stack does,
/// only who hears about it.
///
/// `ObservationQueues::drain` returns owned data now, so "delivered" and "emptied"
/// are the same operation and cannot drift apart. What *can* still break is drain
/// itself — copying instead of consuming.
///
/// Red-check: make `drain` build its `Vec` from `self.additions.iter().rev()` and
/// `self.deletions.iter()` without popping. The two pushes from the unobserved
/// flushes reappear in the pop's notifications, and the spy hears them.
#[test]
fn queues_are_cleared_when_there_are_no_observers() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (first, _r0) = history.add_initial(Probe::new(&log));
    let (_second, _r1) = history.push(Probe::new(&log));
    // The navigator settles every flush, observed or not; that is what drops the
    // observations nobody was listening to.
    settle_unobserved(&mut history);

    // Only now attach an observer: the earlier observations must be gone.
    let spy = spy();
    history.pop(None);
    settle(&mut history, &watching(&spy));

    assert!(
        !spy.notes()
            .iter()
            .any(|note| matches!(note, Note::Push(..))),
        "stale additions from the observer-less flushes leaked: {:?}",
        spy.notes()
    );
    assert_eq!(history.ids(), vec![first]);
}

// ============================================================================
// 7. NEIGHBOUR ANNOUNCEMENTS
// ============================================================================

/// `_flushRouteAnnouncement` (`navigator.dart:4638-4667`) fires
/// `didChangeNext` / `didChangePrevious` after the observers, and caches the last
/// announced value so an unchanged neighbour is not re-announced.
///
/// Red-check: delete the `last_announced_next` / `last_announced_previous`
/// caches; the second flush re-announces and the counts double.
#[test]
fn did_change_next_did_change_previous_ordering() {
    let bottom_log: Log = Log::default();
    let top_log: Log = Log::default();
    let mut history = RouteHistory::new();

    let (bottom, _r0) = history.add_initial(Probe::new(&bottom_log));
    // PORT-CHECK-OK-LOCK: plain data: recording log (Vec<Event>), no Drop
    bottom_log.lock().clear();

    let (top, _r1) = history.push(Probe::new(&top_log));

    // The bottom route learns a route appeared above it.
    assert_eq!(*bottom_log.lock(), vec![Event::DidChangeNext(Some(top))]);
    // The new top learns what is beneath it, after its own didChangeNext(None).
    assert_eq!(
        *top_log.lock(),
        vec![
            Event::Install,
            Event::DidPush,
            Event::DidChangeNext(None),
            Event::DidChangePrevious(Some(bottom)),
        ]
    );

    // A redundant flush announces nothing new.
    // PORT-CHECK-OK-LOCK: plain data: recording log (Vec<Event>), no Drop
    bottom_log.lock().clear();
    // PORT-CHECK-OK-LOCK: plain data: recording log (Vec<Event>), no Drop
    top_log.lock().clear();
    history.flush(true);
    assert!(bottom_log.lock().is_empty(), "no redundant re-announcement");
    assert!(top_log.lock().is_empty());
}

/// `shouldAnnounceChangeToNext` (`navigator.dart:3541-3546`): after a pop, the
/// route beneath already learned via `didPopNext`, so it must **not** also
/// receive `didChangeNext(null)`.
///
/// Red-check: make `should_announce_change_to_next` return `true` always; the
/// bottom route gets a spurious `DidChangeNext(None)` after the pop.
#[test]
fn pop_announces_did_pop_next_not_a_redundant_did_change_next() {
    let bottom_log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (_bottom, _r0) = history.add_initial(Probe::new(&bottom_log));
    let (top, _r1) = history.push(Probe::new(&Log::default()));

    // PORT-CHECK-OK-LOCK: plain data: recording log (Vec<Event>), no Drop
    bottom_log.lock().clear();
    history.pop(None);

    assert_eq!(
        *bottom_log.lock(),
        vec![Event::DidPopNext(top)],
        "didPopNext only — no redundant didChangeNext(None)"
    );
}

// ============================================================================
// 8. DISPOSAL TIMING
// ============================================================================

/// Entries marked for disposal are removed from the history inside the loop but
/// **disposed only after** the observer notifications and the neighbour
/// announcements (`navigator.dart:4571`, `:4585`, `:4589`, `:4609`), so a dying
/// route still receives its final announcements.
///
/// Red-check: dispose inside the `Dispose` arm instead of collecting into `dying`;
/// `Dispose` then precedes the observer's `pop` note.
///
/// This drives `settle`, the test-side twin of `NavigatorShared::apply` — so it
/// pins the *rule*, and would stay green if production's `apply` reordered.
/// `hero_seam_tests::observers_are_notified_before_a_dying_routes_overlay_entry_is_torn_down`
/// pins `apply` itself.
#[test]
fn flush_disposes_removed_routes_after_notifications() {
    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

    struct Tracer {
        settings: RouteSettings,
        order: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Route for Tracer {
        type Output = i32;
        fn settings(&self) -> &RouteSettings {
            &self.settings
        }
        fn dispose(&mut self) {
            self.order.lock().push("dispose");
        }
    }

    struct OrderSpy {
        order: Arc<Mutex<Vec<&'static str>>>,
    }
    impl NavigatorObserver for OrderSpy {
        fn did_pop(&self, _route: RouteId, _previous: Option<RouteId>) {
            self.order.lock().push("observer:pop");
        }
    }

    let mut history = RouteHistory::new();
    let observers: Vec<Arc<dyn NavigatorObserver>> = vec![Arc::new(OrderSpy {
        order: Arc::clone(&order),
    })];
    history.add_initial(Tracer {
        settings: RouteSettings::default(),
        order: Arc::clone(&order),
    });
    history.push(Tracer {
        settings: RouteSettings::default(),
        order: Arc::clone(&order),
    });
    settle(&mut history, &observers);

    // PORT-CHECK-OK-LOCK: plain data: recording log (Vec<Event>), no Drop
    order.lock().clear();
    history.pop(None);
    settle(&mut history, &observers);

    assert_eq!(
        *order.lock(),
        vec!["observer:pop", "dispose"],
        "observers are notified before the dying route is disposed"
    );
}

/// A route that is not `finished_when_popped` parks in `Popping` and is **not**
/// disposed — the `TransitionRoute` shape (`routes.dart:178`). Its future still
/// resolves immediately, because `didPop` completed it (`navigator.dart:458`).
///
/// Red-check: make `handle_pop` always set `Dispose`; the entry vanishes.
#[test]
fn pop_of_an_animating_route_parks_in_popping_but_still_completes() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let mut probe = Probe::new(&log);
    probe.finished_when_popped = false;
    let (top, result) = history.push(probe);

    history.pop(Some(boxed(4)));

    assert_eq!(history.state_of(top), Some(RouteLifecycle::Popping));
    assert_eq!(history.len(), 2, "not yet disposed");
    assert_eq!(result.try_take(), Some(Some(4)), "but already completed");
}

// ============================================================================
// PUSHING / RouteCommand::PushCompleted
// ============================================================================

/// An `Animating` push parks in `Pushing`, which — unlike `Idle` — does **not**
/// set `can_remove_or_add` (`navigator.dart:4499-4512`). So a route beneath it
/// that is `Removing` survives until the push completes.
///
/// Red-check: set `can_remove_or_add = true` in the `Pushing` arm; the replaced
/// route is disposed a flush early.
#[test]
fn animating_push_defers_disposal_of_the_replaced_route_until_it_completes() {
    let old_log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&Log::default()));
    let (_old, old_result) = history.push(Probe::new(&old_log));

    let mut incoming = Probe::new(&Log::default());
    incoming.push = PushCompletion::Animating(TickerFuture::complete());
    let (new_top, _r) = history.push_replacement(incoming, Some(boxed(11)));

    assert_eq!(history.state_of(new_top), Some(RouteLifecycle::Pushing));
    assert_eq!(history.len(), 3, "the replaced route is still held");
    assert_eq!(
        old_result.try_take(),
        Some(Some(11)),
        "but it already completed"
    );

    // A further flush while the push is still in flight reaches the `Pushing`
    // arm, which — unlike `Idle` — must not license the silent removal beneath
    // it (`navigator.dart:4499-4512`). This is what pins the `Pushing` arm; the
    // flush above never enters it, because the entry is handled by the
    // `PushReplace` arm on the way in.
    history.flush(true);
    settle_unobserved(&mut history);
    assert_eq!(history.len(), 3, "still deferred after a redundant flush");
    assert!(!old_log.lock().contains(&Event::Dispose));

    // Stand in for the continuation `NavigatorShared::apply` would have
    // registered on `new_top`'s (already-resolved) `TickerFuture`: raise the
    // command directly, then settle.
    history
        .command_queue()
        .lock()
        .push_back(RouteCommand::PushCompleted(new_top));
    history.flush(true);
    settle_unobserved(&mut history);

    assert_eq!(history.state_of(new_top), Some(RouteLifecycle::Idle));
    assert_eq!(history.len(), 2, "now the replaced route is disposed");
    assert!(old_log.lock().contains(&Event::Dispose));
}

/// `pushReplacement` reports `didReplace`, **not** `didRemove`
/// (`navigator.dart:3300-3305`, `:3435`). Oracle: `'pushReplacement correctly
/// reports didReplace to the observer'`.
///
/// Red-check: make `arm_complete` always set `report_removal_to_observer = true`.
#[test]
fn push_replacement_reports_did_replace_not_did_remove() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (bottom, _r0) = history.add_initial(Probe::new(&log));
    let (old, _r1) = history.push(Probe::new(&log));
    settle_unobserved(&mut history);

    let spy = spy();
    let (new_top, _r2) = history.push_replacement(Probe::new(&log), None);
    settle(&mut history, &watching(&spy));

    assert!(
        spy.notes()
            .contains(&Note::Replace(Some(new_top), Some(old))),
        "{:?}",
        spy.notes()
    );
    assert!(
        !spy.kinds().contains(&"remove"),
        "a replaced route emits no didRemove"
    );
    assert_eq!(history.ids(), vec![bottom, new_top]);
}

/// `'Can push, pop, and replace in sequence'`.
#[test]
fn push_pop_and_replace_in_sequence() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    let (bottom, _r0) = history.add_initial(Probe::new(&log));

    let (second, second_result) = history.push(Probe::new(&log));
    assert_eq!(history.ids(), vec![bottom, second]);
    assert_eq!(history.current(), Some(second));

    history.pop(Some(boxed(1)));
    assert_eq!(second_result.try_take(), Some(Some(1)));
    assert_eq!(history.ids(), vec![bottom]);

    let (third, _r2) = history.push_replacement(Probe::new(&log), None);
    assert_eq!(history.ids(), vec![third], "the bottom route was replaced");
    assert_eq!(history.current(), Some(third));
}

/// The bottom-most route receives exactly one `did_change_previous(None)` when it
/// first appears, and **no** `did_change_next(None)` from the announcement.
///
/// Both follow from Flutter's `notAnnounced` sentinel (`navigator.dart:3204-3212`),
/// which is distinct from `null`:
///
/// - `previous` is `null`, and `null != notAnnounced`, so `didChangePrevious(null)`
///   fires (`:4657-4664`).
/// - `next` is also `null` and also `!= notAnnounced`, so the outer `if` is entered
///   — but `shouldAnnounceChangeToNext` returns `false`, because
///   `lastAnnouncedPoppedNextRoute` and `lastAnnouncedNextRoute` are *both* still
///   the sentinel (`:3541-3546`). `handle_push` already sent `didChangeNext(null)`
///   for a new-first route, so a second one would be redundant.
///
/// **Found by a parity re-check.** FLUI initialised these fields to
/// `None`, so `None != None` was false and the bottom route's
/// `did_change_previous(None)` was silently never sent. `SimpleRoute`'s no-op
/// default masked it; `ModalRoute` drives `changedInternalState()` from
/// `didChangePrevious` and would have missed its initial init.
///
/// Red-check: change `Announced` back to `Option<RouteId>` (i.e. seed the fields
/// with `None` rather than `Announced::Never`); the `DidChangePrevious(None)`
/// disappears.
#[test]
fn bottom_route_receives_its_initial_did_change_previous() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let events = log.lock().clone();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::DidChangePrevious(_)))
            .collect::<Vec<_>>(),
        vec![&Event::DidChangePrevious(None)],
        "exactly one didChangePrevious(null), on the bottom route: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::DidChangeNext(_)))
            .count(),
        1,
        "only handle_add's didChangeNext(null); the announcement suppresses a second"
    );
}

// ============================================================================
// 10. RE-ENTRANCY
// ============================================================================

/// A **directly recursive** `flush` is still forbidden and still loud
/// (`navigator.dart:4452-4453`). This is framework misuse, so `PANIC-POLICY`
/// permits the panic.
///
/// The route-binding command queue did **not** relax this. What changed is that
/// route callbacks no longer reach `flush` at all: `RouteBinding` enqueues a
/// `RouteCommand`, and the running flush drains it (see
/// `route_binding_finalize_during_flush_is_deferred`). So this assert now guards
/// only a genuine recursive call, and it is still tested directly — a `Route`
/// hook receives `&mut self` and cannot reach the history, exactly as the route
/// stack enforces.
///
/// Red-check: delete the `assert!` in `RouteHistory::flush`.
#[test]
#[should_panic(expected = "BUG: flush_history_updates re-entered")]
fn reentrant_flush_panics_with_bug() {
    let mut history = RouteHistory::new();
    history.force_flushing_for_test();
    history.flush(true);
}

// ============================================================================
// 12. THE ROUTE-ANIMATION SEAM
// ============================================================================

/// A route that raises a `RouteCommand` from one of its lifecycle callbacks —
/// the shape of a zero-duration `TransitionRoute`.
///
/// `did_push` no longer has any seam of its own to raise `PushCompleted`
/// through — that command is now raised only by the continuation
/// `NavigatorShared::apply` registers on the future `did_push` hands out
/// (ADR-0064), never by a route calling back into its own binding. So an
/// `Animating` push here — zero-duration or not — parks in `Pushing` until
/// the test raises `PushCompleted` itself, standing in for that continuation.
struct SeamRoute {
    settings: RouteSettings,
    binding: Option<RouteBinding>,
    /// Raised from `did_pop`, i.e. inside the flush that pops it — Flutter's
    /// `OverlayRoute.didPop` → `navigator.finalizeRoute` (`routes.dart:87-94`).
    finalize_on_pop: bool,
    push: PushCompletion,
    finished_when_popped: bool,
}

impl SeamRoute {
    fn new() -> Self {
        Self {
            settings: RouteSettings::default(),
            binding: None,
            finalize_on_pop: false,
            push: PushCompletion::Immediate,
            finished_when_popped: true,
        }
    }

    /// A zero-duration transition: the future `did_push` hands out is already
    /// resolved by the time it returns.
    fn zero_duration_push(mut self) -> Self {
        self.push = PushCompletion::Animating(TickerFuture::complete());
        self
    }

    /// An exit transition that finishes synchronously inside `did_pop`.
    fn finalizing_on_pop(mut self) -> Self {
        self.finalize_on_pop = true;
        self.finished_when_popped = false;
        self
    }
}

impl Route for SeamRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn finished_when_popped(&self) -> bool {
        self.finished_when_popped
    }

    fn did_push(&mut self) -> PushCompletion {
        self.push.clone()
    }

    fn did_pop(&mut self) -> bool {
        if self.finalize_on_pop
            && let Some(binding) = &self.binding
        {
            binding.finalize();
        }
        true
    }
}

/// Drive a history's command queue without a navigator: the test's stand-in for
/// `NavigatorShared::pump_route_commands`'s `wake`.
///
/// Deliberately a **no-op**: the whole point of the command queue is that a command raised
/// during a flush is drained by that flush, so `wake` has nothing to do. Commands
/// raised *outside* a flush are applied by the explicit `flush` the test drives.
fn inert_wake() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

fn binding_for(history: &RouteHistory, id: RouteId) -> RouteBinding {
    RouteBinding::new(
        id,
        history.command_queue(),
        inert_wake(),
        Arc::new(Mutex::new(None)),
        super::binding::RouteRegistries {
            peers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            entries: Arc::new(Mutex::new(std::collections::HashMap::new())),
            subtrees: Arc::new(Mutex::new(std::collections::HashMap::new())),
            modals: Arc::new(Mutex::new(std::collections::HashMap::new())),
            pop_pacing: Arc::new(Mutex::new(std::collections::HashMap::new())),
        },
    )
}

#[test]
fn route_binding_wake_accepts_owner_local_rc_state() {
    let history = RouteHistory::new();
    let route = RouteId::next();
    let wake_calls = Rc::new(Cell::new(0));
    let wake_calls_for_binding = Rc::clone(&wake_calls);
    let binding = RouteBinding::new(
        route,
        history.command_queue(),
        Rc::new(move || wake_calls_for_binding.set(wake_calls_for_binding.get() + 1)),
        Arc::new(Mutex::new(None)),
        super::binding::RouteRegistries {
            peers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            entries: Arc::new(Mutex::new(std::collections::HashMap::new())),
            subtrees: Arc::new(Mutex::new(std::collections::HashMap::new())),
            modals: Arc::new(Mutex::new(std::collections::HashMap::new())),
            pop_pacing: Arc::new(Mutex::new(std::collections::HashMap::new())),
        },
    );

    binding.finalize();

    assert_eq!(
        wake_calls.get(),
        1,
        "RouteBinding wake is an owner-local callback"
    );
}

#[test]
fn completed_signal_accepts_owner_local_rc_state() {
    let completed = CompletedSignal::default();
    let calls = Rc::new(Cell::new(0));
    let calls_for_callback = Rc::clone(&calls);
    completed.on_completed(Rc::new(move || {
        calls_for_callback.set(calls_for_callback.get() + 1);
    }));

    completed.complete();

    assert_eq!(calls.get(), 1, "completed callbacks are owner-local");
}

/// A route raising `finalize()` from `did_pop` — i.e. **inside** the flush that
/// pops it — must not re-enter `flush`. Flutter's `finalizeRoute` handles this
/// with `if (!_flushingHistory)` (`navigator.dart:5825-5828`); FLUI enqueues a
/// `RouteCommand` and the running flush drains it, costing one extra pass.
///
/// Before the command queue existed this shape was structurally unreachable. It is the reason the
/// `BUG:` assert existed.
///
/// Red-check: make `RouteBinding::finalize` call `RouteHistory::finalize_route`
/// directly — it deadlocks on the history mutex (a hang, not a panic), which is
/// exactly why the queue exists.
#[test]
fn route_binding_finalize_during_flush_is_deferred_not_reentrant() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let id = RouteId::next();
    let mut route = SeamRoute::new().finalizing_on_pop();
    route.binding = Some(binding_for(&history, id));
    let (top, result) = history.push_with_id(id, route);
    assert_eq!(history.len(), 2);

    // `did_pop` raises `finalize()` mid-flush. No panic, no hang.
    assert!(history.pop(Some(boxed(5))));

    assert_eq!(
        history.state_of(top),
        None,
        "the finalized route was disposed and dropped"
    );
    assert_eq!(history.len(), 1);
    assert_eq!(result.try_take(), Some(Some(5)), "and it still completed");
    assert_eq!(
        history.last_flush_passes(),
        2,
        "one walk, then one settling pass for the deferred command"
    );
    assert!(!history.has_pending_commands());
}

/// A zero-duration entrance transition — a push whose `TickerFuture` is
/// already resolved by the time `did_push` hands it out — still parks in
/// `Pushing` at this layer, and stays there until an explicit
/// `RouteCommand::PushCompleted` arrives.
///
/// Flutter never sees a zero-duration push settle *before* `handlePush`
/// returns either — `whenCompleteOrCancel` asserts `!_debugLocked` and always
/// arrives on a later microtask (`navigator.dart:3277-3279`). FLUI reaches
/// the same end state through a different route: a route's own `did_push`
/// has no seam to raise `PushCompleted` for itself (`RouteBinding` has no
/// such method — ADR-0064); the continuation that settles a push is
/// registered one layer up, in `NavigatorShared::apply`, which this
/// pure-history layer has no equivalent of. So the command has to be raised
/// from outside — exactly as it would be raised by that continuation in
/// production — and it costs a flush of its own, not a second pass of the
/// same one.
///
/// Red-check: give `SeamRoute::did_push` back a `RouteBinding` seam that
/// raises `PushCompleted` for itself; the entry would then be `Idle` with no
/// explicit command needed at all.
#[test]
fn a_zero_duration_push_still_needs_an_explicit_command_to_settle() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let id = RouteId::next();
    let route = SeamRoute::new().zero_duration_push();
    let (top, _result) = history.push_with_id(id, route);

    assert_eq!(
        history.state_of(top),
        Some(RouteLifecycle::Pushing),
        "an already-resolved future settles nothing on its own"
    );
    assert_eq!(
        history.last_flush_passes(),
        1,
        "nothing was raised to defer"
    );

    history
        .command_queue()
        .lock()
        .push_back(RouteCommand::PushCompleted(top));
    history.flush(true);

    assert_eq!(history.state_of(top), Some(RouteLifecycle::Idle));
    assert!(!history.has_pending_commands());
}

/// A zero-duration push **and** a synchronous pop, end to end: the lifecycle and
/// the overlay outcome must both settle. `FlushOutcome` accumulates — across the
/// passes of one `flush`, and across flushes the caller has not yet settled — so
/// nothing a flush decided can be lost by batching.
///
/// That second half is what `last_outcome`'s `absorb` buys, and it is not a nicety:
/// an outcome owns the dying routes and the notifications, so overwriting it would
/// silently skip a `Route::dispose` and swallow a `did_pop`.
///
/// Red-check (each half fails on its own):
/// * drop `FlushOutcome::absorb`'s `disposed.extend` — the route disposed on the
///   *second* pass never reaches the caller, and its overlay entry leaks;
/// * drop its `notifications.extend` — the observations of every flush but the last
///   vanish.
#[test]
fn zero_duration_push_then_pop_settles_lifecycle_and_overlay_outcome() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let id = RouteId::next();
    let mut route = SeamRoute::new().zero_duration_push().finalizing_on_pop();
    route.binding = Some(binding_for(&history, id));
    let (top, result) = history.push_with_id(id, route);
    // Parks in `Pushing`, not `Idle`: nothing at this layer drains the
    // `PushCompleted` command a zero-duration push still needs (see
    // `a_zero_duration_push_still_needs_an_explicit_command_to_settle`). The
    // pop below runs on a `Pushing` entry exactly as it would on an `Idle`
    // one — `handle_pop` treats every present state alike.
    assert_eq!(history.state_of(top), Some(RouteLifecycle::Pushing));

    history.pop(None);
    let outcome = history.take_outcome().expect("the pop flushed");

    assert!(
        outcome.disposed.contains(&top),
        "the deferred disposal must reach the caller: {:?}",
        outcome.disposed
    );

    // Three un-settled flushes — `add_initial`, `push_with_id`, `pop` — folded into
    // one outcome, in the order they happened.
    let bottom = history.ids()[0];
    assert_eq!(
        outcome.notifications,
        vec![
            Notification::Observed(Observation::Push {
                route: bottom,
                previous: None
            }),
            Notification::TopChanged {
                top: bottom,
                previous_top: None
            },
            Notification::Observed(Observation::Push {
                route: top,
                previous: Some(bottom)
            }),
            Notification::TopChanged {
                top,
                previous_top: Some(bottom)
            },
            Notification::Observed(Observation::Pop {
                route: top,
                previous: Some(bottom)
            }),
            Notification::TopChanged {
                top: bottom,
                previous_top: Some(top)
            },
        ],
        "every flush's observations survive to the caller"
    );

    assert_eq!(history.len(), 1);
    assert!(result.is_completed());
    assert!(!history.has_pending_commands());
}

/// Commands raised **between** flushes (an animation status listener) are
/// applied at the head of the next flush, before the walk sees the history.
///
/// Red-check: delete the `self.apply_pending_commands()` call before the first
/// `flush_once`; the entry is still `Pushing` when the walk reads it, so
/// `can_remove_or_add` stays false.
#[test]
fn commands_raised_between_flushes_apply_at_the_head_of_the_next_one() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));

    let id = RouteId::next();
    let mut animating = Probe::new(&log);
    animating.push = PushCompletion::Animating(TickerFuture::complete());
    let (top, _result) = history.push_with_id(id, animating);
    assert_eq!(history.state_of(top), Some(RouteLifecycle::Pushing));
    assert_eq!(history.last_flush_passes(), 1, "nothing was deferred");

    // Raise it out-of-flush, as the push-completion continuation would.
    history
        .command_queue()
        .lock()
        .push_back(RouteCommand::PushCompleted(top));
    assert!(history.has_pending_commands());

    history.flush(true);

    assert_eq!(history.state_of(top), Some(RouteLifecycle::Idle));
    assert_eq!(history.last_flush_passes(), 1, "applied before the walk");
    assert!(!history.has_pending_commands());
}

/// A command naming a route that has already been disposed and dropped is
/// discarded, not a panic. A `RouteBinding` outlives its route.
///
/// Red-check: `expect()` the entry lookup in `apply_pending_commands`.
#[test]
fn a_command_for_a_vanished_route_is_dropped() {
    let log: Log = Log::default();
    let mut history = RouteHistory::new();
    history.add_initial(Probe::new(&log));
    let (top, _result) = history.push(Probe::new(&log));

    let stale = binding_for(&history, top);
    history.pop(None);
    assert_eq!(history.len(), 1);

    stale.finalize();
    // Stands in for the continuation `NavigatorShared::apply` would raise
    // this through in production — `RouteBinding` itself has no
    // `notify_push_completed` seam any more (ADR-0064).
    history
        .command_queue()
        .lock()
        .push_back(RouteCommand::PushCompleted(top));
    history.flush(true);

    assert_eq!(history.len(), 1, "the stale commands changed nothing");
    assert!(!history.has_pending_commands());
}

/// `RouteCommand` is a plain value: the queue is data, not a callback into the
/// navigator. This is what keeps `route_stack_flush_is_pure_data` true.
///
/// `Finalize` is the only command `RouteBinding` can still raise —
/// `PushCompleted` is now raised directly by `NavigatorShared::await_push`'s
/// continuation, never through a binding (ADR-0064) — but the property this
/// test pins (a binding can only ever name its own route) is unaffected.
#[test]
fn route_commands_are_pre_bound_to_one_route() {
    let history = RouteHistory::new();
    let id = RouteId::next();
    let binding = binding_for(&history, id);

    assert_eq!(binding.route_id(), id);
    binding.finalize();

    let queued: Vec<RouteCommand> = history.command_queue().lock().iter().copied().collect();
    assert_eq!(
        queued,
        vec![RouteCommand::Finalize(id)],
        "a binding can only ever name its own route"
    );
}

// ============================================================================
// 11. PURITY
// ============================================================================

/// This layer is pure data: it must not reach the element tree, the build owner,
/// the render pipeline, or the overlay. ADR-0019's whole sequencing argument
/// depends on it, so check the sources rather than trusting prose.
///
/// # Why `observer.rs` is judged separately
///
/// `NavigatorObserver::did_attach` hands an observer an owned `NavigatorHandle`,
/// so `observer.rs` names `super::navigator` — a module that *does* touch the
/// widget tree. That is the seam, and it is deliberate: Flutter's
/// `NavigatorObserver.navigator` getter is the same edge (`navigator.dart:779`).
/// What must stay true is that observer.rs itself reaches **nothing** in the
/// framework directly, and that the four genuinely pure files never acquire an
/// edge to the navigator, the overlay, the scheduler, or either tree — which is
/// what `PURE_DATA` now forbids by name, and did not before.
///
/// Red-check: add `use crate::overlay::OverlayEntry;` to `history.rs`,
/// `use super::navigator::NavigatorHandle;` to `route.rs`, or give `RouteHistory`
/// back its `observers: Vec<Arc<dyn NavigatorObserver>>` field.
///
/// # `flui_scheduler::TickerFuture`/`TickerCanceled` are not a scheduler edge
///
/// `PushCompletion::Animating` (`route.rs`) and `DeferredEffect::AwaitPush`
/// (`history.rs`) both carry a `TickerFuture` — a plain, inert value a route
/// hands in from *outside* this layer (ADR-0064), never fetched by it. Holding
/// one is exactly as pure as holding the `RouteResult`/`Completer` values
/// `result.rs` already carries. `NAVIGATOR_EDGE` therefore bans the
/// capability tokens a genuine scheduler dependency would need
/// (`UpdateScheduler`, `TickerProvider`, `PostFrameHandle`, `Vsync`, the bare
/// `Ticker` type) rather than the crate path itself — and separately bans
/// `when_complete_or_cancel`, `TickerCompleter`, `TickerDelivery`, and
/// `TickerFuture::pending`, which encode the placement rule this layer must
/// not implement: registering a continuation, or minting a completer/future
/// pair, are both `NavigatorShared::apply`'s job (ADR-0064), never this
/// layer's — it only ever receives an already-pending `TickerFuture` a route
/// handed it and carries it out through `DeferredEffect::AwaitPush`.
#[test]
fn route_stack_flush_is_pure_data() {
    /// Tokens that would mean this layer had grown a dependency on the framework.
    const FRAMEWORK: [&str; 8] = [
        "ElementTree",
        "BuildOwner",
        "PipelineOwner",
        "BuildContext",
        "RebuildHandle",
        "flui_view",
        "flui_rendering",
        "crate::overlay",
    ];
    /// …and, for the four files that are pure *data*, anything that reaches the
    /// navigator, an id minted by either tree, a scheduler *capability* (as
    /// opposed to the inert `TickerFuture`/`TickerCanceled` values — see this
    /// test's own doc), or the placement rule ADR-0064 reserves for
    /// `NavigatorShared::apply`: registering a continuation
    /// (`when_complete_or_cancel`) or minting a completer/future pair
    /// (`TickerCompleter`, `TickerDelivery`, `TickerFuture::pending`).
    ///
    /// `NavigatorObserver` joined this list because an observer holds a
    /// `NavigatorHandle`, so a `RouteHistory` that could *call* one could deadlock
    /// on its own mutex. It now computes `Notification`s and hands them to the
    /// navigator, which delivers them with the lock released.
    const NAVIGATOR_EDGE: [&str; 13] = [
        "super::navigator",
        "NavigatorHandle",
        "NavigatorObserver",
        "OverlayHandle",
        "UpdateScheduler",
        "TickerProvider",
        "PostFrameHandle",
        "SubtreeAnchor",
        "RouteSubtree",
        "Vsync",
        "when_complete_or_cancel",
        "TickerCompleter",
        "TickerDelivery",
    ];
    /// `TickerFuture::pending` names the *minting* call this layer must never
    /// make; a plain substring ban on `"TickerFuture"` would also catch the
    /// type name itself, which this layer legitimately carries as a value.
    const TICKER_FUTURE_PENDING: &str = "TickerFuture::pending";
    /// The bare `Ticker` type — `str::contains` alone would also match the
    /// two identifiers this guard deliberately allows (`TickerFuture`,
    /// `TickerCanceled`), since `"Ticker"` is their prefix.
    fn contains_bare_ticker(code: &str) -> bool {
        const ALLOWED_SUFFIXES: [&str; 2] = ["Future", "Canceled"];
        code.match_indices("Ticker").any(|(index, _)| {
            let after = &code[index + "Ticker".len()..];
            !ALLOWED_SUFFIXES
                .iter()
                .any(|suffix| after.starts_with(suffix))
        })
    }

    /// Line comments are prose: `history.rs` may *name* `NavigatorHandle` while
    /// explaining what it deliberately does not do. A dependency is an import or a
    /// path in code, so strip `//`, `///` and `//!` lines before scanning.
    fn code_only(source: &str) -> String {
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    const PURE_DATA: [(&str, &str); 4] = [
        ("history.rs", include_str!("history.rs")),
        ("route.rs", include_str!("route.rs")),
        ("lifecycle.rs", include_str!("lifecycle.rs")),
        ("result.rs", include_str!("result.rs")),
    ];
    /// Holds one sanctioned edge — the `NavigatorHandle` it hands to `did_attach` —
    /// and no framework dependency of its own.
    const OBSERVER: (&str, &str) = ("observer.rs", include_str!("observer.rs"));

    for (name, source) in PURE_DATA {
        let code = code_only(source);
        for token in FRAMEWORK.iter().chain(&NAVIGATOR_EDGE) {
            assert!(
                !code.contains(token),
                "{name} references `{token}`: the route stack must stay pure data"
            );
        }
        assert!(
            !code.contains(TICKER_FUTURE_PENDING),
            "{name} references `{TICKER_FUTURE_PENDING}`: minting a completer/future \
             pair is `NavigatorShared::apply`'s job, never this layer's"
        );
        assert!(
            !contains_bare_ticker(&code),
            "{name} references the bare `Ticker` type: this layer only ever carries a \
             `TickerFuture` a route already produced"
        );
    }

    let (name, source) = OBSERVER;
    let code = code_only(source);
    for token in FRAMEWORK {
        assert!(
            !code.contains(token),
            "{name} references `{token}`: an observer's only edge is the \
             `NavigatorHandle` it is handed"
        );
    }
}

/// Two invariants of the push-completion seam, checked together since both
/// are "no source file outside the sanctioned ones may spell this out" scans
/// over the same directory (ADR-0064):
///
/// 1. `RouteBinding::notify_push_completed` is deleted — nothing under
///    `src/navigator/` should still *define or call* it. Historical prose
///    naming it (this ADR's own paper trail, in `binding.rs`'s module doc and
///    elsewhere) is not what this pins; only a live definition or call site
///    is.
/// 2. "No route can raise `PushCompleted` for itself" — an identifier-absence
///    grep alone is defeated by a rename, so this also bans *constructing*
///    `RouteCommand::PushCompleted(` outside the sanctioned sites:
///    `binding.rs` (declares the variant), `history.rs` (drains it), and
///    `navigator.rs` (the continuation raises it). `tests.rs` is a fourth,
///    test-only exemption: several pure-`RouteHistory` tests construct it
///    directly to stand in for the continuation `NavigatorShared::apply`
///    would raise in production — that layer has no `NavigatorShared` to
///    drive it any other way.
///
/// Comment lines are stripped before scanning (the same discipline
/// `route_stack_flush_is_pure_data` uses), and every needle is built from
/// concatenated halves so this file's own source — which necessarily spells
/// them out, right here — never trips its own check.
///
/// Red-check: reintroduce `notify_push_completed` (even dead code, even
/// `#[cfg(test)]`) or a call to it anywhere under `src/navigator/`; or have
/// some route's lifecycle hook construct `RouteCommand::PushCompleted`
/// directly instead of returning a `TickerFuture` for the navigator to await.
#[test]
fn no_route_can_report_its_own_push_completion() {
    fn code_only(source: &str) -> String {
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    let notify_fn_needle = format!("fn {}", "notify_push_completed");
    let notify_call_needle = format!(".{}(", "notify_push_completed");
    let push_completed_construction_needle = format!("RouteCommand::{}(", "PushCompleted");
    /// Files allowed to construct `RouteCommand::PushCompleted` — the three
    /// production sites, plus the pure-data layer's own test suite (see this
    /// test's own doc, point 2).
    const PUSH_COMPLETED_CONSTRUCTION_EXEMPT: [&str; 4] =
        ["binding.rs", "navigator.rs", "history.rs", "tests.rs"];

    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/navigator"));
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(dir).expect("src/navigator must exist") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("navigator source files have utf-8 names")
            .to_owned();
        let source = std::fs::read_to_string(&path).expect("readable source file");
        let code = code_only(&source);

        if code.contains(&notify_fn_needle) || code.contains(&notify_call_needle) {
            offenders.push(format!(
                "{file_name}: notify_push_completed still defined or called"
            ));
        }
        if !PUSH_COMPLETED_CONSTRUCTION_EXEMPT.contains(&file_name.as_str())
            && code.contains(&push_completed_construction_needle)
        {
            offenders.push(format!(
                "{file_name}: constructs RouteCommand::PushCompleted directly"
            ));
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}
