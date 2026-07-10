//! Public-API tests for `Hero` (ADR-0021 U6).
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
use flui_animation::Vsync;
use flui_rendering::pipeline::PipelineOwner;
use flui_widgets::prelude::*;
use flui_widgets::{HeroController, PopupRoute, VsyncScope};
use parking_lot::{Mutex, RwLock};

const TRANSITION: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);
/// Enough 16 ms frames to run a 100 ms transition to completion, twice over.
const SETTLE: usize = 16;

/// A `Navigator` whose route transitions tick against `vsync`, with a `HeroController`
/// attached — exactly what an app author writes. Flutter's `MaterialApp` installs one
/// automatically; FLUI has no `HeroControllerScope`, so it is attached by hand.
fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    navigator.add_observer(HeroController::new());
    VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone()))
}

/// How many shuttle `RenderIgnorePointer`s are currently airborne.
fn shuttles(owner: &Arc<RwLock<PipelineOwner>>) -> usize {
    owner
        .read()
        .render_tree()
        .iter()
        .filter(|(_, node)| node.debug_name().ends_with("RenderIgnorePointer"))
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
/// Red-check: attach no controller (drop `navigator.add_observer(...)` in `app`) — no
/// shuttle ever appears and `max == 0`.
#[test]
fn a_hero_push_flight_runs_and_settles() {
    let vsync = Vsync::new();
    let navigator = seeded();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);
    let owner = laid.pipeline_owner();

    let _push = navigator.push(hero_page());
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

    let _push = navigator.push(hero_page());
    assert_eq!(
        run(&mut laid, &owner, SETTLE).1,
        0,
        "the push flight settled first"
    );

    assert!(navigator.pop());
    let (max, end) = run(&mut laid, &owner, SETTLE);

    assert_eq!(max, 1, "the pop flew its own shuttle");
    assert_eq!(end, 0, "and it landed");
    assert_eq!(navigator.route_ids().len(), 1, "back to the base route");
}

/// **A stateful hero child survives the default placeholder** (`heroes_test.dart:1674`).
/// The source hero's child is frozen offstage during the flight, not rebuilt — FLUI's
/// fixed chain preserves its element with no `GlobalKey` (ADR-0021 §7k).
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
    let _push = navigator.push(hero_page());
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
    let _push = navigator.push(
        PageRoute::<i32>::new(move |_ctx, _p, _s| {
            Center::new()
                .child(gate_for_page.clone())
                .into_view()
                .boxed()
        })
        .transition_duration(TRANSITION),
    );
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

    let _b = navigator.push(hero_page());
    let (max_push, _) = run(&mut laid, &owner, 3);
    assert_eq!(max_push, 1, "the first flight is airborne");

    // A third page, same tag, mid-flight, then run to completion.
    let _c = navigator.push(hero_page());
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

    let _popup = navigator.push(
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
    );
    let (max, _end) = run(&mut laid, &owner, SETTLE);
    assert_eq!(max, 0, "a popup is not a PageRoute: no hero flight ever");
}
