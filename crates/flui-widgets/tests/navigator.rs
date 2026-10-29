//! Tests for the `Navigator` that drive a mounted element tree. Private route
//! state is read through the temporary `flui_widgets::__test_access` path
//! (ADR-0083 §4); the handle, export and registry unit tests stay in
//! `src/navigator/navigator_tests.rs`.
//!
//! # Scenarios
//!
//! Navigating to and from a stateful widget, a graceful failure when no
//! navigator is in context, finding the root navigator, push/pop/replace in
//! sequence, and `remove_route`. Expected values are fixed by the documented
//! contract, not by running this code.
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
    let anchor = harness.only_child(overlay);
    let stack = harness.only_child(anchor);
    harness.children_of(stack)
}

// ============================================================================
// TESTS
// ============================================================================

/// `push` installs the route, adds its overlay entry, and rearranges — so the
/// overlay order matches the route stack, bottom → top.
///
/// Red-check: drop the `self.shared.apply(&outcome)` in `NavigatorHandle::push`;
/// the new layer never reaches the overlay.
pub(crate) fn navigator_push_builds_new_route_and_rearranges_overlay() {
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
/// overlay entry. No rearrange is needed here because removing the entry
/// already updated the overlay.
///
/// Red-check: skip the `entry.remove()` loop in `NavigatorShared::apply`; the
/// stale layer stays in the overlay.
pub(crate) fn navigator_pop_removes_top_route_and_completes_result() {
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
pub(crate) fn navigator_of_then_push_from_a_route_build_does_not_deadlock() {
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
// #1161 — the navigator awaits the controller-owned `AnimationRunFuture`
// ============================================================================

/// `PopScope` — ADR-0019's deferred veto, landed via the route's `PopEntry`
/// registry. A `can_pop(false)` scope makes
/// `maybe_pop` refuse-and-report-handled — the route stays, and every scope
/// hears `on_pop_invoked(false)`. A programmatic `pop()` is **not** blocked
/// (`canPop` guards the user's back navigation, not code) and reports `true`.
/// Unmounting the scope deregisters it: the next `maybe_pop` pops normally.
///
/// Red-check: drop the `vetoes_pop` arm from `pop_disposition_of_top` — the
/// first `maybe_pop` pops the route and the stays-put assertion fails.
pub(crate) fn pop_scope_vetoes_maybe_pop_but_not_programmatic_pop() {
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
// Local history
// ============================================================================

pub(crate) mod local_history {
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

    /// **The local-history flow, end to end**: with an
    /// entry on the top route, a pop consumes the **entry** — the route stays,
    /// its future stays pending, observers hear nothing — and the next pop
    /// removes the route itself.
    ///
    /// Red-check: skip the `local_history.pop_last_deferred()` arm in
    /// `ModalRoute::did_pop` — the first `maybe_pop` removes the route and the
    /// stays-put assertion fails.
    pub(crate) fn an_entry_pops_before_the_route_and_observers_stay_silent() {
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
            "the route's future stays pending"
        );
        assert_eq!(
            pops.0.load(Ordering::SeqCst),
            0,
            "observers hear nothing for an entry pop"
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

    /// What [`PopsInBuild`] acquired in `init_state`.
    #[derive(Clone)]
    struct Acquired {
        count: Signal<u32>,
        writer: flui_view::WriterSource,
        local: LocalHistoryHandle,
    }

    /// A page that reads `count` and, while `pops` is above zero, pops its
    /// navigator that many times from inside its own `build`: each flush, and
    /// its local-history pop, run while an element is building.
    #[derive(Clone)]
    struct PopsInBuild {
        navigator: NavigatorHandle,
        pops: Rc<std::cell::Cell<u32>>,
        acquired: Rc<std::cell::RefCell<Option<Acquired>>>,
    }

    impl View for PopsInBuild {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateful(self)
        }
    }

    impl StatefulView for PopsInBuild {
        type State = PopsInBuildState;

        fn create_state(&self) -> Self::State {
            PopsInBuildState {
                count: Signal::default(),
                acquired: Rc::clone(&self.acquired),
            }
        }
    }

    struct PopsInBuildState {
        count: Signal<u32>,
        acquired: Rc<std::cell::RefCell<Option<Acquired>>>,
    }

    impl std::fmt::Debug for PopsInBuildState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PopsInBuildState").finish_non_exhaustive()
        }
    }

    impl ViewState<PopsInBuild> for PopsInBuildState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            self.count = ctx.signal(0);
            let acquired = Acquired {
                count: self.count,
                writer: ctx.writer_source(),
                local: LocalHistoryHandle::maybe_of(ctx).expect("inside a route"),
            };
            let previous = self.acquired.borrow_mut().replace(acquired);
            drop(previous);
        }

        fn build(&self, view: &PopsInBuild, ctx: &dyn BuildContext) -> impl IntoView {
            let side = self.count.get(ctx);
            for _ in 0..view.pops.take() {
                assert!(view.navigator.maybe_pop(), "an entry absorbs the pop");
            }
            SizedBox::new(10.0 + f64::from(side), 10.0)
        }
    }

    /// A pop applied inside `build` owes the entry's `on_remove`; the callback
    /// writes a signal through the `WriterSource` it captured. Delivered after
    /// the frame, the write lands and rebuilds its reader.
    ///
    /// Red-check: fire the owed callbacks synchronously in
    /// `ModalHandle::drain_local_history` — the callback runs inside the
    /// page's `build`, its write is refused with `WrittenDuringBuild`, and the
    /// value stays 1.
    pub(crate) fn local_history_removal_writes_after_the_flush() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let (pops, acquired) = push_page_that_pops(&handle, &mut harness);
        let Acquired {
            count,
            writer,
            local,
        } = acquired;

        let written = Writes::default();
        let _entry = local.add(writing_entry(&written, &writer, count, 5));

        // Arm the pop and rebuild the page: its `build` pops the entry.
        pops.set(1);
        writer
            .write(|cx| count.set(cx, 1))
            .expect("a write from the test");
        harness.tick();
        assert_eq!(pops.get(), 0, "the page popped inside its build");
        assert_eq!(handle.route_ids().len(), 2, "the entry absorbed the pop");
        assert_eq!(
            *written.borrow(),
            [Ok(())],
            "on_remove ran once, after the build, and its write was accepted"
        );
        assert_eq!(writer.write(|cx| count.peek(cx, |n| *n)), Ok(5));
    }

    /// Every write an `on_remove` made, in order.
    type Writes = Rc<std::cell::RefCell<Vec<Result<(), flui_view::SignalError>>>>;

    /// An entry whose `on_remove` writes `value` through `writer` and records
    /// the outcome.
    fn writing_entry(
        written: &Writes,
        writer: &flui_view::WriterSource,
        count: Signal<u32>,
        value: u32,
    ) -> LocalHistoryEntry {
        let written = Rc::clone(written);
        let writer = writer.clone();
        LocalHistoryEntry::new().on_remove(move || {
            written
                .borrow_mut()
                .push(writer.write(|cx| count.set(cx, value)));
        })
    }

    /// Push a [`PopsInBuild`] page and settle it; returns its pop counter and
    /// what it acquired.
    fn push_page_that_pops(
        handle: &NavigatorHandle,
        harness: &mut Harness,
    ) -> (Rc<std::cell::Cell<u32>>, Acquired) {
        let pops = Rc::new(std::cell::Cell::new(0));
        let acquired = Rc::new(std::cell::RefCell::new(None));
        let page = PopsInBuild {
            navigator: handle.clone(),
            pops: Rc::clone(&pops),
            acquired: Rc::clone(&acquired),
        };
        let _route_result = handle.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| page.clone().into_view().boxed())
                .transition_duration(Duration::ZERO),
        );
        harness.tick();
        let acquired = acquired
            .borrow()
            .clone()
            .expect("the page acquired its handles");
        (pops, acquired)
    }

    /// Two removals owed in one frame, the first of which panics: each is its
    /// own post-frame entry, so the second is kept and runs on the next frame,
    /// its write lands, and the navigator and the queue serve the next removal
    /// normally.
    ///
    /// Red-check: queue both owed callbacks as one post-frame closure — the
    /// second is lost with the first panic. (Each `maybe_pop` here is its own
    /// flush; `local_history::tests` covers two callbacks owed by one drain.)
    pub(crate) fn a_panicking_removal_does_not_lose_the_next() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let (pops, acquired) = push_page_that_pops(&handle, &mut harness);
        let Acquired {
            count,
            writer,
            local,
        } = acquired;

        // The most recent entry pops first: the panicking one.
        let written = Writes::default();
        let _writes = local.add(writing_entry(&written, &writer, count, 5));
        let _panics =
            local.add(LocalHistoryEntry::new().on_remove(|| panic!("first on_remove failure")));

        pops.set(2);
        writer
            .write(|cx| count.set(cx, 1))
            .expect("a write from the test");
        let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
        if let Err(payload) = frame {
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("first on_remove failure"),
                "the first panic is the one reported"
            );
        }
        assert_eq!(pops.get(), 0, "both pops ran inside the build");
        // The post-frame drain stops at the panic and keeps the uninvoked
        // tail, in order, for the next frame.
        harness.tick();
        assert_eq!(
            *written.borrow(),
            [Ok(())],
            "the removal after the panicking one still ran, and its write landed"
        );
        assert_eq!(writer.write(|cx| count.peek(cx, |n| *n)), Ok(5));

        // The next operation after containment.
        let _next = local.add(writing_entry(&written, &writer, count, 7));
        assert!(handle.maybe_pop(), "the new entry absorbs the pop");
        harness.tick();
        assert_eq!(*written.borrow(), [Ok(()), Ok(())]);
        assert_eq!(writer.write(|cx| count.peek(cx, |n| *n)), Ok(7));
        assert_eq!(
            handle.route_ids().len(),
            2,
            "the entries absorbed every pop"
        );
    }

    /// The root for [`an_unmounted_navigators_owed_removal_is_severed`]: the
    /// navigator, or in its place a widget that pops it from its own `build`.
    #[derive(Clone)]
    struct Stage {
        navigator: NavigatorHandle,
        mounted: bool,
        pop_in_build: Rc<std::cell::Cell<bool>>,
    }

    impl View for Stage {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl StatelessView for Stage {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if self.mounted {
                return Navigator::new(self.navigator.clone()).into_view().boxed();
            }
            if self.pop_in_build.take() {
                let _ = self.navigator.pop();
            }
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }
    }

    /// A navigator that has unmounted has no post-frame queue. A pop through
    /// a handle that outlives it, made inside another element's `build`,
    /// owes an `on_remove` it can no longer deliver after a frame: the
    /// callback is severed with the navigator, like a disposed route's, and
    /// is never run inside that `build`.
    ///
    /// Red-check: fire the owed callback when there is no queue — it runs
    /// inside `Stage::build`, and its write is refused (the page's signal is
    /// already released with the unmounted page).
    pub(crate) fn an_unmounted_navigators_owed_removal_is_severed() {
        struct Dropped(Rc<std::cell::Cell<u32>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let pop_in_build = Rc::new(std::cell::Cell::new(false));
        let stage = |mounted| Stage {
            navigator: handle.clone(),
            mounted,
            pop_in_build: Rc::clone(&pop_in_build),
        };
        let mut harness = mount(stage(true));
        let (_pops, acquired) = push_page_that_pops(&handle, &mut harness);
        let Acquired {
            count,
            writer,
            local,
        } = acquired;

        let written = Writes::default();
        let drops = Rc::new(std::cell::Cell::new(0));
        let capture = Dropped(Rc::clone(&drops));
        let removal_writes = Rc::clone(&written);
        let removal_writer = writer;
        let _entry = local.add(LocalHistoryEntry::new().on_remove(move || {
            let _ = &capture;
            removal_writes
                .borrow_mut()
                .push(removal_writer.write(|cx| count.set(cx, 5)));
        }));

        harness.swap_root(stage(false));
        harness.tick();
        assert!(!handle.is_mounted(), "the navigator unmounted");

        pop_in_build.set(true);
        harness.swap_root(stage(false));
        harness.tick();
        assert!(!pop_in_build.get(), "the stage popped inside its build");
        assert!(
            written.borrow().is_empty(),
            "a severed on_remove never runs, in a build or after it: {:?}",
            written.borrow()
        );
        assert_eq!(drops.get(), 1, "its captures are dropped once");
    }
}

// ============================================================================
// User gestures
// ============================================================================

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
pub(crate) fn a_panicking_route_hook_leaves_the_navigator_usable() {
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

// ============================================================================
// Closing the window mid-navigation
// ============================================================================

/// **The window closes while a page is mid-transition and its hero is airborne.**
/// A modal page route sits halfway up its entrance curve with a shuttle in the
/// overlay; the window closes and its tree goes with it, while the application
/// keeps its `NavigatorHandle`.
///
/// Observable: the close ends the flight and takes the route's overlay entry out
/// of the overlay; no controller stays registered with the scope's `Vsync`, none
/// runs, and no frame is requested from the close on. Once the application drops
/// its handle as well, the route — its page builder, its `TransitionInner`, its
/// modal registry entry — is released.
///
/// Characterization: this holds on the code as it is.
pub(crate) fn closing_mid_transition_releases_the_route_overlay_and_hero() {
    use std::cell::Cell;
    use std::time::Duration;

    use flui_animation::{Animation as _, Vsync};
    use flui_foundation::ValueKey;
    use flui_view::CloseReason;
    use flui_widgets::__test_access::{
        HeroControllerProbe as _, OverlayEntryProbe as _, RouteProbe as _,
    };
    use flui_widgets::navigator::{Hero, HeroController, NavigatorObserver, PageRoute};
    use flui_widgets::{Center, VsyncScope};

    use crate::common::{lay_out_animated, tight};

    const TRANSITION: Duration = Duration::from_millis(300);
    const FRAME: Duration = Duration::from_millis(16);

    fn hero_page(page: Option<Rc<()>>) -> PageRoute<i32> {
        PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
            let _page = page.clone();
            Center::new()
                .child(Hero::new(
                    ValueKey::new("shared"),
                    SizedBox::new(40.0, 30.0),
                ))
                .into_view()
                .boxed()
        })
        .transition_duration(TRANSITION)
    }

    let vsync = Vsync::new();
    let frame_requests = Rc::new(Cell::new(0_usize));
    vsync.set_frame_requester(Some(Rc::new({
        let frame_requests = Rc::clone(&frame_requests);
        move || frame_requests.set(frame_requests.get() + 1)
    })));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(hero_page(None));
    let controller = HeroController::new();
    navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
        tight(400.0, 400.0),
        vsync.clone(),
    );

    let page = Rc::new(());
    let route = hero_page(Some(Rc::clone(&page)));
    let transition = route.transition_handle();
    let _push = laid.enter_owner_scope(|| navigator.push(route));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);

    let top = navigator.current().expect("the page is pushed");
    let entry = navigator
        .entry_of(top)
        .expect("the page has an overlay entry");
    let value = transition
        .controller()
        .expect("the push installed the controller")
        .value();
    assert!(
        value > 0.0 && value < 1.0,
        "precondition: the entrance is mid-curve ({value})"
    );
    assert_eq!(
        controller.flights().len(),
        1,
        "precondition: the hero is airborne"
    );
    assert!(entry.is_mounted(), "precondition: the page is on screen");
    assert!(
        navigator.route_modal(top).is_some(),
        "precondition: the page is in the modal registry"
    );
    assert!(vsync.has_running(), "precondition: its transition ticks");
    drop(transition);

    laid.request_close(CloseReason::User);
    let requests_at_close = frame_requests.get();
    drop(laid);

    assert_eq!(controller.flights().len(), 0, "the close ended the flight");
    assert!(
        !entry.is_mounted(),
        "the route's overlay entry left the overlay"
    );
    assert!(
        vsync.is_empty(),
        "no controller stays registered with the scope's ticker registry"
    );
    assert!(!vsync.has_running(), "nothing runs after the close");
    assert_eq!(
        frame_requests.get(),
        requests_at_close,
        "nothing requests a frame after the close"
    );

    drop(entry);
    drop(navigator);
    assert_eq!(
        Rc::strong_count(&page),
        1,
        "dropping the application's handle releases the route and its page builder"
    );
}
