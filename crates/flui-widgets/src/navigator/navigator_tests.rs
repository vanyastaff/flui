//! ADR-0019 U3 tests for the private `Navigator`.
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
//! Unlike `tests.rs` (U2's pure-data suite), these drive a real element tree.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_view::BuildContext;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::binding::RouteBindingSlot;
use super::navigator::{Navigator, NavigatorHandle};
use super::overlay_route::{NavigatorRoute, RouteContentBuilder, SimpleRoute};
use super::route::{PushCompletion, Route, RouteSettings};
use crate::SizedBox;
use crate::test_harness::{Harness, mount};

// ============================================================================
// PROBES
// ============================================================================

/// Records every route builder invocation, so "did this route's content build?"
/// is observable.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<Vec<&'static str>>>);

impl Built {
    fn names(&self) -> Vec<&'static str> {
        self.0.lock().clone()
    }
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

/// A route whose subtree captures the navigator its content sees.
///
/// This is the only honest way to test `Navigator::of`: the lookup must run from
/// a `BuildContext` **inside** the navigator's own subtree, which is exactly
/// where a route's content builds.
fn probing_page(sink: &Arc<Mutex<Option<NavigatorHandle>>>, root: bool) -> SimpleRoute<i32> {
    let sink = Arc::clone(sink);
    SimpleRoute::new(move |ctx| {
        let found = if root {
            NavigatorHandle::maybe_of_root(ctx)
        } else {
            NavigatorHandle::maybe_of(ctx)
        };
        *sink.lock() = found;
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
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
    // ADR-0021 §7m: `Navigator::build` wraps its `Overlay` in a `HeroControllerScope`
    // (a `.none`), so the overlay is now one level below the navigator's element.
    let scope = harness.only_child(root);
    let overlay = harness.only_child(scope);
    let stack = harness.only_child(overlay);
    harness.children_of(stack)
}

// ============================================================================
// TESTS
// ============================================================================

/// The seeded initial route is flushed once, on mount, and its content builds on
/// the first frame — Flutter's `restoreState` tail (`navigator.dart:3922-3934`).
///
/// Red-check: delete the `flush` in `NavigatorState::init_state`; the route is
/// never installed and no layer appears.
#[test]
fn navigator_first_route_builds_on_first_frame() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    assert_eq!(built.names(), vec!["/"], "the initial route built");
    assert_eq!(layers(&mut harness).len(), 1, "one overlay layer");
    assert_eq!(handle.route_ids().len(), 1);
    assert!(handle.is_mounted());
}

/// A deep link's synthesized back-stack: several `seed_initial` calls, one flush
/// on mount (`defaultGenerateInitialRoutes`, `navigator.dart:3017-3058`).
///
/// Note what this does **not** prove. Making `seed_initial` flush per seed leaves
/// this test green: three sequential flushes reach the same end state and deliver
/// the same observer order. The single-flush property is only observable through
/// the additions queue's LIFO drain, which U2's
/// `push_adds_route_and_notifies_observer_lifo` already pins. Said plainly rather
/// than claimed here.
///
/// Red-check: drop the `entries.insert` in `seed_initial`; the routes exist but
/// no overlay layer does.
#[test]
fn navigator_seeds_a_back_stack_with_one_flush() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    handle.seed_initial(page(&built, "/a"));
    handle.seed_initial(page(&built, "/a/b"));

    let mut harness = mount(Navigator::new(handle.clone()));

    assert_eq!(handle.route_ids().len(), 3);
    assert_eq!(layers(&mut harness).len(), 3);
    assert!(handle.can_pop(), "a back-stack can pop");
}

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

/// The route beneath stays mounted when another is pushed over it.
/// Oracle: `'Can navigator navigate to and from a stateful widget'`.
///
/// Red-check: have `NavigatorShared::apply` rearrange to only the top entry.
#[test]
fn navigator_push_keeps_the_route_beneath_mounted() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    let first_layer = layers(&mut harness)[0];

    handle.push(page(&built, "second"));
    harness.tick();

    let after = layers(&mut harness);
    assert_eq!(after.len(), 2);
    assert_eq!(after[0], first_layer, "the bottom layer's element survived");
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

/// `remove_route` completes the future too — the U2 invariant, now through the
/// widget. Oracle: `'remove a route whose value is awaited'`.
///
/// Red-check: same as above (`entry.remove()` loop) for the overlay half; for the
/// result half, make `handle_complete` skip `did_complete`.
#[test]
fn navigator_remove_route_completes_result_and_rearranges_overlay() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    let result = handle.push(page(&built, "second"));
    harness.tick();

    let top = handle.current().expect("a top route");
    assert!(handle.remove_route_with(top, 7_i32));
    harness.tick();

    assert_eq!(result.try_take(), Some(Some(7)));
    assert_eq!(handle.overlay().len(), 1);
    assert_eq!(layers(&mut harness).len(), 1);
}

/// A route that refuses `did_pop` stays, and completes nothing
/// (`navigator.dart:3369-3371`). `maybe_pop` still reports the request handled,
/// because `popDisposition` was `pop`.
///
/// Red-check: ignore `did_pop`'s return value in `RouteRecord::did_pop`.
#[test]
fn navigator_maybe_pop_respects_route_refusal() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    let refusing: SimpleRoute<i32> = {
        let built = built.clone();
        SimpleRoute::new(move |_ctx| {
            built.0.lock().push("refusing");
            SizedBox::new(10.0, 10.0).into_view().boxed()
        })
        .refusing_pop()
    };
    let result = handle.push(refusing);
    harness.tick();

    assert!(handle.maybe_pop_with(1_i32), "the pop was handled");
    harness.tick();

    assert_eq!(handle.route_ids().len(), 2, "the route refused and stayed");
    assert!(!result.is_completed(), "a refused pop completes nothing");
    assert_eq!(layers(&mut harness).len(), 2);
}

/// `canPop` (`navigator.dart:5551-5566`): `false` for a lone route, `true` once a
/// second exists, and `true` for a lone route that handles pops internally.
///
/// `maybePop` on a lone route **bubbles** — returns `false` — because
/// `popDisposition` is `isFirst ? bubble : pop` (`:382-390`).
///
/// Red-check: make `RouteHistory::can_pop` return `entries.len() > 1`; the
/// `handling_pop_internally` case flips.
#[test]
fn navigator_can_pop_matches_flutter_contract() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    assert!(!handle.can_pop(), "a single route cannot pop");
    assert!(
        !handle.maybe_pop(),
        "and maybe_pop bubbles rather than popping it"
    );
    assert_eq!(handle.route_ids().len(), 1, "the root route survived");

    handle.push(page(&built, "second"));
    harness.tick();
    assert!(handle.can_pop(), "two routes can pop");
    assert!(handle.maybe_pop());
    harness.tick();
    assert_eq!(handle.route_ids().len(), 1);

    // A lone route that handles pops itself *can* pop.
    let internal = NavigatorHandle::new();
    internal.seed_initial(page(&built, "internal").handling_pop_internally());
    let _harness = mount(Navigator::new(internal.clone()));
    assert!(
        internal.can_pop(),
        "willHandlePopInternally lets the first route claim the pop"
    );
}

/// `Navigator::of` from inside a route's content resolves to the navigator that
/// owns the route — the route's `BuildContext` is a descendant of it.
///
/// **This is not Flutter's self-check**, and U3 could not implement one.
/// `Navigator.of` first tests whether `context` *is* the `NavigatorState`'s own
/// element (`navigator.dart:2947`), which matters only for a context obtained via
/// `GlobalKey<NavigatorState>.currentContext`. FLUI's `walk_strict_ancestors`
/// starts at the parent, and during `build` the element's own node is a hole, so
/// no `BuildContext` API can reach its own state. Since FLUI has no
/// `GlobalKey<NavigatorState>` the case is unreachable — recorded as a correction
/// to ADR-0019 §3.3, which assumed `Navigator::of` would have to close this gap.
///
/// Red-check: make `maybe_of` return `None`. (Swapping it to `find_root_state`
/// leaves this test green — with one navigator, nearest *is* root. The nested
/// test below is what discriminates them.)
#[test]
fn navigator_of_self_check_finds_current_navigator() {
    let sink: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));
    let handle = NavigatorHandle::new();
    handle.seed_initial(probing_page(&sink, false));
    let _harness = mount(Navigator::new(handle.clone()));

    let found = sink
        .lock()
        .clone()
        .expect("Navigator::of found a navigator");
    assert_eq!(
        found.route_ids(),
        handle.route_ids(),
        "the route's context resolved to its own navigator"
    );
}

/// `maybe_of` returns the **nearest** navigator; `maybe_of_root` the outermost.
/// Oracle: `'Navigator.of rootNavigator finds root Navigator'`.
///
/// Red-check: swap `find_state` and `find_root_state` in `maybe_of` /
/// `maybe_of_root`; both assertions invert.
#[test]
fn nested_navigator_lookup_prefers_nearest_and_root_finds_outermost() {
    let nearest: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));
    let root: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));

    // An inner navigator whose only route probes for both.
    let inner = NavigatorHandle::new();
    {
        let (nearest, root) = (Arc::clone(&nearest), Arc::clone(&root));
        inner.seed_initial(SimpleRoute::<i32>::new(move |ctx| {
            *nearest.lock() = NavigatorHandle::maybe_of(ctx);
            *root.lock() = NavigatorHandle::maybe_of_root(ctx);
            SizedBox::new(5.0, 5.0).into_view().boxed()
        }));
    }

    // The outer navigator's only route *is* the inner navigator.
    let outer = NavigatorHandle::new();
    {
        let inner = inner.clone();
        outer.seed_initial(SimpleRoute::<i32>::new(move |_ctx| {
            Navigator::new(inner.clone()).into_view().boxed()
        }));
    }

    let _harness = mount(Navigator::new(outer.clone()));

    let nearest = nearest.lock().clone().expect("nearest navigator");
    let root = root.lock().clone().expect("root navigator");

    assert_eq!(
        nearest.route_ids(),
        inner.route_ids(),
        "maybe_of finds the inner navigator"
    );
    assert_eq!(
        root.route_ids(),
        outer.route_ids(),
        "maybe_of_root finds the outer one"
    );
    assert_ne!(inner.route_ids(), outer.route_ids());
}

/// `Navigator.maybeOf` with no navigator above returns `None` rather than
/// panicking. Oracle: `'Navigator.of fails gracefully when not found in context'`.
///
/// Red-check: `expect()` the lookup in `maybe_of`.
#[test]
fn navigator_maybe_of_returns_none_when_absent() {
    /// `ran` proves the probe built at all; `found` is what the lookup returned.
    #[derive(Clone, Default)]
    struct Seen {
        ran: Arc<AtomicUsize>,
        found: Arc<Mutex<Option<NavigatorHandle>>>,
    }

    let seen = Seen::default();

    #[derive(Clone)]
    struct Probe {
        seen: Seen,
    }
    impl View for Probe {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }
    impl StatelessView for Probe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            self.seen.ran.fetch_add(1, Ordering::Relaxed);
            *self.seen.found.lock() = NavigatorHandle::maybe_of(ctx);
            SizedBox::new(1.0, 1.0)
        }
    }

    let _harness = mount(Probe { seen: seen.clone() });

    assert_eq!(seen.ran.load(Ordering::Relaxed), 1, "the probe built");
    assert!(
        seen.found.lock().is_none(),
        "no navigator above ⇒ None, not a panic"
    );
}

/// A handle outliving its navigator is inert: no panic, no resurrection.
///
/// Flutter's `maybePop` early-returns on `!mounted` (`navigator.dart:5595`), and
/// `_markDirty` is guarded by `if (mounted)` (`overlay.dart:849`).
///
/// Red-check: delete `OverlayState::dispose`; `is_mounted()` stays true and the
/// stale handle schedules a dead element.
#[test]
fn stale_navigator_handle_is_harmless() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));

    let mut harness = mount(Host {
        show: true,
        handle: handle.clone(),
    });
    assert!(handle.is_mounted());

    harness.swap_root(Host {
        show: false,
        handle: handle.clone(),
    });
    assert!(!handle.is_mounted(), "the navigator unmounted");

    built.clear();
    // Every operation on the stale handle is a silent no-op, not a panic.
    handle.push(page(&built, "late"));
    handle.pop();
    assert!(
        handle.maybe_pop(),
        "an unmounted navigator swallows the pop"
    );
    harness.tick();

    assert!(
        !built.contains("late"),
        "nothing was built after unmount: {:?}",
        built.names()
    );
    assert!(!handle.is_mounted(), "and it did not resurrect");
}

/// The order `NavigatorShared::apply` enforces: disposed routes' overlay entries
/// are removed **before** the rearrange (`navigator.dart:4609-4613`), and a flush
/// that asks for no rearrange (`pop`, `remove_route`) still removes them.
///
/// Red-check: move the `rearrange` above the `entry.remove()` loop — the disposed
/// entry is re-inserted into the overlay and the layer count stays 2.
#[test]
fn navigator_flush_rearranges_overlay_after_disposal() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    handle.push(page(&built, "second"));
    harness.tick();
    assert_eq!(handle.overlay().len(), 2);

    handle.pop();
    harness.tick();

    assert_eq!(
        handle.overlay().len(),
        1,
        "the disposed route's entry left the overlay"
    );
    assert_eq!(layers(&mut harness).len(), 1);
}

/// A pushed-then-popped route's entry must not linger in the navigator's
/// `RouteId -> OverlayEntry` map either.
///
/// The overlay itself looks fine even when it does — the entry was removed from
/// the overlay's own list — so only `tracked_entry_count` catches the leak. This
/// is the assertion the first draft of this test was missing.
///
/// Red-check: `entries.get(id)` instead of `entries.remove(id)` in
/// `NavigatorShared::apply`; the map grows to 4 while the overlay still reads 1.
#[test]
fn navigator_drops_overlay_entries_of_disposed_routes() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    for _ in 0..3 {
        handle.push(page(&built, "x"));
        harness.tick();
        handle.pop();
        harness.tick();
    }

    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(handle.overlay().len(), 1, "no stale entries in the overlay");
    assert_eq!(
        handle.tracked_entry_count(),
        1,
        "and none left behind in the navigator's own map"
    );
    assert_eq!(layers(&mut harness).len(), 1);
}

/// Pushing from *inside* a route's build must be possible without deadlock, and
/// must not run under the element-tree borrow — ADR-0019 §3.2's whole point.
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
// THE ROUTE-ANIMATION SEAM (ADR-0020 U5.1)
// ============================================================================

/// A zero-duration transition route: it parks in `Pushing`, then completes its
/// entrance from inside `did_push` — i.e. inside the flush that pushed it — and
/// finalizes itself from inside `did_pop`.
///
/// This is the shape U5.2's `TransitionRoute` will have, minus the
/// `AnimationController`.
struct ZeroDurationRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    binding: RouteBindingSlot,
}

impl ZeroDurationRoute {
    fn new(built: &Built, name: &'static str) -> Self {
        let built = built.clone();
        Self {
            settings: RouteSettings::named(name),
            builder: Arc::new(move |_ctx| {
                built.0.lock().push(name);
                SizedBox::new(10.0, 10.0).into_view().boxed()
            }),
            binding: RouteBindingSlot::new(),
        }
    }
}

impl Route for ZeroDurationRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    /// `TransitionRoute.finishedWhenPopped => controller.isDismissed` — false
    /// while the exit transition runs, so disposal defers to `finalize()`.
    fn finished_when_popped(&self) -> bool {
        false
    }

    fn did_push(&mut self) -> PushCompletion {
        if let Some(binding) = self.binding.get() {
            binding.notify_push_completed();
        }
        PushCompletion::Animating
    }

    fn did_pop(&mut self) -> bool {
        if let Some(binding) = self.binding.get() {
            binding.finalize();
        }
        true
    }
}

impl NavigatorRoute for ZeroDurationRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Arc::clone(&self.builder)
    }

    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        Some(&self.binding)
    }
}

/// The seam, end to end, through a real `Navigator` and `Overlay`.
///
/// Both commands are raised from **inside** a flush, while `NavigatorShared`
/// holds the history mutex. The binding enqueues rather than calling back, and
/// `wake`'s `try_lock` correctly declines. If it locked instead, this test would
/// hang rather than fail — which is why `pump_route_commands` uses `try_lock`.
///
/// Red-check: change `pump_route_commands` to `self.history.lock()`; this test
/// deadlocks (nextest's per-test timeout catches it).
#[test]
fn bound_zero_duration_route_settles_lifecycle_and_overlay() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    let result = handle.push(ZeroDurationRoute::new(&built, "animated"));
    harness.tick();

    assert!(built.contains("animated"), "the pushed route built");
    assert_eq!(handle.route_ids().len(), 2, "the push settled to Idle");
    assert_eq!(handle.overlay().len(), 2);
    assert_eq!(layers(&mut harness).len(), 2);

    // `did_pop` raises `finalize()` mid-flush; the deferred pass disposes it and
    // the accumulated outcome removes its overlay entry.
    assert!(handle.pop_with(7_i32));
    harness.tick();

    assert_eq!(result.try_take(), Some(Some(7)));
    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(
        handle.overlay().len(),
        1,
        "the deferred disposal reached the overlay"
    );
    assert_eq!(handle.tracked_entry_count(), 1, "and the navigator's map");
    assert_eq!(layers(&mut harness).len(), 1);
}

/// A route that never completes its push stays in `Pushing`, and the route below
/// it is not disposed — the deferral must not settle what nothing raised.
///
/// Red-check: make `apply_pending_commands` flip `Pushing → Idle` unconditionally.
#[test]
fn a_route_that_raises_nothing_stays_pushing() {
    struct Animating {
        settings: RouteSettings,
        builder: RouteContentBuilder,
    }
    impl Route for Animating {
        type Output = i32;
        fn settings(&self) -> &RouteSettings {
            &self.settings
        }
        fn did_push(&mut self) -> PushCompletion {
            PushCompletion::Animating
        }
    }
    impl NavigatorRoute for Animating {
        fn content_builder(&self) -> RouteContentBuilder {
            Arc::clone(&self.builder)
        }
    }

    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    handle.push(Animating {
        settings: RouteSettings::named("stuck"),
        builder: Arc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
    });
    harness.tick();

    assert_eq!(handle.route_ids().len(), 2);
    assert_eq!(layers(&mut harness).len(), 2, "both routes are still shown");
}

// ============================================================================
// ARCHITECTURE GUARDS
// ============================================================================

/// ADR-0019 §3.2: the `Navigator` must reach its overlay through an `Arc`, never
/// through a `GlobalKey` — whose registry lookup would take a second lock under
/// the element-tree borrow.
///
/// Red-check: add `use flui_view::GlobalKey;` to `navigator.rs`.
#[test]
fn navigator_uses_no_global_key() {
    const SOURCES: [(&str, &str); 2] = [
        ("navigator.rs", include_str!("navigator.rs")),
        ("overlay_route.rs", include_str!("overlay_route.rs")),
    ];
    for (name, source) in SOURCES {
        // The module docs explain *why* there is no GlobalKey, so only reject it
        // outside comment lines.
        for (number, line) in source.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !code.contains("GlobalKey"),
                "{name}:{} uses GlobalKey: {line}",
                number + 1
            );
        }
    }
}

/// U4 exports a **signed-off surface and nothing more**. The route stack's
/// internals must stay private: leaking `RouteHistory`, `RouteLifecycle`,
/// `RouteEntry`, `ErasedRoute`, `AnyResult`, `FlushOutcome`, `Observation` or the
/// overlay's bookkeeping would freeze implementation detail into semver.
///
/// The positive half — that the approved names *are* exported — is asserted from
/// outside the crate, in `tests/navigator_public.rs`, where a wrong `pub use`
/// fails to compile rather than fails an assertion.
///
/// Red-check: add `pub use history::RouteHistory;` to `navigator/mod.rs`.
#[test]
fn public_no_internal_route_stack_exports() {
    const NAV_MOD: &str = include_str!("mod.rs");
    const LIB: &str = include_str!("../lib.rs");

    const INTERNAL: [&str; 29] = [
        "RouteHistory",
        "RouteLifecycle",
        "RouteEntry",
        "ErasedRoute",
        "AnyResult",
        "FlushOutcome",
        "Observation",
        "Notification",
        "RoutePopDisposition",
        // ADR-0020: the route-animation seam stays private. U5.4 exports the
        // opaque `RouteBindingSlot`, *not* the `RouteBinding` inside it — which is
        // why this check matches whole identifiers.
        "RouteBinding",
        "RouteRegistries",
        "RouteCommand",
        "TransitionRoute",
        "ModalRoute",
        // ADR-0021 U6 signed off the public `Hero` / `HeroController` baseline.
        // The support seams below stay private implementation detail.
        "ModalHandle",
        "FlightDirection",
        "Measurement",
        "RouteSubtree",
        // ADR-0021 U3.5: the Hero registry and handles. The public widget hides these
        // details behind `Hero::new(tag: impl ViewKey, child)`.
        "HeroTag",
        "HeroRegistry",
        "HeroScope",
        "HeroHandle",
        "HeroState",
        "HeroFlightManifest",
        // ADR-0021 U6: `Hero` and `HeroController` are the signed-off public surface;
        // everything else stays crate-private (or, for `HeroState`, `pub` but never
        // re-exported — reachable only as `<Hero as StatefulView>::State`).
        "HeroFlight",
        "FlightManager",
        "Shuttle",
        "ShuttleState",
        "FlightPlan",
    ];

    super::export_guard::assert_not_exported("navigator/mod.rs", NAV_MOD, &INTERNAL);
    super::export_guard::assert_not_exported("lib.rs", LIB, &INTERNAL);
}

/// `Overlay` / `OverlayEntry` / `OverlayHandle` stay **private** after U4.
///
/// `Navigator` needs them, but exporting Flutter's `Overlay` surface is a separate
/// parity gate (ADR-0019 §5 U5, with `ModalRoute` and `OverlayPortal`). Nothing in
/// the signed-off `Navigator` surface names them.
///
/// Red-check: add `pub mod overlay;` to `lib.rs`.
#[test]
fn overlay_stays_private_after_u4() {
    const LIB: &str = include_str!("../lib.rs");

    for line in LIB.lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        assert!(
            !code.starts_with("pub mod overlay"),
            "the overlay module must stay private: {line}"
        );
        if code.starts_with("pub use") {
            assert!(
                !(code.contains("OverlayEntry") || code.contains("OverlayHandle")),
                "the overlay surface must stay private: {line}"
            );
        }
    }
}
