//! ADR-0021 U4: the flight itself.
//!
//! A manifest becomes a shuttle in an overlay entry, two frozen placeholders, and a
//! driven `RectTween`. These tests pin the observable half of that: what is in the
//! overlay, what the heroes look like while it flies, where the shuttle is aimed, and
//! what is left behind when it lands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationStatus};
use flui_foundation::ValueKey;
use flui_geometry::Rect;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Color;
use flui_types::Offset;
use flui_view::ViewExt;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::hero::{Hero, HeroHandle, HeroTag};
use super::hero_controller::{FlightDirection, HeroController};
use super::navigator::{Navigator, NavigatorHandle};
use super::observer::NavigatorObserver;
use super::overlay_route::SimpleRoute;
use super::page_route::{PageRoute, PopupRoute};
use super::transition_route::TransitionHandle;
use crate::test_harness::{Harness, mount};
use crate::{Center, ColoredBox, Column, MainAxisSize, SizedBox};

const TRANSITION: Duration = Duration::from_millis(300);

fn tag(name: &'static str) -> HeroTag {
    HeroTag::new(ValueKey::new(name))
}

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    navigator
}

#[derive(Clone)]
struct Root {
    navigator: NavigatorHandle,
}

impl View for Root {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Root {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Navigator::new(self.navigator.clone())
    }
}

fn mount_navigator(navigator: &NavigatorHandle) -> Harness {
    mount(Root {
        navigator: navigator.clone(),
    })
}

fn install(navigator: &NavigatorHandle) -> Arc<HeroController> {
    let controller = HeroController::new();
    navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    controller
}

/// A `PageRoute` whose hero paints — and therefore can be hit.
///
/// A bare `SizedBox` builds a childless `RenderConstrainedBox`, which is **not**
/// hit-testable: it adds nothing to the hit path, so a shuttle made of one could never
/// swallow a pointer whether or not it is wrapped in an `IgnorePointer`. Giving it a
/// `ColoredBox` (a `RenderDecoratedBox`, which is) is what makes the `IgnorePointer`
/// the thing under test.
fn hittable_hero_page(tag_name: &'static str, w: f32, h: f32) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        Center::new()
            .child(Hero::new(
                ValueKey::new(tag_name),
                SizedBox::new(w, h).child(ColoredBox::new(Color::RED)),
            ))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// A `PageRoute` whose page is one `Hero`, centred so it does not fill the screen.
fn hero_page(tag_name: &'static str, w: f32, h: f32) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        Center::new()
            .child(Hero::new(ValueKey::new(tag_name), SizedBox::new(w, h)))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// Push `source` then `destination`, pumping a frame after each so the second push's
/// post-frame callback measures and launches.
fn fly(
    navigator: &NavigatorHandle,
    harness: &mut Harness,
    source: PageRoute<i32>,
    destination: PageRoute<i32>,
) -> TransitionHandle {
    let _source = navigator.push(source);
    harness.tick();
    let transition = destination.transition_handle();
    let _destination = navigator.push(destination);
    harness.tick();
    transition
}

fn hero_of(navigator: &NavigatorHandle, route_index: usize, tag_name: &'static str) -> HeroHandle {
    navigator
        .route_modal(navigator.route_ids()[route_index])
        .expect("a ModalRoute")
        .heroes()
        .get(&tag(tag_name))
        .expect("a registered hero")
}

// ============================================================================
// The flight exists
// ============================================================================

/// **The slice, end to end.** Two `PageRoute`s share a tag, so the post-frame callback
/// builds a manifest and `_HeroFlight.start` (`heroes.dart:698-736`) turns it into one
/// overlay entry above every route.
///
/// Red-check: delete `self.launch(manifest, …)` from `MeasurementPass::run`.
#[test]
fn a_push_between_two_page_routes_with_a_matching_tag_inserts_one_flight_entry() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let entries_before = navigator.overlay().entry_ids().len();
    let _transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );

    assert_eq!(controller.flights().len(), 1, "one tag, one flight");
    let flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("in the air");

    let entries = navigator.overlay().entry_ids();
    assert_eq!(
        entries.len(),
        entries_before + 2 + 1,
        "the two routes, plus one flight entry"
    );
    assert_eq!(
        entries.last().copied(),
        flight.entry_id(),
        "and the shuttle is above every route (`overlay.insert`, `heroes.dart:734`)"
    );
}

/// `no_flight_when_only_one_route_is_a_page_route`: a `PopupRoute` is not a `PageRoute`
/// (`heroes.dart:916-920`), so no transition is even considered.
///
/// Red-check: drop the `is_page_route` guard from `HeroController::maybe_start`.
#[test]
fn no_flight_when_only_one_route_is_a_page_route() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _page = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();

    let popup = PopupRoute::<i32>::new(|_ctx, _a, _s| {
        Center::new()
            .child(Hero::new(
                ValueKey::new("shared"),
                SizedBox::new(60.0, 45.0),
            ))
            .into_view()
            .boxed()
    });
    let _popup = navigator.push(popup);
    harness.tick();

    assert_eq!(controller.flights().len(), 0);
    assert!(controller.manifests().is_empty());
}

/// Two `PageRoute`s, no shared tag: a manifest is never built, so nothing flies
/// (`heroes.dart:1044-1046`).
///
/// Red-check: make `collect_manifests` fall back to the other route's hero on a tag
/// miss.
#[test]
fn no_flight_when_no_tags_match() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let entries_before = navigator.overlay().entry_ids().len();
    let _transition = fly(
        &navigator,
        &mut harness,
        hero_page("only-here", 30.0, 20.0),
        hero_page("only-there", 60.0, 45.0),
    );

    assert_eq!(controller.flights().len(), 0);
    assert_eq!(
        navigator.overlay().entry_ids().len(),
        entries_before + 2,
        "the two routes, and no flight entry"
    );
}

// ============================================================================
// What the heroes look like while it flies
// ============================================================================

/// `manifest.fromHero.startFlight(shouldIncludedChildInPlaceholder: true)` for a push
/// (`heroes.dart:716-733`): the source hero is replaced by a fixed-size hole whose
/// child is kept **offstage**, so its state survives the flight and the page around it
/// does not reflow.
///
/// Red-check: pass `false` for `shouldIncludeChildInPlaceholder` on a push in
/// `FlightManager::start`.
#[test]
fn the_from_hero_is_hidden_for_the_whole_flight() {
    let navigator = seeded_navigator();
    let _controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );

    let from_hero = hero_of(&navigator, 1, "shared");
    assert_eq!(
        from_hero
            .placeholder_size()
            .map(|size| (size.width.0, size.height.0)),
        Some((30.0, 20.0)),
        "frozen at its committed size"
    );
    assert!(
        from_hero.includes_child(),
        "and its child is preserved offstage (push)"
    );

    harness.tick();
    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|name| name.ends_with("RenderOffstage")),
        "the source hero's child is offstage, not deleted: {names:?}"
    );
}

/// `manifest.toHero.startFlight()` — `shouldIncludedChildInPlaceholder` defaults to
/// `false` (`heroes.dart:381`, `:734`): the destination hero shows a bare hole, because
/// the shuttle is drawing its content in the overlay.
///
/// Red-check: pass `true` to `to_hero.start_flight(…)` in `FlightManager::start`.
#[test]
fn the_to_hero_placeholder_drops_its_child_during_the_flight() {
    let navigator = seeded_navigator();
    let _controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );

    let to_hero = hero_of(&navigator, 2, "shared");
    assert_eq!(
        to_hero
            .placeholder_size()
            .map(|size| (size.width.0, size.height.0)),
        Some((60.0, 45.0))
    );
    assert!(
        !to_hero.includes_child(),
        "the destination hero drops its child while the shuttle carries it"
    );
}

/// `IgnorePointer(child: FadeTransition(…))` (`heroes.dart:594-595`): the shuttle is
/// painted, never hit.
///
/// The pointer is aimed at the centre of the shuttle's own rect, and **every render
/// node in the shuttle's subtree** must be absent from the hit path — not merely a
/// `RenderIgnorePointer` present somewhere in the tree, which proves nothing about
/// what it is wrapping.
///
/// Red-check: `.ignoring(false)` in `ShuttleState::build`.
#[test]
fn the_flight_entry_is_ignore_pointer_not_hit_testable() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _transition = fly(
        &navigator,
        &mut harness,
        hittable_hero_page("shared", 30.0, 20.0),
        hittable_hero_page("shared", 60.0, 45.0),
    );
    let flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("in the air");
    let rect = flight.shuttle_rect();

    // The entry is inserted from a post-frame callback, so its shuttle is built by the
    // *next* frame. Nothing to hit-test before then.
    harness.tick();

    let owner = harness.pipeline_owner();
    let owner = owner.read();

    let gates = descendants_named(&owner, "RenderIgnorePointer");
    assert_eq!(gates.len(), 1, "the flight entry is the only IgnorePointer");
    let shuttle = subtree_of(&owner, gates[0]);
    assert!(
        shuttle.iter().any(|id| owner
            .render_tree()
            .get(*id)
            .is_some_and(|node| node.debug_name().ends_with("RenderDecoratedBox"))),
        "the gate wraps something hit-testable, or this test proves nothing: {shuttle:?}"
    );

    let mut result = flui_interaction::HitTestResult::new();
    let centre = Offset::new(
        rect.min_x() + rect.width() / 2.0,
        rect.min_y() + rect.height() / 2.0,
    );
    owner.hit_test(centre, &mut result);
    let hit: Vec<flui_foundation::RenderId> =
        result.path().iter().map(|entry| entry.target).collect();
    assert!(
        !hit.is_empty(),
        "the point is over the page, so something was hit"
    );

    for node in shuttle {
        assert!(
            !hit.contains(&node),
            "the shuttle must swallow no pointers; hit path: {hit:?}"
        );
    }
}

/// Every render node at or below `root`.
fn subtree_of(
    owner: &PipelineOwner,
    root: flui_foundation::RenderId,
) -> Vec<flui_foundation::RenderId> {
    let tree = owner.render_tree();
    let mut collected = vec![root];
    let mut index = 0;
    while index < collected.len() {
        let node = collected[index];
        collected.extend_from_slice(tree.children(node));
        index += 1;
    }
    collected
}

fn descendants_named(owner: &PipelineOwner, name: &str) -> Vec<flui_foundation::RenderId> {
    owner
        .render_tree()
        .iter()
        .filter(|(_, node)| node.debug_name().ends_with(name))
        .map(|(id, _)| id)
        .collect()
}

// ============================================================================
// Landing
// ============================================================================

/// `_performAnimationUpdate` (`heroes.dart:600-618`): when the animation stops, the
/// overlay entry is removed, `fromHero.endFlight(keepPlaceholder: status.isCompleted)`
/// keeps the source hidden under the new page, and
/// `toHero.endFlight(keepPlaceholder: status.isDismissed)` gives the destination its
/// child back.
///
/// Red-check (each fails on its own):
/// * delete `entry.remove()` from `HeroFlight::finish`;
/// * swap the two `end_flight` arguments — the destination stays a hole and the source
///   reappears under the new page.
#[test]
fn the_flight_entry_is_removed_and_heroes_restored_when_animation_settles() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let entries_before = navigator.overlay().entry_ids().len();
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );
    assert_eq!(controller.flights().len(), 1);

    let from_hero = hero_of(&navigator, 1, "shared");
    let to_hero = hero_of(&navigator, 2, "shared");

    // Land it: the destination route's entrance completes.
    let animation = transition.controller().expect("installed");
    animation.set_value(1.0);
    assert_eq!(animation.status(), AnimationStatus::Completed);
    harness.tick();

    assert_eq!(controller.flights().len(), 0, "the flight ended");
    assert_eq!(
        navigator.overlay().entry_ids().len(),
        entries_before + 2,
        "and took its overlay entry with it"
    );

    assert!(
        from_hero.placeholder_size().is_some(),
        "the source hero stays a hole: it is under the new page \
         (`keepPlaceholder: status.isCompleted`)"
    );
    assert_eq!(
        to_hero.placeholder_size(),
        None,
        "and the destination hero gets its child back"
    );
}

// ============================================================================
// Per-tick re-measure
// ============================================================================

/// `onTick` (`heroes.dart:666-696`): the destination may move between the frame that
/// measured it and the frame the shuttle lands on. Each tick re-reads its **origin** in
/// the destination route's coordinate space and re-aims the tween at it.
///
/// Two things Flutter does that a natural port gets wrong, both asserted:
///
/// * **only the origin is re-read.** `heroRectEnd = toHeroOrigin & heroRectTween.end!.size`
///   (`:685`) keeps the *original* end size.
/// * **`begin` is preserved** (`:685` again): the shuttle keeps interpolating from where
///   it started, not from where it currently is. Re-basing `begin` would make it
///   accelerate every time the destination twitched.
///
/// # The size half cannot fail today, and this says so
///
/// `start_flight` freezes the destination hero at its measured size, so its render box
/// keeps that size for the whole flight — re-reading it and preserving it give the same
/// answer. The `& end.size` is kept because it is what Flutter does, and because it
/// stops being a no-op the moment a `placeholderBuilder` (U5) can hand back a
/// differently-sized placeholder. The assertion below is a regression guard, not a
/// red-checkable proof; mutating `on_tick` to re-read the size leaves it green.
///
/// Red-check (each fails on its own):
/// * delete the `rect.end = …` re-aim from `FlightInner::on_tick`;
/// * also set `rect.begin = self.current_rect()` — `begin` moves.
#[test]
fn destination_hero_move_mid_flight_updates_the_target_rect() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _source = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();

    // The destination hero is **pushed down** by a growing sibling. It cannot be moved
    // by resizing it: `start_flight` freezes it at its measured size, so its own box is
    // a fixed hole for the whole flight — which is the point of the placeholder.
    //
    // The rebuild is driven through the spacer's own `RebuildHandle`, not through
    // `ModalHandle::set_offstage`. Flipping offstage repoints the route's primary
    // animation proxy at `kAlwaysComplete`, whose `Completed` status ends the flight —
    // in FLUI and in Flutter alike, since `_proxyAnimation.parent` is that same proxy
    // (`routes.dart:1958`, `heroes.dart:719-724`, `:601`).
    let mover = Mover::default();
    let mover_for_page = mover.clone();
    let destination = PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        Center::new()
            .child(
                Column::new(vec![
                    mover_for_page.clone().into_view().boxed(),
                    Hero::new(ValueKey::new("shared"), SizedBox::new(60.0, 45.0))
                        .into_view()
                        .boxed(),
                ])
                .main_axis_size(MainAxisSize::Min),
            )
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION);
    let transition = destination.transition_handle();
    let _destination = navigator.push(destination);
    harness.tick();

    let flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("in the air");
    let target_before = flight.target_rect();
    let begin_before = flight.begin_rect();
    assert_eq!(
        (target_before.width().0, target_before.height().0),
        (60.0, 45.0)
    );

    // Grow the spacer and let the destination route lay out again.
    mover.grow();
    harness.tick();

    // Tick the flight: the proxy's parent is the destination route's animation.
    let animation = transition.controller().expect("installed");
    animation.set_value(0.25);

    let target_after = flight.target_rect();
    assert_eq!(
        (target_after.min_y() - target_before.min_y()).0,
        50.0,
        "a 100px spacer above a centred column moves its second child down by 50px, \
         and the tween followed it"
    );
    assert_eq!(
        target_after.min_x(),
        target_before.min_x(),
        "nothing moved it horizontally"
    );
    assert_eq!(
        (target_after.width().0, target_after.height().0),
        (60.0, 45.0),
        "the end size is untouched (though the frozen placeholder makes that \
         unobservable today — see the docs above)"
    );

    assert_eq!(
        rect_origin(begin_before),
        rect_origin(flight.begin_rect()),
        "re-aiming must not re-base `begin` on the current rect"
    );
}

fn rect_origin(rect: Rect) -> (f32, f32) {
    (rect.min_x().0, rect.min_y().0)
}

// ============================================================================
// Divert (U5.1) — a same-tag flight interrupted mid-air is redirected in place
// ============================================================================

/// **push interrupted by pop.** Open a page, then immediately go back while its hero
/// is still flying. Flutter's `_HeroFlight.divert` (`heroes.dart:742-757`) reuses the
/// *same* flight and its *same* overlay entry, repoints the proxy at
/// `ReverseAnimation(newAnimation)`, and reverses the rect tween — the pop retraces
/// the push path backwards, no jump cut.
///
/// Observable: one flight, the **same** `entry_id` as before the pop, and the tween's
/// begin/end swapped (the shuttle now heads back to where it came from).
///
/// Red-check: in `FlightManager::start`, replace the `existing.divert(…); return;`
/// with the U4 end-and-restart (`self.flights.lock().remove(&tag)` + `finish` + a fresh
/// `start`). The entry id then changes and the begin/end are the fresh push tween.
#[test]
fn a_push_flight_interrupted_by_a_pop_diverts_in_place() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();
    let b = hero_page("shared", 60.0, 45.0);
    let b_transition = b.transition_handle();
    let _b = navigator.push(b);
    harness.tick();

    // The push flight is airborne, parked mid-entrance so the pop genuinely reverses.
    b_transition.controller().expect("installed").set_value(0.5);
    let push_flight = controller.flights().get(&tag("shared")).expect("airborne");
    let push_entry = push_flight.entry_id();
    let push_begin = push_flight.begin_rect();
    let push_end = push_flight.target_rect();

    // Go back: B pops while its hero is still in flight.
    assert!(navigator.pop());
    harness.tick();

    assert_eq!(controller.flights().len(), 1, "still exactly one flight");
    let pop_flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("still airborne");
    assert_eq!(
        pop_flight.entry_id(),
        push_entry,
        "the SAME overlay entry — diverted in place, not restarted"
    );
    assert_eq!(navigator.overlay().entry_ids().last().copied(), push_entry);

    // The reverse retraces the push: begin/end swapped. `on_tick` re-reads the
    // destination *origin* after the swap, so compare on size, which it preserves.
    assert_eq!(
        (
            pop_flight.begin_rect().width().0,
            pop_flight.begin_rect().height().0
        ),
        (push_end.width().0, push_end.height().0),
        "the tween now begins where the push was heading"
    );
    assert_eq!(
        (
            pop_flight.target_rect().width().0,
            pop_flight.target_rect().height().0
        ),
        (push_begin.width().0, push_begin.height().0),
        "and ends where the push began"
    );
}

/// A divert keeps **exactly one** active flight and **one** overlay entry for the tag —
/// the whole point of reusing the flight object. This is the same-direction branch
/// (`heroes.dart:781-815`): a third push over an airborne push flight.
///
/// Red-check: same as above — swap `divert` for end-and-restart; a *new* entry appears
/// and the old one is removed, so `entry_id` changes.
#[test]
fn a_same_tag_divert_keeps_one_active_flight_and_one_overlay_entry() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let entries_before = navigator.overlay().entry_ids().len();
    let _first = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );
    let airborne = controller.flights().get(&tag("shared")).expect("airborne");
    let entry_before = airborne.entry_id();
    assert_eq!(controller.flights().len(), 1);

    // A third page, same tag, pushed while the first flight is still airborne.
    let _third = navigator.push(hero_page("shared", 90.0, 70.0));
    harness.tick();

    assert_eq!(
        controller.flights().len(),
        1,
        "still one flight for the tag"
    );
    let diverted = controller
        .flights()
        .get(&tag("shared"))
        .expect("still airborne");
    assert_eq!(
        diverted.entry_id(),
        entry_before,
        "the same overlay entry was redirected, not replaced"
    );
    assert_eq!(
        navigator.overlay().entry_ids().len(),
        entries_before + 3 + 1,
        "three routes and exactly ONE shuttle: {:?}",
        navigator.overlay().entry_ids()
    );
}

/// A divert reaches back through the flight's `NavigatorHandle`, mutates the flight,
/// and repoints its `ProxyAnimation` — whose `set_parent` fires `on_tick`
/// synchronously. `on_tick` locks the same flight state `divert` just wrote, so the
/// lock discipline (release every flight lock before `set_parent`) is load-bearing: get
/// it wrong and this hangs.
///
/// A deadlock hangs rather than fails, so the body runs on a worker thread.
///
/// Red-check: hold `self.inner.rect.lock()` across `self.inner.proxy.set_parent(...)`
/// in `HeroFlight::divert` — `on_tick` then blocks on `rect` and this times out.
#[test]
fn a_divert_does_not_deadlock() {
    const BUDGET: Duration = Duration::from_secs(10);

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let navigator = seeded_navigator();
        let controller = install(&navigator);
        let mut harness = mount_navigator(&navigator);

        let _a = navigator.push(hero_page("shared", 30.0, 20.0));
        harness.tick();
        let b = hero_page("shared", 60.0, 45.0);
        let b_transition = b.transition_handle();
        let _b = navigator.push(b);
        harness.tick();
        b_transition.controller().expect("installed").set_value(0.5);

        assert!(navigator.pop());
        harness.tick();

        assert_eq!(controller.flights().len(), 1);
        let _ = done.send(());
    });

    assert!(
        finished.recv_timeout(BUDGET).is_ok(),
        "a divert deadlocked — a flight lock was held across ProxyAnimation::set_parent"
    );
}

/// The retired-flight discipline from `f7cb228c` survives divert: a **diverted** flight
/// is never parked in `retired` (it stays airborne, same object), and when it finally
/// settles it retires and drains at end-of-frame like any other — not inside a listener.
///
/// Red-check: in `FlightManager::start`, push the existing flight into `retired` before
/// diverting — `retired_count()` is then non-zero while a flight is still airborne.
#[test]
fn a_diverted_flight_retires_only_after_it_finally_settles() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();
    let b = hero_page("shared", 60.0, 45.0);
    let b_transition = b.transition_handle();
    let _b = navigator.push(b);
    harness.tick();
    b_transition.controller().expect("installed").set_value(0.5);

    assert!(navigator.pop());
    harness.tick();

    assert_eq!(controller.flights().len(), 1, "airborne after the divert");
    assert_eq!(
        controller.flights().retired_count(),
        0,
        "a diverted flight is redirected, never retired"
    );

    // Let the pop settle: B's animation runs to dismissed.
    b_transition.controller().expect("installed").set_value(0.0);
    assert_eq!(
        controller.flights().len(),
        0,
        "the settled flight left the active set"
    );
    assert_eq!(
        controller.flights().retired_count(),
        1,
        "parked, awaiting a safe drop"
    );

    harness.tick();
    assert_eq!(
        controller.flights().retired_count(),
        0,
        "and drained at end-of-frame, exactly as an undiverted flight"
    );
}

/// A spacer that can be told to grow, from outside the tree, without touching any
/// route animation.
#[derive(Clone, Default)]
struct Mover {
    tall: Arc<AtomicBool>,
    rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
}

impl Mover {
    /// Grow, and schedule the rebuild that makes it visible. `RebuildHandle` is
    /// acquired in `init_state` and fired from here — never from `build` (trigger #22).
    fn grow(&self) {
        self.tall.store(true, Ordering::SeqCst);
        if let Some(rebuild) = self.rebuild.lock().as_ref() {
            rebuild.schedule();
        }
    }
}

impl View for Mover {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Mover {
    type State = MoverState;

    fn create_state(&self) -> Self::State {
        MoverState {
            tall: Arc::clone(&self.tall),
            rebuild: Arc::clone(&self.rebuild),
        }
    }
}

struct MoverState {
    tall: Arc<AtomicBool>,
    rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
}

impl ViewState<Mover> for MoverState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        *self.rebuild.lock() = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &Mover, _ctx: &dyn BuildContext) -> impl IntoView {
        let height = if self.tall.load(Ordering::SeqCst) {
            100.0
        } else {
            0.0
        };
        SizedBox::new(1.0, height)
    }
}

// ============================================================================
// Cleanup — retired flights are drained deterministically
// ============================================================================

/// **The retention fix.** A flight ends from inside its own `ProxyAnimation` status
/// listener, where dropping it would free the animation the listener is running under.
/// So `FlightManager::finish` parks it in `retired` and schedules a drain — and that
/// drain must run at **end-of-frame**, not at the next hero measurement. Otherwise a
/// single transition with no follow-up leaks the whole flight graph (`HeroHandle`s,
/// the shuttle `BoxedView`, the animation, and via `HeroHandle::owner` the
/// `PipelineOwner`) until some unrelated hero activity happens.
///
/// The middle assertions pin the retire-not-drop discipline itself: the moment the
/// status listener runs, the flight is out of the active set but still parked.
///
/// Red-check: in `FlightManager::finish`, delete the `self.schedule_drain()` call — the
/// only remaining drain is `MeasurementPass`'s head, and with no second transition
/// `retired_count()` stays `1` forever. This test then fails on the final assertion.
#[test]
fn a_completed_flight_is_drained_after_its_frame_without_another_transition() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );
    assert_eq!(controller.flights().len(), 1, "airborne");
    assert_eq!(
        controller.flights().retired_count(),
        0,
        "nothing retired yet"
    );

    // Land it. The status listener runs synchronously inside `set_value`.
    let animation = transition.controller().expect("installed");
    animation.set_value(1.0);

    assert_eq!(
        controller.flights().len(),
        0,
        "removed from the active set the instant its listener runs"
    );
    assert_eq!(
        controller.flights().retired_count(),
        1,
        "parked in `retired`, not yet dropped — we were inside its listener"
    );

    // One frame later, and **no second hero transition**, `retired` is empty.
    harness.tick();

    assert_eq!(
        controller.flights().retired_count(),
        0,
        "the end-of-frame drain ran; the flight graph was released without waiting \
         for another measurement pass"
    );
}

/// Coalescing: many flights finishing in one frame schedule exactly **one** drain
/// (`FlightManager::schedule_drain`'s `drain_scheduled` guard).
///
/// Two shared tags fly on one push; the destination route's single animation lands
/// both at once, so both status listeners fire in the same turn.
///
/// Red-check: remove the `if self.drain_scheduled.swap(true, …) { return; }` guard from
/// `schedule_drain` — two flights then schedule two drains.
#[test]
fn many_flights_landing_in_one_frame_schedule_one_drain() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let two_heroes = |a: &'static str, b: &'static str, wa: f32, wb: f32| {
        PageRoute::<i32>::new(move |_ctx, _p, _s| {
            Column::new(vec![
                Hero::new(ValueKey::new(a), SizedBox::new(wa, 20.0))
                    .into_view()
                    .boxed(),
                Hero::new(ValueKey::new(b), SizedBox::new(wb, 20.0))
                    .into_view()
                    .boxed(),
            ])
            .main_axis_size(MainAxisSize::Min)
            .into_view()
            .boxed()
        })
        .transition_duration(TRANSITION)
    };

    let _source = navigator.push(two_heroes("one", "two", 30.0, 40.0));
    harness.tick();
    let destination = two_heroes("one", "two", 60.0, 80.0);
    let transition = destination.transition_handle();
    let _destination = navigator.push(destination);
    harness.tick();

    assert_eq!(controller.flights().len(), 2, "two tags, two flights");
    assert_eq!(controller.flights().drains_scheduled(), 0);

    let animation = transition.controller().expect("installed");
    animation.set_value(1.0);

    assert_eq!(controller.flights().retired_count(), 2, "both parked");
    assert_eq!(
        controller.flights().drains_scheduled(),
        1,
        "two flights, one coalesced drain"
    );

    harness.tick();
    assert_eq!(
        controller.flights().retired_count(),
        0,
        "both drained together"
    );
}

/// **pop interrupted by push.** A back-navigation's hero is still flying when a new
/// page is pushed. Flutter's `_HeroFlight.divert` pop→push branch (`heroes.dart:758-780`)
/// keeps the same flight and entry, drives the proxy from
/// `newAnimation.drive(Tween(begin: oldValue, end: 1.0))`, hands the old source its
/// placeholder back, and re-aims from the old end to the new destination.
///
/// Observable: one flight, the same entry, and the direction flipped `Pop → Push`.
///
/// Red-check: in `FlightManager::start`, end-and-restart instead of diverting — the
/// entry id changes and the flight is a brand-new pop... er, push, losing continuity.
#[test]
fn a_pop_flight_interrupted_by_a_push_diverts_in_place() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    // Push A, then B, and settle B so the push flight ends cleanly.
    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();
    let b = hero_page("shared", 60.0, 45.0);
    let b_transition = b.transition_handle();
    let _b = navigator.push(b);
    harness.tick();
    b_transition.controller().expect("installed").set_value(1.0);
    harness.tick();
    assert_eq!(controller.flights().len(), 0, "the push flight settled");

    // Pop B: a fresh pop flight B→A. Keep it airborne, mid-reverse.
    assert!(navigator.pop());
    harness.tick();
    b_transition.controller().expect("installed").set_value(0.5);
    let pop_flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("pop airborne");
    assert_eq!(pop_flight.direction(), FlightDirection::Pop);
    let pop_entry = pop_flight.entry_id();

    // Push C while the pop is still flying.
    let _c = navigator.push(hero_page("shared", 90.0, 70.0));
    harness.tick();

    assert_eq!(
        controller.flights().len(),
        1,
        "still one flight for the tag"
    );
    let diverted = controller
        .flights()
        .get(&tag("shared"))
        .expect("still airborne");
    assert_eq!(
        diverted.entry_id(),
        pop_entry,
        "the pop flight was redirected in place, keeping its overlay entry"
    );
    assert_eq!(
        diverted.direction(),
        FlightDirection::Push,
        "and now runs as a push toward the new route"
    );
    assert_eq!(
        navigator.overlay().entry_ids().last().copied(),
        pop_entry,
        "one shuttle, still on top"
    );
}

/// The same-direction divert (`heroes.dart:781-812`) **transfers the placeholders**:
/// the old pair get their `endFlight(keepPlaceholder: true)`, and the new pair are
/// frozen by `startFlight`. A push→push divert (third page over an airborne push)
/// leaves the *new* source hero holding a with-child placeholder and the new
/// destination a bare one.
///
/// Red-check: delete the `new_from.start_flight(new_dir == Push)` /
/// `new_to.start_flight(false)` calls from the same-direction branch of
/// `HeroFlight::divert` — the new heroes never freeze, so `placeholder_size()` is `None`.
#[test]
fn a_same_direction_divert_transfers_the_placeholders() {
    let navigator = seeded_navigator();
    let _controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();
    let _b = navigator.push(hero_page("shared", 60.0, 45.0));
    harness.tick();

    // A third page diverts the airborne A→B push into A→C.
    let _c = navigator.push(hero_page("shared", 90.0, 70.0));
    harness.tick();

    // Routes: 0 seeded, 1 = A, 2 = B (new source), 3 = C (new destination).
    let b_hero = hero_of(&navigator, 2, "shared");
    let c_hero = hero_of(&navigator, 3, "shared");

    assert_eq!(
        b_hero.placeholder_size().map(|s| (s.width.0, s.height.0)),
        Some((60.0, 45.0)),
        "B is now the flight's source, frozen at its size"
    );
    assert!(
        b_hero.includes_child(),
        "and keeps its child offstage — it is the *from* hero of a push"
    );
    assert_eq!(
        c_hero.placeholder_size().map(|s| (s.width.0, s.height.0)),
        Some((90.0, 70.0)),
        "C is the new destination, a bare hole"
    );
    assert!(
        !c_hero.includes_child(),
        "the destination drops its child — the shuttle carries it"
    );
}

// ============================================================================
// U5.2 — onTick fade-out when the destination is lost mid-flight
// ============================================================================

/// A page whose hero can be removed from outside the tree, without touching the route
/// animation — the harness capability the fade-out test needs. Flipping `present` and
/// firing the stored `RebuildHandle` rebuilds the page without its `Hero`, so the
/// destination hero unmounts while its route (and its animation) keep running.
#[derive(Clone, Default)]
struct HeroGate {
    present: Arc<AtomicBool>,
    rebuild: Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>,
    tag_name: &'static str,
}

impl HeroGate {
    fn showing(tag_name: &'static str) -> Self {
        Self {
            present: Arc::new(AtomicBool::new(true)),
            rebuild: Arc::new(parking_lot::Mutex::new(None)),
            tag_name,
        }
    }
    fn remove_hero(&self) {
        self.present.store(false, Ordering::SeqCst);
        if let Some(rebuild) = self.rebuild.lock().as_ref() {
            rebuild.schedule();
        }
    }
}

impl View for HeroGate {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}
impl StatefulView for HeroGate {
    type State = HeroGateState;
    fn create_state(&self) -> Self::State {
        HeroGateState {
            present: Arc::clone(&self.present),
            rebuild: Arc::clone(&self.rebuild),
            tag_name: self.tag_name,
        }
    }
}
struct HeroGateState {
    present: Arc<AtomicBool>,
    rebuild: Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>,
    tag_name: &'static str,
}
impl ViewState<HeroGate> for HeroGateState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        *self.rebuild.lock() = Some(ctx.rebuild_handle());
    }
    fn build(&self, _view: &HeroGate, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.present.load(Ordering::SeqCst) {
            Hero::new(ValueKey::new(self.tag_name), SizedBox::new(60.0, 45.0))
                .into_view()
                .boxed()
        } else {
            // Same footprint, no hero — the destination is *lost*, not merely resized.
            SizedBox::new(60.0, 45.0).into_view().boxed()
        }
    }
}

/// **The `onTick` fade-out, end to end** (`heroes.dart:687-692`): *"The toHero no longer
/// exists or it's no longer the flight's destination. Continue flying while fading
/// out."* When `toHero.context.findRenderObject()` yields nothing, the flight keeps its
/// overlay entry and drives `_heroOpacity` down instead of aborting.
///
/// The destination hero is removed with a `HeroGate` — a rebuild that drops the `Hero`
/// without advancing the route animation — so the flight's driver stays mid-air. Ticks
/// after that find no destination and begin the fade. The entry survives; only the
/// driving animation settling removes it.
///
/// Red-check (each fails on its own):
/// * delete the `else { fade_from = Some(...) }` arm from `FlightInner::on_tick` — the
///   opacity stays `1.0` after the destination is lost;
/// * in `on_tick`, set `fade_from` but leave the `opacity` computation at `1.0`.
#[test]
fn a_destination_lost_mid_flight_fades_out_without_ending_the_flight() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();

    let gate = HeroGate::showing("shared");
    let gate_for_page = gate.clone();
    let b = PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(gate_for_page.clone())
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION);
    let b_transition = b.transition_handle();
    let _b = navigator.push(b);
    harness.tick();

    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let entry = flight.entry_id().expect("has an overlay entry");

    let controller_b = b_transition.controller().expect("installed");
    controller_b.set_value(0.5);
    assert_eq!(
        flight.opacity(),
        1.0,
        "opaque while the destination is present"
    );

    // Remove the destination hero. The route — and its animation — stay.
    gate.remove_hero();
    harness.tick();

    // Advance: the first post-loss tick arms the fade; the next drives it down.
    controller_b.set_value(0.6);
    assert_eq!(
        controller.flights().len(),
        1,
        "the destination was lost, not the animation — the flight lives on"
    );
    controller_b.set_value(0.8);

    let faded = flight.opacity();
    assert!(
        faded > 0.0 && faded < 1.0,
        "the shuttle is fading, not gone: opacity = {faded}"
    );

    // The overlay entry is still present — only a settled animation removes it.
    let still = controller
        .flights()
        .get(&tag("shared"))
        .expect("still airborne");
    assert_eq!(still.entry_id(), Some(entry), "same entry, still flying");
    assert!(
        navigator.overlay().entry_ids().contains(&entry),
        "the flight entry outlives the lost destination"
    );
}

/// The other half: a fade-out still ends the flight — and removes the entry — once the
/// **driving animation** settles, not when the destination is lost.
///
/// Red-check: in `HeroFlight::finish`, guard on `fade_from.is_some()` and skip the
/// `entry.remove()` — a faded-out flight then leaks its overlay entry forever.
#[test]
fn a_faded_out_flight_still_removes_its_entry_when_the_animation_settles() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = navigator.push(hero_page("shared", 30.0, 20.0));
    harness.tick();

    let gate = HeroGate::showing("shared");
    let gate_for_page = gate.clone();
    let b = PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(gate_for_page.clone())
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION);
    let b_transition = b.transition_handle();
    let _b = navigator.push(b);
    harness.tick();

    let entry = controller
        .flights()
        .get(&tag("shared"))
        .expect("airborne")
        .entry_id()
        .expect("entry");
    let entries_with_flight = navigator.overlay().entry_ids().len();

    let controller_b = b_transition.controller().expect("installed");
    controller_b.set_value(0.5);
    gate.remove_hero();
    harness.tick();
    controller_b.set_value(0.8);
    assert_eq!(controller.flights().len(), 1, "fading");

    // Settle the driver: B's entrance completes.
    controller_b.set_value(1.0);
    harness.tick();

    assert_eq!(
        controller.flights().len(),
        0,
        "the settled animation ended the flight"
    );
    assert!(
        !navigator.overlay().entry_ids().contains(&entry),
        "and removed its overlay entry"
    );
    assert_eq!(
        navigator.overlay().entry_ids().len(),
        entries_with_flight - 1,
    );
}
