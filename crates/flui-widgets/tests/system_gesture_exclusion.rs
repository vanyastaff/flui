//! `SystemGestureExclusion`: the device-pixel rectangles a window hands the OS, collected
//! from layout on every composited frame and re-sent only when the projection, the scale
//! or the display changed (ADR-0217 §5).
//!
//! Each scenario reads what the headless window's backend handed the OS.

use flui_foundation::geometry::{DevicePoint, DeviceRect, Matrix4};
use flui_painting::styling::Color;
use flui_platform_api::DisplayId;
use flui_platform_api::input_regions::{DeviceInputRegion, InputRegionKind};
use flui_testing::log_capture;
use flui_view::{IntoView, View};
use flui_widgets::row;
use flui_widgets::{
    ClipRect, ColoredBox, Positioned, SizedBox, Stack, SystemGestureExclusion, Transform,
};

use crate::common::{LaidOut, lay_out, tight};

fn device(left: i32, top: i32, right: i32, bottom: i32) -> DeviceInputRegion {
    DeviceInputRegion::new(
        InputRegionKind::SystemGestureExclusion,
        DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
    )
}

/// A 200-px window painted `background`, with `content` positioned at (`left`, `top`).
fn window(background: Color, left: f64, top: f64, content: impl IntoView) -> impl View {
    Stack::new(row![
        SizedBox::new(200.0, 200.0).child(ColoredBox::new(background)),
        Positioned::new(content).left(left).top(top),
    ])
}

fn excluded(width: f64, height: f64) -> SystemGestureExclusion {
    SystemGestureExclusion::new().child(SizedBox::new(width, height))
}

const GREY: Color = Color::rgb(240, 240, 240);

fn mounted(content: impl IntoView) -> LaidOut {
    lay_out(window(GREY, 10.0, 20.0, content), tight(200.0, 200.0))
}

pub(crate) fn input_regions_reach_the_os_only_when_changed() {
    let mut laid = mounted(excluded(30.0, 40.0));
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(10, 20, 40, 60)]]
    );

    // A composited frame that changes only the paint re-sends nothing.
    laid.pump_widget(window(
        Color::rgb(1, 2, 3),
        10.0,
        20.0,
        excluded(30.0, 40.0),
    ));
    assert!(laid.did_paint_last_frame(), "the frame was composited");
    assert_eq!(laid.window().input_region_updates().len(), 1);

    laid.pump_widget(window(GREY, 50.0, 20.0, excluded(30.0, 40.0)));
    laid.pump_widget(window(
        GREY,
        50.0,
        20.0,
        excluded(30.0, 40.0).excluding(false),
    ));
    assert_eq!(
        laid.window().input_region_updates(),
        vec![
            vec![device(10, 20, 40, 60)],
            vec![device(50, 20, 80, 60)],
            vec![],
        ]
    );
}

pub(crate) fn a_transformed_region_reports_its_bounding_box() {
    let laid =
        mounted(Transform::rotation(std::f64::consts::FRAC_PI_4).child(excluded(20.0, 20.0)));
    // A 20-px square rotated about its centre (20, 30) spans ±10√2 ≈ ±14.14 around it.
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(5, 15, 35, 45)]]
    );
}

pub(crate) fn a_clipped_region_reports_the_intersection() {
    let clipped = ClipRect::new().child(SizedBox::new(40.0, 40.0).child(Stack::new(row![
        Positioned::new(excluded(100.0, 100.0)).left(20.0).top(20.0),
    ])));
    let laid = mounted(clipped);
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(30, 40, 50, 60)]]
    );
}

pub(crate) fn a_region_beyond_the_window_is_clamped_to_it() {
    let laid = lay_out(
        window(GREY, 180.0, -10.0, excluded(50.0, 50.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(180, 0, 200, 40)]]
    );
}

pub(crate) fn a_scale_change_resends_unchanged_logical_regions() {
    let mut laid = mounted(excluded(30.0, 40.0));
    laid.set_scale_factor(2.0);
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(10, 20, 40, 60)], vec![device(20, 40, 80, 120)]]
    );
}

pub(crate) fn a_display_change_at_equal_scale_resends_regions() {
    let mut laid = mounted(excluded(30.0, 40.0));
    laid.move_to_display(Some(DisplayId(1)));
    laid.move_to_display(Some(DisplayId(2)));
    // Moving within a display changes nothing for the OS.
    laid.move_to_display(Some(DisplayId(2)));
    let at = vec![device(10, 20, 40, 60)];
    assert_eq!(
        laid.window().input_region_updates(),
        vec![at.clone(), at.clone(), at]
    );
}

pub(crate) fn a_display_appearing_or_vanishing_resends_regions() {
    let mut laid = mounted(excluded(30.0, 40.0));
    laid.move_to_display(Some(DisplayId(1)));
    laid.move_to_display(None);
    let at = vec![device(10, 20, 40, 60)];
    assert_eq!(
        laid.window().input_region_updates(),
        vec![at.clone(), at.clone(), at],
        "None to Some and Some to None are both changes"
    );
}

pub(crate) fn fractional_region_edges_round_outward() {
    let mut laid = lay_out(
        window(GREY, 10.1, 0.0, excluded(0.2, 7.0)),
        tight(200.0, 200.0),
    );
    laid.set_scale_factor(1.5);
    // [15.15, 15.45] × [0, 10.5] covers one pixel column, never none.
    assert_eq!(
        laid.window().input_region_updates().last(),
        Some(&vec![device(15, 0, 16, 11)])
    );
}

/// A sibling region beside `content` at the origin, so a frame that reports only the
/// sibling was composited and dropped `content`'s region.
fn beside_a_sibling(content: impl IntoView) -> impl View {
    Stack::new(row![
        SizedBox::new(200.0, 200.0).child(ColoredBox::new(GREY)),
        Positioned::new(excluded(5.0, 5.0)).left(0.0).top(0.0),
        Positioned::new(content).left(100.0).top(100.0),
    ])
}

pub(crate) fn a_degenerate_transform_drops_the_region_with_a_diagnostic() {
    let overflowing = Transform::new(Matrix4::scaling(1e200, 1e-200, 1.0))
        .child(Transform::new(Matrix4::scaling(1e200, 1e-200, 1.0)).child(excluded(10.0, 10.0)));
    let (laid, log) =
        log_capture::capture(|| lay_out(beside_a_sibling(overflowing), tight(200.0, 200.0)));
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(0, 0, 5, 5)]]
    );
    assert_eq!(
        log.count_containing("input region dropped"),
        1,
        "{}",
        log.render_at_least(tracing::Level::WARN)
    );
}

pub(crate) fn a_region_crossing_the_perspective_horizon_is_dropped() {
    // w = 1 - 0.02·x: positive at the region's left edge, -1 at its right edge, while
    // every corner quotient stays finite.
    let mut perspective = Matrix4::IDENTITY;
    *perspective.get_mut(3, 0) = -0.02;
    let (laid, log) = log_capture::capture(|| {
        lay_out(
            beside_a_sibling(Transform::new(perspective).child(excluded(100.0, 10.0))),
            tight(200.0, 200.0),
        )
    });
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(0, 0, 5, 5)]]
    );
    assert_eq!(
        log.count_containing("perspective horizon"),
        1,
        "{}",
        log.render_at_least(tracing::Level::WARN)
    );
}

pub(crate) fn a_region_in_a_scrolled_list_item_reports_the_scrolled_rect() {
    use flui_view::ViewExt;
    use flui_widgets::ListView;
    let list = ListView::builder(20, 50.0, |index| {
        (index < 20).then(|| {
            if index == 3 {
                SystemGestureExclusion::new()
                    .child(SizedBox::new(200.0, 50.0))
                    .boxed()
            } else {
                SizedBox::new(200.0, 50.0).boxed()
            }
        })
    })
    .offset(120.0);
    let laid = lay_out(list, tight(200.0, 200.0));
    // Item 3 starts at 150 in the list; scrolled by 120 it sits at 30.
    assert_eq!(
        laid.window().input_region_updates(),
        vec![vec![device(0, 30, 200, 80)]]
    );
}
