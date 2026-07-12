//! Tests for the private `Navigator`.
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
//! Unlike `tests.rs` (the route stack's pure-data suite), these drive a real element tree.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_view::BuildContext;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::binding::RouteBindingSlot;
use super::navigator::{
    Navigator, NavigatorCommand, NavigatorCommandError, NavigatorCommandOutcome,
    NavigatorCommandTarget, NavigatorHandle,
};
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

#[test]
fn navigator_handle_is_owner_affine_but_command_target_is_send_sync() {
    static_assertions::assert_not_impl_any!(NavigatorHandle: Send, Sync);
    static_assertions::assert_impl_all!(NavigatorCommandTarget: Send, Sync);
    static_assertions::assert_impl_all!(NavigatorCommand: Send, Sync);
}

#[test]
fn navigator_command_applies_only_on_owner_thread() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let route = handle.push(page(&built, "/next"));
    let target = handle.command_target();

    assert_eq!(
        NavigatorCommand::pop(target).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Popped(true))
    );
    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(
        route.try_take(),
        Some(None),
        "a pop without result completes with None"
    );
}

#[test]
fn navigator_command_wrong_thread_is_typed_error() {
    let handle = NavigatorHandle::new();
    let target = handle.command_target();

    let outcome = std::thread::spawn(move || NavigatorCommand::pop(target).apply_on_owner())
        .join()
        .expect("worker did not panic");

    assert_eq!(outcome, Err(NavigatorCommandError::WrongOwnerThread));
}

#[test]
fn navigator_command_dead_target_is_typed_error() {
    let target = {
        let handle = NavigatorHandle::new();
        handle.command_target()
    };

    assert_eq!(
        NavigatorCommand::maybe_pop(target).apply_on_owner(),
        Err(NavigatorCommandError::OwnerGone)
    );
}

#[test]
fn navigator_command_can_remove_specific_route() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let pushed = handle.push(page(&built, "/details"));
    let route = *handle.route_ids().last().expect("pushed route id");

    assert_eq!(
        NavigatorCommand::remove_route(handle.command_target(), route).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Removed(true))
    );
    assert_eq!(handle.route_ids().len(), 1);
    assert_eq!(
        pushed.try_take(),
        Some(None),
        "removed routes complete their future with None by default"
    );
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
/// the additions queue's LIFO drain, which the route stack's
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

/// `remove_route` completes the future too — the route stack's invariant, now
/// through the widget. Oracle: `'remove a route whose value is awaited'`.
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
/// **This is not Flutter's self-check**, and this widget could not implement one.
/// `Navigator.of` first tests whether `context` *is* the `NavigatorState`'s own
/// element (`navigator.dart:2947`), which matters only for a context obtained via
/// `GlobalKey<NavigatorState>.currentContext`. FLUI's `walk_strict_ancestors`
/// starts at the parent, and during `build` the element's own node is a hole, so
/// no `BuildContext` API can reach its own state. Since FLUI has no
/// `GlobalKey<NavigatorState>` the case is unreachable — recorded as a correction
/// to an earlier assumption that `Navigator::of` would have to close this gap.
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

/// A zero-duration transition route: it parks in `Pushing`, then completes its
/// entrance from inside `did_push` — i.e. inside the flush that pushed it — and
/// finalizes itself from inside `did_pop`.
///
/// This is the shape `TransitionRoute` will have, minus the
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
            builder: Rc::new(move |_ctx| {
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
        Rc::clone(&self.builder)
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
            Rc::clone(&self.builder)
        }
    }

    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    handle.push(Animating {
        settings: RouteSettings::named("stuck"),
        builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
    });
    harness.tick();

    assert_eq!(handle.route_ids().len(), 2);
    assert_eq!(layers(&mut harness).len(), 2, "both routes are still shown");
}

// ============================================================================
// ARCHITECTURE GUARDS
// ============================================================================

/// The `Navigator` must reach its overlay through an `Arc`, never
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

/// This crate exports a **signed-off surface and nothing more**. The route stack's
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

    const INTERNAL: [&str; 35] = [
        "RouteHistory",
        "RouteLifecycle",
        "RouteEntry",
        "ErasedRoute",
        "AnyResult",
        "FlushOutcome",
        "Observation",
        "Notification",
        "RoutePopDisposition",
        // The route-animation seam stays private. Only the
        // opaque `RouteBindingSlot` is exported, *not* the `RouteBinding` inside it —
        // which is why this check matches whole identifiers.
        "RouteBinding",
        "RouteRegistries",
        "RouteCommand",
        "TransitionRoute",
        "ModalRoute",
        // The public `Hero` / `HeroController` baseline additionally exports
        // `FlightDirection` because `flight_shuttle_builder` takes it.
        // The support seams below stay private implementation detail.
        "ModalHandle",
        "Measurement",
        "RouteSubtree",
        // The Hero registry and handles. The public widget hides these
        // details behind `Hero::new(tag: impl ViewKey, child)`.
        "HeroTag",
        "HeroRegistry",
        "HeroScope",
        // `HeroMode` is public; the inherited carrier of its AND-composed
        // `enabled` flag stays private.
        "HeroModeScope",
        "HeroHandle",
        "HeroState",
        "HeroFlightManifest",
        // `Hero` and `HeroController` are the signed-off public surface;
        // everything else stays crate-private (or, for `HeroState`, `pub` but never
        // re-exported — reachable only as `<Hero as StatefulView>::State`).
        "HeroFlight",
        "FlightManager",
        "Shuttle",
        "ShuttleState",
        "FlightPlan",
        // The deferred `PopScope` landed 2026-07-10: the widget is
        // public; the route-side registry and its ambient carrier stay private.
        "PopEntryRegistry",
        "PopEntryScope",
        // The local-history mechanism is crate-private; the
        // public surface is gated on the first Catalog consumer.
        "LocalHistoryRegistry",
        "LocalHistoryScope",
        "LocalHistoryHandle",
        "LocalHistoryEntryHandle",
    ];

    super::export_guard::assert_not_exported("navigator/mod.rs", NAV_MOD, &INTERNAL);
    super::export_guard::assert_not_exported("lib.rs", LIB, &INTERNAL);
}

/// `Overlay` / `OverlayEntry` / `OverlayHandle` stay **private**.
///
/// `Navigator` needs them, but exporting Flutter's `Overlay` surface is a separate
/// parity gate, with `ModalRoute` and `OverlayPortal`. Nothing in
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

/// `NavigatorHandle::push_replacement` — Flutter's `pushReplacement`
/// (`navigator.dart:5245-5268`): the top is swapped in place, so the stack depth and
/// overlay layer count are unchanged, and the **replaced** route's future resolves
/// with the delivered result (`complete(result, isReplaced: true)`).
///
/// Red-check: route `push_replacement_erased` through `push_with_id` — a third layer
/// appears and the replaced future never resolves.
#[test]
fn navigator_push_replacement_swaps_the_top_in_place() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    let second = handle.push(page(&built, "second"));
    harness.tick();
    assert_eq!(layers(&mut harness).len(), 2);

    let _third = handle.push_replacement_with(page(&built, "third"), 7_i32);
    harness.tick();

    assert!(built.contains("third"), "the replacement route built");
    assert_eq!(
        layers(&mut harness).len(),
        2,
        "replaced in place, not stacked"
    );
    assert_eq!(handle.route_ids().len(), 2);
    assert_eq!(
        second.try_take(),
        Some(Some(7)),
        "the replaced route's future resolves with the delivered result"
    );
}

/// `NavigatorHandle::push_and_remove_until` — Flutter's `pushAndRemoveUntil`
/// (`navigator.dart:5347-5371`): one flush pushes the new route and removes every
/// present route beneath the old top until `keep` answers `true`. Removed routes'
/// futures complete with `None` (`:5360`).
///
/// Red-check: skip the downward walk in `push_and_remove_until_with_id` — four
/// routes remain and `second` stays pending.
#[test]
fn navigator_push_and_remove_until_clears_down_to_the_kept_route() {
    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    let root = handle.route_ids()[0];

    let second = handle.push(page(&built, "second"));
    let _third = handle.push(page(&built, "third"));
    harness.tick();
    assert_eq!(layers(&mut harness).len(), 3);

    let _home = handle.push_and_remove_until(page(&built, "home"), |id| id == root);
    harness.tick();

    assert_eq!(
        handle.route_ids().len(),
        2,
        "only the kept root and the new route survive"
    );
    assert_eq!(handle.route_ids()[0], root, "the kept route is untouched");
    assert_eq!(layers(&mut harness).len(), 2);
    assert_eq!(
        second.try_take(),
        Some(None),
        "a removed route's future completes with None"
    );
}

/// The per-route focus scope. Each `ModalRoute` wraps its page in
/// `FocusScope::with_external_node` (`routes.dart:1201-1202`) and, while current,
/// holds the manager's **active scope** (FLUI's analogue of `setFirstFocus`
/// chaining, `routes.dart:1692`, `:1137`): pushing a cover unfocuses a field left
/// focused on the covered route, and popping back restores both the active scope
/// and the remembered field focus.
///
/// Red-check: drop the `activate_focus_scope` call from `did_change_next(None)`
/// — the pop neither reclaims the scope nor restores the field.
#[test]
fn route_focus_scope_confines_and_restores_keyboard_focus() {
    use flui_interaction::routing::{FocusManager, FocusNode};

    use crate::navigator::PageRoute;
    use crate::{Focus, SizedBox as SizedBoxW};

    let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();
    manager.set_active_scope(None);

    let field = FocusNode::with_debug_label("page-a-field");
    let field_for_page = Arc::clone(&field);
    let handle = NavigatorHandle::new();
    handle.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Focus::new(SizedBoxW::new(10.0, 10.0))
            .focus_node(Arc::clone(&field_for_page))
            .into_view()
            .boxed()
    }));
    let mut harness = mount(Navigator::new(handle.clone()));

    let scope_a = manager.active_scope();
    assert_ne!(
        scope_a.as_focus_node().id(),
        manager.root_scope().as_focus_node().id(),
        "the seeded route's scope became the active scope"
    );

    field.request_focus();
    assert!(field.has_primary_focus(), "sanity: the field took focus");

    // Cover A: its field must stop receiving keys, and B's scope activates.
    let _b = handle.push(PageRoute::<i32>::new(|_ctx, _p, _s| {
        SizedBoxW::new(10.0, 10.0).into_view().boxed()
    }));
    harness.tick();
    assert!(
        !field.has_primary_focus(),
        "a covered route's field is unfocused"
    );
    let scope_b = manager.active_scope();
    assert_ne!(
        scope_b.as_focus_node().id(),
        scope_a.as_focus_node().id(),
        "the pushed route's scope became the active scope"
    );

    // Pop back: A reclaims the active scope AND the remembered field focus.
    assert!(handle.pop());
    harness.tick();
    assert_eq!(
        manager.active_scope().as_focus_node().id(),
        scope_a.as_focus_node().id(),
        "the revealed route reclaimed the active scope"
    );
    assert!(
        field.has_primary_focus(),
        "the remembered field focus is restored on pop"
    );

    manager.unfocus();
    manager.set_active_scope(None);
}

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

    use crate::PopScope;
    use crate::navigator::PageRoute;

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
            .on_pop_invoked(move |did_pop| outcomes.lock().push(did_pop));
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

/// A disposed `PopScope` deregisters (`unregisterPopEntry`, `routes.dart:2126`):
/// after the guarded route pops, `maybe_pop` on what remains is not vetoed.
#[test]
fn a_disposed_pop_scope_stops_vetoing() {
    use crate::PopScope;
    use crate::navigator::PageRoute;

    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);
    let _second = handle.push(page(&built, "second"));
    harness.tick();

    let _guarded = handle.push(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        PopScope::new(SizedBox::new(10.0, 10.0))
            .can_pop(false)
            .into_view()
            .boxed()
    }));
    harness.tick();
    assert_eq!(handle.route_ids().len(), 3);

    assert!(handle.maybe_pop(), "vetoed while the scope is mounted");
    assert_eq!(handle.route_ids().len(), 3);

    assert!(handle.pop(), "force the guarded route off");
    harness.tick();
    assert_eq!(handle.route_ids().len(), 2);

    assert!(
        handle.maybe_pop(),
        "the second route's maybe_pop is handled"
    );
    harness.tick();
    assert_eq!(
        handle.route_ids().len(),
        1,
        "no stale veto survives the scope's dispose"
    );
}

/// A `PopScope` callback may call back into the navigator — filed by the
/// ADR-0025 critique: the fan-out used to run inside the flush, under the
/// non-reentrant history lock, so even a `can_pop()` read from the callback
/// deadlocked same-thread. Delivery now defers through
/// `FlushOutcome::pop_invoked` and `apply` fires it with no lock held, still
/// synchronously within the `pop`/`maybe_pop` call (the outcomes-ordering
/// tests above pin that), and before observers hear `didPop`
/// (`navigator.dart:3372` before `:4527`).
///
/// Red-check (the shipped bug): fan out from `ModalRoute::on_pop_invoked`
/// again — both phases hang and the watchdog fails the test.
#[test]
fn pop_scope_callbacks_may_call_back_into_the_navigator() {
    use std::time::Duration;

    use crate::PopScope;
    use crate::navigator::PageRoute;

    const BUDGET: Duration = Duration::from_secs(10);

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);

        let observed: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let observed_for_scope = Arc::clone(&observed);
        let handle_for_scope = handle.clone();
        let _guarded = handle.push(PageRoute::<i32>::new(move |_ctx, _p, _s| {
            let observed = Arc::clone(&observed_for_scope);
            let navigator = handle_for_scope.clone();
            PopScope::new(SizedBox::new(10.0, 10.0))
                .can_pop(false)
                .on_pop_invoked(move |_did_pop| {
                    // The re-entrant read that used to deadlock.
                    observed.lock().push(navigator.can_pop());
                })
                .into_view()
                .boxed()
        }));
        harness.tick();

        // Refused path: `maybe_pop` → veto → deferred fan-out.
        assert!(handle.maybe_pop());
        // Forced path: `pop` → flush Pop arm → deferred fan-out.
        assert!(handle.pop());
        harness.tick();

        assert_eq!(
            observed.lock().as_slice(),
            [true, false],
            "both callbacks re-entered the navigator; the second reads the \
             post-pop stack (the route is already finalized when its callback \
             runs — a correction against `routes.dart:90-92`)"
        );
        let _ = done.send(());
    });

    assert!(
        finished.recv_timeout(BUDGET).is_ok(),
        "a PopScope callback calling back into the navigator deadlocked — \
         the fan-out ran under the history lock"
    );
}

// ============================================================================
// Local history (routes.dart:747-973)
// ============================================================================

mod local_history {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;
    use crate::PopScope;
    use crate::navigator::PageRoute;
    use crate::navigator::local_history::{LocalHistoryEntry, LocalHistoryHandle};
    use crate::navigator::observer::NavigatorObserver;
    use crate::navigator::route::RouteId;

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
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            *self.sink.lock() = LocalHistoryHandle::maybe_of(ctx);
        }

        fn build(&self, _view: &HandleProbe, _ctx: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(10.0, 10.0)
        }
    }

    /// A modal page whose content captures the route's [`LocalHistoryHandle`]
    /// on mount.
    fn page_with_handle(
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

    /// A **single** route with an entry claims the pop: `can_pop` answers
    /// `true` through `will_handle_pop_internally` (`history.rs`'s bottom-route
    /// arm ≙ `routes.dart:970-972`), and the pop consumes the entry while the
    /// lone route stays.
    #[test]
    fn a_single_route_with_an_entry_claims_can_pop() {
        let handle = NavigatorHandle::new();
        let sink = Arc::new(Mutex::new(None));
        handle.seed_initial(page_with_handle(&sink, Duration::ZERO));
        let mut harness = mount(Navigator::new(handle.clone()));
        let local = sink.lock().clone().expect("captured");

        assert!(!handle.can_pop(), "a lone route cannot pop");
        let _entry = local.add(LocalHistoryEntry::new());
        assert!(handle.can_pop(), "an entry claims the pop internally");

        assert!(handle.maybe_pop());
        harness.tick();
        assert_eq!(handle.route_ids().len(), 1, "the lone route stays");
        assert!(!handle.can_pop(), "and the claim is gone with the entry");
    }

    /// A `PopScope` veto beats local history — Flutter checks `_popEntries`
    /// **before** the local-history layer (`routes.dart:2033-2042` over
    /// `:940-947`): `maybe_pop` refuses without consuming the entry, and a
    /// programmatic `pop()` (which skips the veto) consumes it.
    #[test]
    fn a_pop_scope_veto_beats_local_history() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);

        let sink = Arc::new(Mutex::new(None));
        let sink_for_page = Arc::clone(&sink);
        let _guarded = handle.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                PopScope::new(HandleProbe {
                    sink: Arc::clone(&sink_for_page),
                })
                .can_pop(false)
                .into_view()
                .boxed()
            })
            .transition_duration(Duration::ZERO),
        );
        harness.tick();
        let local = sink.lock().clone().expect("captured");

        let removed = Arc::new(AtomicUsize::new(0));
        let removed_for_entry = Arc::clone(&removed);
        let _entry = local.add(LocalHistoryEntry::new().on_remove(move || {
            removed_for_entry.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(handle.maybe_pop(), "the veto handles the attempt");
        assert_eq!(
            removed.load(Ordering::SeqCst),
            0,
            "the entry survives a veto"
        );
        assert_eq!(handle.route_ids().len(), 2);

        assert!(handle.pop(), "a programmatic pop skips the veto");
        harness.tick();
        assert_eq!(removed.load(Ordering::SeqCst), 1, "and consumes the entry");
        assert_eq!(handle.route_ids().len(), 2, "the route still stays");
    }

    /// `on_remove` may call back into the navigator on **both** trigger paths
    /// — the deferred in-flush pop and the direct `remove()` — because neither
    /// runs under the history lock (the `7b038dee` shape).
    ///
    /// Red-check: fire `on_remove` inside `ModalRoute::did_pop` instead of
    /// deferring — the pop phase hangs into the watchdog.
    #[test]
    fn on_remove_may_call_back_into_the_navigator() {
        const BUDGET: Duration = Duration::from_secs(10);
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let built = Built::default();
            let (handle, mut harness) = navigator_with(&built);
            let sink = Arc::new(Mutex::new(None));
            let _route = handle.push(page_with_handle(&sink, Duration::ZERO));
            harness.tick();
            let local = sink.lock().clone().expect("captured");

            let observed: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
            let observed_for_entry = Arc::clone(&observed);
            let navigator = handle.clone();
            let entry = local.add(LocalHistoryEntry::new().on_remove(move || {
                observed_for_entry.lock().push(navigator.can_pop());
            }));

            assert!(handle.maybe_pop(), "in-flush path");
            let navigator = handle.clone();
            let observed_for_entry = Arc::clone(&observed);
            let entry2 = local.add(LocalHistoryEntry::new().on_remove(move || {
                observed_for_entry.lock().push(navigator.can_pop());
            }));
            entry2.remove(); // direct path
            let _ = entry;

            assert_eq!(observed.lock().len(), 2, "both paths re-entered");
            let _ = done.send(());
        });
        assert!(
            finished.recv_timeout(BUDGET).is_ok(),
            "an on_remove calling back into the navigator deadlocked"
        );
    }

    /// `remove()` fires exactly once and is idempotent (`routes.dart:902-927`,
    /// with atomic linearization); once the last entry
    /// is gone, the next pop takes the route.
    #[test]
    fn remove_fires_once_and_releases_the_internal_claim() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let sink = Arc::new(Mutex::new(None));
        let _route = handle.push(page_with_handle(&sink, Duration::ZERO));
        harness.tick();
        let local = sink.lock().clone().expect("captured");

        let removed = Arc::new(AtomicUsize::new(0));
        let removed_for_entry = Arc::clone(&removed);
        let entry = local.add(LocalHistoryEntry::new().on_remove(move || {
            removed_for_entry.fetch_add(1, Ordering::SeqCst);
        }));

        entry.remove();
        entry.remove(); // idempotent
        assert_eq!(removed.load(Ordering::SeqCst), 1, "exactly once");

        assert!(handle.maybe_pop(), "no entry left: the route itself pops");
        harness.tick();
        assert_eq!(handle.route_ids().len(), 1);
    }

    /// A `LocalHistoryEntry` **without** an `on_remove` is still an entry: Flutter's
    /// `onRemove` is nullable (`routes.dart:711`) and the entry still absorbs the
    /// pop. FLUI's `claim()` returned `None` both for "already claimed" and for "no
    /// callback to fire", so `pop_last_deferred` treated a bare entry as a lost race
    /// and kept popping — a lone bare entry let the pop take **the whole route**,
    /// and a bare entry above a live one let one back-press eat **two** entries.
    ///
    /// Red-check: make `claim()` answer `Option<OnRemoveCallback>` again — the
    /// route pops on the first `maybe_pop` and the stays-put assertion fails.
    #[test]
    fn a_callback_less_entry_absorbs_the_pop_like_any_other() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let sink: Arc<Mutex<Option<LocalHistoryHandle>>> = Arc::new(Mutex::new(None));
        let sink_for_page = Arc::clone(&sink);
        let route = handle.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                HandleProbe {
                    sink: Arc::clone(&sink_for_page),
                }
                .into_view()
                .boxed()
            })
            .transition_duration(Duration::ZERO),
        );
        harness.tick();
        let local = sink.lock().clone().expect("captured");

        // A bare entry — no `on_remove`.
        let _bare = local.add(LocalHistoryEntry::new());

        assert!(handle.maybe_pop(), "the entry absorbs the pop");
        harness.tick();
        assert_eq!(
            handle.route_ids().len(),
            2,
            "the route stays: a callback-less entry is still an entry"
        );
        assert_eq!(
            route.try_take(),
            None,
            "and the route's future must not resolve"
        );

        // With the entry gone, the next pop takes the route.
        assert!(handle.maybe_pop());
        harness.tick();
        assert_eq!(handle.route_ids().len(), 1);
    }

    /// One back-press consumes exactly **one** entry, even when the top entry has no
    /// callback: the bare entry must not be skipped as a race-loser, which would let
    /// a single pop eat the entry below it and fire *its* `on_remove`.
    #[test]
    fn one_pop_consumes_exactly_one_entry() {
        use std::sync::atomic::AtomicUsize;

        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let sink: Arc<Mutex<Option<LocalHistoryHandle>>> = Arc::new(Mutex::new(None));
        let sink_for_page = Arc::clone(&sink);
        let _route = handle.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                HandleProbe {
                    sink: Arc::clone(&sink_for_page),
                }
                .into_view()
                .boxed()
            })
            .transition_duration(Duration::ZERO),
        );
        harness.tick();
        let local = sink.lock().clone().expect("captured");

        let deep_removals = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&deep_removals);
        let _deep = local.add(LocalHistoryEntry::new().on_remove(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let _bare_on_top = local.add(LocalHistoryEntry::new());

        assert!(handle.maybe_pop(), "the bare entry absorbs this pop");
        harness.tick();
        assert_eq!(
            deep_removals.load(Ordering::SeqCst),
            0,
            "the entry below must not be consumed by the same back-press"
        );
        assert_eq!(handle.route_ids().len(), 2, "and the route stays");

        assert!(handle.maybe_pop(), "the second pop takes the deeper entry");
        harness.tick();
        assert_eq!(deep_removals.load(Ordering::SeqCst), 1);
        assert_eq!(handle.route_ids().len(), 2, "the route still stays");
    }

    /// Route teardown severs: live entries drop **without** firing (Flutter
    /// GC-drops `_localHistory`; dispose never touches it), late adds are
    /// inert, and a late `remove()` is a no-op (FLUI divergence, named in the
    /// module docs — keeping callbacks past dispose is the Arc-cycle leak).
    #[test]
    fn dispose_severs_live_entries_without_firing() {
        let built = Built::default();
        let (handle, mut harness) = navigator_with(&built);
        let sink = Arc::new(Mutex::new(None));
        let _route = handle.push(page_with_handle(&sink, Duration::ZERO));
        harness.tick();
        let local = sink.lock().clone().expect("captured");

        let removed = Arc::new(AtomicUsize::new(0));
        let removed_for_entry = Arc::clone(&removed);
        let entry = local.add(LocalHistoryEntry::new().on_remove(move || {
            removed_for_entry.fetch_add(1, Ordering::SeqCst);
        }));

        // Remove the whole route out from under its entry.
        let ids = handle.route_ids();
        assert!(handle.remove_route(ids[1]));
        harness.tick();
        assert_eq!(handle.route_ids().len(), 1);
        assert_eq!(
            removed.load(Ordering::SeqCst),
            0,
            "a dying route's entries drop un-fired"
        );

        entry.remove();
        assert_eq!(removed.load(Ordering::SeqCst), 0, "late remove is a no-op");

        let late =
            local.add(LocalHistoryEntry::new().on_remove(|| {
                unreachable!("BUG: an entry added to a disposed route must be inert")
            }));
        late.remove();
    }
}

/// A route transition activates its focus scope, which **restores the remembered
/// focus** — moving the primary focus fires user focus-change listeners (the
/// `Focus` widget's `on_focus_change`, and its rebuild). Those must not run under
/// the navigator's history lock: the mutex is not reentrant, so a listener that
/// touches the navigator (even `can_pop()`) deadlocks the same thread.
///
/// This is the deadlock class already fixed for the `PopScope` fan-out; the focus
/// path never got the same treatment — `activate_focus_scope` was called straight
/// from `did_push` / `did_add` / `did_pop_next` / `did_change_next`, every one of
/// which runs inside the flush.
///
/// The pop is what triggers it: the revealed route restores the field it
/// remembers, so the listener fires from inside the flush.
///
/// Red-check: call `activate_focus_scope` from those hooks again — the pop hangs.
#[test]
fn a_focus_listener_may_call_back_into_the_navigator_during_a_transition() {
    use std::time::Duration;

    use flui_interaction::routing::{FocusManager, FocusNode};

    use crate::navigator::PageRoute;
    use crate::{Focus, SizedBox as Box2};

    let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let built = Built::default();
    let (handle, mut harness) = navigator_with(&built);

    let observed: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
    let field = FocusNode::with_debug_label("deadlock-field");
    let field_for_page = Arc::clone(&field);
    let observed_for_page = Arc::clone(&observed);
    let navigator = handle.clone();

    let _a = handle.push(
        PageRoute::<i32>::new(move |_ctx, _p, _s| {
            let observed = Arc::clone(&observed_for_page);
            let navigator = navigator.clone();
            Focus::new(Box2::new(10.0, 10.0))
                .focus_node(Arc::clone(&field_for_page))
                .on_focus_change(move |_focused| {
                    // The re-entrant read that deadlocks under the lock.
                    observed.lock().push(navigator.can_pop());
                })
                .into_view()
                .boxed()
        })
        .transition_duration(Duration::ZERO),
    );
    harness.tick();
    field.request_focus();

    // Cover it, then reveal it: the reveal restores the remembered focus
    // from inside the flush, firing the user listener.
    let _b = handle.push(
        PageRoute::<i32>::new(|_ctx, _p, _s| Box2::new(10.0, 10.0).into_view().boxed())
            .transition_duration(Duration::ZERO),
    );
    harness.tick();
    assert!(handle.pop());
    harness.tick();

    assert!(
        !observed.lock().is_empty(),
        "the focus listener must actually have fired — otherwise this test \
         proves nothing about where it runs"
    );

    manager.unfocus();
}
