//! `Scrollbar`: the bar's geometry along both axes and both text directions,
//! its input (thumb drag, track paging, the wheel over it), its fade, and the
//! scroll observer scope that binds it to the scroll view nearest below.
//! Rows of the `scrollbar` table in `contracts.rs`.

use std::rc::Rc;
use std::time::Duration;

use flui_animation::{MotionOverride, SharedCurve, Vsync, curves};
use flui_foundation::geometry::{Axis, Rect};
use flui_interaction::PointerKind;
use flui_painting::display_list::DrawOp;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_testing::widgets::SharedSignal;
use flui_view::environment::VsyncScope;
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};
use flui_widgets::{
    Column, Directionality, ScrollBehavior, ScrollConfiguration, ScrollController, ScrollView,
    ScrollableDetails, Scrollbar, SingleChildScrollView, SizedBox,
};

use crate::common::{LaidOut, lay_out, lay_out_animated, tight};

const THUMB: Color = Color::rgba(200, 10, 10, 255);
const INNER_THUMB: Color = Color::rgba(10, 200, 10, 255);
const TRACK: Color = Color::rgba(10, 10, 200, 255);
const FRAME: Duration = Duration::from_millis(16);
/// The box every case lays out in.
const SIDE: f64 = 300.0;

fn same_rgb(a: Color, b: Color) -> bool {
    a.to_rgba8()[..3] == b.to_rgba8()[..3]
}

/// Every rectangle painted in `color`, in window coordinates, with its alpha.
fn painted(laid: &LaidOut, color: Color) -> Vec<(Rect, u8)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| {
            let (rect, paint) = match &command.op {
                DrawOp::Rect { rect, paint } => (*rect, paint),
                DrawOp::RRect { rrect, paint } => (rrect.rect, paint),
                _ => return None,
            };
            same_rgb(paint.color, color).then(|| {
                (
                    command.transform.transform_rect(&rect),
                    paint.color.to_rgba8()[3],
                )
            })
        })
        .collect()
}

/// The one thumb painted in `color`.
fn thumb(laid: &LaidOut, color: Color) -> Rect {
    let thumbs = painted(laid, color);
    assert_eq!(thumbs.len(), 1, "one thumb is painted: {thumbs:?}");
    thumbs[0].0
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{what}: {actual} != {expected}"
    );
}

/// A vertical scroll view of `extent` logical pixels of content.
fn column(controller: &ScrollController, extent: f64) -> SingleChildScrollView {
    SingleChildScrollView::new()
        .controller(controller.clone())
        .child(SizedBox::new(SIDE, extent))
}

/// A horizontal scroll view of `extent` logical pixels of content.
fn row(controller: &ScrollController, extent: f64, height: f64) -> SingleChildScrollView {
    SingleChildScrollView::new()
        .scroll_direction(Axis::Horizontal)
        .controller(controller.clone())
        .child(SizedBox::new(extent, height))
}

fn shown(child: impl IntoView) -> Scrollbar {
    Scrollbar::new(child)
        .thumb_visibility(true)
        .thumb_color(THUMB)
}

fn animated(root: impl IntoView) -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), root.into_view()),
        tight(SIDE, SIDE),
        vsync.clone(),
    );
    (laid, vsync)
}

/// The thumb's length is the visible share of the track, it travels the rest
/// of the track as the position goes from one end to the other, and it sits
/// on the trailing edge of a vertical list (the left one under RTL), the
/// bottom of a horizontal one, and starts from the far end when reversed.
pub(crate) fn the_thumb_follows_the_position_on_both_axes_and_directions() {
    fn vertical() {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(column(&scroll, 3000.0)), tight(SIDE, SIDE));
        // 300 of 3000 show: a tenth of the track, at the right edge.
        let at_start = thumb(&laid, THUMB);
        assert_eq!(
            at_start,
            Rect::from_ltwh(294.0, 0.0, 6.0, 30.0),
            "thumb at the start"
        );
        scroll.jump_to(1350.0).expect("no build is running");
        laid.tick();
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(294.0, 135.0, 6.0, 30.0),
            "halfway down, the thumb is halfway along its travel"
        );
        scroll.jump_to(2700.0).expect("no build is running");
        laid.tick();
        assert_close(thumb(&laid, THUMB).bottom(), SIDE, "thumb at the end");
    }
    fn right_to_left() {
        let scroll = ScrollController::new();
        let laid = lay_out(
            Directionality::new(TextDirection::Rtl, shown(column(&scroll, 3000.0))),
            tight(SIDE, SIDE),
        );
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(0.0, 0.0, 6.0, 30.0),
            "a vertical bar trails on the left under RTL"
        );
    }
    fn horizontal() {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(row(&scroll, 3000.0, SIDE)), tight(SIDE, SIDE));
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(0.0, 294.0, 30.0, 6.0),
            "a horizontal bar sits at the bottom"
        );
        scroll.jump_to(1350.0).expect("no build is running");
        laid.tick();
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(135.0, 294.0, 30.0, 6.0),
            "and travels along the width"
        );
    }
    fn reversed() {
        let scroll = ScrollController::new();
        let laid = lay_out(
            shown(column(&scroll, 3000.0).reverse(true)),
            tight(SIDE, SIDE),
        );
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(294.0, 270.0, 6.0, 30.0),
            "a reversed list starts at the bottom of the track"
        );
    }
    fn margins_and_track() {
        let scroll = ScrollController::new();
        let laid = lay_out(
            shown(column(&scroll, 3000.0))
                .thickness(8.0)
                .main_axis_margin(10.0)
                .cross_axis_margin(2.0)
                .radius(4.0)
                .track_visibility(true)
                .track_color(TRACK),
            tight(SIDE, SIDE),
        );
        let track = painted(&laid, TRACK);
        assert_eq!(
            track.iter().map(|(rect, _)| *rect).collect::<Vec<_>>(),
            [Rect::from_ltwh(290.0, 10.0, 8.0, 280.0)],
            "the track keeps both margins"
        );
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(290.0, 10.0, 8.0, 28.0),
            "the thumb is a tenth of the shortened track"
        );
    }
    fn content_that_fits() {
        let scroll = ScrollController::new();
        let laid = lay_out(shown(column(&scroll, 200.0)), tight(SIDE, SIDE));
        assert!(
            painted(&laid, THUMB).is_empty(),
            "no thumb while nothing scrolls"
        );
    }
    fn direction_flips_under_a_kept_bar() {
        // The bar beside its list, handed back as the very same view while
        // only the direction around it changes: nothing rebuilds the bar's
        // view, only its dependency on the direction.
        let scroll = ScrollController::new();
        let rtl = SharedSignal::new(false);
        let kept: BoxedView = Scrollbar::new(SizedBox::new(SIDE, 100.0))
            .controller(
                scroll.clone(),
                flui_rendering::constraints::AxisDirection::TopToBottom,
            )
            .thumb_visibility(true)
            .thumb_color(THUMB)
            .boxed();
        let mut laid = lay_out(
            Column::new(vec![
                SizedBox::new(SIDE, 200.0)
                    .child(column(&scroll, 3000.0))
                    .boxed(),
                Directed {
                    rtl: rtl.clone(),
                    child: kept,
                }
                .boxed(),
            ]),
            tight(SIDE, SIDE),
        );
        assert_eq!(thumb(&laid, THUMB).left(), 294.0, "trailing on the right");
        rtl.update(|rtl| *rtl = true);
        laid.pump();
        assert_eq!(
            thumb(&laid, THUMB).left(),
            0.0,
            "the thumb moves to the left edge with the direction"
        );
    }
    crate::common::cases::run_cases(
        "scrollbar geometry",
        &[
            ("vertical", vertical as fn()),
            ("right to left", right_to_left),
            ("horizontal", horizontal),
            ("reversed", reversed),
            ("margins and track", margins_and_track),
            ("content that fits", content_that_fits),
            (
                "direction flips under a kept bar",
                direction_flips_under_a_kept_bar,
            ),
        ],
    );
}

/// The thumb never leaves a track shorter than its minimum length, and past
/// either end it shrinks with the overscroll below that minimum.
pub(crate) fn the_thumb_stays_inside_short_tracks_and_shrinks_with_overscroll() {
    fn short_track() {
        let scroll = ScrollController::new();
        let mut laid = lay_out(
            shown(
                SingleChildScrollView::new()
                    .controller(scroll.clone())
                    .child(SizedBox::new(100.0, 900.0)),
            ),
            tight(100.0, 10.0),
        );
        assert_eq!(
            thumb(&laid, THUMB),
            Rect::from_ltwh(94.0, 0.0, 6.0, 10.0),
            "the minimum gives way to a 10px track"
        );
        scroll.jump_to(890.0).expect("no build is running");
        laid.tick();
        assert_close(
            thumb(&laid, THUMB).bottom(),
            10.0,
            "thumb ends at the track",
        );
        // A full-track thumb has no travel to drag.
        laid.dispatch_pointer_down(97.0, 5.0);
        laid.dispatch_pointer_move(97.0, 25.0);
        laid.dispatch_pointer_move(97.0, 205.0);
        laid.dispatch_pointer_up(97.0, 205.0);
        assert_eq!(scroll.pixels(), 890.0, "a full-track thumb has no travel");
    }
    fn minimum_length() {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(column(&scroll, 30_000.0)), tight(SIDE, SIDE));
        assert_close(thumb(&laid, THUMB).height(), 18.0, "the minimum length");
        scroll.jump_to(29_700.0).expect("no build is running");
        laid.tick();
        assert_close(
            thumb(&laid, THUMB).bottom(),
            SIDE,
            "a long list ends at the track",
        );
    }
    fn overscroll() {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(column(&scroll, 30_000.0)), tight(SIDE, SIDE));
        // Half a viewport past the start: half of the viewport shows.
        scroll.place_for_test(-150.0);
        laid.tick();
        let shrunk = thumb(&laid, THUMB);
        assert_close(shrunk.top(), 0.0, "an overscrolled thumb stays at its end");
        assert_close(
            shrunk.height(),
            1.5,
            "the minimum is gone a fifth of a viewport past the end",
        );
        scroll.place_for_test(-30.0);
        laid.tick();
        // 270 of 30 300 visible, and the minimum of 18 at half strength.
        assert_close(
            thumb(&laid, THUMB).height(),
            9.0,
            "the minimum shrinks with the overscroll",
        );
    }
    crate::common::cases::run_cases(
        "scrollbar track containment",
        &[
            ("short track", short_track as fn()),
            ("minimum length", minimum_length),
            ("overscroll", overscroll),
        ],
    );
}

/// A scroll frame repaints the bar and rebuilds nothing.
pub(crate) fn scrolling_repaints_the_bar_without_rebuilding() {
    let scroll = ScrollController::new();
    let mut laid = lay_out(shown(column(&scroll, 3000.0)), tight(SIDE, SIDE));
    let before = thumb(&laid, THUMB);
    scroll.jump_to(1350.0).expect("no build is running");
    laid.tick();
    let built = laid.with_build_owner_mut(|owner| owner.last_frame_build_report().elements_built);
    assert_eq!(built, 0, "a scroll frame builds no element");
    assert!(laid.did_paint_last_frame(), "the frame repainted");
    assert_ne!(thumb(&laid, THUMB), before, "and the thumb moved");
}

/// Dragging the thumb moves the position by the content the thumb's travel
/// stands for, with a mouse as with a finger, and takes the position from a
/// running animation before it moves.
pub(crate) fn dragging_the_thumb_moves_the_position_with_any_device() {
    fn drag(kind: PointerKind) {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(column(&scroll, 3000.0)), tight(SIDE, SIDE));
        laid.dispatch_pointer_down_with_kind(297.0, 10.0, kind);
        laid.dispatch_pointer_move(297.0, 60.0);
        let started = scroll.pixels();
        // 27 of the thumb's 270 px of travel: a tenth of the 2700 px extent.
        laid.dispatch_pointer_move(297.0, 87.0);
        assert_close(
            scroll.pixels(),
            started + 270.0,
            "the thumb maps to the extent",
        );
        laid.dispatch_pointer_move(297.0, 900.0);
        assert_eq!(
            scroll.pixels(),
            2700.0,
            "{kind:?}: the drag stops at the end"
        );
        laid.dispatch_pointer_up(297.0, 900.0);
        scroll.jump_to(0.0).expect("no build is running");
        laid.tick();
        assert_eq!(scroll.pixels(), 0.0, "the next jump still works");
    }
    fn mouse() {
        drag(PointerKind::Mouse);
    }
    fn touch() {
        drag(PointerKind::Touch);
    }
    fn over_an_animation() {
        let scroll = ScrollController::new();
        let (mut laid, _vsync) = animated(shown(column(&scroll, 5000.0)));
        scroll
            .animate_to(
                1000.0,
                Duration::from_millis(300),
                SharedCurve::new(curves::LINEAR),
            )
            .expect("no build is running");
        for _ in 0..3 {
            laid.pump_for(FRAME);
        }
        assert!(scroll.pixels() > 0.0 && scroll.pixels() < 1000.0);
        let top = thumb(&laid, THUMB).top();
        laid.dispatch_pointer_down_with_kind(297.0, top + 5.0, PointerKind::Mouse);
        laid.dispatch_pointer_move(297.0, top + 25.0);
        laid.dispatch_pointer_move(297.0, top + 45.0);
        let dragged = scroll.pixels();
        laid.dispatch_pointer_up(297.0, top + 45.0);
        for _ in 0..24 {
            laid.pump_for(FRAME);
        }
        assert_eq!(scroll.pixels(), dragged, "the old animation stays stopped");
        assert!(!scroll.position().is_scrolling());
    }
    crate::common::cases::run_cases(
        "scrollbar thumb drag",
        &[
            ("mouse", mouse as fn()),
            ("touch", touch),
            ("over an animation", over_an_animation),
        ],
    );
}

/// A tap on the track pages toward it by most of a viewport; a tap on the
/// thumb does not page; a wheel over the bar scrolls what it follows.
pub(crate) fn the_track_pages_and_the_wheel_scrolls_over_the_bar() {
    let scroll = ScrollController::new();
    let (mut laid, _vsync) = animated(shown(column(&scroll, 3000.0)));
    tap(&laid, 297.0, 200.0);
    run(&mut laid, Duration::from_millis(200));
    assert_close(scroll.pixels(), 240.0, "a tap below the thumb pages down");
    let thumb_at = thumb(&laid, THUMB);
    tap(&laid, 297.0, thumb_at.top() + 1.0);
    run(&mut laid, Duration::from_millis(200));
    assert_close(scroll.pixels(), 240.0, "a tap on the thumb does not page");
    tap(&laid, 297.0, 1.0);
    run(&mut laid, Duration::from_millis(200));
    assert_close(scroll.pixels(), 0.0, "a tap above the thumb pages up");
    laid.dispatch_scroll(297.0, 150.0, 0.0, 50.0);
    run(&mut laid, Duration::from_millis(200));
    assert!(
        scroll.pixels() > 0.0,
        "a wheel over the bar scrolls the list"
    );
}

/// The bar shows while its scroll view scrolls or jumps, stays for the time
/// to fade, then fades out; under reduced motion it appears and goes at once.
pub(crate) fn the_bar_fades_in_while_scrolling_and_out_after_it_stops() {
    fn faded(reduced: bool) {
        let scroll = ScrollController::new();
        let (mut laid, _vsync) =
            animated(Scrollbar::new(column(&scroll, 3000.0)).thumb_color(THUMB));
        if reduced {
            laid.set_motion_override(MotionOverride::Reduce);
            laid.pump_for(Duration::ZERO);
        }
        assert!(painted(&laid, THUMB).is_empty(), "hidden before any scroll");
        // A finger drags the content.
        laid.dispatch_pointer_down(150.0, 200.0);
        laid.dispatch_pointer_move(150.0, 150.0);
        laid.dispatch_pointer_move(150.0, 100.0);
        run(
            &mut laid,
            if reduced {
                Duration::ZERO
            } else {
                Duration::from_millis(400)
            },
        );
        let shown = painted(&laid, THUMB);
        assert_eq!(shown.len(), 1, "shown while scrolling");
        assert_eq!(shown[0].1, 255, "fully shown");
        laid.dispatch_pointer_up(150.0, 100.0);
        // Any fling the release starts runs out first.
        let mut frames = 0;
        while scroll.position().is_scrolling() {
            laid.pump_for(FRAME);
            frames += 1;
            assert!(frames < 300, "the scroll settles");
        }
        laid.pump_for(FRAME);
        if reduced {
            assert!(
                painted(&laid, THUMB).is_empty(),
                "gone once the scroll stops"
            );
            return;
        }
        assert_eq!(
            painted(&laid, THUMB).len(),
            1,
            "shown when the scroll stops"
        );
        run(&mut laid, Duration::from_millis(400));
        assert_eq!(
            painted(&laid, THUMB).len(),
            1,
            "still shown before the time to fade"
        );
        run(&mut laid, Duration::from_millis(1000));
        assert!(painted(&laid, THUMB).is_empty(), "faded out");
        // A jump shows the bar again.
        scroll.jump_to(1000.0).expect("no build is running");
        laid.pump_for(FRAME);
        assert_eq!(painted(&laid, THUMB).len(), 1, "a jump shows the bar");
        run(&mut laid, Duration::from_millis(1200));
        assert!(painted(&laid, THUMB).is_empty(), "and it fades again");
    }
    fn full() {
        faded(false);
    }
    fn reduced() {
        faded(true);
    }
    crate::common::cases::run_cases(
        "scrollbar fade",
        &[("full motion", full as fn()), ("reduced motion", reduced)],
    );
}

/// A bar follows the scroll view at depth zero below it: a list nested inside
/// that one neither replaces nor moves the outer bar, and a bar over the
/// nested list follows it alone.
pub(crate) fn a_bar_follows_the_nearest_scroll_view_and_ignores_nested_ones() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let nested = |inner_bar: bool| {
        // A strip of a fixed size: a horizontal scroll view in a column.
        let strip = SizedBox::new(SIDE, 100.0).child(if inner_bar {
            Scrollbar::new(row(&inner, 3000.0, 100.0))
                .thumb_visibility(true)
                .thumb_color(INNER_THUMB)
                .boxed()
        } else {
            row(&inner, 3000.0, 100.0).boxed()
        });
        shown(
            SingleChildScrollView::new()
                .controller(outer.clone())
                .child(Column::new(vec![
                    strip.boxed(),
                    SizedBox::new(SIDE, 2900.0).boxed(),
                ])),
        )
    };
    let mut laid = lay_out(nested(false), tight(SIDE, SIDE));
    assert_eq!(
        thumb(&laid, THUMB),
        Rect::from_ltwh(294.0, 0.0, 6.0, 30.0),
        "the outer bar follows the outer list"
    );
    inner.jump_to(1000.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        thumb(&laid, THUMB),
        Rect::from_ltwh(294.0, 0.0, 6.0, 30.0),
        "the inner list neither captures nor moves the outer bar"
    );
    laid.pump_widget(nested(true));
    outer.jump_to(1350.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        thumb(&laid, THUMB),
        Rect::from_ltwh(294.0, 135.0, 6.0, 30.0),
        "the outer bar keeps following the outer list"
    );
    outer.jump_to(0.0).expect("no build is running");
    laid.tick();
    // The inner strip is 300 wide over 3000 at 1000: a tenth of the width,
    // 1000 of 2700 along its 270 px of travel, at the strip's bottom.
    assert_eq!(
        thumb(&laid, INNER_THUMB),
        Rect::from_ltwh(100.0, 94.0, 30.0, 6.0),
        "the inner bar follows the inner list"
    );
}

/// A bar follows the scroll view that replaces the one it followed, and a
/// controller handed to the same scroll view.
pub(crate) fn a_bar_follows_a_replaced_scroll_view_or_controller() {
    let first = ScrollController::new();
    let second = ScrollController::new();
    second.jump_to(0.0).expect("no build is running");
    let mut laid = lay_out(shown(column(&first, 3000.0)), tight(SIDE, SIDE));
    laid.pump_widget(shown(column(&second, 3000.0)));
    second.jump_to(1350.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        thumb(&laid, THUMB).top(),
        135.0,
        "the bar follows the swapped controller"
    );
    // Another tree shape: the old scroll view unmounts, a new one mounts.
    let third = ScrollController::new();
    laid.pump_widget(shown(
        SizedBox::new(SIDE, SIDE).child(column(&third, 6000.0)),
    ));
    assert_eq!(
        thumb(&laid, THUMB),
        Rect::from_ltwh(294.0, 0.0, 6.0, 18.0),
        "the bar follows the scroll view that replaced the old one"
    );
    second.jump_to(0.0).expect("no build is running");
    third.jump_to(5700.0).expect("no build is running");
    laid.tick();
    assert_close(thumb(&laid, THUMB).bottom(), SIDE, "and only that one");
}

/// A `ScrollBehavior` adds its bar to every scroll view under it, except one
/// a bar above already follows.
pub(crate) fn a_scroll_behavior_builds_the_bar_of_its_scroll_views() {
    #[derive(Debug)]
    struct Bars;
    impl ScrollBehavior for Bars {
        fn build_scrollbar(
            &self,
            _ctx: &dyn BuildContext,
            child: BoxedView,
            details: &ScrollableDetails,
        ) -> BoxedView {
            Scrollbar::for_scrollable(details, child)
                .thumb_visibility(true)
                .thumb_color(INNER_THUMB)
                .boxed()
        }
    }
    let behavior: Rc<dyn ScrollBehavior> = Rc::new(Bars);
    let scroll = ScrollController::new();
    let mut laid = lay_out(
        ScrollConfiguration::new(Rc::clone(&behavior), column(&scroll, 3000.0)),
        tight(SIDE, SIDE),
    );
    assert_eq!(
        thumb(&laid, INNER_THUMB),
        Rect::from_ltwh(294.0, 0.0, 6.0, 30.0),
        "the behavior's bar"
    );
    scroll.jump_to(1350.0).expect("no build is running");
    laid.tick();
    assert_eq!(
        thumb(&laid, INNER_THUMB).top(),
        135.0,
        "follows its scroll view"
    );
    laid.pump_widget(ScrollConfiguration::new(
        Rc::clone(&behavior),
        shown(column(&scroll, 3000.0)),
    ));
    assert_eq!(thumb(&laid, THUMB).top(), 135.0, "the explicit bar shows");
    assert!(
        painted(&laid, INNER_THUMB).is_empty(),
        "and the behavior adds no second one"
    );
    // A bar that follows another controller is not the bar of the scroll
    // view under it, which keeps the one its behavior gives it.
    let elsewhere = ScrollController::new();
    laid.pump_widget(ScrollConfiguration::new(
        Rc::clone(&behavior),
        shown(column(&scroll, 3000.0)).controller(
            elsewhere,
            flui_rendering::constraints::AxisDirection::TopToBottom,
        ),
    ));
    assert_eq!(
        thumb(&laid, INNER_THUMB).top(),
        135.0,
        "the scroll view under a bar of another controller keeps its own bar"
    );
}

/// A bar told to show its thumb with no scroll view under it is a mistake
/// debug builds report.
#[cfg(debug_assertions)]
pub(crate) fn a_shown_thumb_without_a_scroll_view_fails_loudly() {
    // A bar that only shows while scrolling has nothing to report.
    let _ = lay_out(Scrollbar::new(SizedBox::new(SIDE, SIDE)), tight(SIDE, SIDE));
    let failed = std::panic::catch_unwind(|| {
        lay_out(shown(SizedBox::new(SIDE, SIDE)), tight(SIDE, SIDE));
    });
    let payload = failed.expect_err("the bar with nothing to follow panics");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("RenderCustomPaint panicked during paint"),
        "the bar's painter reports it: {message}"
    );
}

/// A finger's press and release at one point.
fn tap(laid: &LaidOut, x: f64, y: f64) {
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);
}

/// Frames of 16 ms until `duration` has passed.
fn run(laid: &mut LaidOut, duration: Duration) {
    let mut left = duration;
    laid.pump_for(Duration::ZERO);
    while !left.is_zero() {
        let step = left.min(FRAME);
        laid.pump_for(step);
        left -= step;
    }
}

/// A wheel tick of one line at `(x, y)`.
fn wheel_line(laid: &LaidOut, x: f64, y: f64) {
    use flui_platform::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerPosition, ScrollDelta, ScrollEvent, ScrollUnit,
    };
    laid.dispatch_pointer_event(&PointerEvent::Scroll(ScrollEvent::new(
        PointerInfo::new(
            PointerId::try_from(1_u64).expect("nonzero pointer"),
            PointerKind::Mouse,
        ),
        flui_platform::EventTime::from_nanos(1),
        PointerPosition::try_new(flui_foundation::geometry::Point::new(x, y))
            .expect("finite wheel position"),
        ScrollDelta::try_new(ScrollUnit::Lines, 0.0, 1.0).expect("finite wheel distance"),
    )));
}

/// A line of the wheel over the bar moves the list as far as over its
/// content, by the distances the scroll view names.
pub(crate) fn a_wheel_over_the_bar_keeps_the_scroll_views_line_distance() {
    // Each tick goes to a fresh tree, so no latch from the first carries
    // the second to the content.
    let line_at = |x: f64| {
        let scroll = ScrollController::new();
        let distances = flui_widgets::WheelScrollDistances::try_new(70.0, 10.0).expect("positive");
        let (mut laid, _vsync) =
            animated(shown(column(&scroll, 3000.0).wheel_distances(distances)));
        wheel_line(&laid, x, 150.0);
        run(&mut laid, Duration::from_millis(300));
        scroll.pixels()
    };
    assert_close(line_at(150.0), 70.0, "a line over the content");
    assert_close(line_at(297.0), 70.0, "a line over the bar moves as far");
}

/// A bar that starts following a scroll view already under way shows for
/// that run, though no change of activity follows.
pub(crate) fn a_bar_attached_mid_scroll_shows_for_the_run_under_way() {
    let scroll = ScrollController::new();
    let layout = |with_bar: bool| {
        let beside: BoxedView = if with_bar {
            Scrollbar::new(SizedBox::new(SIDE, 100.0))
                .controller(
                    scroll.clone(),
                    flui_rendering::constraints::AxisDirection::TopToBottom,
                )
                .thumb_color(THUMB)
                .boxed()
        } else {
            SizedBox::new(SIDE, 100.0).boxed()
        };
        Column::new(vec![
            SizedBox::new(SIDE, 200.0)
                .child(column(&scroll, 3000.0))
                .boxed(),
            beside,
        ])
    };
    let (mut laid, vsync) = animated(layout(false));
    scroll
        .animate_to(
            2000.0,
            Duration::from_secs(2),
            SharedCurve::new(curves::LINEAR),
        )
        .expect("no build is running");
    run(&mut laid, Duration::from_millis(100));
    assert!(scroll.position().is_scrolling(), "the run is under way");
    laid.pump_widget(VsyncScope::new(vsync, layout(true)));
    run(&mut laid, Duration::from_millis(400));
    assert!(scroll.position().is_scrolling(), "still under way");
    assert_eq!(painted(&laid, THUMB).len(), 1, "the bar shows for it");
}

/// A second finger that lands on the content or the track while the first
/// holds the thumb leaves the first finger's drag of the thumb as it was.
pub(crate) fn a_second_finger_leaves_the_thumb_drag_to_the_first() {
    use flui_foundation::geometry::Offset;
    use flui_testing::TouchScript;
    for (name, second) in [
        ("content", Offset::new(150.0, 150.0)),
        ("track", Offset::new(297.0, 250.0)),
    ] {
        let scroll = ScrollController::new();
        let mut laid = lay_out(shown(column(&scroll, 3000.0)), tight(SIDE, SIDE));
        let script = TouchScript::new(name)
            .down(1, Offset::new(297.0, 10.0))
            .down(2, second)
            .move_to(1, Offset::new(297.0, 60.0))
            .move_to(1, Offset::new(297.0, 87.0))
            .up(1)
            .up(2)
            .build()
            .expect("every finger lifts");
        laid.replay(&script);
        assert!(
            scroll.pixels() > 200.0,
            "{name}: the first finger dragged the thumb, to {}",
            scroll.pixels()
        );
    }
}

/// `child` under the direction `rtl` names, the very same child view after
/// every change of it.
#[derive(Clone, StatefulView)]
struct Directed {
    rtl: SharedSignal<bool>,
    child: BoxedView,
}

struct DirectedState {
    rtl: SharedSignal<bool>,
}

impl StatefulView for Directed {
    type State = DirectedState;

    fn create_state(&self) -> Self::State {
        DirectedState {
            rtl: self.rtl.clone(),
        }
    }
}

impl ViewState<Directed> for DirectedState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rtl.bind(ctx);
    }

    fn build(&self, view: &Directed, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = if view.rtl.get(ctx) {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        };
        Directionality::new(direction, view.child.clone())
    }
}

impl std::fmt::Debug for Directed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Directed")
    }
}

impl std::fmt::Debug for DirectedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DirectedState")
    }
}
