//! Tests for the private [`ModalRoute`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/lib/src/widgets/routes.dart` — `TransitionRoute.
//! _handleStatusChanged` (`:293-321`), `ModalRoute.offstage` (`:1949-1962`),
//! `ModalRoute.changedInternalState` (`:2221-2231`), `createOverlayEntries`
//! (`:2350-2356`). Expected values are read from the reference, not from running
//! this code.
//!
//! # What is *not* proven here
//!
//! That `RenderOffstage` lays its child out at real geometry and then suppresses
//! paint, hit-test and semantics is pinned by `harness_offstage_*` in
//! `flui-objects`. That `RenderTheater` skips its first
//! `skip_count` children is pinned by `harness_theater_*`. These
//! tests prove the **wiring**: that a `ModalRoute` puts those render objects in
//! the tree with the flags its own state says they should have.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationStatus};
use flui_types::Color;
use flui_view::prelude::*;

use super::modal_route::{ModalHandle, ModalRoute};
use super::navigator::{Navigator, NavigatorHandle};
use super::overlay_route::SimpleRoute;
use super::route::RouteId;
use crate::SizedBox;
use crate::test_harness::{Harness, mount};

const FRAME: Duration = Duration::from_millis(300);

/// Counts how many times a route's page builder ran.
#[derive(Clone, Default)]
struct Built(Arc<AtomicUsize>);

impl Built {
    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

/// Counts `create_state` on a leaf, so "was this route's subtree destroyed and
/// rebuilt?" is observable — the whole point of `maintain_state == false`.
#[derive(Clone)]
struct Probe(Arc<AtomicUsize>);

impl View for Probe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        self.0.fetch_add(1, Ordering::Relaxed);
        ProbeState
    }
}

struct ProbeState;

impl ViewState<Probe> for ProbeState {
    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A modal whose page is a state-counting [`Probe`].
fn modal(built: &Built, creations: &Arc<AtomicUsize>) -> ModalRoute<i32> {
    let (built, creations) = (built.clone(), Arc::clone(creations));
    ModalRoute::new(
        FRAME,
        Rc::new(
            move |_ctx: &dyn BuildContext, _animation: &_, _secondary: &_| {
                built.0.fetch_add(1, Ordering::Relaxed);
                Probe(Arc::clone(&creations)).into_view().boxed()
            },
        ),
    )
}

fn plain_page() -> SimpleRoute<i32> {
    SimpleRoute::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed())
}

/// A navigator with `bottom` seeded, mounted and settled.
fn navigator_with_seed() -> (NavigatorHandle, Harness, RouteId) {
    let handle = NavigatorHandle::new();
    handle.seed_initial(plain_page());
    let harness = mount(Navigator::new(handle.clone()));
    let bottom = handle.route_ids()[0];
    (handle, harness, bottom)
}

/// Run the modal's entrance transition to completion, then settle the owner
/// status bridge and the overlay rebuild it schedules.
///
/// FLUI's `AnimationController` returns no `TickerFuture`, so a test drives the
/// transition by hand — `set_value(1.0)` fires the `Completed` status, which is
/// queued by the animation listener and drained from owner-local `ModalScope`
/// build. The resulting `OverlayEntry.opaque` write schedules the overlay
/// rebuild that applies occlusion on the following tick.
fn complete_entrance(
    transition: &super::transition_route::TransitionHandle,
    harness: &mut Harness,
) {
    let controller = transition
        .controller()
        .expect("install must have created the controller");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::Completed);
    harness.tick();
    harness.tick();
}

// ============================================================================
// opaque — `_handleStatusChanged` (routes.dart:293-321)
// ============================================================================

/// `case completed: overlayEntries.first.opaque = opaque` (`routes.dart:296`).
///
/// Previously this write had nowhere to go. Now it drops the route below out of
/// the tree entirely, because that route has no `maintain_state`.
#[test]
fn modal_opaque_route_occludes_the_route_below_once_its_transition_completes() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator
        .entry_of(bottom)
        .expect("seeded route has an entry");

    let route = modal(&Built::default(), &Arc::new(AtomicUsize::new(0))).opaque(true);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();

    let top = *navigator.route_ids().last().expect("the modal is on top");
    let top_entry = navigator.entry_of(top).expect("the modal has an entry");

    assert!(!top_entry.opaque(), "a route mid-transition never occludes");
    assert!(bottom_entry.is_mounted());

    complete_entrance(&transition, &mut harness);

    assert!(top_entry.opaque(), "completed: opaque = opaque");
    assert!(
        !bottom_entry.is_mounted(),
        "the occluded route has no maintain_state, so it left the tree"
    );
}

/// `case forward: case reverse: overlayEntries.first.opaque = false`
/// (`routes.dart:303-305`). A moving route shows the ones beneath it through the
/// transition, so it must un-occlude them.
#[test]
fn modal_opaque_route_clears_opaque_while_it_moves() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator
        .entry_of(bottom)
        .expect("seeded route has an entry");

    let route = modal(&Built::default(), &Arc::new(AtomicUsize::new(0))).opaque(true);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    let top = *navigator.route_ids().last().expect("the modal is on top");
    let top_entry = navigator.entry_of(top).expect("the modal has an entry");
    assert!(top_entry.opaque());

    // Start moving away: `reverse()` from 1.0 fires `Reverse`.
    let controller = transition.controller().expect("installed");
    controller.reverse().expect("reverse from 1.0");
    harness.tick();
    harness.tick();

    assert_eq!(controller.status(), AnimationStatus::Reverse);
    assert!(!top_entry.opaque(), "a moving route clears opaque");
    assert!(
        bottom_entry.is_mounted(),
        "the route below is rebuilt, so it shows through the transition"
    );
}

/// A non-opaque modal — Flutter's `PopupRoute.opaque => false`
/// (`routes.dart:2391`) — never occludes, even when settled.
#[test]
fn modal_non_opaque_route_never_occludes() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator
        .entry_of(bottom)
        .expect("seeded route has an entry");

    let route = modal(&Built::default(), &Arc::new(AtomicUsize::new(0)));
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    let top = *navigator.route_ids().last().expect("the modal is on top");
    assert!(!navigator.entry_of(top).expect("entry").opaque());
    assert!(bottom_entry.is_mounted());
}

// ============================================================================
// maintainState — routes.dart:1893, :2230
// ============================================================================

/// `maintainState == false`: an occluded route is unmounted and its subtree state
/// **destroyed**. Uncovering it creates fresh state. This is the contract routes
/// below a `PageRoute` rely on, and it is what `RenderTheater` skip-count support
/// made observable.
#[test]
fn modal_covered_route_without_maintain_state_is_unmounted_and_loses_its_state() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let creations = Arc::new(AtomicUsize::new(0));
    let covered = modal(&Built::default(), &creations).maintain_state(false);
    let _covered_result = navigator.push(covered);
    harness.tick();
    assert_eq!(creations.load(Ordering::Relaxed), 1);

    let coverer = modal(&Built::default(), &Arc::new(AtomicUsize::new(0))).opaque(true);
    let coverer_transition = coverer.transition_handle();
    let _coverer_result = navigator.push(coverer);
    harness.tick();
    complete_entrance(&coverer_transition, &mut harness);

    let covered_id = navigator.route_ids()[1];
    let covered_entry = navigator.entry_of(covered_id).expect("entry");
    assert!(
        !covered_entry.is_mounted(),
        "maintain_state == false: the covered route leaves the tree"
    );

    // Uncover it: reversing the coverer clears its `opaque`.
    coverer_transition
        .controller()
        .expect("installed")
        .reverse()
        .expect("reverse from 1.0");
    harness.tick();
    harness.tick();

    assert!(covered_entry.is_mounted());
    assert_eq!(
        creations.load(Ordering::Relaxed),
        2,
        "the destroyed subtree is rebuilt with fresh state"
    );
}

/// `maintainState == true` (`_modalScope.maintainState = maintainState`,
/// `routes.dart:2230`): the covered route stays built. The overlay then hands it
/// to `RenderTheater` as one of the first `skip_count` children, which is where
/// it stops being laid out — proven by `harness_theater_*`, not here.
#[test]
fn modal_covered_route_with_maintain_state_stays_mounted() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let creations = Arc::new(AtomicUsize::new(0));
    let covered = modal(&Built::default(), &creations).maintain_state(true);
    let _covered_result = navigator.push(covered);
    harness.tick();

    let coverer = modal(&Built::default(), &Arc::new(AtomicUsize::new(0))).opaque(true);
    let coverer_transition = coverer.transition_handle();
    let _coverer_result = navigator.push(coverer);
    harness.tick();
    complete_entrance(&coverer_transition, &mut harness);

    let covered_id = navigator.route_ids()[1];
    assert!(
        navigator.entry_of(covered_id).expect("entry").is_mounted(),
        "maintain_state == true keeps the covered route in the tree"
    );
    assert_eq!(
        creations.load(Ordering::Relaxed),
        1,
        "and its state is never destroyed"
    );
}

/// `install()` publishes `maintainState` onto the entry, as Flutter does at
/// `createOverlayEntries` (`routes.dart:2353-2355`). Without it the flag would
/// live only on the route and the overlay would read the `false` default.
#[test]
fn modal_install_publishes_maintain_state_onto_the_overlay_entry() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let _result = navigator.push(modal(&Built::default(), &Arc::new(AtomicUsize::new(0))));
    harness.tick();

    let id = *navigator.route_ids().last().expect("pushed");
    assert!(
        navigator.entry_of(id).expect("entry").maintain_state(),
        "ModalRoute defaults maintain_state to true, and install must publish it"
    );
}

// ============================================================================
// changedInternalState — routes.dart:2221-2231
// ============================================================================

/// `changedInternalState` republishes `maintainState` onto the entry
/// (`routes.dart:2230`) and marks it dirty (`:2228`).
///
/// The rebuild seen *here* comes from the `maintainState` write, which is
/// `_didChangeEntryOpacity` — an overlay `setState`, as in Flutter. The separate
/// `mark_entry_needs_build()` is pinned by
/// [`modal_setting_offstage_to_the_same_value_is_a_noop`], where `maintainState`
/// does not change and the entry must still rebuild. A red-check that deleted
/// `mark_entry_needs_build` left *this* test green, which is how that was found.
#[test]
fn modal_changed_internal_state_rebuilds_only_this_entry_and_republishes_maintain_state() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let built = Built::default();
    let route = modal(&built, &Arc::new(AtomicUsize::new(0)));
    let modal_handle: ModalHandle = route.handle();
    let _result = navigator.push(route);
    harness.tick();
    assert_eq!(built.get(), 1);

    let id = *navigator.route_ids().last().expect("pushed");
    let entry = navigator.entry_of(id).expect("entry");
    assert!(entry.maintain_state());

    modal_handle.set_maintain_state(false);
    harness.tick();

    assert!(
        !entry.maintain_state(),
        "changedInternalState republishes maintainState onto the entry"
    );
    assert_eq!(built.get(), 2, "and the entry rebuilds");
}

/// `if (_offstage == value) return;` (`routes.dart:1952-1954`) — a no-op setter
/// must not schedule a rebuild.
#[test]
fn modal_setting_offstage_to_the_same_value_is_a_noop() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let built = Built::default();
    let route = modal(&built, &Arc::new(AtomicUsize::new(0)));
    let modal_handle = route.handle();
    let _result = navigator.push(route);
    harness.tick();
    assert_eq!(built.get(), 1);

    modal_handle.set_offstage(false);
    harness.tick();
    assert_eq!(built.get(), 1, "no change, no rebuild");

    modal_handle.set_offstage(true);
    harness.tick();
    assert_eq!(built.get(), 2, "a real change rebuilds the entry");
}

// ============================================================================
// offstage / barrier — the render objects a modal builds
// ============================================================================

/// The page is always wrapped in an [`Offstage`](crate::Offstage), so a
/// `set_offstage(true)` route keeps its real geometry: `RenderOffstage` is still
/// in the render tree, laid out, and its child with it. What `RenderOffstage`
/// then suppresses — paint, hit-test, semantics — is pinned by
/// `harness_offstage_*` in `flui-objects`, and is **not** re-proven here.
///
/// The barrier is the observable half: `buildModalBarrier` skips it when the
/// route is offstage (`routes.dart:2301`), so the `ColoredBox` it paints — a
/// `RenderDecoratedBox` — disappears.
#[test]
fn modal_offstage_keeps_the_page_but_drops_the_barrier() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let creations = Arc::new(AtomicUsize::new(0));
    let route = modal(&Built::default(), &creations).barrier_color(Color::RED);
    let modal_handle = route.handle();
    let _result = navigator.push(route);
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|name| name.ends_with("RenderOffstage")),
        "the page is wrapped in an Offstage; render objects: {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("RenderDecoratedBox")),
        "the barrier paints its colour while the route is onstage: {names:?}"
    );
    assert!(
        names.iter().any(|name| name.ends_with("RenderTheater")),
        "the overlay builds a Theater, not a Stack: {names:?}"
    );

    modal_handle.set_offstage(true);
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|name| name.ends_with("RenderOffstage")),
        "an offstage page is still laid out — its render object stays: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.ends_with("RenderDecoratedBox")),
        "an offstage route builds no barrier: {names:?}"
    );
    assert_eq!(
        creations.load(Ordering::Relaxed),
        1,
        "going offstage must not destroy the page's state"
    );
}

/// A non-dismissible modal builds an `AbsorbPointer` and no gesture recogniser;
/// a dismissible one wraps it in a `GestureDetector` whose tap pops the route
/// (`modal_barrier.dart`'s `onDismiss ?? Navigator.maybePop`).
///
/// **Divergence, not parity.** FLUI has no `ModalBarrier`, no `BlockSemantics`
/// and no `barrierLabel`; the barrier absorbs pointers only. See the module docs.
#[test]
fn modal_barrier_absorbs_pointers_and_a_dismissible_one_adds_a_gesture_detector() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();
    let _result = navigator.push(modal(&Built::default(), &Arc::new(AtomicUsize::new(0))));
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names
            .iter()
            .any(|name| name.ends_with("RenderAbsorbPointer")),
        "every modal barrier absorbs pointers: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.ends_with("RenderListener")),
        "a non-dismissible barrier installs no gesture recogniser: {names:?}"
    );

    let (navigator, mut harness, _bottom) = navigator_with_seed();
    let dismissible =
        modal(&Built::default(), &Arc::new(AtomicUsize::new(0))).barrier_dismissible(true);
    let _result = navigator.push(dismissible);
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|name| name.ends_with("RenderListener")),
        "a dismissible barrier listens for the dismiss tap: {names:?}"
    );
}

// ============================================================================
// Privacy
// ============================================================================

/// `ModalRoute` and `ModalHandle` stay private: they are the
/// implementation `PageRoute` / `PopupRoute` are built on, and exporting them as
/// extensible bases is a separate sign-off.
///
/// Red-check: add `pub use modal_route::ModalRoute;` to `navigator/mod.rs`.
#[test]
fn modal_route_is_not_exported() {
    super::export_guard::assert_not_exported(
        "lib.rs",
        include_str!("../lib.rs"),
        &["ModalRoute", "ModalHandle", "ModalScope"],
    );
    super::export_guard::assert_not_exported(
        "navigator/mod.rs",
        include_str!("mod.rs"),
        &["ModalRoute", "ModalHandle", "ModalScope"],
    );
}

/// [`ModalHandle`] is an owned capability: every clone names the
/// same route, so a handle taken before `push_bound` still drives it afterwards.
#[test]
fn modal_handle_is_cloneable_and_shares_state() {
    let route: ModalRoute<i32> = ModalRoute::new(
        FRAME,
        Rc::new(|_ctx: &dyn BuildContext, _a: &_, _s: &_| {
            SizedBox::new(10.0, 10.0).into_view().boxed()
        }),
    );
    let a = route.handle();
    let b: ModalHandle = a.clone();

    assert!(!a.offstage());
    // Unpushed: no binding, so `changed_internal_state` is inert; the flag flips
    // anyway, which is what makes a pre-push `set_offstage` legal.
    b.set_offstage(true);
    assert!(a.offstage(), "both handles name the same route");
}
