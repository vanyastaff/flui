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
//! landed. Entry-count and internal-state assertions go through the temporary
//! test-access path instead (`hero_flight.rs`, ADR-0083 §4).
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/heroes_test.dart` — `'Heroes animate'`,
//! `'Stateful hero child state survives flight'` (`:1674`), `'Destination hero
//! disappears mid-flight'` (`:1233`), `'Hero push transition interrupted by a pop'`
//! (`:1063`), `'One route, two heroes, same tag, throws'` (`:1004` — FLUI logs).

use std::time::Duration;

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

// ============================================================================
// Advanced hooks — public surface, better-than-Flutter placeholder shape
// ============================================================================

// ============================================================================
// Automatic attach, scope.none, nested isolation, manual path
// ============================================================================
