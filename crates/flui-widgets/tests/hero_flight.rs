//! The flight itself.
//!
//! A manifest becomes a shuttle in an overlay entry, two frozen placeholders, and a
//! driven `RectTween`. These tests pin the observable half of that: what is in the
//! overlay, what the heroes look like while it flies, where the shuttle is aimed, and
//! what is left behind when it lands.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Curves, Interval};
use flui_foundation::ValueKey;
use flui_foundation::geometry::Rect;
use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_widgets::__test_access::{
    HeroControllerProbe as _, HeroTag, NavigatorProbe as _, OverlayProbe as _, RouteProbe as _,
    TransitionHandle,
};
use flui_widgets::navigator::{
    Hero, HeroController, Navigator, NavigatorHandle, NavigatorObserver, PageRoute, SimpleRoute,
};
use flui_widgets::{Center, SizedBox};

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

// ============================================================================
// The flight exists
// ============================================================================

// ============================================================================
// What the heroes look like while it flies
// ============================================================================

// ============================================================================
// Landing
// ============================================================================

// ============================================================================
// Per-tick re-measure
// ============================================================================

// ============================================================================
// Divert — a same-tag flight interrupted mid-air is redirected in place
// ============================================================================

/// **push interrupted by pop.** Open a page, then immediately go back while its hero
/// is still flying. The divert reuses the *same* flight and its *same* overlay
/// entry, repoints the proxy at the reversed new animation, and reverses the rect
/// tween — the pop retraces the push path backwards, no jump cut.
///
/// Observable: one flight, the **same** `entry_id` as before the pop, and the tween's
/// begin/end swapped (the shuttle now heads back to where it came from).
///
/// Red-check: in `FlightManager::start`, replace the `existing.divert(…); return;`
/// with an end-and-restart (`self.flights.lock().remove(&tag)` + `finish` + a fresh
/// `start`). The entry id then changes and the begin/end are the fresh push tween.
pub(crate) fn a_push_flight_interrupted_by_a_pop_diverts_in_place() {
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

// ============================================================================
// Cleanup — retired flights are drained deterministically
// ============================================================================

// ============================================================================
// onTick fade-out when the destination is lost mid-flight
// ============================================================================

// ============================================================================
// Flight easing — `Hero.curve` / `Hero.reverse_curve`
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

/// `Hero::curve` shapes the flight, and a push eases on the **destination** hero's
/// curve. `Interval::linear(0.9, 1.0)` reads 0.0 until 90% of the transition —
/// so halfway through, the shuttle has not left its begin rect.
///
/// Red-check: resolve the curve from `from_hero` for a push in `MeasurementPass::launch`
/// — the source's default fastOutSlowIn applies and the shuttle is mid-flight.
pub(crate) fn a_push_eases_on_the_destination_hero_curve() {
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
            hero.curve(Interval::linear(0.9, 1.0))
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

/// A hero that shrinks to nothing along an overshooting curve: the curve passes 1
/// near the end, which extrapolates the rect past its zero-size end. The shuttle's
/// width and height stay non-negative on every sampled frame.
pub(crate) fn a_shrinking_flight_with_overshoot_keeps_a_non_negative_size() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 100.0, 80.0),
        hero_page_with("shared", 0.0, 0.0, |hero| hero.curve(Curves::EaseOutBack)),
    );
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    for step in 0..=20 {
        let t = f64::from(step) / 20.0;
        harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(t));
        let rect = flight.shuttle_rect();
        assert!(
            rect.width() >= 0.0 && rect.height() >= 0.0,
            "t = {t}: shuttle rect {rect:?}"
        );
    }
}

// ============================================================================
// HeroMode
// ============================================================================
