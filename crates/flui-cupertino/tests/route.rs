//! `cupertino_page_route` end-to-end coverage — a real [`NavigatorHandle`]
//! mounted under a real [`Vsync`] clock, matching `flui-material`'s
//! `tests/show_dialog.rs` harness. Proves the slide transition's actual
//! geometry, the transition-only barrier dim, the 500ms duration, and the
//! default-on back-gesture detector — not just that the builder compiles.

mod common;

use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_animation::Vsync;
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
/// jump cut, and not settled before the duration elapses.
///
/// Red-check: replace `cupertino_page_transitions` with the framework
/// default (jump-cut) `transitions` builder — the "still mid-flight at the
/// transition's midpoint" assertion below fails, since a jump cut is already
/// at rest the instant it mounts.
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

    // Halfway through the 500ms transition: still animating, not settled.
    laid.pump_for(TRANSITION / 2);
    let midpoint_dx = primary_slide_dx(&laid);
    assert!(
        (0.05..0.95).contains(&midpoint_dx),
        "the page must still be sliding at the transition's midpoint: {midpoint_dx}"
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
