//! The `HeroController` measurement skeleton.
//!
//! These tests prove that the seams built for measurement — `box_size` / `transform_to`,
//! post-frame after layout, observer attachment, `RouteSubtree`,
//! `PostFrameHandle`, notification outside the history lock, and the
//! offstage animation proxies — **compose** into a destination rect.
//!
//! They do not prove the flight overlay itself; `hero_flight_tests` owns that layer.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_foundation::ValueKey;

use super::hero::{Hero, HeroTag};
use super::hero_controller::{FlightDirection, HeroController};
use super::navigator::{Navigator, NavigatorHandle};
use super::observer::NavigatorObserver;
use super::overlay_route::SimpleRoute;
use super::page_route::{PageRoute, PopupRoute};
use crate::test_harness::{Harness, PostFrameCapability, mount, mount_with_capabilities};
use crate::{Center, SizedBox};

/// `Harness::mount` roots the tree at tight 800x600, and a `ModalRoute`'s page fills
/// its `Stack(fit: expand)` — so a route's subtree measures the screen.
const SCREEN: flui_types::Size = flui_types::Size::new(
    flui_types::geometry::px(800.0),
    flui_types::geometry::px(600.0),
);

const TRANSITION: Duration = Duration::from_millis(300);

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    navigator
}

fn page_route() -> PageRoute<i32> {
    PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
        SizedBox::new(30.0, 18.0).into_view().boxed()
    })
    .transition_duration(TRANSITION)
}

/// A root that can drop its `Navigator`, so a controller can be left holding a
/// detached handle.
#[derive(Clone)]
struct Root {
    navigator: NavigatorHandle,
    show: bool,
}

impl View for Root {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Root {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show {
            Navigator::new(self.navigator.clone()).boxed()
        } else {
            crate::Text::new("gone").boxed()
        }
    }
}

fn mount_navigator(navigator: &NavigatorHandle) -> Harness {
    mount(Root {
        navigator: navigator.clone(),
        show: true,
    })
}

fn unmount_navigator(harness: &mut Harness, navigator: &NavigatorHandle) {
    harness.swap_root(Root {
        navigator: navigator.clone(),
        show: false,
    });
}

fn install(navigator: &NavigatorHandle) -> Arc<HeroController> {
    let controller = HeroController::new();
    navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    controller
}

// ============================================================================
// Attachment
// ============================================================================

/// The controller stores its `NavigatorHandle` at `did_attach` and drops it at
/// `did_detach` — Flutter's `NavigatorObserver._navigators` Expando
/// (`navigator.dart:3836`, `:4108`), which is what `HeroController.navigator` reads.
///
/// Red-check: delete `*self.navigator.lock() = None;` from
/// `HeroController::did_detach`.
#[test]
fn the_controller_holds_its_navigator_only_while_attached() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    assert!(controller.navigator().is_none(), "not mounted yet");

    let mut harness = mount_navigator(&navigator);
    assert!(controller.navigator().is_some());

    unmount_navigator(&mut harness, &navigator);
    assert!(controller.navigator().is_none(), "detached");
}

// ============================================================================
// Scheduling
// ============================================================================

/// `didChangeTop` schedules **one** post-frame measurement and reads nothing
/// (`heroes.dart:968`). The measurement list stays empty until a frame completes:
/// that is the difference between "scheduled" and "ran", and the reason
/// `HeroController` can afford to be an observer at all.
///
/// Red-check: call `HeroController::measure` directly from `maybe_start` instead of
/// scheduling it — `measurements()` is non-empty before `tick()`.
#[test]
fn a_top_change_schedules_exactly_one_post_frame_measurement() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    // The seeded `SimpleRoute` is not a `PageRoute`, so pushing over it is not a
    // flight: `fromRoute is! PageRoute` (`heroes.dart:916-920`).
    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    assert_eq!(controller.scheduled_count(), 0);

    // PageRoute -> PageRoute: eligible.
    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    assert_eq!(
        controller.scheduled_count(),
        1,
        "one schedule per eligible top change"
    );
    assert!(
        controller.measurements().is_empty(),
        "scheduled, not run: the observer callback read no geometry"
    );

    harness.tick();
    assert_eq!(controller.measurements().len(), 1, "the frame ran it");
}

/// A `PopupRoute` is not a `PageRoute` (`pages.dart:58-61`, encoded as
/// `TransitionGroup`), so no flight is prepared over one.
///
/// Red-check: drop the `is_page_route` guard from `HeroController::maybe_start`.
#[test]
fn a_non_page_route_schedules_nothing() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _page = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let popup = PopupRoute::<i32>::new(|_ctx, _a, _s| SizedBox::new(5.0, 5.0).into_view().boxed());
    let _popup = harness.enter_owner_scope(|| navigator.push(popup));
    harness.tick();

    assert_eq!(controller.scheduled_count(), 0);
    assert!(controller.measurements().is_empty());
}

// ============================================================================
// Measurement — the whole point
// ============================================================================

/// **The composition test.** In one frame:
///
/// 1. `did_change_top` forces the destination offstage (`heroes.dart:967`), which
///    swaps its primary animation to `kAlwaysComplete` (`routes.dart:1958`) and
///    marks its overlay entry dirty;
/// 2. the frame rebuilds the route's page with that completed animation, lays it out,
///    and commits;
/// 3. the post-frame callback of *that same frame* resolves the
///    route's `RouteSubtree` and reads `box_size` / `transform_to`.
///
/// So the measurement is the destination's **final** geometry, taken before its
/// entrance transition has moved a pixel.
///
/// Red-check (each fails on its own):
/// * delete `sync_animation_proxies`'s primary arm — `to_animation_while_offstage`
///   is the live `0.0`, not `1.0`;
/// * delete `sync_animation_proxies` from `set_offstage` — nothing notifies the
///   scope's relay, so the page is never rebuilt with the completed animation;
/// * call `measure` inline instead of scheduling — `measurements()` is non-empty
///   before the frame runs.
///
/// Note `mark_entry_needs_build` is *not* on that list. It rebuilds the overlay
/// **entry** (the `Offstage` wrapper and the barrier), not the scope, so deleting it
/// leaves this measurement intact — the route is measured correctly while still
/// being painted. `modal_route_tests::modal_offstage_keeps_the_page_but_drops_the_barrier`
/// is what guards it.
#[test]
fn the_post_frame_callback_measures_the_offstage_destination_in_the_same_frame() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    let from = navigator.current().expect("pushed");

    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    let to = navigator.current().expect("pushed");
    assert_eq!(controller.scheduled_count(), 1);

    harness.tick();

    let measurements = controller.measurements();
    assert_eq!(measurements.len(), 1);
    let measurement = measurements[0];

    assert_eq!(measurement.from, from);
    assert_eq!(measurement.to, to);
    assert_eq!(
        measurement.direction,
        Some(FlightDirection::Push),
        "the destination is running forward (heroes.dart:928-929)"
    );
    assert_eq!(
        measurement.to_animation_while_offstage, 1.0,
        "the offstage frame laid the destination out at its FINAL position — \
         without the proxy swap this is the live controller's 0.0"
    );
    assert_eq!(
        measurement.to_size,
        Some(SCREEN),
        "committed layout, read after the pipeline ran"
    );
    assert!(
        measurement.to_transform.is_some(),
        "transform_to resolved against the render root"
    );
}

/// `_startHeroTransition` puts the destination back onstage before it measures
/// (`heroes.dart:987`); the geometry stays committed until the next layout, so the
/// route is visible again on the very next frame.
///
/// Red-check: delete `destination.set_offstage(false)` from `HeroController::measure`
/// — the route is stranded offstage forever.
#[test]
fn the_destination_is_restored_onstage_by_the_measurement() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let second = page_route();
    let modal = second.modal_handle();
    let _second = harness.enter_owner_scope(|| navigator.push(second));
    assert!(modal.offstage(), "forced offstage by did_change_top");

    harness.tick();

    assert!(!modal.offstage(), "and restored by the post-frame callback");
    assert_eq!(controller.measurements().len(), 1);
}

/// A pop is classified from the **source** route running backwards
/// (`heroes.dart:926-927`), not from `didPop` — which `HeroController` does not even
/// override.
///
/// The source is parked mid-entrance first. The harness mounts no `VsyncScope`, so a
/// route's controller never advances on its own; `reverse()` from a
/// resting `0.0` snaps straight to `Dismissed`, and a `Dismissed` source is not a
/// pop — it is a route that never entered. Parking it at `0.5` is what makes the
/// reversal observable, and it is what a real 300 ms transition would look like when
/// the user pops mid-flight.
///
/// Red-check: swap the two arms of `FlightDirection::classify`.
#[test]
fn popping_classifies_the_flight_as_a_pop() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let second = page_route();
    let transition = second.transition_handle();
    let _second = harness.enter_owner_scope(|| navigator.push(second));
    harness.tick();

    // Park the source mid-entrance, so popping it reverses rather than snapping to
    // dismissed.
    let source = transition
        .controller()
        .expect("install() created the controller");
    source.set_value(0.5);
    source.forward().expect("a parked controller can resume");

    assert!(harness.enter_owner_scope(|| navigator.pop()));
    harness.tick();

    let directions: Vec<_> = controller
        .measurements()
        .iter()
        .map(|measurement| measurement.direction)
        .collect();
    assert_eq!(
        directions,
        vec![Some(FlightDirection::Push), Some(FlightDirection::Pop)],
        "the push, then the pop"
    );
}

// ============================================================================
// Staleness and safety
// ============================================================================

/// A controller whose navigator has left the tree schedules nothing — Flutter's
/// `if (navigator == null) return;` (`heroes.dart:970-972`).
///
/// Two independent layers, asserted separately because either alone would let this
/// test pass while the other rotted:
///
/// 1. the controller dropped its handle at `did_detach`;
/// 2. the navigator's own capabilities died with the tree, so even a controller that
///    somehow still held a handle could not schedule or measure through it.
///
/// Red-check: delete `*self.navigator.lock() = None;` from `did_detach` (layer 1),
/// or delete the `post_frame`/`render_tree` teardown from `NavigatorState::dispose`
/// (layer 2).
#[test]
fn a_detached_controller_is_inert() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    let before = controller.scheduled_count();

    unmount_navigator(&mut harness, &navigator);

    // Layer 2: the capabilities are gone with the tree that named them.
    assert!(navigator.post_frame_handle().is_none());
    assert!(navigator.render_tree().is_none());

    // Layer 1: and the controller no longer holds the navigator at all.
    assert!(controller.navigator().is_none());

    // The stack still exists — `NavigatorHandle` owns it — so this pushes for real.
    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    assert_eq!(
        controller.scheduled_count(),
        before,
        "a detached controller schedules nothing"
    );
}

/// A measurement scheduled while the navigator was mounted, whose frame arrives after
/// it is gone, must **not** record anything — `if (navigator == null || overlay ==
/// null) return;` at the top of `_startHeroTransition` (`heroes.dart:993-997`).
///
/// This is the callback-side guard, distinct from the scheduling-side one above: the
/// controller was attached and did schedule, and only then did the tree go away.
///
/// Red-check: delete the `if !navigator.is_mounted() { return; }` guard from
/// `HeroController::measure` — a `Measurement` lands with `to_size: None`, which a
/// future `RectTween` would happily interpolate from.
#[test]
fn a_measurement_whose_navigator_vanished_before_the_frame_records_nothing() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    assert_eq!(controller.scheduled_count(), 1, "the measurement is queued");
    assert!(controller.measurements().is_empty(), "but has not run");

    // The frame that would have run it also unmounts the navigator.
    unmount_navigator(&mut harness, &navigator);

    assert!(
        controller.measurements().is_empty(),
        "a measurement must not record against a navigator that has left the tree"
    );
}

/// The measurement is scheduled from an observer callback, which runs with no
/// navigator lock held. Installing a `HeroController` — which reaches back
/// through its owner-local `NavigatorHandle` for `route_modal`, `route_peer` and
/// `post_frame_handle` from inside `did_change_top` — must not deadlock.
///
/// Red-check: in `NavigatorShared::mutate`, call `apply(outcome)` inside the
/// `history.lock()` scope; this test hangs.
#[test]
fn a_hero_controller_does_not_deadlock_the_observer_callback() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);

    let mut harness = mount_navigator(&navigator);
    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    assert!(harness.enter_owner_scope(|| navigator.pop()));
    harness.tick();

    assert_eq!(controller.measurements().len(), 2);
}

/// Nested navigators are **out of scope**: a controller answers only about the
/// navigator that attached it. Flutter needs a `HeroControllerScope` for this
/// (`navigator.dart:3995-4046`) and FLUI has none.
///
/// The controller must never reach for a root navigator — `NavigatorHandle::maybe_of_root`
/// exists, and using it here would silently make an inner controller measure outer
/// routes. This pins that an inner navigator's pushes are invisible to an outer
/// navigator's controller.
///
/// Red-check: make `HeroController::maybe_start` resolve its navigator from anywhere
/// but `self.navigator`.
#[test]
fn a_controller_observes_only_the_navigator_that_attached_it() {
    let outer = seeded_navigator();
    let inner = seeded_navigator();
    let outer_controller = install(&outer);
    let inner_controller = install(&inner);

    let mut harness = mount_navigator(&outer);
    let mut inner_harness = mount_navigator(&inner);

    let _outer_push = harness.enter_owner_scope(|| outer.push(page_route()));
    let _outer_push2 = harness.enter_owner_scope(|| outer.push(page_route()));
    harness.tick();

    assert_eq!(outer_controller.scheduled_count(), 1);
    assert_eq!(
        inner_controller.scheduled_count(),
        0,
        "the inner controller heard nothing about the outer navigator"
    );

    let _inner_push = inner_harness.enter_owner_scope(|| inner.push(page_route()));
    let _inner_push2 = inner_harness.enter_owner_scope(|| inner.push(page_route()));
    inner_harness.tick();

    assert_eq!(inner_controller.scheduled_count(), 1);
    assert_eq!(outer_controller.scheduled_count(), 1, "and vice versa");
}

/// Two eligible top changes in one frame schedule two measurements, and both run.
/// A counter that deduplicated would be a silent behavior change.
#[test]
fn every_eligible_top_change_gets_its_own_measurement() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    let _third = harness.enter_owner_scope(|| navigator.push(page_route()));
    assert_eq!(controller.scheduled_count(), 2, "page->page, page->page");

    harness.tick();
    assert_eq!(controller.measurements().len(), 2);

    let counted = Arc::new(AtomicUsize::new(controller.measurements().len()));
    assert_eq!(counted.load(Ordering::SeqCst), 2);
}

/// **A capability that cannot be acquired must not be paid for first.**
///
/// `BuildContext::post_frame_handle()` is an `Option`, so a binding may install none.
/// `maybe_start` therefore acquires it *before* flipping the destination offstage:
/// Flutter's `addPostFrameCallback` cannot fail (`heroes.dart:967-968`), FLUI's can,
/// and the only code that ever calls `set_offstage(false)` is the measurement that
/// failure would have scheduled. Flip first and bail, and the destination is stranded
/// offstage forever — invisible, unhittable, and with its animation pinned at `1.0`.
///
/// The top change here is fully eligible: two `PageRoute`s, both `ModalRoute`s, a
/// live navigator, an attached controller. Only the capability is missing.
///
/// Red-check: move `destination.set_offstage(…)` back above the
/// `let Some(post_frame) = navigator.post_frame_handle()` guard in
/// `HeroController::maybe_start`.
#[test]
fn without_a_post_frame_capability_the_destination_is_left_onstage() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_with_capabilities(
        Root {
            navigator: navigator.clone(),
            show: true,
        },
        PostFrameCapability::Absent,
    );

    // The controller attached, so it is not the `navigator == None` path being tested.
    assert!(controller.navigator().is_some());
    assert!(
        navigator.post_frame_handle().is_none(),
        "this binding installed no post-frame capability"
    );

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let second = page_route();
    let modal = second.modal_handle();
    let _second = harness.enter_owner_scope(|| navigator.push(second));

    assert_eq!(
        controller.scheduled_count(),
        0,
        "nothing can be scheduled without the capability"
    );
    assert!(
        !modal.offstage(),
        "so the destination must never have been forced offstage"
    );

    // And it stays that way: no frame restores what was never flipped.
    harness.tick();
    assert!(!modal.offstage());
    assert!(controller.measurements().is_empty());
}

/// A present but inactive owner-local capability is also a typed scheduling
/// failure. The controller must enqueue successfully before hiding the route;
/// otherwise an owner-scope mistake would strand the destination offstage.
#[test]
fn inactive_local_lane_never_strands_the_destination_offstage() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();

    let second = page_route();
    let modal = second.modal_handle();
    let _second = navigator.push(second); // deliberately outside the owner scope

    assert_eq!(controller.scheduled_count(), 0);
    assert!(
        !modal.offstage(),
        "InactiveLane must be handled before the destination is hidden"
    );
    harness.tick();
    assert!(!modal.offstage());
}

// ============================================================================
// Hero discovery and manifests
// ============================================================================

/// A `PageRoute` whose page is a single `Hero` with `tag_name`, sized `w`x`h`.
///
/// `Center` because a `ModalRoute`'s page fills the screen under `Stack(fit: expand)`;
/// without it every hero would measure 800x600 and the two rects would be
/// indistinguishable.
fn hero_page_route(tag_name: &'static str, w: f32, h: f32) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        Center::new()
            .child(Hero::new(ValueKey::new(tag_name), SizedBox::new(w, h)))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// **The composition, end to end.** Two `PageRoute`s share a tag; the post-frame
/// callback finds both heroes through their routes' registries, measures each in its
/// own route's coordinate space, and records one manifest.
///
/// This is `_startHeroTransition`'s matching loop (`heroes.dart:1014-1060`) with the
/// flight removed: no overlay entry, no `RectTween`, no shuttle.
///
/// The destination's rect is the one that matters: it is measured **while the route is
/// offstage**, so its animation reads `1.0` and its layout is where the page will
/// finally rest — not where its entrance transition currently has it.
///
/// Red-check (each fails on its own):
/// * delete `manifests.lock().extend(self.collect_manifests())` from `MeasurementPass::run`;
/// * make `collect_manifests` fall back to the other route's hero on a tag miss — the
///   shared tag still resolves here, so this one is pinned by
///   `controller_ignores_tags_present_on_only_one_route` instead;
/// * swap the arguments of `transform_to` in `HeroHandle::bounding_box_in` — both rects
///   collapse to the origin.
#[test]
fn controller_collects_matching_tags_and_records_one_manifest() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first =
        harness.enter_owner_scope(|| navigator.push(hero_page_route("shared", 30.0, 20.0)));
    harness.tick();
    let from = navigator.current().expect("pushed");

    let _second =
        harness.enter_owner_scope(|| navigator.push(hero_page_route("shared", 60.0, 45.0)));
    let to = navigator.current().expect("pushed");
    harness.tick();

    let manifests = controller.manifests();
    assert_eq!(manifests.len(), 1, "one shared tag, one manifest");

    let manifest = &manifests[0];
    assert_eq!(manifest.tag, HeroTag::new(ValueKey::new("shared")));
    assert_eq!((manifest.from_route, manifest.to_route), (from, to));
    assert_eq!(manifest.direction, Some(FlightDirection::Push));

    assert_eq!(
        (manifest.from_rect.width().0, manifest.from_rect.height().0),
        (30.0, 20.0),
        "the source hero, in the source route's space"
    );
    assert_eq!(
        (manifest.to_rect.width().0, manifest.to_rect.height().0),
        (60.0, 45.0),
        "the destination hero, in the destination route's space"
    );
    assert!(manifest.from_rect.is_finite() && manifest.to_rect.is_finite());

    // Both heroes are centred in their own 800x600 route, so neither sits at the
    // origin — which is what a swapped `transform_to` would produce.
    assert!(manifest.from_rect.min.x.0 > 0.0 && manifest.from_rect.min.y.0 > 0.0);
    assert!(manifest.to_rect.min.x.0 > 0.0 && manifest.to_rect.min.y.0 > 0.0);
}

/// `final toHero = toHeroes[tag]; if (toHero == null) …` (`heroes.dart:1044-1046`) — a
/// tag on only one route is not a flight.
///
/// Red-check: make `collect_manifests` fall back to the other route's hero when a tag
/// misses (`from_heroes.get(&tag).or_else(|| to_heroes.get(&tag))`, and the mirror) —
/// an unpaired tag then flies from its own rect to its own rect.
#[test]
fn controller_ignores_tags_present_on_only_one_route() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _first =
        harness.enter_owner_scope(|| navigator.push(hero_page_route("only-here", 30.0, 20.0)));
    harness.tick();

    let _second =
        harness.enter_owner_scope(|| navigator.push(hero_page_route("only-there", 60.0, 45.0)));
    harness.tick();

    assert_eq!(
        controller.measurements().len(),
        1,
        "the top change was eligible and measured"
    );
    assert!(
        controller.manifests().is_empty(),
        "but no tag is shared, so nothing would fly: {:?}",
        controller.manifests()
    );
}

/// A hero that leaves its route before the measuring frame takes its tag with it, so
/// no flight is prepared for it.
///
/// The source route is rebuilt without its hero between the top change and the frame
/// that measures; `HeroState::dispose` deregisters it, and the route's registry — the
/// thing `collect_manifests` reads — is empty by the time the post-frame callback runs.
///
/// The assertion is on the **registry**, not only on `manifests()`. Two independent
/// guards keep an absent hero from flying (the tag lookup, and `bounding_box_in`
/// answering `None` for a detached node), so no single mutation reddens
/// `manifests().is_empty()` — a test that asserted only that would pass with the
/// deregistration deleted.
///
/// # What is *not* claimed
///
/// `collect_manifests` also bails when a route has no `RouteSubtree` and when
/// `bounding_box_in` answers `None`. Neither is reachable end-to-end today: by the time
/// the post-frame callback runs, both routes have built and laid out in that same
/// frame, and an unmounted hero has already deregistered. Both are ported because
/// Flutter's `_boundingBoxFor` asserts `box.hasSize` there and would crash, and both
/// are pinned where they *are* testable —
/// `hero_tests::{an_unmounted_hero_measures_to_none, a_hero_bounding_box_is_none_before_layout_commits}`
/// and `a_non_finite_rect_is_never_flown`.
///
/// Red-check: delete `registry.deregister(…)` from `HeroState::dispose`.
#[test]
fn controller_skips_a_hero_that_left_its_route_before_the_measuring_frame() {
    use std::sync::atomic::AtomicBool;

    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    // The source page shows its hero only while `keep_hero` is set.
    let keep_hero = Arc::new(AtomicBool::new(true));
    let keep_for_page = Arc::clone(&keep_hero);
    let source = PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        if keep_for_page.load(Ordering::SeqCst) {
            Center::new()
                .child(Hero::new(
                    ValueKey::new("shared"),
                    SizedBox::new(30.0, 20.0),
                ))
                .into_view()
                .boxed()
        } else {
            Center::new()
                .child(SizedBox::new(30.0, 20.0))
                .into_view()
                .boxed()
        }
    })
    .transition_duration(TRANSITION);
    let source_modal = source.modal_handle();
    let _first = harness.enter_owner_scope(|| navigator.push(source));
    harness.tick();

    // Drop the hero, and dirty the source's overlay entry so the same frame that
    // measures also rebuilds it without the hero.
    keep_hero.store(false, Ordering::SeqCst);
    source_modal.set_maintain_state(false);

    let _second =
        harness.enter_owner_scope(|| navigator.push(hero_page_route("shared", 60.0, 45.0)));
    harness.tick();

    assert_eq!(controller.measurements().len(), 1, "it still measured");
    let source_registry = navigator
        .route_modal(navigator.route_ids()[1])
        .expect("the source is a ModalRoute")
        .heroes();
    assert_eq!(
        source_registry.len(),
        0,
        "the departed hero deregistered from its route"
    );
    assert!(
        controller.manifests().is_empty(),
        "so the tag is unpaired and nothing would fly: {:?}",
        controller.manifests()
    );
}

/// `_HeroFlightManifest.isValid` (`heroes.dart:530`):
/// `toHeroLocation.isFinite && (isDiverted || fromHeroLocation.isFinite)`.
///
/// Unit-tested directly, because no reachable FLUI configuration produces a non-finite
/// rect today — every rect comes from `box_size` and `transform_to`. Asserting it
/// end-to-end would assert nothing. See `is_valid_flight`'s docs.
///
/// Red-check: `to_rect.is_finite() || from_rect.is_finite()` in `is_valid_flight`.
#[test]
fn a_non_finite_rect_is_never_flown() {
    use super::hero_controller::is_valid_flight;
    use flui_geometry::Rect;
    use flui_types::geometry::px;

    let finite = Rect::from_ltwh(px(0.0), px(0.0), px(10.0), px(10.0));
    let infinite = Rect::from_ltwh(px(0.0), px(0.0), px(f32::INFINITY), px(10.0));
    let nan = Rect::from_ltwh(px(f32::NAN), px(0.0), px(10.0), px(10.0));

    assert!(is_valid_flight(finite, finite));
    assert!(!is_valid_flight(infinite, finite), "an infinite source");
    assert!(
        !is_valid_flight(finite, infinite),
        "an infinite destination"
    );
    assert!(!is_valid_flight(nan, finite), "a NaN origin");
}

/// **A `HeroController` cannot be shared by two mounted navigators** (Flutter's
/// "can not be shared", `navigator.dart:4010-4027`). The second
/// navigator's attach is refused: the controller stays with the first, sound, rather
/// than silently pointing at the second.
///
/// Red-check: drop the shared-controller guard in `HeroController::did_attach` (let it
/// overwrite) — the controller then names the *second* navigator and this fails.
#[test]
fn a_hero_controller_shared_by_two_mounted_navigators_keeps_the_first() {
    let controller = HeroController::new();

    let first = seeded_navigator();
    first.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    let mut first_harness = mount_navigator(&first);
    assert!(
        controller
            .navigator()
            .is_some_and(|nav| nav.is_same(&first)),
        "the controller attaches to the first navigator"
    );

    // A second, still-mounted navigator tries to take the same controller.
    let second = seeded_navigator();
    second.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    let mut second_harness = mount_navigator(&second);

    assert!(
        controller
            .navigator()
            .is_some_and(|nav| nav.is_same(&first)),
        "the second attach was refused; the controller still names the first navigator"
    );
    assert!(
        !controller
            .navigator()
            .is_some_and(|nav| nav.is_same(&second)),
        "and never the second"
    );

    // When the first unmounts, the controller is freed and the second can claim it.
    unmount_navigator(&mut first_harness, &first);
    let _ = &mut second_harness;
}

/// **Automatic attach adds exactly one controller**: a bare
/// `Navigator` with no `HeroControllerScope` creates its own default `HeroController`.
///
/// Red-check: delete the `None => { … observers.push(HeroController::new()) }` arm from
/// `NavigatorState::init_state` — the count is 0.
#[test]
fn a_bare_navigator_auto_defaults_exactly_one_controller() {
    let navigator = seeded_navigator();
    let _harness = mount_navigator(&navigator);
    assert_eq!(
        navigator.hero_observer_count(),
        1,
        "the Navigator created its own default hero controller"
    );
}

/// **A hand-attached controller suppresses the auto-default**: the marker
/// `NavigatorObserver::observes_hero_flights` lets `init_state` skip the default, so
/// there is exactly one controller — not the manual one plus a default.
///
/// Red-check: make `HeroController::observes_hero_flights` return `false` — the
/// auto-default is added too and the count is 2.
#[test]
fn a_manual_controller_suppresses_the_auto_default() {
    let navigator = seeded_navigator();
    navigator.add_observer(HeroController::new() as Arc<dyn NavigatorObserver>);
    let _harness = mount_navigator(&navigator);
    assert_eq!(
        navigator.hero_observer_count(),
        1,
        "the manual controller suppressed the auto-default — exactly one, not two"
    );
}

/// The same suppression holds when a controller is hand-attached **after** mount.
/// `NavigatorHandle::add_observer` documents that already-mounted observers attach at
/// once, so the auto-default must be replaced rather than left beside the manual
/// controller.
///
/// Red-check: delete `take_auto_hero_observer()` from `NavigatorHandle::add_observer` —
/// the count stays 2 and the old auto-controller is still attached.
#[test]
fn a_manual_controller_added_after_mount_replaces_the_auto_default() {
    let navigator = seeded_navigator();
    let _harness = mount_navigator(&navigator);
    assert_eq!(
        navigator.hero_observer_count(),
        1,
        "mount created the default hero controller"
    );

    let manual = HeroController::new();
    navigator.add_observer(Arc::clone(&manual) as Arc<dyn NavigatorObserver>);

    assert_eq!(
        navigator.hero_observer_count(),
        1,
        "the manual controller replaced the auto-default instead of doubling it"
    );
    assert!(
        manual
            .navigator()
            .is_some_and(|handle| handle.is_same(&navigator)),
        "the newly-added controller attached immediately to the mounted navigator"
    );
}

/// `HeroControllerScope::none` attaches nothing and suppresses the auto-default: zero
/// controllers, no flights.
///
/// Red-check: treat `Some(None)` like `None` in `init_state` (auto-default) — count is 1.
#[test]
fn a_scope_none_leaves_no_controller() {
    use super::hero_controller_scope::HeroControllerScope;

    let navigator = seeded_navigator();
    let _harness = mount(HeroControllerScope::none(Navigator::new(navigator.clone())));
    assert_eq!(
        navigator.hero_observer_count(),
        0,
        "HeroControllerScope::none blocks the auto-default"
    );
}
