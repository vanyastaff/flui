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

use std::{rc::Rc, sync::Arc};

use flui_scheduler::TickerFuture;
use parking_lot::Mutex;

use super::binding::{RouteBinding, RouteCommand};
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

// ============================================================================
// 7. NEIGHBOUR ANNOUNCEMENTS
// ============================================================================

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
/// `observers_are_notified_before_a_dying_routes_overlay_entry_is_torn_down`
/// (`crates/flui-widgets/tests/hero_seam.rs`) pins `apply` itself.
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

    order.lock().clear();
    history.pop(None);
    settle(&mut history, &observers);

    assert_eq!(
        *order.lock(),
        vec!["observer:pop", "dispose"],
        "observers are notified before the dying route is disposed"
    );
}

// ============================================================================
// PUSHING / RouteCommand::PushCompleted
// ============================================================================

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

// ============================================================================
// 11. PURITY
// ============================================================================
