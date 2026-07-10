//! ADR-0021 U2 **acceptance gate**: can a post-frame callback measure a route that
//! was forced offstage in the *same* frame?
//!
//! # The question, and why it gates U2
//!
//! `HeroController._maybeStartHeroTransition` (`heroes.dart:964-973`) does exactly
//! two things before it measures anything:
//!
//! ```dart
//! toRoute.offstage = toRoute.animation!.value == 0.0;   // :967
//! WidgetsBinding.instance.addPostFrameCallback((_) {     // :968
//!   _startHeroTransition(fromRoute, toRoute, …);         // measures both heroes
//! });
//! ```
//!
//! with the comment *"Once this frame completes, we'll know where the heroes in the
//! `to` route are going to end up"* (`:964-966`). The whole design rests on that
//! claim. `ModalRoute.offstage`'s setter calls `setState` + `changedInternalState`
//! (`routes.dart:1951-1962`, `:2221-2231`), marking the route's subtree dirty, and
//! the frame's build + layout then runs before the post-frame phase.
//!
//! If FLUI's frame order does not deliver that, U2's `PostFrameHandle` seam would be
//! built on a false assumption and every flight would start from a stale rect.
//!
//! # Why the route under test is a *newly pushed* one
//!
//! A route already laid out in an earlier frame has committed geometry regardless of
//! this frame's ordering — such a test would pass for the wrong reason. Here the
//! `PageRoute` is pushed and forced offstage **between** frames, so it has never been
//! built or laid out. The post-frame callback either sees geometry this frame's
//! pipeline produced, or it sees nothing.
//!
//! U1.5 (ADR-0021 §7c) is what makes this answerable at all: before it,
//! `pump_frame` never drained the post-frame queue.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::RenderId;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_view::prelude::*;
use parking_lot::{Mutex, RwLock};

use super::navigator::{Navigator, NavigatorHandle};
use super::overlay_route::{RouteAnimation, SimpleRoute};
use super::page_route::PageRoute;
use crate::SizedBox;
use crate::test_harness::mount;

fn leaf(_ctx: &dyn BuildContext, _a: &RouteAnimation, _s: &RouteAnimation) -> BoxedView {
    SizedBox::new(30.0, 18.0).into_view().boxed()
}

/// Every `RenderOffstage` node in the tree. A `ModalRoute` wraps its page in one
/// (`modal_route.rs::build_scope`), so its presence *is* the route's page having
/// been built, and its geometry is that page's committed layout.
/// What a post-frame callback recorded about one offstage route's page.
type MeasuredNode = (RenderId, Option<Size>);

fn offstage_nodes(owner: &PipelineOwner) -> Vec<RenderId> {
    owner
        .render_tree()
        .iter()
        .filter(|(_, node)| node.debug_name().ends_with("RenderOffstage"))
        .map(|(id, _)| id)
        .collect()
}

/// **The gate.** A `PageRoute` pushed and forced offstage between frames is built,
/// laid out, and its geometry committed — all before the post-frame callback of the
/// very next `pump_frame` runs.
///
/// Red-check: reorder `Scheduler::drive_frame` to `end_frame()` before `pipeline()`
/// (the pre-U1.5 production order). The callback then observes zero offstage nodes
/// and this test fails.
#[test]
fn a_route_forced_offstage_has_committed_geometry_in_the_same_frames_post_frame_callback() {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut harness = mount(Navigator::new(navigator.clone()));

    let owner = harness.pipeline_owner();
    assert!(
        offstage_nodes(&owner.read()).is_empty(),
        "the seeded SimpleRoute has no ModalRoute Offstage wrapper"
    );

    // Push a route that has NEVER been built, and force it offstage — Flutter's
    // `toRoute.offstage = …` (`heroes.dart:967`), before its post-frame callback.
    let route = PageRoute::<i32>::new(leaf);
    let modal = route.modal_handle();
    let _result = navigator.push(route);
    modal.set_offstage(true);

    assert!(
        offstage_nodes(&owner.read()).is_empty(),
        "the pushed route must not be laid out until a frame runs — otherwise this \\
         test would pass on stale geometry"
    );

    // What the post-frame callback saw, recorded from inside the frame.
    let calls = Arc::new(AtomicUsize::new(0));
    let observed: Arc<Mutex<Vec<MeasuredNode>>> = Arc::new(Mutex::new(Vec::new()));

    let calls_cb = Arc::clone(&calls);
    let observed_cb = Arc::clone(&observed);
    let owner_cb: Arc<RwLock<PipelineOwner>> = Arc::clone(&owner);
    harness
        .scheduler()
        .add_post_frame_callback(Box::new(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            let owner = owner_cb.read();
            *observed_cb.lock() = offstage_nodes(&owner)
                .into_iter()
                .map(|id| (id, owner.box_size(id)))
                .collect();
        }));

    // One real frame, through `HeadlessBinding::pump_frame`. The callback is never
    // invoked by this test.
    harness.tick();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "pump_frame must drain the post-frame queue exactly once"
    );

    let seen = observed.lock().clone();
    assert_eq!(
        seen.len(),
        1,
        "the offstage route's page must have been BUILT during this frame; saw {seen:?}"
    );
    let (_id, size) = seen[0];
    assert!(
        size.is_some(),
        "the offstage route's page must have COMMITTED LAYOUT by the post-frame \\
         phase of the same frame; got {size:?}"
    );
}

/// The other half of the claim: the geometry the callback sees is *real*, not a
/// zero-sized placeholder. `RenderOffstage` lays its child out under the incoming
/// constraints and reports `constraints.smallest()` (ADR-0020 U5.0); under the
/// theater's tight constraints that is the full route size, so an offstage page is
/// measurable — which is the entire point of `ModalRoute.offstage`.
#[test]
fn the_offstage_routes_committed_geometry_is_real_not_zero() {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut harness = mount(Navigator::new(navigator.clone()));
    let owner = harness.pipeline_owner();

    let route = PageRoute::<i32>::new(leaf);
    let modal = route.modal_handle();
    let _result = navigator.push(route);
    modal.set_offstage(true);

    let observed: Arc<Mutex<Option<Size>>> = Arc::new(Mutex::new(None));
    let observed_cb = Arc::clone(&observed);
    let owner_cb = Arc::clone(&owner);
    harness
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            let owner = owner_cb.read();
            *observed_cb.lock() = offstage_nodes(&owner)
                .first()
                .and_then(|id| owner.box_size(*id));
        }));

    harness.tick();

    let size = observed.lock().expect("the offstage page was laid out");
    assert!(
        size.width.0 > 0.0 && size.height.0 > 0.0,
        "an offstage route must be laid out at real geometry, got {size:?}"
    );
}

/// The second, separable claim — and the one HeroController leans on for a route
/// that is **already mounted** (every pop, and the push case once the `to` route
/// has been built by an earlier frame).
///
/// `ModalRoute.offstage`'s setter marks the route's subtree dirty
/// (`routes.dart:1955-1962` → `changedInternalState`, `:2221-2231`). That rebuild
/// must land in the *same* frame whose post-frame callback measures, or the
/// measurement reads the onstage layout.
///
/// Written because a red-check exposed the gap: deleting
/// `changed_internal_state`'s `mark_entry_needs_build()` left
/// `a_route_forced_offstage_has_committed_geometry_in_the_same_frames_post_frame_callback`
/// **green** — the `push` alone was building the route there, so that test says
/// nothing about the offstage dirty.
///
/// Red-check: delete `mark_entry_needs_build()` from
/// `modal_route::changed_internal_state`.
#[test]
fn setting_offstage_on_a_mounted_route_rebuilds_it_before_the_post_frame_callback() {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut harness = mount(Navigator::new(navigator.clone()));

    // A page that counts how many times it has been built.
    let builds = Arc::new(AtomicUsize::new(0));
    let builds_for_page = Arc::clone(&builds);
    let route = PageRoute::<i32>::new(move |_ctx, _animation, _secondary| {
        builds_for_page.fetch_add(1, Ordering::SeqCst);
        SizedBox::new(30.0, 18.0).into_view().boxed()
    });
    let modal = route.modal_handle();
    let _result = navigator.push(route);

    // Settle: the route is mounted and laid out by an earlier frame.
    harness.tick();
    let builds_before = builds.load(Ordering::SeqCst);
    assert!(
        builds_before > 0,
        "the route is mounted before the frame under test"
    );

    // Now force it offstage, as `HeroController` does (`heroes.dart:967`).
    modal.set_offstage(true);

    let builds_seen_by_callback = Arc::new(AtomicUsize::new(0));
    let seen_cb = Arc::clone(&builds_seen_by_callback);
    let builds_cb = Arc::clone(&builds);
    harness
        .scheduler()
        .add_post_frame_callback(Box::new(move |_| {
            seen_cb.store(builds_cb.load(Ordering::SeqCst), Ordering::SeqCst);
        }));

    harness.tick();

    assert!(
        builds_seen_by_callback.load(Ordering::SeqCst) > builds_before,
        "the offstage rebuild must complete before the post-frame callback runs; \
         builds before the frame = {builds_before}, seen by the callback = {}",
        builds_seen_by_callback.load(Ordering::SeqCst),
    );
}
