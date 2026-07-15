//! Public-API tests for `Hero`.
//!
//! Driven through the real `flui_widgets::prelude` surface, a real `Vsync`, and a
//! `HeadlessBinding` frame — the production path. If `Hero` or `HeroController` were
//! not exported, this file would not compile.
//!
//! A flight is observed the only way public API allows: by scanning the render tree
//! (`LaidOut::pipeline_owner`) for the shuttle's `RenderIgnorePointer`
//! (`heroes.dart:594`), across the whole transition rather than at one fragile frame.
//! `max == 1` means a single shuttle flew and never stacked; `end == 0` means it
//! landed. Entry-count and internal-state assertions stay crate-internal
//! (`navigator::hero_flight_tests`).
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/heroes_test.dart` — `'Heroes animate'`,
//! `'Stateful hero child state survives flight'` (`:1674`), `'Destination hero
//! disappears mid-flight'` (`:1233`), `'Hero push transition interrupted by a pop'`
//! (`:1063`), `'One route, two heroes, same tag, throws'` (`:1004` — FLUI logs).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::{LaidOut, lay_out_animated, tight};
use flui_animation::{Animatable, Vsync};
use flui_geometry::Rect;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_widgets::prelude::*;
use flui_widgets::{FlightDirection, HeroController, HeroControllerScope, PopupRoute, VsyncScope};
use parking_lot::{Mutex, RwLock};

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
fn shuttles(owner: &Arc<RwLock<PipelineOwner>>) -> usize {
    render_count(owner, "RenderIgnorePointer")
}

fn render_count(owner: &Arc<RwLock<PipelineOwner>>, suffix: &str) -> usize {
    owner
        .read()
        .render_tree()
        .iter()
        .filter(|(_, node)| node.debug_name().ends_with(suffix))
        .count()
}

/// Pump `frames` and report `(max shuttles seen at any frame, shuttles at the end)`.
///
/// The maximum is counted *including* the state on entry, so a divert pushed just
/// before the call is observed. Deterministic: `pump_for` advances a virtual clock, so
/// the animation timeline is fixed run to run.
fn run(laid: &mut LaidOut, owner: &Arc<RwLock<PipelineOwner>>, frames: usize) -> (usize, usize) {
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
#[test]
fn a_hero_push_flight_runs_and_settles() {
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

/// A pop flight runs the same way, in reverse, and settles clean.
///
/// Red-check: in `HeroController::did_change_top`, pass `Some(top)` as both from and to
/// — a pop then has `from == to`, no flight starts, and `max == 0`.
#[test]
fn a_hero_pop_flight_runs_and_settles() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    assert_eq!(
        run(&mut laid, &owner, SETTLE).1,
        0,
        "the push flight settled first"
    );

    assert!(laid.enter_owner_scope(|| navigator.pop()));
    let (max, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(max, 1, "the pop flew its own shuttle");
    assert_eq!(end, 0, "and it landed");
    assert_eq!(navigator.route_ids().len(), 1, "back to the base route");
}

/// **A stateful hero child survives the default placeholder** (`heroes_test.dart:1674`).
/// The source hero's child is frozen offstage during the flight, not rebuilt — FLUI's
/// fixed chain preserves its element with no `GlobalKey`.
///
/// Red-check: revert `HeroState::build` to the toggling shape — the source child is
/// rebuilt when the flight starts and `create_state` runs twice.
#[test]
fn a_stateful_hero_child_survives_the_default_placeholder() {
    #[derive(Clone)]
    struct Counter(Arc<AtomicUsize>);
    impl View for Counter {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl StatefulView for Counter {
        type State = CounterState;
        fn create_state(&self) -> Self::State {
            self.0.fetch_add(1, Ordering::SeqCst);
            CounterState
        }
    }
    struct CounterState;
    impl ViewState<Counter> for CounterState {
        fn build(&self, _v: &Counter, _c: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(30.0, 20.0)
        }
    }

    let creations = Arc::new(AtomicUsize::new(0));
    let creations_for_page = Arc::clone(&creations);
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(Hero::new(
                ValueKey::new("shared"),
                Counter(Arc::clone(&creations_for_page)),
            ))
            .into_view()
            .boxed()
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);
    assert_eq!(creations.load(Ordering::SeqCst), 1, "built once");

    // Push a matching hero page: the seeded page's hero becomes the flight's *source*
    // and its child is frozen offstage while the shuttle carries a fresh copy.
    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    run(&mut laid, &owner, SETTLE);

    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "the source child's state survived the flight — no rebuild, no GlobalKey"
    );
}

/// **The destination disappears mid-flight** (`heroes_test.dart:1233`): the shuttle
/// fades and the flight completes without panicking or leaking its entry.
///
/// Red-check: delete `entry.remove()` from `HeroFlight::finish` — the shuttle's
/// `RenderIgnorePointer` outlives the settled animation and `end != 0`.
#[test]
fn a_destination_lost_mid_flight_does_not_panic_or_leak() {
    #[derive(Clone)]
    struct Gate {
        present: Arc<AtomicBool>,
        rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
    }
    impl View for Gate {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl StatefulView for Gate {
        type State = GateState;
        fn create_state(&self) -> Self::State {
            GateState {
                present: Arc::clone(&self.present),
                rebuild: Arc::clone(&self.rebuild),
            }
        }
    }
    struct GateState {
        present: Arc<AtomicBool>,
        rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
    }
    impl ViewState<Gate> for GateState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            *self.rebuild.lock() = Some(ctx.rebuild_handle());
        }
        fn build(&self, _v: &Gate, _c: &dyn BuildContext) -> impl IntoView {
            if self.present.load(Ordering::SeqCst) {
                Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                    .into_view()
                    .boxed()
            } else {
                SizedBox::new(30.0, 20.0).into_view().boxed()
            }
        }
    }

    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let gate = Gate {
        present: Arc::new(AtomicBool::new(true)),
        rebuild: Arc::new(Mutex::new(None)),
    };
    let gate_for_page = gate.clone();
    let _push = laid.enter_owner_scope(|| {
        navigator.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                Center::new()
                    .child(gate_for_page.clone())
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });
    // Let the flight get airborne, then lose the destination hero.
    let (airborne, _) = run(&mut laid, &owner, 3);
    assert_eq!(airborne, 1, "a shuttle took off");
    gate.present.store(false, Ordering::SeqCst);
    if let Some(rebuild) = gate.rebuild.lock().as_ref() {
        rebuild.schedule();
    }

    // Fly on to completion; the flight fades and lands without panicking or leaking.
    let (_max, end) = run(&mut laid, &owner, SETTLE);
    assert_eq!(end, 0, "the faded flight still removed its entry");
    assert_eq!(navigator.route_ids().len(), 2);
}

/// A same-tag push while a flight is airborne **diverts** it — one shuttle at a time,
/// never two. (Entry preservation is pinned crate-internally in
/// `hero_flight_tests::a_same_tag_divert_keeps_one_active_flight_and_one_overlay_entry`;
/// the public signal is the shuttle count staying at one throughout.)
///
/// Red-check: in `FlightManager::start`, end-and-restart instead of diverting — a
/// second shuttle can coexist and `max` reaches 2.
#[test]
fn a_same_tag_divert_does_not_stack_shuttles() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _b = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max_push, _) = run(&mut laid, &owner, 3);
    assert_eq!(max_push, 1, "the first flight is airborne");

    // A third page, same tag, mid-flight, then run to completion.
    let _c = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max_divert, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(
        max_push.max(max_divert),
        1,
        "one shuttle throughout the divert — never two"
    );
    assert_eq!(end, 0, "and it lands");
    assert_eq!(navigator.route_ids().len(), 3);
}

/// No flight over a non-`PageRoute` (`heroes.dart:916-920`): a `PopupRoute` carrying a
/// same-tag hero raises no shuttle.
///
/// Red-check: drop the `is_page_route` guard from `HeroController::maybe_start` — a
/// shuttle then appears over the popup and `max == 1`.
#[test]
fn no_flight_over_a_popup_route() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _popup = laid.enter_owner_scope(|| {
        navigator.push(
            PopupRoute::<i32>::new(|_ctx, _p, _s| {
                Center::new()
                    .child(Hero::new(
                        ValueKey::new("shared"),
                        SizedBox::new(30.0, 20.0),
                    ))
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });
    let (max, _end) = run(&mut laid, &owner, SETTLE);
    assert_eq!(max, 0, "a popup is not a PageRoute: no hero flight ever");
}

// ============================================================================
// Advanced hooks — public surface, better-than-Flutter placeholder shape
// ============================================================================

#[derive(Clone)]
struct CountingRectTween {
    begin: Rect,
    end: Rect,
    transforms: Arc<AtomicUsize>,
}

impl Animatable<Rect> for CountingRectTween {
    fn transform(&self, t: f32) -> Rect {
        self.transforms.fetch_add(1, Ordering::SeqCst);
        flui_animation::RectTween::new(self.begin, self.end).transform(t)
    }
}

/// `Hero::create_rect_tween` is the path the flight actually samples, not just a
/// stored callback. The custom tween counts `transform` calls while the shuttle is
/// airborne.
///
/// Red-check: ignore `rect_factory` in `FlightInner::current_rect` — the shuttle still
/// flies, but `transforms == 0`.
#[test]
fn create_rect_tween_shapes_the_public_flight() {
    let transforms = Arc::new(AtomicUsize::new(0));
    let transforms_for_route = Arc::clone(&transforms);
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| {
        navigator.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                let transforms_for_tween = Arc::clone(&transforms_for_route);
                Center::new()
                    .child(
                        Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                            .create_rect_tween(move |begin, end| CountingRectTween {
                                begin,
                                end,
                                transforms: Arc::clone(&transforms_for_tween),
                            }),
                    )
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });

    let (max, _end) = run(&mut laid, &owner, 3);

    assert_eq!(max, 1, "the custom tween was attached to a real flight");
    assert!(
        transforms.load(Ordering::SeqCst) > 0,
        "the flight sampled the custom Rect tween"
    );
}

/// `Hero::flight_shuttle_builder` replaces the default destination-child shuttle.
/// The builder returns a `ColoredBox`, so the overlay contains a `RenderDecoratedBox`
/// while the flight is airborne.
///
/// Red-check: always use `to_hero.shuttle_child()` in `FlightManager::start` — the
/// builder call count stays zero and no decorated shuttle appears.
#[test]
fn flight_shuttle_builder_replaces_the_public_shuttle() {
    let builds = Arc::new(AtomicUsize::new(0));
    let builds_for_route = Arc::clone(&builds);
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| {
        navigator.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                let builds_for_builder = Arc::clone(&builds_for_route);
                Center::new()
                    .child(
                        Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                            .flight_shuttle_builder(move |_animation, direction, _from, _to| {
                                assert_eq!(direction, FlightDirection::Push);
                                builds_for_builder.fetch_add(1, Ordering::SeqCst);
                                ColoredBox::new(Color::RED)
                            }),
                    )
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });

    let (max, _end) = run(&mut laid, &owner, 3);

    assert_eq!(max, 1, "the flight is airborne");
    assert!(
        builds.load(Ordering::SeqCst) > 0,
        "the custom shuttle builder ran"
    );
    assert!(
        render_count(&owner, "RenderDecoratedBox") > 0,
        "the custom ColoredBox shuttle reached the overlay"
    );
}

/// FLUI's `Hero::placeholder` deliberately does **not** expose the child. The real
/// child stays offstage in a stable slot and the custom visual is a sibling, so the
/// child's state survives both becoming the push source and the pop destination.
///
/// Red-check: make the placeholder branch return only `build_placeholder(size)` while
/// in flight — `create_state` runs again when the child is reinserted.
#[test]
fn custom_placeholder_preserves_hero_child_state_through_push_and_pop() {
    #[derive(Clone)]
    struct Counter {
        creations: Arc<AtomicUsize>,
        live: Arc<AtomicUsize>,
    }
    impl View for Counter {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl StatefulView for Counter {
        type State = CounterState;
        fn create_state(&self) -> Self::State {
            self.creations.fetch_add(1, Ordering::SeqCst);
            self.live.fetch_add(1, Ordering::SeqCst);
            CounterState {
                live: Arc::clone(&self.live),
            }
        }
    }
    struct CounterState {
        live: Arc<AtomicUsize>,
    }
    impl Drop for CounterState {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl ViewState<Counter> for CounterState {
        fn build(&self, _v: &Counter, _c: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(30.0, 20.0)
        }
    }

    fn hold_space(size: Size) -> impl IntoView {
        SizedBox::new(size.width.0, size.height.0).child(ColoredBox::new(Color::GREEN))
    }

    let creations = Arc::new(AtomicUsize::new(0));
    let live = Arc::new(AtomicUsize::new(0));
    let creations_for_page = Arc::clone(&creations);
    let live_for_page = Arc::clone(&live);
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(
                Hero::new(
                    ValueKey::new("shared"),
                    Counter {
                        creations: Arc::clone(&creations_for_page),
                        live: Arc::clone(&live_for_page),
                    },
                )
                .placeholder(hold_space),
            )
            .into_view()
            .boxed()
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);
    assert_eq!(creations.load(Ordering::SeqCst), 1, "initial child");
    assert_eq!(live.load(Ordering::SeqCst), 1, "one route child is mounted");

    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    assert_eq!(run(&mut laid, &owner, SETTLE).1, 0, "push landed");
    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "the custom placeholder preserved the push source"
    );
    assert_eq!(
        live.load(Ordering::SeqCst),
        1,
        "the route child stayed mounted"
    );

    assert!(laid.enter_owner_scope(|| navigator.pop()));
    assert_eq!(run(&mut laid, &owner, SETTLE).1, 0, "pop landed");
    assert_eq!(
        creations.load(Ordering::SeqCst),
        2,
        "the returning destination also becomes the fresh shuttle; preserving the \
         route child means there is no third state"
    );
    assert_eq!(
        live.load(Ordering::SeqCst),
        1,
        "the temporary shuttle child was dropped and the original route child remains"
    );
}

/// A hero page whose shuttle builder records the flight `direction` it is handed and
/// draws a `ColoredBox` so the flight is observable. Both routes carry it, so the pop —
/// where the seeded hero is the destination — resolves its builder too.
fn direction_recording_page(seen: Arc<Mutex<Vec<FlightDirection>>>) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _p, _s| {
        let seen = Arc::clone(&seen);
        Center::new()
            .child(
                Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                    .flight_shuttle_builder(move |_animation, direction, _from, _to| {
                        seen.lock().push(direction);
                        ColoredBox::new(Color::RED)
                    }),
            )
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// Both a push and a pop hand the shuttle builder the matching [`FlightDirection`]
/// (`heroes.dart:466-480`). A push runs 0→1 (`Push`); a pop runs the source in reverse
/// (`Pop`).
///
/// Red-check: swap the arms of `FlightDirection::classify` — the push flight reports
/// `Pop` and the first assertion fails.
#[test]
fn push_and_pop_hand_the_matching_direction_to_the_shuttle_builder() {
    let seen = Arc::new(Mutex::new(Vec::<FlightDirection>::new()));
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(direction_recording_page(Arc::clone(&seen)));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push =
        laid.enter_owner_scope(|| navigator.push(direction_recording_page(Arc::clone(&seen))));
    assert_eq!(run(&mut laid, &owner, SETTLE).1, 0, "the push settled");
    assert_eq!(
        seen.lock().as_slice(),
        [FlightDirection::Push],
        "the push flight is handed Push"
    );

    assert!(laid.enter_owner_scope(|| navigator.pop()));
    assert_eq!(run(&mut laid, &owner, SETTLE).1, 0, "the pop settled");
    assert_eq!(
        seen.lock().last(),
        Some(&FlightDirection::Pop),
        "the pop flight is handed Pop"
    );
}

/// A same-tag transition that interrupts an airborne flight **diverts** it, and the
/// divert rebuilds the shuttle through the hook — not the default child
/// (`heroes.dart:793`, `:573`).
///
/// Red-check: in `HeroFlight::divert`'s same-direction branch, replace the
/// `inflate_shuttle(...)` call with `new_to.shuttle_child()` — the builder is not
/// re-invoked on the divert and the count does not grow.
#[test]
fn a_divert_rebuilds_the_shuttle_through_the_hook() {
    let builds = Arc::new(AtomicUsize::new(0));
    let make_page = {
        let builds = Arc::clone(&builds);
        move || {
            let builds = Arc::clone(&builds);
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                let builds = Arc::clone(&builds);
                Center::new()
                    .child(
                        Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                            .flight_shuttle_builder(move |_animation, _direction, _from, _to| {
                                builds.fetch_add(1, Ordering::SeqCst);
                                ColoredBox::new(Color::RED)
                            }),
                    )
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION)
        }
    };

    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(make_page());
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _b = laid.enter_owner_scope(|| navigator.push(make_page()));
    assert_eq!(
        run(&mut laid, &owner, 3).0,
        1,
        "the first flight is airborne"
    );
    let after_first = builds.load(Ordering::SeqCst);
    assert!(
        after_first >= 1,
        "the first shuttle was built through the hook"
    );

    // A third same-tag page mid-flight diverts the airborne flight.
    let _c = laid.enter_owner_scope(|| navigator.push(make_page()));
    run(&mut laid, &owner, SETTLE);
    assert!(
        builds.load(Ordering::SeqCst) > after_first,
        "the divert rebuilt the shuttle through the hook, not the default child"
    );
}

/// The custom placeholder is actually shown in the hero's vacated place during flight —
/// not merely constructed — and is cleared once the hero is home. The placeholder is on
/// the **destination** (pushed) hero, which clears its placeholder on a completed push
/// (`to_hero.end_flight(status.is_dismissed())` = clear; `heroes.dart:615`). A source
/// hero would instead *keep* its placeholder while covered (`:614`), so this uses the
/// destination to observe the placeholder appearing and then disappearing. The seeded
/// page is a plain hero with the default (non-decorated) shuttle, so the only decorated
/// box in the tree is the placeholder.
///
/// Red-check: in `HeroState::build`'s placeholder branch, gate the `layers.push(...)` on
/// `placeholder.is_none()` (show it only when *not* in flight) — nothing decorated
/// appears mid-flight and the airborne assertion fails.
#[test]
fn a_custom_placeholder_is_shown_during_the_flight() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);
    assert_eq!(
        render_count(&owner, "RenderDecoratedBox"),
        0,
        "no placeholder is shown before the flight"
    );

    let _push = laid.enter_owner_scope(|| {
        navigator.push(
            PageRoute::<i32>::new(move |_ctx, _p, _s| {
                Center::new()
                    .child(
                        Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0)).placeholder(
                            |size| {
                                SizedBox::new(size.width.0, size.height.0)
                                    .child(ColoredBox::new(Color::GREEN))
                            },
                        ),
                    )
                    .into_view()
                    .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(shuttles(&owner) >= 1, "a flight is airborne");
    assert!(
        render_count(&owner, "RenderDecoratedBox") >= 1,
        "the custom placeholder occupies the destination hero's place during flight"
    );

    assert_eq!(run(&mut laid, &owner, SETTLE).1, 0, "the flight landed");
    assert_eq!(
        render_count(&owner, "RenderDecoratedBox"),
        0,
        "the placeholder is gone once the destination hero is home"
    );
}

// ============================================================================
// Automatic attach, scope.none, nested isolation, manual path
// ============================================================================

/// `HeroControllerScope::none` blocks the auto-default: no controller under it, so no
/// flights — Flutter's `HeroControllerScope.none` (`navigator.dart:861`).
///
/// Red-check: treat `Some(None)` like `None` in `NavigatorState::init_state` (fall
/// through to the auto-default) — a shuttle then appears and `max == 1`.
#[test]
fn a_hero_controller_scope_none_disables_flights() {
    let vsync = Vsync::new();
    let navigator = seeded();
    // `.none` wraps the `VsyncScope`+`Navigator`, so it is the Navigator's ancestor and
    // resolves in its `init_state`.
    let root = HeroControllerScope::none(app(&vsync, &navigator));
    let mut laid = lay_out_animated(root, tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max, _end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(max, 0, "HeroControllerScope::none disables hero flights");
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "the push still happens — just no flight"
    );
}

/// A **custom** controller supplied through `HeroControllerScope::new` flies heroes.
/// This is the manual path in its blessed, ambient form.
///
/// Red-check: drop the `Some(Some(controller)) => …` arm from
/// `NavigatorState::init_state` — the scope's controller is never attached and no
/// shuttle appears.
#[test]
fn a_controller_from_a_scope_flies_heroes() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let root = HeroControllerScope::new(HeroController::new(), app(&vsync, &navigator));
    let mut laid = lay_out_animated(root, tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| navigator.push(hero_page()));
    let (max, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(max, 1, "the scope's controller flies exactly one shuttle");
    assert_eq!(end, 0, "and it lands");
}

/// A **nested** navigator does not inherit the outer navigator's controller.
/// `Navigator::build` wraps its overlay in `HeroControllerScope::none`, so a navigator
/// mounted inside an outer route resolves `Some(None)` — no controller, no
/// auto-default — and a push **on the inner navigator itself** flies nothing
/// (matching Flutter's `:5955`). This is about the *auto-default controller*, not
/// cross-navigator visibility: see
/// `a_hero_flies_from_a_nested_navigators_current_route_to_the_outer_navigator` for a
/// flight the OUTER navigator's own controller drives through the nested route.
///
/// Red-check: drop the `HeroControllerScope::none(...)` wrap from `Navigator::build` —
/// the inner navigator then auto-defaults and its heroes fly, so `max == 1`.
#[test]
fn a_nested_navigator_does_not_fly_heroes_by_default() {
    let vsync = Vsync::new();
    let inner = NavigatorHandle::new();
    inner.seed_initial(hero_page());
    let inner_for_page = inner.clone();

    let outer = NavigatorHandle::new();
    outer.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Navigator::new(inner_for_page.clone()).into_view().boxed()
    }));

    let mut laid = lay_out_animated(app(&vsync, &outer), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);

    // Push a matching hero page onto the INNER navigator.
    let _inner_push = inner.push(hero_page());
    let (max, _end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(
        max, 0,
        "the nested navigator is isolated — its heroes do not fly on the outer controller"
    );
    assert_eq!(inner.route_ids().len(), 2, "though the inner push happened");
}

/// **Cross-navigator flight.** A hero sitting in a nested `Navigator`'s current
/// route flies to a route pushed on the OUTER navigator, driven by the outer's own
/// controller — Flutter's nested-`Navigator` branch of `Hero._allHeroesFor`
/// (`heroes.dart:317-333`): the walk from `fromRoute.subtreeContext` does not stop at
/// the nested `Navigator`, and invites a hero found there because its own route
/// (`ModalRoute.of(hero)`) `isCurrent` and `is PageRoute`.
///
/// Red-check (this is the new behavior): before this change, `MeasurementPass`
/// matched only `ModalHandle::heroes()` — the FROM route's own registry, which never
/// contains a hero registered on a *different* (nested) route. `max` was `0`.
#[test]
fn a_hero_flies_from_a_nested_navigators_current_route_to_the_outer_navigator() {
    let vsync = Vsync::new();
    let inner = NavigatorHandle::new();
    inner.seed_initial(hero_page());
    let inner_for_page = inner.clone();

    let outer = NavigatorHandle::new();
    outer.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Navigator::new(inner_for_page.clone()).into_view().boxed()
    }));

    let mut laid = lay_out_animated(app(&vsync, &outer), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);

    // Push a matching hero page onto the OUTER navigator — the outer's own default
    // controller drives this flight.
    let _outer_push = laid.enter_owner_scope(|| outer.push(hero_page()));
    let (max, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(
        max, 1,
        "the hero flies from the nested navigator's current route to the outer push"
    );
    assert_eq!(end, 0, "and it lands");
}

/// **Isolation counter-case, faithful to the oracle.** A hero whose own route is
/// covered inside its nested `Navigator` (no longer `isCurrent` there) does not fly,
/// even though it is still mounted offstage and reachable in the subtree —
/// `Hero._allHeroesFor`'s guard is `heroRoute.isCurrent && heroRoute is PageRoute`
/// (`heroes.dart:330-333`), not whether the nested navigator has its own
/// `HeroController`: that scope is never consulted by the walk at all.
///
/// Red-check: drop the `is_page_route`/`current()` guard from the
/// `NestedHeroSource` closure (resolve unconditionally to the inner navigator's
/// registry) — the covered hero would then match and `max` would read `1`.
#[test]
fn a_covered_nested_route_does_not_contribute_its_heroes_to_an_outer_flight() {
    let vsync = Vsync::new();
    let inner = NavigatorHandle::new();
    inner.seed_initial(hero_page());
    // Cover the hero-bearing route inside the INNER navigator: it is still mounted
    // (`maintain_state` defaults to `true`) but is no longer `isCurrent` there.
    inner.push(
        PageRoute::<i32>::new(|_ctx, _p, _s| Center::new().into_view().boxed())
            .transition_duration(TRANSITION),
    );
    let inner_for_page = inner.clone();

    let outer = NavigatorHandle::new();
    outer.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Navigator::new(inner_for_page.clone()).into_view().boxed()
    }));

    let mut laid = lay_out_animated(app(&vsync, &outer), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();
    laid.pump_for(FRAME);

    let _outer_push = laid.enter_owner_scope(|| outer.push(hero_page()));
    let (max, _end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(
        max, 0,
        "a hero covered inside its own nested navigator does not join an outer flight"
    );
}

/// **`HeroMode` grounds a subtree, through the public API** (`heroes.dart:1124-1152`).
/// A destination hero under `HeroMode::new(…).enabled(false)` never raises a shuttle;
/// the transition still runs and settles.
///
/// Red-check: drop the `hero_mode_enabled` filter from
/// `MeasurementPass::collect_manifests` — a shuttle flies and `max` reads 1.
#[test]
fn a_hero_under_a_disabled_hero_mode_does_not_fly_publicly() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = laid.enter_owner_scope(|| {
        navigator.push(
            PageRoute::<i32>::new(|_ctx, _p, _s| {
                HeroMode::new(Center::new().child(Hero::new(
                    ValueKey::new("shared"),
                    SizedBox::new(30.0, 20.0),
                )))
                .enabled(false)
                .into_view()
                .boxed()
            })
            .transition_duration(TRANSITION),
        )
    });

    let (max, end) = run(&mut laid, &owner, SETTLE);
    assert_eq!(max, 0, "a disabled HeroMode grounds the hero");
    assert_eq!(end, 0, "and the transition still settles cleanly");
}
