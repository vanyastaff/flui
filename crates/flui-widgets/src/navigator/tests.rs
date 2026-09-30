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
//! no build owner, no render pipeline, no overlay.

use std::{rc::Rc, sync::Arc};

use parking_lot::Mutex;

use super::binding::RouteBinding;
use super::history::RouteHistory;
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

/// An erased pop result, as `RouteHistory::pop` takes one.
fn boxed(value: i32) -> super::route::AnyResult {
    super::route::AnyResult::new(value)
}

// ============================================================================
// 1. LIFECYCLE ORDER
// ============================================================================

// ============================================================================
// 2-3. PUSH / OBSERVER ADDITIONS
// ============================================================================

// ============================================================================
// 3-5. RESULT CHANNEL
// ============================================================================

/// The completer fires exactly once. Dart's `Completer.complete` throws on the
/// second call; `_RouteEntry.complete`'s `>= remove` early-return
/// (`navigator.dart:3431`) is what stops it being reached.
///
/// Red-check: delete that early-return in `RouteEntry::arm_complete`; the second
/// `remove_route` re-arms a disposed-or-removing entry.
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

// ============================================================================
// 6. DELETIONS ARE FIFO
// ============================================================================

// ============================================================================
// 7. NEIGHBOUR ANNOUNCEMENTS
// ============================================================================

// ============================================================================
// 8. DISPOSAL TIMING
// ============================================================================

// ============================================================================
// PUSHING / RouteCommand::PushCompleted
// ============================================================================

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
fn reentrant_flush_panics_with_bug() {
    let mut history = RouteHistory::new();
    history.force_flushing_for_test();
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| history.flush(true)))
        .expect_err("a re-entered flush must panic");
    let message = refused
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| refused.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("BUG: flush_history_updates re-entered"),
        "reentrant_flush_panics_with_bug: unexpected diagnostic {message:?}"
    );
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

// ============================================================================
// 11. PURITY
// ============================================================================

/// History failure paths: double completion, re-entered flush, deferred finalization and
/// registrations that re-enter the registry from `Drop`. Each row runs to completion and a
/// failing row is named.
#[test]
fn history_reentrancy_and_completion_contracts() {
    let rows: [(&str, fn()); 4] = [
        (
            "double_pop_or_double_remove_does_not_double_complete",
            double_pop_or_double_remove_does_not_double_complete,
        ),
        ("reentrant_flush_panics_with_bug", reentrant_flush_panics_with_bug),
        (
            "route_binding_finalize_during_flush_is_deferred_not_reentrant",
            route_binding_finalize_during_flush_is_deferred_not_reentrant,
        ),
        (
            "a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry",
            super::navigator_tests::a_registration_dropped_while_replacing_or_clearing_may_re_enter_the_registry,
        ),
    ];
    let mut failures = Vec::new();
    for (name, row) in rows {
        if std::panic::catch_unwind(row).is_err() {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "history rows failed: {failures:?}");
}
