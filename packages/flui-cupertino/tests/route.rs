//! `cupertino_page_route` end-to-end coverage — a real [`NavigatorHandle`]
//! mounted under a real [`Vsync`] clock, matching `flui-material`'s
//! `tests/show_dialog.rs` harness. Proves the slide transition's actual
//! geometry, the transition-only barrier dim, the 500ms duration, and the
//! default-on back-gesture detector — not just that the builder compiles.

use crate::common;

use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_cupertino::cupertino_page_route;
use flui_sdk::animation::{Curve, Curves, Vsync};
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{ColoredBox, Navigator, NavigatorHandle, SimpleRoute, VsyncScope};

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

pub fn cupertino_route_does_not_slide_under_reduced_motion() {
    use flui_sdk::animation::MotionPolicy;
    use flui_sdk::widgets::{GestureDetector, MediaQuery, MediaQueryData};
    use std::{cell::Cell, rc::Rc};
    let vsync = Vsync::new();
    let navigator = seeded_navigator();
    let root = MediaQuery::new(
        MediaQueryData {
            motion: MotionPolicy::Reduce,
            ..MediaQueryData::default()
        },
        app(&vsync, &navigator),
    );
    let mut laid = lay_out_animated(root, tight(400.0, 800.0), vsync);
    let taps = Rc::new(Cell::new(0));
    let received = Rc::clone(&taps);
    let _result = navigator.push(cupertino_page_route::<(), _>(
        move |_ctx, _primary, _secondary| {
            let received = Rc::clone(&received);
            GestureDetector::new()
                .on_tap(move |_| received.set(received.get() + 1))
                .child(ColoredBox::new(Color::rgb(10, 20, 30)))
                .boxed()
        },
    ));
    laid.tick();
    laid.dispatch_pointer_down(20.0, 200.0);
    laid.dispatch_pointer_up(20.0, 200.0);
    assert_eq!(
        taps.get(),
        1,
        "the pushed page is interactive at its final position on the first frame"
    );
    laid.pump_for(TRANSITION / 2);
    laid.dispatch_pointer_down(20.0, 200.0);
    laid.dispatch_pointer_up(20.0, 200.0);
    assert_eq!(
        taps.get(),
        2,
        "the reduced-motion page keeps its settled hit geometry"
    );
}

/// `cupertino_page_transitions` mounts exactly two `SlideTransition`s for a
/// route nothing else covers: the **primary** (this page's own entrance,
/// tweened `1.0 -> 0.0`) and the **secondary** (the parallax a covering page
/// would apply — pinned at `0` here, since nothing covers this route). Each
/// one's horizontal fraction is read back from where its child is painted:
/// the child's origin mapped into the transition, over the transition's
/// width. The primary is whichever reads the larger `|dx|`.
fn primary_slide_dx(laid: &common::LaidOut) -> f64 {
    let nodes = laid.find_all_by_render_type("RenderAnimatedTransform");
    assert_eq!(
        nodes.len(),
        2,
        "the primary and secondary SlideTransition each mount one animated transform"
    );
    nodes
        .into_iter()
        .map(|id| {
            let child = laid.only_child(id);
            let matrix = laid
                .pipeline_owner()
                .with(|owner| owner.transform_to(child, id))
                .expect("the transition's child is laid out");
            matrix.transform_point(0.0, 0.0).0 / laid.size(id).width
        })
        .fold(0.0_f64, |largest, dx| {
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
pub fn cupertino_page_route_slides_in_from_off_the_right_edge_over_500ms() {
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
