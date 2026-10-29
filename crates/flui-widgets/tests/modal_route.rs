//! Tests for the private [`ModalRoute`], reached through the temporary
//! `flui_widgets::__test_access` path (ADR-0083 §4).
//!
//! # Scenarios
//!
//! The route's opacity on transition completion, its offstage state, state
//! maintenance, and overlay-entry creation. Expected values are fixed by the
//! documented contract, not by running this code.
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
use flui_testing::widgets::StateCreations;
use flui_view::prelude::*;

use flui_widgets::__test_access::{
    ModalRoute, NavigatorProbe as _, OverlayEntryProbe as _, TransitionHandle,
};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{Navigator, NavigatorHandle, RouteId, SimpleRoute};

use crate::common::harness::{Harness, mount};

const FRAME: Duration = Duration::from_millis(300);

/// Counts how many times a route's page builder ran.
#[derive(Clone, Default)]
struct Built(Arc<AtomicUsize>);

/// A modal whose page is a state-counting [`StateCreations`]: "was this
/// route's subtree destroyed and rebuilt?" is what `maintain_state == false` changes.
fn modal(built: &Built, creations: &Arc<AtomicUsize>) -> ModalRoute<i32> {
    let (built, creations) = (built.clone(), Arc::clone(creations));
    ModalRoute::new(
        FRAME,
        Rc::new(
            move |_ctx: &dyn BuildContext, _animation: &_, _secondary: &_| {
                built.0.fetch_add(1, Ordering::Relaxed);
                StateCreations::counting(&creations).into_view().boxed()
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
/// This drives the transition by hand rather than awaiting the `AnimationRunFuture`
/// `did_push` returns — `set_value(1.0)` fires the `ForwardSettled` status, which is
/// queued by the animation listener and drained from owner-local `ModalScope`
/// build. The resulting `OverlayEntry.opaque` write schedules the overlay
/// rebuild that applies occlusion on the following tick.
fn complete_entrance(transition: &TransitionHandle, harness: &mut Harness) {
    let controller = transition
        .controller()
        .expect("install must have created the controller");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::ForwardSettled);
    harness.tick();
    harness.tick();
}

// ============================================================================
// opaque — applied when the transition completes
// ============================================================================

/// On completion the route's first overlay entry takes the route's `opaque` value.
///
/// Previously this write had nowhere to go. Now it drops the route below out of
/// the tree entirely, because that route has no `maintain_state`.
pub(crate) fn modal_opaque_route_occludes_the_route_below_once_its_transition_completes() {
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
// maintainState
// ============================================================================

// ============================================================================
// changedInternalState
// ============================================================================

// ============================================================================
// offstage / barrier — the render objects a modal builds
// ============================================================================

/// A non-dismissible modal builds an `AbsorbPointer` and no gesture recogniser;
/// a dismissible one wraps it in a `GestureDetector` whose tap pops the route
/// (`on_dismiss` or `maybe_pop`).
///
/// FLUI has no `ModalBarrier`, no `BlockSemantics` and no `barrierLabel`; the
/// barrier absorbs pointers only. See the module docs.
pub(crate) fn modal_barrier_absorbs_pointers_and_a_dismissible_one_adds_a_gesture_detector() {
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
// Teardown
// ============================================================================

/// **Dropping the last `NavigatorHandle` after an ordinary push clears the modal
/// registry** (#1066). A modal route is pushed and settles, so the navigator's
/// modal registry holds its `ModalHandle`, and through it the route's page
/// builder. The window goes, then the application drops its handle, the last
/// one: the registry's entry goes with the route, and the page builder's
/// captures are released.
///
/// Characterization: this holds on the code as it is.
pub(crate) fn dropping_the_last_navigator_handle_after_push_clears_the_modal_registry() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();
    let built = Built::default();
    let creations = Arc::new(AtomicUsize::new(0));
    let route = modal(&built, &creations);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);
    drop(transition);

    let top = *navigator.route_ids().last().expect("the modal is on top");
    assert!(
        navigator.route_modal(top).is_some(),
        "precondition: the modal registry holds the pushed route"
    );
    assert_eq!(creations.load(Ordering::Relaxed), 1, "its page is built");

    drop(harness);
    assert!(
        Arc::strong_count(&built.0) > 1,
        "the application's handle still owns the route once the window is gone"
    );

    drop(navigator);
    assert_eq!(
        Arc::strong_count(&built.0),
        1,
        "the last handle's drop releases the route and its modal registry entry"
    );
    assert_eq!(Arc::strong_count(&creations), 1);
}
