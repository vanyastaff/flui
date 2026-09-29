//! Tests for the private [`ModalRoute`], reached through the temporary
//! `flui_widgets::__test_access` path (ADR-0083 §4). Its export boundary and
//! handle keep unit tests in `src/navigator/modal_route_tests.rs`.
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
use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::routing::FocusNode;
use flui_painting::styling::Color;
use flui_view::prelude::*;

use flui_widgets::__test_access::{
    ModalRoute, NavigatorProbe as _, OverlayEntryProbe as _, TransitionHandle,
};
use flui_widgets::navigator::{Navigator, NavigatorHandle, RouteId, SimpleRoute};
use flui_widgets::{Column, Focus, SizedBox};

use crate::common::harness::{Harness, mount};

const FRAME: Duration = Duration::from_millis(300);

/// Counts how many times a route's page builder ran.
#[derive(Clone, Default)]
struct Built(Arc<AtomicUsize>);

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

fn focus_modal(first: Rc<FocusNode>, second: Rc<FocusNode>) -> ModalRoute<i32> {
    ModalRoute::new(
        FRAME,
        Rc::new(move |_ctx, _animation, _secondary| {
            Column::new(vec![
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Rc::clone(&first))
                    .into_view()
                    .boxed(),
                Focus::new(SizedBox::new(10.0, 10.0))
                    .focus_node(Rc::clone(&second))
                    .into_view()
                    .boxed(),
            ])
            .into_view()
            .boxed()
        }),
    )
    .maintain_state(true)
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
/// This drives the transition by hand rather than awaiting the `TickerFuture`
/// `did_push` returns — `set_value(1.0)` fires the `Completed` status, which is
/// queued by the animation listener and drained from owner-local `ModalScope`
/// build. The resulting `OverlayEntry.opaque` write schedules the overlay
/// rebuild that applies occlusion on the following tick.
fn complete_entrance(transition: &TransitionHandle, harness: &mut Harness) {
    let controller = transition
        .controller()
        .expect("install must have created the controller");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::Completed);
    harness.tick();
    harness.tick();
}

#[test]
fn root_tab_traversal_stays_inside_the_top_mounted_route() {
    let bottom_first = FocusNode::with_debug_label("bottom first");
    let bottom_second = FocusNode::with_debug_label("bottom second");
    let top_first = FocusNode::with_debug_label("top first");
    let top_second = FocusNode::with_debug_label("top second");
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(focus_modal(
        Rc::clone(&bottom_first),
        Rc::clone(&bottom_second),
    ));
    let mut harness = mount(Navigator::new(navigator.clone()));
    assert!(bottom_first.has_primary_focus());

    let _result = navigator.push(focus_modal(Rc::clone(&top_first), Rc::clone(&top_second)));
    harness.tick();
    assert!(
        top_first.has_primary_focus(),
        "activating the new route restores focus inside its own scope"
    );

    assert!(harness.focus_manager().dispatch_key_event(&KeyEvent {
        state: KeyState::Down,
        key: Key::Named(NamedKey::Tab),
        modifiers: Modifiers::empty(),
        ..KeyEvent::default()
    }));
    assert!(top_second.has_primary_focus());
    assert!(!bottom_first.has_primary_focus());
    assert!(!bottom_second.has_primary_focus());
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

// ============================================================================
// changedInternalState — routes.dart:2221-2231
// ============================================================================

// ============================================================================
// offstage / barrier — the render objects a modal builds
// ============================================================================

/// The page is always wrapped in an [`Offstage`](flui_widgets::Offstage), so a
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
