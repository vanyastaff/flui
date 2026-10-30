//! `PageIndicator`: the dots drawn at a fractional position, the window over many pages, the
//! announced page, the controlled indicator's taps and steps, the observed one's lack of
//! both, right-to-left order, and reduced motion.

use std::num::NonZeroUsize;
use std::time::Duration;

use flui_animation::{ConstantAnimation, MotionOverride, Vsync};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_painting::{DrawOp, PaintStyle};
use flui_rendering::layer::Layer;
use flui_testing::{Action, ActionRequest, NodeId, TreeId};
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    ColoredBox, Column, Directionality, Expanded, PageController, PageIndicator,
    PageIndicatorStyle, PageIndicatorStyleError, PageView, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, pager_alternatives, tight};

const SIDE: f64 = 300.0;
const ACTIVE: Color = Color::rgb(0x1d, 0x1b, 0x20);

/// One drawn dot: its centre's x, its radius and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dot {
    x: f64,
    radius: f64,
    color: Color,
}

fn dots(laid: &LaidOut) -> Vec<Dot> {
    let mut dots = Vec::new();
    for (_, node) in laid.layer_tree().expect("a painted frame").iter() {
        if let Layer::Picture(picture) = node.layer() {
            for command in picture.picture().commands() {
                if let DrawOp::Circle {
                    center,
                    radius,
                    paint,
                } = &command.op
                {
                    assert_eq!(paint.style, PaintStyle::Fill);
                    let (x, _) = command.transform.transform_point(center.x, center.y);
                    dots.push(Dot {
                        x,
                        radius: *radius,
                        color: paint.color,
                    });
                }
            }
        }
    }
    dots
}

fn mount(view: impl IntoView) -> LaidOut {
    mount_on(&Vsync::new(), view)
}

fn mount_on(vsync: &Vsync, view: impl IntoView) -> LaidOut {
    let tree = VsyncScope::new(vsync.clone(), view).boxed();
    let mut laid = lay_out_animated(tree, tight(SIDE, SIDE), vsync.clone());
    laid.enable_semantics();
    laid.tick();
    laid
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
}

fn observed(position: f64, count: usize) -> BoxedView {
    flui_widgets::Center::new()
        .child(PageIndicator::observed(
            ConstantAnimation::new(position),
            count,
        ))
        .boxed()
}

/// The indicator's announced value, if it announces one.
fn announced(laid: &LaidOut) -> Option<String> {
    laid.a11y_tree()
        .expect("published semantics")
        .nodes()
        .find_map(|node| {
            node.value()
                .filter(|value| value.starts_with("Page"))
                .map(str::to_owned)
        })
}

fn indicator_node(laid: &LaidOut) -> NodeId {
    laid.a11y_tree()
        .expect("published semantics")
        .nodes()
        .find(|node| node.value().is_some_and(|value| value.starts_with("Page")))
        .expect("the indicator's node")
        .id()
}

fn supports(laid: &LaidOut, action: Action) -> bool {
    laid.a11y_tree()
        .expect("published semantics")
        .nodes()
        .find(|node| node.value().is_some_and(|value| value.starts_with("Page")))
        .expect("the indicator's node")
        .supports_action(action)
}

fn act(laid: &mut LaidOut, action: Action) {
    let node = indicator_node(laid);
    laid.invoke_semantics_action(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data: None,
    })
    .expect("the request resolves");
    settle(laid);
}

fn pages(count: u8) -> Vec<BoxedView> {
    (0..count)
        .map(|index| ColoredBox::new(Color::rgb(40 * index, 20, 30)).boxed())
        .collect()
}

/// A pager over `count` pages with a controlled indicator under it, the dots centred in the
/// bottom 16 px.
fn pager(controller: &PageController, count: u8) -> Column {
    Column::new(vec![
        Expanded::new(
            PageView::new(pages(count), pager_alternatives()).controller(controller.clone()),
        )
        .boxed(),
        PageIndicator::controlled(controller, usize::from(count)).boxed(),
    ])
}

/// The y of the dots under a pager: the middle of the bottom 16 px.
const ROW: f64 = SIDE - 8.0;

fn tap(laid: &mut LaidOut, x: f64) {
    laid.dispatch_pointer_down(x, ROW);
    laid.dispatch_pointer_up(x, ROW);
    settle(laid);
}

#[test]
fn between_two_pages_both_dots_are_half_lit() {
    let laid = mount(observed(1.5, 5));
    let drawn = dots(&laid);
    let xs: Vec<f64> = drawn.iter().map(|dot| dot.x - drawn[0].x).collect();
    assert_eq!(
        xs,
        [0.0, 16.0, 32.0, 48.0, 64.0],
        "five 8 px dots 8 px apart"
    );
    let inactive = ACTIVE.with_opacity(0.3);
    let half = Color::lerp(inactive, ACTIVE, 0.5);
    assert_eq!(drawn[1].color, half);
    assert_eq!(drawn[2].color, half);
    for index in [0, 3, 4] {
        assert_eq!(drawn[index].color, inactive, "dot {index}");
    }
}

#[test]
fn the_announced_page_is_the_nearest_and_a_tie_reads_as_the_later_one() {
    for (position, expected) in [
        (1.5, "Page 3 of 5"),
        (1.49, "Page 2 of 5"),
        (0.0, "Page 1 of 5"),
        (9.0, "Page 5 of 5"),
        (-3.0, "Page 1 of 5"),
    ] {
        let laid = mount(observed(position, 5));
        assert_eq!(announced(&laid).as_deref(), Some(expected), "at {position}");
    }
}

#[test]
fn a_position_past_the_end_lights_the_last_dot() {
    let laid = mount(observed(9.0, 5));
    let drawn = dots(&laid);
    assert_eq!(drawn.len(), 5);
    assert_eq!(drawn[4].color, ACTIVE, "the fifth dot");
}

#[test]
fn a_position_that_is_not_finite_keeps_the_last_finite_one() {
    let vsync = Vsync::new();
    let mut laid = mount_on(&vsync, observed(f64::NAN, 5));
    assert_eq!(
        announced(&laid).as_deref(),
        Some("Page 1 of 5"),
        "page 0 before any finite position"
    );
    laid.pump_widget(VsyncScope::new(vsync.clone(), observed(2.0, 5)));
    assert_eq!(announced(&laid).as_deref(), Some("Page 3 of 5"));
    for position in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        laid.pump_widget(VsyncScope::new(vsync.clone(), observed(position, 5)));
        assert_eq!(
            announced(&laid).as_deref(),
            Some("Page 3 of 5"),
            "{position} keeps page 2"
        );
        assert_eq!(dots(&laid)[2].color, ACTIVE, "{position} draws page 2");
    }
}

#[test]
fn one_page_or_none_hides_the_indicator() {
    for count in [0, 1] {
        let laid = mount(observed(0.0, count));
        assert_eq!(dots(&laid), [], "{count} pages draw nothing");
        assert_eq!(announced(&laid), None, "{count} pages announce nothing");
    }
    let controller = PageController::new();
    let laid = mount(flui_widgets::Center::new().child(PageIndicator::controlled(&controller, 0)));
    assert_eq!(announced(&laid), None, "an empty pager offers no actions");
}

#[test]
fn many_pages_show_a_window_whose_edges_shrink() {
    let laid = mount(observed(0.0, 20));
    let drawn = dots(&laid);
    assert_eq!(drawn.len(), 5, "a window of five");
    let radii: Vec<f64> = drawn.iter().map(|dot| dot.radius).collect();
    assert_eq!(radii, [4.0, 4.0, 4.0, 4.0, 2.0], "pages lie past the end");
    assert_eq!(announced(&laid).as_deref(), Some("Page 1 of 20"));

    let laid = mount(observed(10.0, 20));
    let drawn = dots(&laid);
    let radii: Vec<f64> = drawn.iter().map(|dot| dot.radius).collect();
    assert_eq!(radii, [2.0, 4.0, 4.0, 4.0, 2.0], "pages lie both ways");
    assert_eq!(drawn[2].color, ACTIVE, "the current page is centred");
    assert_eq!(announced(&laid).as_deref(), Some("Page 11 of 20"));
}

#[test]
fn the_observed_indicator_offers_no_actions_and_takes_no_tap() {
    let laid = mount(observed(2.0, 5));
    assert!(!supports(&laid, Action::Increment));
    assert!(!supports(&laid, Action::Decrement));
    assert!(!supports(&laid, Action::Click));
}

#[test]
fn the_controlled_indicator_turns_pages_by_tap_and_step_within_the_ends() {
    let controller = PageController::new();
    let mut laid = mount(pager(&controller, 3));
    assert_eq!(announced(&laid).as_deref(), Some("Page 1 of 3"));
    assert!(supports(&laid, Action::Increment));
    assert!(
        !supports(&laid, Action::Decrement),
        "nothing before the first page"
    );

    let left = (SIDE - 40.0) / 2.0;
    tap(&mut laid, left + 36.0);
    assert_eq!(
        controller.page(),
        Some(1.0),
        "a tap at the end turns forward"
    );
    assert_eq!(announced(&laid).as_deref(), Some("Page 2 of 3"));

    act(&mut laid, Action::Increment);
    assert_eq!(controller.page(), Some(2.0));
    assert!(
        !supports(&laid, Action::Increment),
        "nothing after the last page"
    );
    assert!(supports(&laid, Action::Decrement));
    tap(&mut laid, left + 36.0);
    assert_eq!(controller.page(), Some(2.0), "no page past the last");

    tap(&mut laid, left + 4.0);
    assert_eq!(
        controller.page(),
        Some(1.0),
        "a tap at the start turns back"
    );
    act(&mut laid, Action::Decrement);
    assert_eq!(controller.page(), Some(0.0));
}

#[test]
fn before_the_first_layout_the_indicator_shows_and_turns_from_the_initial_page() {
    let controller = PageController::with_params(3, flui_widgets::ViewportFraction::FULL);
    let vsync = Vsync::new();
    let mut laid = mount_on(
        &vsync,
        flui_widgets::Center::new().child(PageIndicator::controlled(&controller, 5)),
    );
    assert_eq!(controller.page(), None, "no pager has been laid out");
    assert_eq!(announced(&laid).as_deref(), Some("Page 4 of 5"));
    assert_eq!(dots(&laid)[3].color, ACTIVE);
    assert!(supports(&laid, Action::Decrement));
    act(&mut laid, Action::Increment);
    laid.pump_widget(VsyncScope::new(vsync, pager(&controller, 5)));
    settle(&mut laid);
    assert_eq!(
        controller.page(),
        Some(4.0),
        "the pager opens on the turned page"
    );
}

#[test]
fn right_to_left_the_first_dot_is_on_the_right_and_the_left_turns_forward() {
    let controller = PageController::new();
    let mut laid = mount(Directionality::new(
        TextDirection::Rtl,
        pager(&controller, 3),
    ));
    let left = (SIDE - 40.0) / 2.0;
    let drawn = dots(&laid);
    assert_eq!(drawn[0].x - drawn[2].x, 32.0, "page 0 on the right");
    assert_eq!(drawn[0].color, ACTIVE);
    tap(&mut laid, left + 4.0);
    assert_eq!(controller.page(), Some(1.0), "the left is the reading end");
    act(&mut laid, Action::Increment);
    assert_eq!(
        controller.page(),
        Some(2.0),
        "increase turns forward in reading order"
    );
}

#[test]
fn reduced_motion_turns_the_page_at_once() {
    let controller = PageController::new();
    let mut laid = mount(pager(&controller, 3));
    laid.set_motion_override(MotionOverride::Reduce);
    laid.tick();
    let left = (SIDE - 40.0) / 2.0;
    laid.dispatch_pointer_down(left + 36.0, ROW);
    laid.dispatch_pointer_up(left + 36.0, ROW);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(controller.page(), Some(1.0), "no turning animation");
    assert_eq!(dots(&laid)[1].color, ACTIVE, "the dots jump with it");
}

#[test]
fn a_style_refuses_sizes_that_cannot_be_drawn() {
    let five = NonZeroUsize::new(5).expect("five");
    for (dot, gap, expected) in [
        (0.0, 8.0, PageIndicatorStyleError::Dot(0.0)),
        (-1.0, 8.0, PageIndicatorStyleError::Dot(-1.0)),
        (
            f64::INFINITY,
            8.0,
            PageIndicatorStyleError::Dot(f64::INFINITY),
        ),
        (8.0, -1.0, PageIndicatorStyleError::Gap(-1.0)),
        (
            8.0,
            f64::INFINITY,
            PageIndicatorStyleError::Gap(f64::INFINITY),
        ),
        (f64::MAX, f64::MAX, PageIndicatorStyleError::Overflow),
    ] {
        assert_eq!(
            PageIndicatorStyle::try_new(dot, gap, five),
            Err(expected),
            "{dot} {gap}"
        );
    }
    assert!(
        PageIndicatorStyle::try_new(f64::MAX / 16.0, 0.0, five).is_ok(),
        "extreme but finite sizes are drawn"
    );
}
