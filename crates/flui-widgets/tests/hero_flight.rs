//! The flight itself.
//!
//! A manifest becomes a shuttle in an overlay entry, two frozen placeholders, and a
//! driven `RectTween`. These tests pin the observable half of that: what is in the
//! overlay, what the heroes look like while it flies, where the shuttle is aimed, and
//! what is left behind when it lands.

use std::cell::{Cell, RefCell};
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

struct AsymmetricHeroPath {
    endpoints: flui_animation::RectTween,
    bend: f64,
}

impl flui_animation::Animatable for AsymmetricHeroPath {
    type Value = Rect;

    fn transform(&self, t: f64) -> Rect {
        let rect = flui_animation::Animatable::transform(&self.endpoints, t);
        Rect::from_ltwh(
            rect.min_x() + self.bend * t * t * (1.0 - t),
            rect.min_y(),
            rect.width(),
            rect.height(),
        )
    }
}

pub(crate) fn a_nonlinear_hero_pop_retraces_the_airborne_push() {
    for source_bend in [None, Some(-120.0)] {
        let navigator = seeded_navigator();
        let controller = install(&navigator);
        let mut harness = mount_navigator(&navigator);
        let transition = fly(
            &navigator,
            &mut harness,
            hero_page_with("shared", 30.0, 20.0, move |hero| {
                let hero = hero.curve(Curves::Linear);
                match source_bend {
                    Some(bend) => hero.create_rect_tween(move |begin, end| AsymmetricHeroPath {
                        endpoints: flui_animation::RectTween { begin, end },
                        bend,
                    }),
                    None => hero,
                }
            }),
            hero_page_with("shared", 60.0, 45.0, |hero| {
                hero.curve(Curves::Linear)
                    .create_rect_tween(|begin, end| AsymmetricHeroPath {
                        endpoints: flui_animation::RectTween { begin, end },
                        bend: 160.0,
                    })
            }),
        );
        let route_animation = transition.controller().expect("installed");
        let push = controller.flights().get(&tag("shared")).expect("airborne");
        let entry = push.entry_id();
        let samples = [0.2, 0.4, 0.7].map(|t| {
            harness.enter_owner_scope(|| route_animation.set_value(t));
            (t, push.shuttle_rect())
        });

        assert!(harness.enter_owner_scope(|| navigator.pop()));
        harness.tick();
        let pop = controller.flights().get(&tag("shared")).expect("returning");
        assert_eq!(pop.entry_id(), entry, "the same shuttle returns");
        for (t, expected) in samples.into_iter().rev() {
            harness.enter_owner_scope(|| route_animation.set_value(t));
            assert_rect_close(pop.shuttle_rect(), expected, "pop retraces the push path");
        }
        if source_bend.is_some() {
            let next_page = hero_page_with("shared", 90.0, 75.0, |hero| {
                hero.curve(Curves::Linear)
                    .create_rect_tween(|begin, end| AsymmetricHeroPath {
                        endpoints: flui_animation::RectTween { begin, end },
                        bend: -240.0,
                    })
            });
            let next_transition = next_page.transition_handle();
            let _next = harness.enter_owner_scope(|| navigator.push(next_page));
            harness.tick();
            let next_animation = next_transition.controller().expect("installed");
            harness.enter_owner_scope(|| next_animation.set_value(0.5));
            let redirected = controller
                .flights()
                .get(&tag("shared"))
                .expect("redirected");
            assert_eq!(
                redirected.entry_id(),
                entry,
                "redirection keeps the shuttle"
            );
            // The new push covers the unfinished 0.2→1.0 interval; halfway is 0.6.
            let expected = flui_animation::Animatable::transform(
                &AsymmetricHeroPath {
                    endpoints: flui_animation::RectTween {
                        begin: redirected.begin_rect(),
                        end: redirected.target_rect(),
                    },
                    bend: -240.0,
                },
                0.6,
            );
            assert_rect_close(
                redirected.shuttle_rect(),
                expected,
                "a new destination selects its own forward path",
            );
            harness.enter_owner_scope(|| next_animation.set_value(1.0));
        } else {
            harness.enter_owner_scope(|| route_animation.set_value(0.0));
        }
        harness.tick();
        assert_eq!(controller.flights().len(), 0, "the flight lands");
    }
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

struct LocalRectMapping {
    endpoints: flui_animation::RectTween,
    hook: Rc<dyn Fn()>,
    phase: &'static str,
}

impl flui_animation::Animatable for LocalRectMapping {
    type Value = Rect;

    fn transform(&self, t: f64) -> Rect {
        if self.phase == "transform" {
            (self.hook)();
        }
        flui_animation::Animatable::transform(&self.endpoints, t)
    }
}

impl Drop for LocalRectMapping {
    fn drop(&mut self) {
        if self.phase == "drop" {
            (self.hook)();
        }
    }
}

pub(crate) fn cancelling_a_hero_mapping_does_not_refreeze_the_previous_page() {
    struct PassiveHeroObserver;
    impl NavigatorObserver for PassiveHeroObserver {
        fn observes_hero_flights(&self) -> bool {
            true
        }
    }

    let frame = Duration::from_millis(16);
    for phase in ["factory", "transform", "drop"] {
        let vsync = flui_animation::Vsync::new();
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(hero_page("shared", 30.0, 20.0));
        let mut laid = crate::common::lay_out_animated(
            flui_widgets::VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
            crate::common::tight(400.0, 400.0),
            vsync,
        );
        let armed = Rc::new(Cell::new(false));
        let cancelled = Rc::new(Cell::new(false));
        let taps = Rc::new(Cell::new(0));
        let hook: Rc<dyn Fn()> = {
            let navigator = navigator.clone();
            let armed = Rc::clone(&armed);
            let cancelled = Rc::clone(&cancelled);
            Rc::new(move || {
                if armed.replace(false) {
                    cancelled.set(true);
                    navigator.add_observer(Arc::new(PassiveHeroObserver));
                }
            })
        };
        let page = PageRoute::<i32>::new({
            let taps = Rc::clone(&taps);
            move |_ctx, _primary, _secondary| {
                let taps = Rc::clone(&taps);
                let hook = Rc::clone(&hook);
                Center::new()
                    .child(
                        Hero::new(
                            ValueKey::new("shared"),
                            flui_widgets::GestureDetector::new()
                                .behavior(flui_widgets::HitTestBehavior::Opaque)
                                .on_tap(move |_| taps.set(taps.get() + 1))
                                .child(SizedBox::new(60.0, 45.0)),
                        )
                        .curve(Curves::Linear)
                        .create_rect_tween(move |begin, end| {
                            if phase == "factory" {
                                hook();
                            }
                            LocalRectMapping {
                                endpoints: flui_animation::RectTween::new(begin, end),
                                hook: Rc::clone(&hook),
                                phase,
                            }
                        }),
                    )
                    .into_view()
                    .boxed()
            }
        })
        .transition_duration(TRANSITION);
        let _push = laid.enter_owner_scope(|| navigator.push(page));
        laid.pump_for(frame);
        laid.pump_for(frame);
        let post_frame = laid.post_frame_handle();
        let _divert = laid.enter_owner_scope(|| {
            post_frame
                .schedule(move |_| armed.set(true))
                .expect("the mounted presentation accepts the callback");
            navigator.push(hero_page("shared", 90.0, 75.0))
        });
        laid.pump_for(frame);
        assert!(cancelled.get(), "the {phase} callout cancelled the divert");
        laid.pump_for(frame);
        assert!(laid.enter_owner_scope(|| navigator.pop()));
        for _ in 0..24 {
            laid.pump_for(frame);
        }
        laid.dispatch_pointer_down(200.0, 200.0);
        laid.dispatch_pointer_up(200.0, 200.0);
        laid.pump_for(frame);
        assert_eq!(taps.get(), 1, "{phase}: the restored page accepts taps");
    }
}

fn rect_mapping_reentry(phase: &'static str) {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let armed = Rc::new(Cell::new(false));
    let busy = Rc::new(Cell::new(false));
    let nested = Rc::new(Cell::new(None));
    let hook: Rc<dyn Fn()> = {
        let controller = Arc::downgrade(&controller);
        let armed = Rc::clone(&armed);
        let busy = Rc::clone(&busy);
        let nested = Rc::clone(&nested);
        Rc::new(move || {
            if !armed.get() || busy.replace(true) {
                return;
            }
            let controller = controller.upgrade().expect("live owner");
            let flight = controller.flights().get(&tag("shared")).expect("airborne");
            nested.set(Some(flight.shuttle_rect()));
            busy.set(false);
        })
    };
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, move |hero| {
            let hook = Rc::clone(&hook);
            hero.curve(Curves::Linear)
                .create_rect_tween(move |begin, end| {
                    if phase == "factory" {
                        hook();
                    }
                    LocalRectMapping {
                        endpoints: flui_animation::RectTween::new(begin, end),
                        hook: Rc::clone(&hook),
                        phase,
                    }
                })
        }),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    armed.set(true);
    let rect = flight.shuttle_rect();
    assert_rect_close(
        nested
            .get()
            .expect("owner-local mapping reentered its flight"),
        rect,
        "reentrant mapping reads the same committed flight geometry",
    );
    assert_rect_close(
        rect,
        flight.begin_rect().lerp(flight.target_rect(), 0.5),
        "custom mapping drives the shuttle",
    );
    armed.set(false);
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(1.0));
    harness.tick();
    assert_eq!(
        controller.flights().len(),
        0,
        "the flight still lands after reentry"
    );
}

#[test]
fn hero_rect_mappings_are_owner_local_and_reentrant() {
    use crate::common::child_process;

    if let Some(case) = child_process::selected_case() {
        let phase = match case.as_str() {
            "factory" => "factory",
            "transform" => "transform",
            "drop" => "drop",
            _ => panic!("unknown mapping phase"),
        };
        rect_mapping_reentry(phase);
        child_process::pass();
    }
    child_process::run_rows(
        "hero_flight::hero_rect_mappings_are_owner_local_and_reentrant",
        &["factory", "transform", "drop"],
    );
}

struct FailingRectMapping {
    endpoints: flui_animation::RectTween,
    armed: Rc<Cell<bool>>,
    drops: Rc<Cell<usize>>,
    phase: &'static str,
}

impl flui_animation::Animatable for FailingRectMapping {
    type Value = Rect;

    fn transform(&self, t: f64) -> Rect {
        assert!(
            !(self.armed.get() && matches!(self.phase, "transform" | "competing")),
            "mapping evaluation"
        );
        flui_animation::Animatable::transform(&self.endpoints, t)
    }
}

impl Drop for FailingRectMapping {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        assert!(
            !(self.armed.get() && matches!(self.phase, "drop" | "competing")),
            "mapping retirement"
        );
    }
}

fn rect_mapping_failure(phase: &'static str) {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let armed = Rc::new(Cell::new(false));
    let drops = Rc::new(Cell::new(0));
    let factory_armed = Rc::clone(&armed);
    let factory_drops = Rc::clone(&drops);
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, move |hero| {
            let armed = Rc::clone(&factory_armed);
            let drops = Rc::clone(&factory_drops);
            hero.curve(Curves::Linear)
                .create_rect_tween(move |begin, end| {
                    assert!(!(armed.get() && phase == "factory"), "factory evaluation");
                    FailingRectMapping {
                        endpoints: flui_animation::RectTween::new(begin, end),
                        armed: Rc::clone(&armed),
                        drops: Rc::clone(&drops),
                        phase,
                    }
                })
        }),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let before = drops.get();
    armed.set(true);
    let result = catch_unwind(AssertUnwindSafe(|| flight.shuttle_rect()));
    armed.set(false);

    if phase == "healthy" {
        assert!(result.is_ok(), "healthy mapping retires normally");
        assert_eq!(drops.get(), before + 1);
    } else {
        let expected = match phase {
            "factory" => "factory evaluation",
            "transform" | "competing" => "mapping evaluation",
            "drop" => "mapping retirement",
            _ => panic!("unknown failure phase"),
        };
        let failure = result.expect_err("the authored failure propagates");
        assert_eq!(failure.downcast_ref::<&str>().copied(), Some(expected));
        if matches!(phase, "transform" | "competing" | "factory") {
            assert_eq!(
                drops.get(),
                before,
                "incoming failure retains opaque mapping ownership"
            );
        } else {
            assert_eq!(drops.get(), before + 1);
        }
    }
    assert_rect_close(
        flight.shuttle_rect(),
        flight.begin_rect().lerp(flight.target_rect(), 0.5),
        "the next read recovers through the same flight",
    );
    assert!(drops.get() > before, "healthy mapping destruction resumes");
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(1.0));
    harness.tick();
    assert_eq!(controller.flights().len(), 0, "the recovered flight lands");
}

#[test]
fn hero_rect_mapping_failures_preserve_recovery() {
    use crate::common::child_process;

    if let Some(case) = child_process::selected_case() {
        let phase = match case.as_str() {
            "healthy" => "healthy",
            "factory" => "factory",
            "transform" => "transform",
            "drop" => "drop",
            "competing" => "competing",
            _ => panic!("unknown mapping case"),
        };
        rect_mapping_failure(phase);
        child_process::pass();
    }
    child_process::run_rows(
        "hero_flight::hero_rect_mapping_failures_preserve_recovery",
        &["healthy", "factory", "transform", "drop", "competing"],
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

struct SampledHeroPath {
    endpoints: flui_animation::RectTween,
    sample: Rc<Cell<Option<Rect>>>,
}

impl flui_animation::Animatable for SampledHeroPath {
    type Value = Rect;

    fn transform(&self, t: f64) -> Rect {
        self.sample
            .get()
            .unwrap_or_else(|| flui_animation::Animatable::transform(&self.endpoints, t))
    }
}

fn exercise_hero_geometry_sample(sample: Rect, accepted: Option<Rect>) {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let replacement = Rc::new(Cell::new(None));
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, {
            let replacement = Rc::clone(&replacement);
            move |hero| {
                let replacement = Rc::clone(&replacement);
                hero.curve(Curves::Linear)
                    .create_rect_tween(move |begin, end| SampledHeroPath {
                        endpoints: flui_animation::RectTween::new(begin, end),
                        sample: Rc::clone(&replacement),
                    })
            }
        }),
    );
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let entry = flight.entry_id();
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    harness.tick();
    let previous = flight.shuttle_rect();
    assert!(previous.is_finite() && previous.width() > 0.0 && previous.height() > 0.0);

    replacement.set(Some(sample));
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.75));
    let sampled = flight.shuttle_rect();
    assert!(
        sampled.is_finite(),
        "mapping sample must not publish non-finite coordinates: {sampled:?}"
    );
    assert!(
        sampled.width().is_finite() && sampled.height().is_finite(),
        "finite corners must not overflow the published size"
    );
    assert_rect_close(
        sampled,
        accepted.unwrap_or(previous),
        "invalid mapping preserves the last accepted geometry",
    );
    // The same producer is used by the actual overlay's Positioned shuttle.
    harness.tick();
    assert_eq!(
        flight.entry_id(),
        entry,
        "invalid geometry does not retire the flight"
    );
    assert_eq!(controller.flights().len(), 1);

    replacement.set(None);
    let next = flight.shuttle_rect();
    assert_rect_close(
        next,
        flight.begin_rect().lerp(flight.target_rect(), 0.75),
        "the next finite mapping recovers on the same flight",
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(1.0));
    harness.tick();
    assert_eq!(controller.flights().len(), 0, "the recovered flight lands");
    assert_rect_close(
        flight.shuttle_rect(),
        flight.target_rect(),
        "landing retains the final accepted geometry",
    );
}

fn nan_hero_origin() {
    exercise_hero_geometry_sample(Rect::from_ltrb(f64::NAN, 10.0, 30.0, 40.0), None);
}

fn nan_hero_extent() {
    exercise_hero_geometry_sample(Rect::from_ltrb(10.0, 20.0, f64::NAN, 40.0), None);
}

fn infinite_hero_coordinate() {
    exercise_hero_geometry_sample(Rect::from_ltrb(10.0, f64::NEG_INFINITY, 30.0, 40.0), None);
}

fn overflowing_hero_extent() {
    exercise_hero_geometry_sample(Rect::from_ltrb(-f64::MAX, 20.0, f64::MAX, 40.0), None);
}

fn overflowing_hero_height() {
    exercise_hero_geometry_sample(Rect::from_ltrb(10.0, -f64::MAX, 30.0, f64::MAX), None);
}

fn inverted_finite_hero_extent() {
    exercise_hero_geometry_sample(
        Rect::from_ltrb(20.0, 10.0, 5.0, 1.0),
        Some(Rect::from_ltwh(20.0, 10.0, 0.0, 0.0)),
    );
}

fn underflowing_inverted_hero_extent() {
    exercise_hero_geometry_sample(
        Rect::from_ltrb(f64::MAX, f64::MAX, -f64::MAX, -f64::MAX),
        Some(Rect::from_ltwh(f64::MAX, f64::MAX, 0.0, 0.0)),
    );
}

pub(crate) fn hero_geometry_samples_preserve_the_last_finite_shuttle() {
    crate::common::cases::run_cases(
        "hero geometry samples",
        &[
            ("nan_origin", nan_hero_origin),
            ("nan_extent", nan_hero_extent),
            ("infinite_coordinate", infinite_hero_coordinate),
            ("overflowing_extent", overflowing_hero_extent),
            ("overflowing_height", overflowing_hero_height),
            ("inverted_finite_extent", inverted_finite_hero_extent),
            (
                "underflowing_inverted_extent",
                underflowing_inverted_hero_extent,
            ),
        ],
    );
}

struct ReentrantHeroPath {
    endpoints: flui_animation::RectTween,
    candidate: Option<Rect>,
    hook: Rc<dyn Fn()>,
    phase: &'static str,
}

impl flui_animation::Animatable for ReentrantHeroPath {
    type Value = Rect;

    fn transform(&self, t: f64) -> Rect {
        let candidate = self
            .candidate
            .unwrap_or_else(|| flui_animation::Animatable::transform(&self.endpoints, t));
        if self.phase == "transform" {
            (self.hook)();
        }
        candidate
    }
}

impl Drop for ReentrantHeroPath {
    fn drop(&mut self) {
        if self.phase == "drop" {
            (self.hook)();
        }
    }
}

fn exercise_hero_geometry_authority(phase: &'static str, cancel: bool) {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let armed = Rc::new(Cell::new(false));
    let replacement = Rc::new(Cell::new(None));
    let nested = Rc::new(Cell::new(None));
    let newer = Rect::from_ltwh(101.0, 109.0, 30.0, 40.0);
    let hook: Rc<dyn Fn()> = {
        let armed = Rc::clone(&armed);
        let replacement = Rc::clone(&replacement);
        let nested = Rc::clone(&nested);
        let controller = Arc::downgrade(&controller);
        Rc::new(move || {
            if !armed.replace(false) {
                return;
            }
            if cancel {
                // Public lifecycle cancellation of the same mounted observer;
                // adding another manual observer would leave this one attached.
                controller.upgrade().expect("live owner").did_detach();
            } else {
                replacement.set(Some(newer));
                let flight = controller
                    .upgrade()
                    .expect("live owner")
                    .flights()
                    .get(&tag("shared"))
                    .expect("airborne");
                nested.set(Some(flight.shuttle_rect()));
            }
        })
    };
    let transition = fly(
        &navigator,
        &mut harness,
        hero_page("shared", 30.0, 20.0),
        hero_page_with("shared", 60.0, 45.0, {
            let replacement = Rc::clone(&replacement);
            move |hero| {
                let replacement = Rc::clone(&replacement);
                let hook = Rc::clone(&hook);
                hero.curve(Curves::Linear)
                    .create_rect_tween(move |begin, end| {
                        let candidate = replacement.get();
                        if phase == "factory" {
                            hook();
                        }
                        ReentrantHeroPath {
                            endpoints: flui_animation::RectTween::new(begin, end),
                            candidate,
                            hook: Rc::clone(&hook),
                            phase,
                        }
                    })
            }
        }),
    );
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    harness.tick();
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    let previous = flight.shuttle_rect();
    if cancel {
        replacement.set(Some(newer));
    }
    armed.set(true);
    let outer = harness.enter_owner_scope(|| flight.shuttle_rect());
    assert!(!armed.get(), "the actual mapping callback ran");
    if cancel {
        assert_eq!(
            controller.flights().len(),
            0,
            "the callback withdrew the flight"
        );
        assert_rect_close(
            outer,
            previous,
            "cancelled mapping cannot publish its in-flight result",
        );
        assert_rect_close(
            flight.shuttle_rect(),
            previous,
            "a retained ended flight keeps its last accepted geometry",
        );
        harness.enter_owner_scope(|| controller.did_attach(navigator.clone()));
        let page = hero_page("shared", 90.0, 75.0);
        let next = page.transition_handle();
        let _route = harness.enter_owner_scope(|| navigator.push(page));
        harness.tick();
        let recovered = controller
            .flights()
            .get(&tag("shared"))
            .expect("the next flight is admitted");
        assert!(recovered.shuttle_rect().is_finite());
        harness.enter_owner_scope(|| next.controller().expect("installed").set_value(1.0));
        harness.tick();
        assert_eq!(
            controller.flights().len(),
            0,
            "the next flight lands after cancellation"
        );
    } else {
        assert_eq!(
            nested.get(),
            Some(newer),
            "the newer read committed its mapping"
        );
        assert_rect_close(
            outer,
            newer,
            "an outer mapping cannot replace a newer reentrant sample",
        );
        assert_rect_close(
            flight.shuttle_rect(),
            newer,
            "the next read retains the accepted geometry",
        );
        harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(1.0));
        harness.tick();
        assert_eq!(controller.flights().len(), 0, "the recovered flight lands");
    }
}

fn reentrant_geometry_factory() {
    exercise_hero_geometry_authority("factory", false);
}
fn reentrant_geometry_transform() {
    exercise_hero_geometry_authority("transform", false);
}
fn reentrant_geometry_retirement() {
    exercise_hero_geometry_authority("drop", false);
}
fn cancelled_geometry_factory() {
    exercise_hero_geometry_authority("factory", true);
}
fn cancelled_geometry_transform() {
    exercise_hero_geometry_authority("transform", true);
}
fn cancelled_geometry_retirement() {
    exercise_hero_geometry_authority("drop", true);
}

pub(crate) fn hero_geometry_reentry_preserves_the_newer_authority() {
    crate::common::cases::run_cases(
        "hero geometry authority",
        &[
            ("reentrant_factory", reentrant_geometry_factory),
            ("reentrant_transform", reentrant_geometry_transform),
            ("reentrant_retirement", reentrant_geometry_retirement),
            ("cancelled_factory", cancelled_geometry_factory),
            ("cancelled_transform", cancelled_geometry_transform),
            ("cancelled_retirement", cancelled_geometry_retirement),
        ],
    );
}

#[derive(Clone, Debug)]
struct OverrideHeroCurve {
    armed: Arc<std::sync::atomic::AtomicBool>,
    sample: f64,
}

impl flui_animation::Curve for OverrideHeroCurve {
    fn transform(&self, t: f64) -> f64 {
        if self.armed.load(std::sync::atomic::Ordering::SeqCst) {
            self.sample
        } else {
            t
        }
    }
}

#[derive(Clone)]
struct DisappearingHero {
    visible: Rc<Cell<bool>>,
    rebuild: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
    curve: OverrideHeroCurve,
}

impl View for DisappearingHero {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for DisappearingHero {
    type State = DisappearingHeroState;
    fn create_state(&self) -> Self::State {
        DisappearingHeroState {
            rebuild: Rc::clone(&self.rebuild),
        }
    }
}

struct DisappearingHeroState {
    rebuild: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
}
impl ViewState<DisappearingHero> for DisappearingHeroState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &DisappearingHero, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.visible.get() {
            Center::new()
                .child(
                    Hero::new(ValueKey::new("shared"), SizedBox::new(60.0, 45.0))
                        .curve(view.curve.clone()),
                )
                .boxed()
        } else {
            SizedBox::new(60.0, 45.0).boxed()
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum MissingDestinationPath {
    Push,
    Pop,
    PushToPop,
    PopToPush,
}

fn missing_destination_fade_case(
    sample: Option<f64>,
    path: MissingDestinationPath,
    redirect: bool,
) {
    let navigator = seeded_navigator();
    let controller = install(&navigator);
    let mut harness = mount_navigator(&navigator);
    let visible = Rc::new(Cell::new(true));
    let rebuild = Rc::new(RefCell::new(None));
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let page = DisappearingHero {
        visible: Rc::clone(&visible),
        rebuild: Rc::clone(&rebuild),
        curve: OverrideHeroCurve {
            armed: Arc::clone(&armed),
            sample: sample.unwrap_or(0.0),
        },
    };
    let curve = page.curve.clone();
    let disappearing =
        PageRoute::<i32>::new(move |_, _, _| page.clone().boxed()).transition_duration(TRANSITION);
    let (transition, pop) = match path {
        MissingDestinationPath::Push => (
            fly(
                &navigator,
                &mut harness,
                hero_page("shared", 30.0, 20.0),
                disappearing,
            ),
            false,
        ),
        MissingDestinationPath::Pop | MissingDestinationPath::PushToPop => {
            let transition = fly(
                &navigator,
                &mut harness,
                disappearing,
                hero_page_with("shared", 30.0, 20.0, move |hero| hero.curve(curve.clone())),
            );
            let before = controller
                .flights()
                .get(&tag("shared"))
                .expect("push flight")
                .entry_id();
            let t = if matches!(path, MissingDestinationPath::Pop) {
                1.0
            } else {
                0.7
            };
            harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(t));
            harness.tick();
            assert!(harness.enter_owner_scope(|| navigator.pop()));
            harness.tick();
            if matches!(path, MissingDestinationPath::PushToPop) {
                assert_eq!(
                    controller
                        .flights()
                        .get(&tag("shared"))
                        .expect("diverted")
                        .entry_id(),
                    before,
                    "push to pop reuses its shuttle"
                );
            }
            (transition, true)
        }
        MissingDestinationPath::PopToPush => {
            let previous = fly(
                &navigator,
                &mut harness,
                hero_page("shared", 30.0, 20.0),
                hero_page("shared", 40.0, 30.0),
            );
            harness.enter_owner_scope(|| previous.controller().expect("installed").set_value(0.7));
            harness.tick();
            let entry = controller
                .flights()
                .get(&tag("shared"))
                .expect("push flight")
                .entry_id();
            assert!(harness.enter_owner_scope(|| navigator.pop()));
            harness.tick();
            harness.enter_owner_scope(|| previous.controller().expect("installed").set_value(0.4));
            harness.tick();
            let transition = disappearing.transition_handle();
            let _next = harness.enter_owner_scope(|| navigator.push(disappearing));
            harness.tick();
            assert_eq!(
                controller
                    .flights()
                    .get(&tag("shared"))
                    .expect("redirected")
                    .entry_id(),
                entry,
                "pop to push reuses its shuttle"
            );
            (transition, false)
        }
    };
    let flight = controller.flights().get(&tag("shared")).expect("airborne");
    harness.enter_owner_scope(|| transition.controller().expect("installed").set_value(0.5));
    harness.tick();
    assert_eq!(flight.opacity(), 1.0);
    let mounted_heroes = harness
        .elements_of_type(std::any::TypeId::of::<Hero>())
        .len();
    armed.store(sample.is_some(), std::sync::atomic::Ordering::SeqCst);
    visible.set(false);
    let handle = rebuild
        .borrow()
        .as_ref()
        .expect("mounted destination")
        .clone();
    handle.schedule(flui_view::RebuildReason::StateChange);
    harness.tick();
    assert_eq!(
        harness
            .elements_of_type(std::any::TypeId::of::<Hero>())
            .len(),
        mounted_heroes - 1,
        "{path:?}: the destination Hero actually unmounted"
    );
    harness.enter_owner_scope(|| {
        transition
            .controller()
            .expect("installed")
            .set_value(if pop { 0.4 } else { 0.6 });
    });
    harness.tick();
    assert!(
        flight.opacity().is_finite() && (0.0..=1.0).contains(&flight.opacity()),
        "fade publishes valid opacity"
    );
    let opacity = flight.opacity();
    assert!(
        opacity > 0.0,
        "spatial overshoot does not instantly hide the shuttle"
    );
    assert_eq!(
        controller.flights().len(),
        1,
        "a missing destination still fades in its existing flight"
    );
    armed.store(false, std::sync::atomic::Ordering::SeqCst);
    harness.enter_owner_scope(|| {
        transition
            .controller()
            .expect("installed")
            .set_value(if pop { 0.25 } else { 0.75 });
    });
    harness.tick();
    assert!(
        flight.opacity() > 0.0 && flight.opacity() < opacity,
        "fade advances with route time after the spatial curve recovers"
    );
    if redirect {
        visible.set(true);
        handle.schedule(flui_view::RebuildReason::StateChange);
        harness.tick();
        let next = hero_page("shared", 90.0, 70.0);
        let next_transition = next.transition_handle();
        let entry = flight.entry_id();
        let _next = harness.enter_owner_scope(|| navigator.push(next));
        harness.tick();
        let redirected = controller
            .flights()
            .get(&tag("shared"))
            .expect("redirected");
        assert_eq!(redirected.entry_id(), entry, "the fading shuttle is reused");
        harness.enter_owner_scope(|| {
            next_transition
                .controller()
                .expect("installed")
                .set_value(0.5);
        });
        harness.tick();
        assert_eq!(
            redirected.opacity(),
            1.0,
            "a visible new destination restores opacity"
        );
        harness.enter_owner_scope(|| {
            next_transition
                .controller()
                .expect("installed")
                .set_value(1.0);
        });
        harness.tick();
        assert_eq!(controller.flights().len(), 0, "the redirected flight lands");
        return;
    }
    harness.enter_owner_scope(|| {
        transition
            .controller()
            .expect("installed")
            .set_value(if pop { 0.0 } else { 1.0 });
    });
    harness.tick();
    assert_eq!(
        controller.flights().len(),
        0,
        "the missing-destination flight still lands"
    );
    assert_eq!(
        flight.opacity(),
        0.0,
        "retained readers observe the completed fade"
    );
}

fn missing_destination_fade_paths(sample: Option<f64>) {
    for path in [
        MissingDestinationPath::Push,
        MissingDestinationPath::Pop,
        MissingDestinationPath::PushToPop,
        MissingDestinationPath::PopToPush,
    ] {
        missing_destination_fade_case(sample, path, false);
    }
}

fn missing_destination_linear() {
    missing_destination_fade_paths(None);
}
fn missing_destination_overshoot() {
    missing_destination_fade_paths(Some(1.25));
}
fn missing_destination_undershoot() {
    missing_destination_fade_paths(Some(-0.25));
}
fn missing_destination_nan() {
    missing_destination_fade_paths(Some(f64::NAN));
}
fn missing_destination_infinity() {
    missing_destination_fade_paths(Some(f64::INFINITY));
}
fn missing_destination_negative_infinity() {
    missing_destination_fade_paths(Some(f64::NEG_INFINITY));
}

fn missing_destination_redirects_to_a_visible_hero() {
    missing_destination_fade_case(None, MissingDestinationPath::Push, true);
}

pub(crate) fn disappearing_hero_destination_keeps_a_finite_fade() {
    crate::common::cases::run_cases(
        "missing destination fade",
        &[
            ("linear", missing_destination_linear),
            (
                "visible_redirection",
                missing_destination_redirects_to_a_visible_hero,
            ),
            ("overshoot", missing_destination_overshoot),
            ("undershoot", missing_destination_undershoot),
            ("nan", missing_destination_nan),
            ("infinity", missing_destination_infinity),
            ("negative_infinity", missing_destination_negative_infinity),
        ],
    );
}
