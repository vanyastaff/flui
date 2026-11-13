//! The iOS page slide end to end: a [`CupertinoApp::router`] under a real
//! [`Vsync`] clock, a page pushed through its `Router`. Proves the slide the
//! app installs for its routes (ADR-0221): its geometry over the 500 ms
//! duration, its edge shadow, and its reduced-motion settle.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_cupertino::CupertinoApp;
use flui_sdk::animation::{Curve, Vsync, curves};
use flui_sdk::painting::{Color, DrawOp};
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{ColoredBox, Routable, Router, RouterHandle};

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

#[derive(Routable, Clone, Debug, PartialEq)]
enum Screen {
    #[route("/")]
    Home,
    #[route("/next")]
    Next,
}

/// Where the home page leaves its router handle.
type RouterSlot = Rc<RefCell<Option<RouterHandle<Screen>>>>;

/// The home page: black, recording its router.
#[derive(Clone, StatelessView)]
struct Home(RouterSlot);

impl StatelessView for Home {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.0.borrow_mut() = Router::<Screen>::of(ctx).ok();
        ColoredBox::new(Color::rgb(0, 0, 0))
    }
}

/// A `CupertinoApp` rooted in a Router whose `Next` page `next` builds, and
/// the slot its home page fills with the router's handle.
fn app(vsync: &Vsync, next: impl Fn() -> BoxedView + 'static) -> (impl View, RouterSlot) {
    let slot = RouterSlot::default();
    let home = Rc::clone(&slot);
    let router = Router::new(Screen::Home, move |screen: &Screen, _ctx| match screen {
        Screen::Home => Home(Rc::clone(&home)).boxed(),
        Screen::Next => next(),
    });
    (
        VsyncScope::new(vsync.clone(), CupertinoApp::router(router)),
        slot,
    )
}

fn push_next(slot: &RouterSlot) {
    slot.borrow()
        .clone()
        .expect("the home page is mounted under the Router")
        .push(Screen::Next)
        .expect("the Router takes the push");
}

fn next_page() -> BoxedView {
    ColoredBox::new(Color::rgb(10, 20, 30)).into_view().boxed()
}

pub fn cupertino_route_does_not_slide_under_reduced_motion() {
    use flui_sdk::animation::MotionPolicy;
    use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
    use flui_sdk::widgets::GestureDetector;
    use std::cell::Cell;
    let vsync = Vsync::new();
    let taps = Rc::new(Cell::new(0));
    let received = Rc::clone(&taps);
    let (app, slot) = app(&vsync, move || {
        let received = Rc::clone(&received);
        GestureDetector::new()
            .on_tap(move |_| received.set(received.get() + 1))
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
            .boxed()
    });
    let root = MediaQuery::new(
        MediaQueryData {
            motion: MotionPolicy::Reduce,
            ..MediaQueryData::default()
        },
        app,
    );
    let mut laid = lay_out_animated(root, tight(400.0, 800.0), vsync);
    push_next(&slot);
    // The Router places the page in the first frame, and the page takes
    // pointers from the next.
    laid.tick();
    laid.tick();
    laid.dispatch_pointer_down(20.0, 200.0);
    laid.dispatch_pointer_up(20.0, 200.0);
    assert_eq!(
        taps.get(),
        1,
        "the pushed page is interactive at its final position once its run settles"
    );
    laid.pump_for(TRANSITION / 2);
    laid.dispatch_pointer_down(20.0, 200.0);
    laid.dispatch_pointer_up(20.0, 200.0);
    assert_eq!(
        taps.get(),
        2,
        "the reduced-motion page keeps its settled hit geometry"
    );
    assert!(
        edge_shadows(&laid).is_empty(),
        "a page that does not move casts no edge shadow"
    );
}

/// The blur of every shadow painted in the last frame.
fn edge_shadows(laid: &common::LaidOut) -> Vec<f64> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Shadow { elevation, .. } => Some(elevation),
            _ => None,
        })
        .collect()
}

/// While the page slides in it casts a shadow off its leading edge, grown in
/// part way at the midpoint; settled at either end it casts none.
///
/// Red-check: wrap the primary slide's child without the
/// `DecoratedBoxTransition` — the midpoint frame paints no shadow.
pub fn cupertino_route_casts_its_edge_shadow_only_while_moving() {
    let vsync = Vsync::new();
    let (app, slot) = app(&vsync, next_page);
    let mut laid = lay_out_animated(app, tight(400.0, 800.0), vsync);
    push_next(&slot);
    laid.tick();
    assert!(
        edge_shadows(&laid).is_empty(),
        "the page waiting off screen casts no shadow"
    );

    laid.pump_for(TRANSITION / 2);
    let shadows = edge_shadows(&laid);
    assert_eq!(shadows.len(), 1, "the moving page casts one edge shadow");
    // The full shadow blurs over 12 logical pixels; it grows in from none.
    assert!(
        shadows[0] > 0.0 && shadows[0] < 12.0,
        "half way the shadow has grown part way in: blur {}",
        shadows[0]
    );

    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert!(
        edge_shadows(&laid).is_empty(),
        "the settled page casts no shadow"
    );
}

/// The shared `ParallaxPageTransition` mounts two `SlideTransition`s for each
/// page: the **primary** (the page's own entrance, tweened `1.0 -> 0.0`) and
/// the **secondary** (the parallax a covering page applies). The pushed page
/// is the top of the stack, so its two come last in paint order, after the
/// home page's; nothing covers it, so its secondary stays at `0`. Each one's
/// horizontal fraction is read back from where its child is painted: the
/// child's origin mapped into the transition, over the transition's width.
/// The primary is whichever reads the larger `|dx|`.
fn pushed_primary_slide_dx(laid: &common::LaidOut) -> f64 {
    let nodes = laid.find_all_by_render_type("RenderAnimatedTransform");
    assert_eq!(
        nodes.len(),
        4,
        "the home page and the pushed page each mount a primary and a secondary SlideTransition"
    );
    nodes[2..]
        .iter()
        .map(|&id| {
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

/// A page pushed under a `CupertinoApp` slides in from fully off the right
/// edge of the viewport to flush with it, over the oracle's 500ms
/// `kTransitionDuration` — not a jump cut, and not settled before the
/// duration elapses. The midpoint value is pinned against
/// `curves::FAST_EASE_IN_TO_SLOW_EASE_OUT`'s own math (the exact curve
/// `route.dart`'s `_setupAnimation` applies to the primary position), not a
/// loose "still mid-flight" range a plain linear interpolation would also
/// satisfy.
///
/// Red-check: drop the `PageTransitions` the app installs — the page jump
/// cuts, and the exact-midpoint assertion below fails, since a jump cut is
/// already at rest the instant it mounts. Red-check 2: swap the primary curve
/// for `curves::LINEAR` in `flui-widgets`' `parallax_transition.rs` —
/// `midpoint_dx` reads `0.5` (linear at `t=0.5`), which is `> 0.05` away from
/// `FastEaseInToSlowEaseOut`'s real value asserted below, so the
/// tight-tolerance assertion fails.
pub fn cupertino_app_slides_a_page_in_from_off_the_right_edge_over_500ms() {
    let vsync = Vsync::new();
    let (app, slot) = app(&vsync, next_page);
    let mut laid = lay_out_animated(app, tight(400.0, 800.0), vsync);
    push_next(&slot);
    laid.tick();

    let start_dx = pushed_primary_slide_dx(&laid);
    assert!(
        (start_dx - 1.0).abs() < 0.01,
        "the page must start fully off-screen to the right (dx == 1.0): {start_dx}"
    );

    // Halfway through the 500ms transition: still animating, not settled,
    // and at the exact value `FastEaseInToSlowEaseOut` (not linear) predicts.
    laid.pump_for(TRANSITION / 2);
    let midpoint_dx = pushed_primary_slide_dx(&laid);
    let curved_progress = curves::FAST_EASE_IN_TO_SLOW_EASE_OUT.transform(0.5);
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
    let settled_dx = pushed_primary_slide_dx(&laid);
    assert!(
        settled_dx.abs() < 0.01,
        "the page must settle flush with the viewport's left edge (dx == 0.0) \
         once the transition completes: {settled_dx}"
    );
}
