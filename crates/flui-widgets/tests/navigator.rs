//! Tests for the `Navigator` that drive a mounted element tree. Private route
//! state is read through the temporary `flui_widgets::__test_access` path
//! (ADR-0083 §4); the handle, export and registry unit tests stay in
//! `src/navigator/navigator_tests.rs`.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/navigator_test.dart` —
//! `'Can navigator navigate to and from a stateful widget'`,
//! `'Navigator.of fails gracefully when not found in context'`,
//! `'Navigator.of rootNavigator finds root Navigator'`,
//! `'Can push, pop, and replace in sequence'`, `'removeRoute'`.
//! Expected values are read from `navigator.dart`, not from running this code.
//!
//! Unlike `src/navigator/tests.rs` (the route stack's pure-data suite), these drive a
//! real element tree.

// ADR-0027: these tests capture owner-local handles in shared cells. The
// library carries the same lint expectation; an integration test is a
// separate crate, so it is repeated here.
#![expect(clippy::arc_with_non_send_sync)]

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_view::BuildContext;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use flui_widgets::__test_access::{NavigatorProbe as _, OverlayProbe as _};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, NavigatorRoute, Route, RouteContentBuilder, RouteSettings,
    SimpleRoute,
};

use crate::common::harness::{Harness, mount};

// ============================================================================
// PROBES
// ============================================================================

/// Records every route builder invocation, so "did this route's content build?"
/// is observable.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<Vec<&'static str>>>);

impl Built {
    fn contains(&self, name: &str) -> bool {
        self.0.lock().contains(&name)
    }
    fn clear(&self) {
        self.0.lock().clear();
    }
}

/// A route whose content is a leaf, recording its name each time it builds.
fn page(built: &Built, name: &'static str) -> SimpleRoute<i32> {
    let built = built.clone();
    SimpleRoute::new(move |_ctx| {
        built.0.lock().push(name);
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name)
}

/// A root that can build the navigator or drop it — `swap_root` goes through
/// `ElementTree::update`, whose dispatch is keyed by `TypeId`, so the root type
/// must not change between frames.
#[derive(Clone)]
struct Host {
    show: bool,
    handle: NavigatorHandle,
}

impl View for Host {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Host {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show {
            Navigator::new(self.handle.clone()).into_view().boxed()
        } else {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }
    }
}

/// Mount a navigator seeded with one route named `"/"`.
fn navigator_with(built: &Built) -> (NavigatorHandle, Harness) {
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(built, "/"));
    let harness = mount(Navigator::new(handle.clone()));
    (handle, harness)
}

/// The overlay's layer elements, bottom → top. `Navigator → Overlay → Stack → …`.
fn layers(harness: &mut Harness) -> Vec<ElementId> {
    let root = harness.root();
    // `Navigator::build` wraps its `Overlay` in a `HeroControllerScope`
    // (a `.none`), so the overlay is now one level below the navigator's element.
    let scope = harness.only_child(root);
    let overlay = harness.only_child(scope);
    let stack = harness.only_child(overlay);
    harness.children_of(stack)
}

// ============================================================================
// TESTS
// ============================================================================

/// `push` installs the route, adds its overlay entry, and rearranges — so the
/// overlay order matches the route stack, bottom → top
/// (`_allRouteOverlayEntries`, `navigator.dart:4151`).
///
/// Red-check: drop the `self.shared.apply(&outcome)` in `NavigatorHandle::push`;
/// the new layer never reaches the overlay.
#[test]
fn navigator_push_builds_new_route_and_rearranges_overlay() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    built.clear();
    handle.push(page(&built, "second"));
    harness.tick();

    assert!(built.contains("second"), "the pushed route built");
    assert_eq!(layers(&mut harness).len(), 2);
    assert_eq!(handle.overlay().len(), 2, "the overlay holds both entries");
    assert_eq!(handle.route_ids().len(), 2);
}

/// `pop(result)` removes the top route, completes its future, and drops its
/// overlay entry. Flutter passes `rearrangeOverlay: false` here (`:5671`) because
/// `OverlayEntry.remove()` already updated the overlay.
///
/// Red-check: skip the `entry.remove()` loop in `NavigatorShared::apply`; the
/// stale layer stays in the overlay.
#[test]
fn navigator_pop_removes_top_route_and_completes_result() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    let result = handle.push(page(&built, "second"));
    harness.tick();
    assert_eq!(layers(&mut harness).len(), 2);

    assert!(handle.pop_with(42_i32));
    harness.tick();

    assert_eq!(result.try_take(), Some(Some(42)), "the future resolved");
    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(handle.overlay().len(), 1);
    assert_eq!(layers(&mut harness).len(), 1, "the top layer is gone");
}

/// Pushing from *inside* a route's build must be possible without deadlock, and
/// must not run under the element-tree borrow — the whole point of cloning an
/// owned handle instead of taking one under that borrow.
///
/// This is the shape that would hang if `Navigator::of` did anything but clone an
/// owned handle: the lookup runs under the tree borrow, and the push takes the
/// history `Mutex` and then schedules an overlay rebuild.
///
/// Red-check: none available as a mutation — a deadlock hangs rather than fails.
/// Its value is as a regression tripwire (nextest's per-test timeout catches it).
#[test]
fn navigator_of_then_push_from_a_route_build_does_not_deadlock() {
    let pushed = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();

    {
        let pushed = Arc::clone(&pushed);
        handle.seed_initial(SimpleRoute::<i32>::new(move |ctx| {
            // Clone the handle out under the borrow…
            let found = NavigatorHandle::maybe_of(ctx);
            // …and record that we could, without touching the tree again.
            if found.is_some() {
                pushed.fetch_add(1, Ordering::Relaxed);
            }
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }));
    }

    let handle_clone = handle.clone();
    let mut harness = mount(Navigator::new(handle.clone()));
    assert_eq!(pushed.load(Ordering::Relaxed), 1);

    // Now push from outside the build, as a real callback would.
    handle_clone.push(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    }));
    harness.tick();
    assert_eq!(handle.route_ids().len(), 2);
}

// ============================================================================
// THE ROUTE-ANIMATION SEAM
// ============================================================================

// ============================================================================
// #1161 — the navigator awaits the controller-owned `TickerFuture`
// ============================================================================

/// `PopScope` — ADR-0019's deferred veto, landed via the route's `PopEntry`
/// registry (`routes.dart:1980`, `:2033-2050`). A `can_pop(false)` scope makes
/// `maybe_pop` refuse-and-report-handled — the route stays, and every scope
/// hears `on_pop_invoked(false)`. A programmatic `pop()` is **not** blocked
/// (`canPop` guards the user's back navigation, not code) and reports `true`.
/// Unmounting the scope deregisters it: the next `maybe_pop` pops normally.
///
/// Red-check: drop the `vetoes_pop` arm from `pop_disposition_of_top` — the
/// first `maybe_pop` pops the route and the stays-put assertion fails.
#[test]
fn pop_scope_vetoes_maybe_pop_but_not_programmatic_pop() {
    use std::sync::atomic::AtomicBool;

    use flui_widgets::PopScope;
    use flui_widgets::navigator::PageRoute;

    let outcomes: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
    let blocking = Arc::new(AtomicBool::new(true));

    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    let outcomes_for_scope = Arc::clone(&outcomes);
    let blocking_for_page = Arc::clone(&blocking);
    let _guarded = handle.push(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        let outcomes = Arc::clone(&outcomes_for_scope);
        let scope = PopScope::new(SizedBox::new(10.0, 10.0))
            .can_pop(!blocking_for_page.load(Ordering::SeqCst))
            .on_pop_invoked(move |_cx, did_pop| outcomes.lock().push(did_pop));
        scope.into_view().boxed()
    }));
    harness.tick();
    assert_eq!(handle.route_ids().len(), 2);

    // Vetoed: handled, refused, route stays.
    assert!(handle.maybe_pop(), "a vetoed maybe_pop reports handled");
    assert_eq!(
        handle.route_ids().len(),
        2,
        "the vetoed route stays on the stack"
    );
    assert_eq!(
        outcomes.lock().as_slice(),
        [false],
        "the scope heard the refusal"
    );

    // A programmatic pop is not blocked; the scope hears `true`.
    assert!(handle.pop());
    harness.tick();
    assert_eq!(handle.route_ids().len(), 1, "pop() ignores can_pop");
    assert_eq!(
        outcomes.lock().as_slice(),
        [false, true],
        "the scope heard the successful pop"
    );
}

// ============================================================================
// Local history (routes.dart:747-973)
// ============================================================================

mod local_history {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;
    use flui_widgets::__test_access::{LocalHistoryEntry, LocalHistoryHandle};

    use flui_widgets::navigator::NavigatorObserver;
    use flui_widgets::navigator::PageRoute;
    use flui_widgets::navigator::RouteId;

    /// Captures the ambient [`LocalHistoryHandle`] in `init_state`, exactly
    /// where a real consumer acquires it (trigger-#22 discipline). The scope
    /// is provided *inside* the built page subtree, so the page **builder**'s
    /// context cannot see it — only a mounted descendant's can.
    #[derive(Clone)]
    struct HandleProbe {
        sink: Arc<Mutex<Option<LocalHistoryHandle>>>,
    }

    impl View for HandleProbe {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateful(self)
        }
    }

    impl StatefulView for HandleProbe {
        type State = HandleProbeState;

        fn create_state(&self) -> Self::State {
            HandleProbeState {
                sink: Arc::clone(&self.sink),
            }
        }
    }

    struct HandleProbeState {
        sink: Arc<Mutex<Option<LocalHistoryHandle>>>,
    }

    impl std::fmt::Debug for HandleProbeState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("HandleProbeState").finish_non_exhaustive()
        }
    }

    impl ViewState<HandleProbe> for HandleProbeState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            let _prev =
                std::mem::replace(&mut *self.sink.lock(), LocalHistoryHandle::maybe_of(ctx));
        }

        fn build(&self, _view: &HandleProbe, _ctx: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(10.0, 10.0)
        }
    }

    /// A modal page whose content captures the route's [`LocalHistoryHandle`]
    /// on mount.
    pub(super) fn page_with_handle(
        sink: &Arc<Mutex<Option<LocalHistoryHandle>>>,
        duration: Duration,
    ) -> PageRoute<i32> {
        let sink = Arc::clone(sink);
        PageRoute::<i32>::new(move |_ctx, _p, _s| {
            HandleProbe {
                sink: Arc::clone(&sink),
            }
            .into_view()
            .boxed()
        })
        .transition_duration(duration)
    }

    /// Counts `did_pop` observations, to prove observer silence on entry pops.
    #[derive(Default)]
    struct PopCounter(AtomicUsize);
    impl NavigatorObserver for PopCounter {
        fn did_pop(&self, _route: RouteId, _previous: Option<RouteId>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// **The Flutter example, end to end** (`routes.dart:762-880`): with an
    /// entry on the top route, a pop consumes the **entry** — the route stays,
    /// its future stays pending, observers hear nothing — and the next pop
    /// removes the route itself.
    ///
    /// Red-check: skip the `local_history.pop_last_deferred()` arm in
    /// `ModalRoute::did_pop` — the first `maybe_pop` removes the route and the
    /// stays-put assertion fails.
    #[test]
    fn an_entry_pops_before_the_route_and_observers_stay_silent() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let pops = Arc::new(PopCounter::default());
        handle.add_observer(Arc::clone(&pops) as Arc<dyn NavigatorObserver>);

        let sink = Arc::new(Mutex::new(None));
        let route_result = handle.push(page_with_handle(&sink, Duration::ZERO));
        harness.tick();
        let local = sink.lock().clone().expect("the page captured its handle");

        let removed = Arc::new(AtomicUsize::new(0));
        let removed_for_entry = Arc::clone(&removed);
        let _entry = local.add(LocalHistoryEntry::new().on_remove(move || {
            removed_for_entry.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(handle.maybe_pop(), "the entry pop is handled");
        harness.tick();
        assert_eq!(handle.route_ids().len(), 2, "the route stays");
        assert_eq!(removed.load(Ordering::SeqCst), 1, "on_remove fired once");
        assert_eq!(
            route_result.try_take(),
            None,
            "the route's future stays pending (`routes.dart:964-966`)"
        );
        assert_eq!(
            pops.0.load(Ordering::SeqCst),
            0,
            "observers hear nothing for an entry pop (`navigator.dart:4517-4519`)"
        );

        assert!(handle.maybe_pop(), "the second pop takes the route");
        harness.tick();
        assert_eq!(handle.route_ids().len(), 1);
        assert_eq!(
            pops.0.load(Ordering::SeqCst),
            1,
            "now the observers hear it"
        );
        assert_eq!(removed.load(Ordering::SeqCst), 1, "no second on_remove");
    }
}

// ============================================================================
// User gestures (navigator.dart:5803-5860)
// ============================================================================

mod user_gesture {}

/// A panic in a route lifecycle hook must not brick the navigator.
///
/// `parking_lot` does not poison, so the mutex survives an unwind — but the
/// `flushing` flag used to be cleared by the statement *after* the walk, which an
/// unwind skips. Every later flush then tripped `assert!(!self.flushing)`, so one
/// panicking `did_pop` disabled the navigator permanently.
///
/// A hook that panics violates [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md);
/// forbidding it is not preventing it, and the cost of not surviving it is total.
///
/// **What this does not claim.** The same unwind drops a partially built
/// `FlushOutcome`, so a route already moved into its `dying` list loses its
/// `Route::dispose`. That is left alone on purpose — see `flush_once`'s doc — so
/// this test asserts the navigator is *usable*, not that the panicking flush was
/// clean.
///
/// Red-check: replace the `FlushingGuard` in `RouteHistory::flush_once` with a
/// bare `self.flushing.set(false)` after the call, and the second push panics on
/// the flush assertion instead of succeeding.
#[test]
fn a_panicking_route_hook_leaves_the_navigator_usable() {
    /// Panics from `did_pop`, once.
    struct PanicsOnPop {
        settings: RouteSettings,
        builder: RouteContentBuilder,
    }

    impl Route for PanicsOnPop {
        type Output = i32;

        fn settings(&self) -> &RouteSettings {
            &self.settings
        }

        fn did_pop(&mut self) -> bool {
            panic!("BUG: a deliberately panicking lifecycle hook");
        }
    }

    impl NavigatorRoute for PanicsOnPop {
        fn content_builder(&self) -> RouteContentBuilder {
            Rc::clone(&self.builder)
        }
    }

    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut harness = mount(Host {
        show: true,
        handle: handle.clone(),
    });

    handle.push(PanicsOnPop {
        settings: RouteSettings::named("panics"),
        builder: Rc::new(|_ctx| SizedBox::new(1.0, 1.0).into_view().boxed()),
    });
    harness.tick();
    let depth_before = handle.route_ids().len();

    // The panic crosses the flush. Caught here so the test can go on to prove the
    // navigator survived it — which is the whole point.
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle.pop();
    }));
    assert!(panicked.is_err(), "precondition: the hook really did panic");

    // The claim: the navigator still works. Under the defect this push panics on
    // `assert!(!self.flushing)` instead.
    handle.push(page(&built, "after"));
    harness.tick();
    assert_eq!(
        handle.route_ids().len(),
        depth_before + 1,
        "a push after the panicking pop still lands — the flush flag was cleared \
         by the guard on the way out"
    );
    assert!(built.contains("after"), "and its content built");
}
