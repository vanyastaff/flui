//! `cupertino_page_route` end-to-end coverage — a real [`NavigatorHandle`]
//! mounted under a real [`Vsync`] clock, matching `flui-material`'s
//! `tests/show_dialog.rs` harness. Proves the slide transition's actual
//! geometry, the transition-only barrier dim, the 500ms duration, and the
//! default-on back-gesture detector — not just that the builder compiles.

mod common;

use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_animation::{Curve, Curves, Vsync};
use flui_cupertino::cupertino_page_route;
use flui_types::Color;
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, Navigator, NavigatorHandle, SimpleRoute, VsyncScope};

/// `CupertinoRouteTransitionMixin.kTransitionDuration` (`route.dart`, oracle
/// tag `3.44.0`).
const TRANSITION: Duration = Duration::from_millis(500);
/// The per-pump virtual-time step.
const FRAME: Duration = Duration::from_millis(50);
/// Enough pumps to carry `TRANSITION` past its end — matching
/// `flui-material/tests/show_dialog.rs`'s identical `+ 2` budget: one frame
/// because the first pump after a controller starts only anchors `t = 0`,
/// plus one more margin frame.
const PUMPS: usize = (TRANSITION.as_millis() / FRAME.as_millis()) as usize + 2;

const _: () = assert!(
    (PUMPS as u128) * FRAME.as_millis() > TRANSITION.as_millis(),
    "PUMPS * FRAME must carry the transition past its end"
);

fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone()))
}

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        ColoredBox::new(Color::rgb(0, 0, 0)).into_view().boxed()
    }));
    navigator
}

/// Parses `RenderFractionalTranslation`'s `"translation"` diagnostic
/// (`format!("({}, {})", dx, dy)`, `fractional_translation.rs`) back into
/// its two components.
fn parse_translation(property: &str) -> (f32, f32) {
    let trimmed = property.trim_matches(['(', ')']);
    let mut parts = trimmed.split(", ");
    let dx: f32 = parts
        .next()
        .expect("translation has a dx component")
        .parse()
        .expect("dx is a float");
    let dy: f32 = parts
        .next()
        .expect("translation has a dy component")
        .parse()
        .expect("dy is a float");
    (dx, dy)
}

/// `cupertino_page_transitions` mounts exactly two `RenderFractionalTranslation`
/// nodes for a route nothing else covers: the **primary** (this page's own
/// entrance, tweened `1.0 -> 0.0`) and the **secondary** (the parallax a
/// covering page would apply — pinned at `(0, 0)` here, since nothing covers
/// this route). The primary is whichever of the two reads the larger `|dx|`
/// at any given moment — true by construction, since the secondary never
/// moves in this scenario.
fn primary_slide_dx(laid: &common::LaidOut) -> f32 {
    let nodes = laid.find_all_by_render_type("RenderFractionalTranslation");
    assert_eq!(
        nodes.len(),
        2,
        "the primary and secondary SlideTransition each mount one FractionalTranslation"
    );
    nodes
        .into_iter()
        .map(|id| {
            parse_translation(
                &laid
                    .render_property(id, "translation")
                    .expect("FractionalTranslation always reports its translation"),
            )
            .0
        })
        .fold(0.0_f32, |largest, dx| {
            if dx.abs() > largest.abs() {
                dx
            } else {
                largest
            }
        })
}

/// The pushed page slides in from fully off the right edge of the viewport
/// to flush with it, over the oracle's 500ms `kTransitionDuration` — not a
/// jump cut, and not settled before the duration elapses. The midpoint value
/// is pinned against `Curves::FastEaseInToSlowEaseOut`'s own math (the exact
/// curve `route.dart`'s `_setupAnimation` applies to the primary position),
/// not a loose "still mid-flight" range a plain linear interpolation would
/// also satisfy.
///
/// Red-check: replace `cupertino_page_transitions` with the framework
/// default (jump-cut) `transitions` builder — the exact-midpoint assertion
/// below fails, since a jump cut is already at rest the instant it mounts.
/// Red-check 2: swap the primary curve for `Curves::Linear` in
/// `route.rs` — `midpoint_dx` reads `0.5` (linear at `t=0.5`), which is
/// `> 0.05` away from `FastEaseInToSlowEaseOut`'s real value asserted below,
/// so the tight-tolerance assertion fails.
#[test]
fn cupertino_page_route_slides_in_from_off_the_right_edge_over_500ms() {
    let vsync = Vsync::new();
    let navigator = seeded_navigator();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 800.0), vsync);

    let _result = navigator.push(cupertino_page_route::<(), _>(
        |_ctx, _primary, _secondary| ColoredBox::new(Color::rgb(10, 20, 30)).into_view().boxed(),
    ));
    laid.tick();

    let start_dx = primary_slide_dx(&laid);
    assert!(
        (start_dx - 1.0).abs() < 0.01,
        "the page must start fully off-screen to the right (dx == 1.0): {start_dx}"
    );

    // Halfway through the 500ms transition: still animating, not settled,
    // and at the exact value `FastEaseInToSlowEaseOut` (not linear) predicts.
    laid.pump_for(TRANSITION / 2);
    let midpoint_dx = primary_slide_dx(&laid);
    let curved_progress = Curves::FastEaseInToSlowEaseOut.transform(0.5);
    let expected_midpoint_dx = 1.0 - curved_progress;
    assert!(
        (midpoint_dx - expected_midpoint_dx).abs() < 0.01,
        "at raw progress 0.5, FastEaseInToSlowEaseOut.transform(0.5) = {curved_progress}, so \
         dx must read {expected_midpoint_dx} (1.0 - curved progress), not linear's 0.5: \
         got {midpoint_dx}"
    );
    assert!(
        (midpoint_dx - 0.5).abs() > 0.05,
        "sanity: the curved midpoint must be measurably different from a plain linear \
         interpolation's 0.5, or this assertion isn't actually distinguishing the two: \
         midpoint_dx={midpoint_dx}"
    );

    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    let settled_dx = primary_slide_dx(&laid);
    assert!(
        settled_dx.abs() < 0.01,
        "the page must settle flush with the viewport's left edge (dx == 0.0) \
         once the transition completes: {settled_dx}"
    );
}

/// The secondary (covered-page) slide drifts toward `dx = -1/3` once a
/// second route is pushed on top — `_kMiddleLeftTween` (`route.dart`, oracle
/// tag `3.44.0`) — exercised here for the first time: every other test in
/// this file pushes only one route, so its secondary animation stays pinned
/// at its `kAlwaysDismissedAnimation` rest value (`dx == 0`) the whole time.
///
/// Red-check: change `middle_left_tween()`'s end value in `route.rs` from
/// `-1.0 / 3.0` to `0.0` (a no-op parallax) — this test's assertion fails,
/// since no node then settles anywhere near `-1/3`.
#[test]
fn a_covered_pages_secondary_slide_drifts_toward_negative_one_third() {
    let vsync = Vsync::new();
    let navigator = seeded_navigator();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 800.0), vsync);

    // Push route A and let its own entrance fully settle.
    let _a = navigator.push(cupertino_page_route::<(), _>(
        |_ctx, _primary, _secondary| ColoredBox::new(Color::rgb(10, 20, 30)).into_view().boxed(),
    ));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    // Push route B on top of A, and let ITS entrance fully settle too — this
    // is what drives A's secondary animation via `did_change_next`.
    let _b = navigator.push(cupertino_page_route::<(), _>(
        |_ctx, _primary, _secondary| ColoredBox::new(Color::rgb(40, 50, 60)).into_view().boxed(),
    ));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    let nodes = laid.find_all_by_render_type("RenderFractionalTranslation");
    assert_eq!(
        nodes.len(),
        4,
        "two pushed cupertino_page_routes each mount a primary + secondary FractionalTranslation"
    );
    let dxs: Vec<f32> = nodes
        .iter()
        .map(|&id| {
            parse_translation(
                &laid
                    .render_property(id, "translation")
                    .expect("FractionalTranslation always reports its translation"),
            )
            .0
        })
        .collect();

    let closest_to_negative_third = dxs
        .iter()
        .copied()
        .min_by(|a, b| {
            (a - (-1.0_f32 / 3.0))
                .abs()
                .total_cmp(&(b - (-1.0_f32 / 3.0)).abs())
        })
        .expect("four FractionalTranslation nodes exist");
    assert!(
        (closest_to_negative_third - (-1.0 / 3.0)).abs() < 0.02,
        "route A's covered secondary must settle at dx ≈ -1/3 once route B fully covers it: \
         all dxs = {dxs:?}"
    );
}

/// `_kCupertinoPageTransitionBarrierColor` — `cupertino_page_route` sets a
/// barrier, where a plain `PageRoute` sets none by default.
///
/// Red-check: drop the `.barrier_color(barrier_color())` call from
/// `cupertino_page_route` — the `DecoratedBox` count stays equal before and
/// after the push, and this test's second assertion fails.
#[test]
fn cupertino_page_route_paints_a_transition_barrier_dim() {
    let vsync = Vsync::new();
    let navigator = seeded_navigator();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 800.0), vsync);

    let before = laid.find_all_by_render_type("RenderDecoratedBox").len();

    // The pushed page's own content is deliberately NOT `ColoredBox`-backed
    // (a bare `SizedBox` mounts no `DecoratedBox` of its own) — the only
    // `RenderDecoratedBox` this push can add is the barrier's.
    let _result = navigator.push(cupertino_page_route::<(), _>(
        |_ctx, _primary, _secondary| flui_widgets::SizedBox::new(10.0, 10.0).into_view().boxed(),
    ));
    laid.tick();

    let after = laid.find_all_by_render_type("RenderDecoratedBox").len();
    assert_eq!(
        after,
        before + 1,
        "cupertino_page_route's barrier_color must add exactly one DecoratedBox \
         (the barrier's own paint), with no ColoredBox in the pushed page's content: \
         before={before}, after={after}"
    );
}

/// `back_gesture(true)` is the default — `cupertino_page_route` mounts the
/// edge-swipe-back detector's `Listener` unconditionally, matching
/// `CupertinoRouteTransitionMixin`'s unconditional wiring under
/// `TargetPlatform.iOS`.
///
/// Red-check: build with `PageRoute::back_gesture(false)` instead — no
/// `RenderListener` is ever mounted, and this test's assertion fails.
#[test]
fn cupertino_page_route_mounts_the_back_gesture_detector_by_default() {
    let vsync = Vsync::new();
    let navigator = seeded_navigator();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 800.0), vsync);

    let _result = navigator.push(cupertino_page_route::<(), _>(
        |_ctx, _primary, _secondary| ColoredBox::new(Color::rgb(10, 20, 30)).into_view().boxed(),
    ));
    laid.tick();

    assert!(
        laid.find_by_render_type("RenderListener").is_some(),
        "back_gesture(true) must mount the edge-swipe-back Listener"
    );
}
