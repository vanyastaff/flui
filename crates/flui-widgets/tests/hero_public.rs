//! Public-API tests for `Hero`.
//!
//! Driven through the real `flui_widgets::prelude` surface, a real `Vsync`, and a
//! `HeadlessBinding` frame — the production path. If `Hero` or `HeroController` were
//! not exported, this file would not compile.
//!
//! A flight is observed the only way public API allows: by scanning the render tree
//! (`LaidOut::pipeline_owner`) for the shuttle's `RenderIgnorePointer`
//! across the whole transition rather than at one fragile frame.
//! `max == 1` means a single shuttle flew and never stacked; `end == 0` means it
//! landed. Entry-count and internal-state assertions go through the temporary
//! test-access path instead (`hero_flight.rs`, ADR-0083 §4).
//!
//! # Scenarios
//!
//! Heroes animate; a stateful hero child's state survives the flight; a
//! destination hero disappearing mid-flight is handled; a push interrupted by
//! a pop is diverted; one route with two heroes of the same tag logs instead
//! of throwing.

use std::time::Duration;
use std::{cell::Cell, rc::Rc, sync::Arc};

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_rendering::pipeline::PipelineCell;
use flui_widgets::VsyncScope;
use flui_widgets::prelude::*;

const TRANSITION: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);
/// Enough 16 ms frames to run a 100 ms transition to completion, twice over.
const SETTLE: usize = 16;

/// A `Navigator` whose route transitions tick against `vsync` — and **nothing else**.
/// No `HeroControllerScope`, no manual `add_observer`: the Navigator creates its own
/// default `HeroController`, so heroes fly with zero boilerplate. This
/// is exactly what an app author writes.
fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone()))
}

/// How many shuttle `RenderIgnorePointer`s are currently airborne.
fn shuttles(owner: &PipelineCell) -> usize {
    render_count(owner, "RenderIgnorePointer")
}

fn render_count(owner: &PipelineCell, suffix: &str) -> usize {
    owner.with(|owner| {
        owner
            .render_tree()
            .iter()
            .filter(|(_, node)| node.debug_name().ends_with(suffix))
            .count()
    })
}

/// Pump `frames` and report `(max shuttles seen at any frame, shuttles at the end)`.
///
/// The maximum is counted *including* the state on entry, so a divert pushed just
/// before the call is observed. Deterministic: `pump_for` advances a virtual clock, so
/// the animation timeline is fixed run to run.
fn run(laid: &mut LaidOut, owner: &PipelineCell, frames: usize) -> (usize, usize) {
    let mut max = shuttles(owner);
    for _ in 0..frames {
        laid.pump_for(FRAME);
        max = max.max(shuttles(owner));
    }
    (max, shuttles(owner))
}

/// A `PageRoute` whose page centres one `Hero` tagged `"shared"`.
fn hero_page() -> PageRoute<i32> {
    PageRoute::<i32>::new(|_ctx, _p, _s| {
        Center::new()
            .child(Hero::new(
                ValueKey::new("shared"),
                SizedBox::new(30.0, 20.0),
            ))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

fn seeded() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(hero_page());
    navigator
}

/// **A push flight runs and settles, through the public API.** Pushing a second hero
/// page raises a shuttle in the overlay; running the transition to completion lands it.
///
/// This is the **automatic** path: no controller is attached by hand. A shuttle proves
/// the Navigator created its own default controller.
///
/// Red-check: delete the `None => { … observers.push(HeroController::new()) }` arm from
/// `NavigatorState::init_state` — no controller, no shuttle, `max == 0`.
pub(crate) fn a_hero_push_flight_runs_and_settles() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(max, 1, "exactly one shuttle flew");
    assert_eq!(end, 0, "and it landed — no shuttle remains");
    assert_eq!(navigator.route_ids().len(), 2, "the push completed");
}

pub(crate) fn replacing_the_hero_observer_inside_its_builder_cancels_the_flight() {
    for divert in [false, true] {
        let vsync = Vsync::new();
        let navigator = seeded();
        let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
        let owner = laid.pipeline_owner();
        if divert {
            let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
            laid.pump_for(FRAME);
            laid.pump_for(FRAME);
            assert_eq!(shuttles(&owner), 1, "the old flight is already airborne");
        }
        let replacement = HeroController::new();
        let replaced = Rc::new(Cell::new(false));
        let taps = Rc::new(Cell::new(0));
        let route = PageRoute::<i32>::new({
            let navigator = navigator.clone();
            let replacement = Arc::clone(&replacement);
            let replaced = Rc::clone(&replaced);
            let taps = Rc::clone(&taps);
            move |_ctx, _primary, _secondary| {
                let navigator = navigator.clone();
                let replacement = Arc::clone(&replacement);
                let replaced = Rc::clone(&replaced);
                let taps = Rc::clone(&taps);
                Center::new()
                    .child(
                        Hero::new(
                            ValueKey::new("shared"),
                            GestureDetector::new()
                                .behavior(flui_widgets::HitTestBehavior::Opaque)
                                .on_tap(move |_| taps.set(taps.get() + 1))
                                .child(SizedBox::new(60.0, 45.0)),
                        )
                        .flight_shuttle_builder(
                            move |_animation, _direction, _from, to| {
                                if !replaced.replace(true) {
                                    navigator.add_observer(replacement.clone());
                                }
                                to.clone()
                            },
                        ),
                    )
                    .into_view()
                    .boxed()
            }
        })
        .transition_duration(TRANSITION);

        let _push = laid.enter_owner_scope(|| navigator.push(route));
        laid.pump_for(FRAME);
        assert!(
            replaced.get(),
            "the real shuttle builder replaced its observer"
        );
        laid.pump_for(FRAME);
        assert_eq!(
            shuttles(&owner),
            0,
            "a detached observer cannot publish the flight its builder cancelled"
        );
        laid.dispatch_pointer_down(200.0, 200.0);
        laid.dispatch_pointer_up(200.0, 200.0);
        laid.pump_for(FRAME);
        assert_eq!(
            taps.get(),
            1,
            "cancellation restores the new destination child"
        );

        let _next = laid.enter_owner_scope(|| navigator.push(hero_page()));
        let (max, end) = run(&mut laid, &owner, SETTLE);
        assert_eq!(max, 1, "the replacement controller flies one fresh shuttle");
        assert_eq!(end, 0, "the replacement flight lands");
    }
}

pub(crate) fn replacing_the_hero_observer_cancels_queued_measurement() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    laid.enter_owner_scope(|| navigator.add_observer(HeroController::new()));
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        shuttles(&owner),
        0,
        "cancelled measurement cannot launch a flight"
    );
    let _next = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max, end) = run(&mut laid, &owner, SETTLE);
    assert_eq!(max, 1, "the new observer admits a fresh flight");
    assert_eq!(end, 0);
}

pub(crate) fn a_failed_hero_builder_restores_its_child_and_allows_a_fresh_flight() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for divert in [false, true] {
        let vsync = Vsync::new();
        let navigator = seeded();
        let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
        let owner = laid.pipeline_owner();
        if divert {
            let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
            laid.pump_for(FRAME);
            laid.pump_for(FRAME);
            assert_eq!(shuttles(&owner), 1);
        }
        let fail_once = Rc::new(Cell::new(true));
        let taps = Rc::new(Cell::new(0));
        let page = PageRoute::<i32>::new({
            let fail_once = Rc::clone(&fail_once);
            let taps = Rc::clone(&taps);
            move |_ctx, _primary, _secondary| {
                let taps = Rc::clone(&taps);
                let fail_once = Rc::clone(&fail_once);
                Center::new()
                    .child(
                        Hero::new(
                            ValueKey::new("shared"),
                            GestureDetector::new()
                                .behavior(flui_widgets::HitTestBehavior::Opaque)
                                .on_tap(move |_| taps.set(taps.get() + 1))
                                .child(SizedBox::new(60.0, 45.0)),
                        )
                        .flight_shuttle_builder(
                            move |_animation, _direction, _from, to| {
                                assert!(!fail_once.replace(false), "authored Hero builder failure");
                                to.clone()
                            },
                        ),
                    )
                    .into_view()
                    .boxed()
            }
        })
        .transition_duration(TRANSITION);
        let _push = laid.enter_owner_scope(|| navigator.push(page));
        let failure = catch_unwind(AssertUnwindSafe(|| laid.pump_for(FRAME)))
            .expect_err("the authored builder failure propagates");
        assert_eq!(
            failure.downcast_ref::<&str>().copied(),
            Some("authored Hero builder failure"),
        );
        laid.pump_for(FRAME);
        assert_eq!(shuttles(&owner), 0, "failed construction leaves no shuttle");
        laid.dispatch_pointer_down(200.0, 200.0);
        laid.dispatch_pointer_up(200.0, 200.0);
        laid.pump_for(FRAME);
        assert_eq!(
            taps.get(),
            1,
            "the destination child is restored for hit testing"
        );
        let _next = laid.enter_owner_scope(|| navigator.push(hero_page()));
        let (max, end) = run(&mut laid, &owner, SETTLE);
        assert_eq!(
            max, 1,
            "the same controller admits another flight after failure"
        );
        assert_eq!(end, 0);
    }
}

pub(crate) fn hero_builder_cancellation_and_failure_account_for_the_matched_tail() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for behavior in ["cancel", "one_failure", "competing"] {
        let replace = behavior == "cancel";
        let source = || {
            PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
                Column::new((
                    Hero::new(ValueKey::new("one"), SizedBox::new(30.0, 20.0)),
                    Hero::new(ValueKey::new("two"), SizedBox::new(30.0, 20.0)),
                ))
                .into_view()
                .boxed()
            })
            .transition_duration(TRANSITION)
        };
        let vsync = Vsync::new();
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(source());
        let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
        let owner = laid.pipeline_owner();
        let calls = Rc::new(Cell::new(0));
        let page = PageRoute::<i32>::new({
            let navigator = navigator.clone();
            let calls = Rc::clone(&calls);
            move |_ctx, _primary, _secondary| {
                let hero = |name: &'static str| {
                    let navigator = navigator.clone();
                    let calls = Rc::clone(&calls);
                    Hero::new(ValueKey::new(name), SizedBox::new(60.0, 45.0))
                        .flight_shuttle_builder(move |_animation, _direction, _from, to| {
                            let previous = calls.get();
                            calls.set(previous + 1);
                            if previous == 0 {
                                if replace {
                                    navigator.add_observer(HeroController::new());
                                } else {
                                    panic!("first matched Hero builder failure");
                                }
                            } else if previous == 1 && behavior == "competing" {
                                panic!("second matched Hero builder failure");
                            }
                            to.clone()
                        })
                };
                Column::new((hero("one"), hero("two"))).into_view().boxed()
            }
        })
        .transition_duration(TRANSITION);
        let _push = laid.enter_owner_scope(|| navigator.push(page));
        let result = catch_unwind(AssertUnwindSafe(|| laid.pump_for(FRAME)));
        if replace {
            result.expect("observer replacement is a healthy cancellation");
        } else {
            let failure = result.expect_err("the first builder failure propagates after the tail");
            assert_eq!(
                failure.downcast_ref::<&str>().copied(),
                Some("first matched Hero builder failure")
            );
        }
        laid.pump_for(FRAME);
        assert_eq!(
            calls.get(),
            if replace { 1 } else { 2 },
            "cancellation refuses the tail; failure delivers its healthy peer"
        );
        assert_eq!(
            shuttles(&owner),
            usize::from(behavior == "one_failure"),
            "only admitted healthy work flies"
        );
        run(&mut laid, &owner, SETTLE);
        let _next = laid.enter_owner_scope(|| navigator.push(source()));
        let (max, end) = run(&mut laid, &owner, SETTLE);
        assert_eq!(max, 2, "both tags remain usable after recovery");
        assert_eq!(end, 0);
    }
}

// ============================================================================
// Advanced hooks — public surface, placeholder shape
// ============================================================================

// ============================================================================
// Automatic attach, scope.none, nested isolation, manual path
// ============================================================================
