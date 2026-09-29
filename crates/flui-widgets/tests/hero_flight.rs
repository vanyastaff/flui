//! The flight itself.
//!
//! A manifest becomes a shuttle in an overlay entry, two frozen placeholders, and a
//! driven `RectTween`. These tests pin the observable half of that: what is in the
//! overlay, what the heroes look like while it flies, where the shuttle is aimed, and
//! what is left behind when it lands.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_animation::{Animatable, Animation, AnimationStatus, Curve, Curves, RectTween, Threshold};
use flui_foundation::ValueKey;
use flui_foundation::geometry::Rect;
use flui_view::ViewExt;
use flui_view::prelude::*;
use parking_lot::Mutex;

use flui_widgets::__test_access::{
    HeroControllerProbe as _, HeroHandle, HeroTag, NavigatorProbe as _, OverlayProbe as _,
    RouteProbe as _, TransitionHandle,
};
use flui_widgets::navigator::{
    Hero, HeroController, HeroMode, Navigator, NavigatorHandle, NavigatorObserver, PageRoute,
    SimpleRoute,
};
use flui_widgets::{Center, Column, MainAxisSize, SizedBox};

use crate::common::harness::{Harness, mount};

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

/// A `PageRoute` whose page is one `Hero`, centred so it does not fill the screen.
fn hero_page(tag_name: &'static str, w: f64, h: f64) -> PageRoute<i32> {
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
    let _source = harness.enter_owner_scope(|| navigator.push(source));
    harness.tick();
    let transition = destination.transition_handle();
    let _destination = harness.enter_owner_scope(|| navigator.push(destination));
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
            .map(|size| (size.width, size.height)),
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
    harness.enter_owner_scope(|| animation.set_value(1.0));
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
/// stops being a no-op the moment a `placeholderBuilder` can hand back a
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

    let _source = harness.enter_owner_scope(|| navigator.push(hero_page("shared", 30.0, 20.0)));
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
    let _destination = harness.enter_owner_scope(|| navigator.push(destination));
    harness.tick();

    let flight = controller
        .flights()
        .get(&tag("shared"))
        .expect("in the air");
    let target_before = flight.target_rect();
    let begin_before = flight.begin_rect();
    assert_eq!(
        (target_before.width(), target_before.height()),
        (60.0, 45.0)
    );

    // Grow the spacer and let the destination route lay out again.
    mover.grow();
    harness.tick();

    // Tick the flight: the proxy's parent is the destination route's animation.
    let animation = transition.controller().expect("installed");
    harness.enter_owner_scope(|| animation.set_value(0.25));
    harness.tick();

    let target_after = flight.target_rect();
    assert_eq!(
        (target_after.min_y() - target_before.min_y()),
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
        (target_after.width(), target_after.height()),
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

fn rect_origin(rect: Rect) -> (f64, f64) {
    (rect.min_x(), rect.min_y())
}

// ============================================================================
// Divert — a same-tag flight interrupted mid-air is redirected in place
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
/// with an end-and-restart (`self.flights.lock().remove(&tag)` + `finish` + a fresh
/// `start`). The entry id then changes and the begin/end are the fresh push tween.
#[test]
fn a_push_flight_interrupted_by_a_pop_diverts_in_place() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _a = harness.enter_owner_scope(|| navigator.push(hero_page("shared", 30.0, 20.0)));
    harness.tick();
    let b = hero_page("shared", 60.0, 45.0);
    let b_transition = b.transition_handle();
    let _b = harness.enter_owner_scope(|| navigator.push(b));
    harness.tick();

    // The push flight is airborne, parked mid-entrance so the pop genuinely reverses.
    harness.enter_owner_scope(|| b_transition.controller().expect("installed").set_value(0.5));
    let push_flight = controller.flights().get(&tag("shared")).expect("airborne");
    let push_entry = push_flight.entry_id();
    let push_begin = push_flight.begin_rect();
    let push_end = push_flight.target_rect();

    // Go back: B pops while its hero is still in flight.
    assert!(harness.enter_owner_scope(|| navigator.pop()));
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
            pop_flight.begin_rect().width(),
            pop_flight.begin_rect().height()
        ),
        (push_end.width(), push_end.height()),
        "the tween now begins where the push was heading"
    );
    assert_eq!(
        (
            pop_flight.target_rect().width(),
            pop_flight.target_rect().height()
        ),
        (push_begin.width(), push_begin.height()),
        "and ends where the push began"
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

        let _a = harness.enter_owner_scope(|| navigator.push(hero_page("shared", 30.0, 20.0)));
        harness.tick();
        let b = hero_page("shared", 60.0, 45.0);
        let b_transition = b.transition_handle();
        let _b = harness.enter_owner_scope(|| navigator.push(b));
        harness.tick();
        harness.enter_owner_scope(|| b_transition.controller().expect("installed").set_value(0.5));

        assert!(harness.enter_owner_scope(|| navigator.pop()));
        harness.tick();

        assert_eq!(controller.flights().len(), 1);
        let _ = done.send(());
    });

    assert!(
        finished.recv_timeout(BUDGET).is_ok(),
        "a divert deadlocked — a flight lock was held across ProxyAnimation::set_parent"
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
    /// acquired in `init_state` and fired from here — never from `build`.
    fn grow(&self) {
        self.tall.store(true, Ordering::SeqCst);
        if let Some(rebuild) = self.rebuild.lock().as_ref() {
            rebuild.schedule(flui_view::RebuildReason::AnimationTick);
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let _prev = self.rebuild.lock().replace(ctx.rebuild_handle());
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
/// The flight lands from owner-local rebuild, not from the data-plane status
/// listener. The same frame removes it from the active set and drains the retired
/// queue at end-of-frame.
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

    // Land it. The data-plane status listener records the terminal status; the
    // owner-local shuttle drains it on the next frame.
    let animation = transition.controller().expect("installed");
    harness.enter_owner_scope(|| animation.set_value(1.0));

    harness.tick();

    assert_eq!(controller.flights().len(), 0, "the flight ended");
    assert_eq!(
        controller.flights().retired_count(),
        0,
        "the end-of-frame drain ran; the flight graph was released without waiting \
         for another measurement pass"
    );
}

// ============================================================================
// onTick fade-out when the destination is lost mid-flight
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
            rebuild.schedule(flui_view::RebuildReason::AnimationTick);
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let _prev = self.rebuild.lock().replace(ctx.rebuild_handle());
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

    let _a = harness.enter_owner_scope(|| navigator.push(hero_page("shared", 30.0, 20.0)));
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
    let _b = harness.enter_owner_scope(|| navigator.push(b));
    harness.tick();

    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let entry = flight.entry_id().expect("has an overlay entry");

    let controller_b = b_transition.controller().expect("installed");
    harness.enter_owner_scope(|| controller_b.set_value(0.5));
    assert_eq!(
        flight.opacity(),
        1.0,
        "opaque while the destination is present"
    );

    // Remove the destination hero. The route — and its animation — stay.
    gate.remove_hero();
    harness.tick();

    // Advance: the first post-loss tick arms the fade; the next drives it down.
    harness.enter_owner_scope(|| controller_b.set_value(0.6));
    harness.tick();
    assert_eq!(
        controller.flights().len(),
        1,
        "the destination was lost, not the animation — the flight lives on"
    );
    harness.enter_owner_scope(|| controller_b.set_value(0.8));
    harness.tick();

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

// ============================================================================
// Flight easing — `Hero.curve` / `Hero.reverse_curve` (heroes.dart:472-491)
// ============================================================================

/// A `hero_page` whose `Hero` is customized by `configure` — a flight curve, say.
fn hero_page_with(
    tag_name: &'static str,
    w: f64,
    h: f64,
    configure: impl Fn(Hero) -> Hero + 'static,
) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        Center::new()
            .child(configure(Hero::new(
                ValueKey::new(tag_name),
                SizedBox::new(w, h),
            )))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// All four extents of `actual` match `expected` to within a thousandth of a pixel.
fn assert_rect_close(actual: Rect, expected: Rect, what: &str) {
    for (got, want, edge) in [
        (actual.min_x(), expected.min_x(), "left"),
        (actual.min_y(), expected.min_y(), "top"),
        (actual.width(), expected.width(), "width"),
        (actual.height(), expected.height(), "height"),
    ] {
        assert!(
            (got - want).abs() < 1e-3,
            "{what}: {edge} is {got}, expected {want}"
        );
    }
}

/// The default flight easing is `Curves.fastOutSlowIn`, not linear: `launch` wraps the
/// route animation in the manifest's `CurvedAnimation` (`heroes.dart:472-491`), and
/// `Hero.curve` defaults to `Curves.fastOutSlowIn` (`:181`).
///
/// Red-check: hand `FlightPlan` the raw route animation in `MeasurementPass::launch` —
/// the shuttle sits at the linear midpoint instead of the eased point.
#[test]
fn a_default_flight_eases_on_fast_out_slow_in() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page("shared", 60.0, 45.0),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));

    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let tween = RectTween {
        begin: flight.begin_rect(),
        end: flight.target_rect(),
    };
    let eased = Curves::FastOutSlowIn.transform(0.5);
    assert!(
        (eased - 0.5).abs() > 0.2,
        "sanity: fastOutSlowIn is visibly non-linear at t = 0.5 (got {eased})"
    );
    assert_rect_close(
        flight.shuttle_rect(),
        tween.transform(eased),
        "halfway through the push, the shuttle sits at the fastOutSlowIn point",
    );
}

/// `Hero::curve` shapes the flight, and a push eases on the **destination** hero's
/// curve (`heroes.dart:479`). `Threshold(0.9)` reads 0.0 until 90% of the transition —
/// so halfway through, the shuttle has not left its begin rect.
///
/// Red-check: resolve the curve from `from_hero` for a push in `MeasurementPass::launch`
/// — the source's default fastOutSlowIn applies and the shuttle is mid-flight.
#[test]
fn a_push_eases_on_the_destination_hero_curve() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let configure_calls = Rc::new(Cell::new(0));
    let configure_calls_for_page = Rc::clone(&configure_calls);

    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, move |hero| {
            configure_calls_for_page.set(configure_calls_for_page.get() + 1);
            hero.curve(Threshold::new(0.9))
        }),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    assert!(
        configure_calls.get() > 0,
        "hero page configuration must accept owner-local Rc<Cell<_>> state"
    );

    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    assert_rect_close(
        flight.shuttle_rect(),
        flight.begin_rect(),
        "below the destination hero's threshold curve, the shuttle has not moved",
    );
}

/// `Hero::reverse_curve` overrides the flipped default, and a pop reads it from the
/// **source** hero (`heroes.dart:483-484`). `Threshold(0.9)` reads 0.0 at t = 0.5 and
/// the flight reverses it, so the shuttle is parked exactly on its destination.
///
/// Red-check: ignore `reverse_curve()` in `MeasurementPass::launch` — the flipped
/// fastOutSlowIn default applies and the shuttle is mid-flight.
#[test]
fn a_pop_eases_on_the_source_hero_reverse_curve() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, |hero| {
            hero.reverse_curve(Threshold::new(0.9))
        }),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(1.0));
    harness.tick();
    assert_eq!(controller.flights().len(), 0, "the push flight settled");

    assert!(harness.enter_owner_scope(|| navigator.pop()));
    harness.tick();
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));

    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    assert_rect_close(
        flight.shuttle_rect(),
        flight.target_rect(),
        "below the source hero's reverse-curve threshold, the reversed flight reads 1.0",
    );
}

// ============================================================================
// HeroMode (heroes.dart:1124-1152, :335-337)
// ============================================================================

/// A `hero_page` under a chain of `HeroMode` scopes, outermost first.
fn hero_mode_page(
    tag_name: &'static str,
    w: f64,
    h: f64,
    modes: &'static [bool],
) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        let mut view = Center::new()
            .child(Hero::new(ValueKey::new(tag_name), SizedBox::new(w, h)))
            .into_view()
            .boxed();
        for &enabled in modes.iter().rev() {
            view = HeroMode::new(view).enabled(enabled).into_view().boxed();
        }
        view
    })
    .transition_duration(TRANSITION)
}

/// A destination hero under `HeroMode(enabled: false)` does not fly: Flutter's
/// flight-time walk never visits it (`heroes.dart:335-337`), so its tag is missing
/// from the destination map and no manifest is built (`:1044-1046`).
///
/// Red-check: drop the `hero_mode_enabled` filter from `collect_manifests`.
#[test]
fn a_hero_under_a_disabled_hero_mode_does_not_fly() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);

    let _transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_mode_page("shared", 60.0, 45.0, &[false]),
    );

    assert_eq!(controller.flights().len(), 0, "the disabled hero stays put");
    assert!(controller.manifests().is_empty(), "no manifest was built");
}
