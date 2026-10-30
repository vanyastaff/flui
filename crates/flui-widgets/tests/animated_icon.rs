//! `AnimatedIcon` and `AnimatedIconData`: checked data, the frame drawn at a progress (read
//! from the recorded path), the progress policy, right-to-left mirroring, the theme's size and
//! color, reduced motion, and semantics.

use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, loose};
use flui_animation::{AnimationController, ConstantAnimation, Cubic, MotionOverride, Vsync};
use flui_foundation::geometry::Point;
use flui_painting::display_list::DrawOp;
use flui_painting::paint::PathCommand;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_testing::a11y::Role;
use flui_widgets::{
    AnimatedIcon, AnimatedIconData, Directionality, IconCommand, IconCommandBuf, IconDataError,
    IconPath, IconPathBuf, IconTheme, IconThemeData, VsyncScope,
};

/// A bar that slides down: its corner moves from (4, 4) to (4, 14) over two frames, the second
/// half of the way eased.
const BAR: AnimatedIconData = AnimatedIconData::from_static(
    24.0,
    &[0.0, 0.5, 1.0],
    &[],
    &[IconPath::new(
        &[1.0, 1.0, 0.5],
        &[
            IconCommand::move_to(&[[4.0, 4.0], [4.0, 9.0], [4.0, 14.0]]),
            IconCommand::line_to(&[[20.0, 4.0], [20.0, 9.0], [20.0, 14.0]]),
            IconCommand::cubic_to(&[
                [20.0, 6.0, 20.0, 8.0, 20.0, 8.0],
                [20.0, 11.0, 20.0, 13.0, 20.0, 13.0],
                [20.0, 16.0, 20.0, 18.0, 20.0, 18.0],
            ]),
            IconCommand::line_to(&[[4.0, 8.0], [4.0, 13.0], [4.0, 18.0]]),
            IconCommand::close(),
        ],
    )],
    true,
);

fn mount(view: impl flui_view::View) -> LaidOut {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(VsyncScope::new(vsync.clone(), view), loose(200.0), vsync);
    laid.pump_for(Duration::ZERO);
    laid
}

/// The one path drawn: its points, and its color.
fn drawn(laid: &LaidOut) -> (Vec<Point<f64>>, Color) {
    let mut paths: Vec<_> = laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Path { path, paint } => Some((
                path.commands()
                    .flat_map(|command| match command {
                        PathCommand::MoveTo(p) | PathCommand::LineTo(p) => vec![p],
                        PathCommand::CubicTo(a, b, p) => vec![a, b, p],
                        PathCommand::QuadraticTo(a, p) => vec![a, p],
                        PathCommand::Close => Vec::new(),
                    })
                    .collect::<Vec<_>>(),
                paint.color,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1, "one path: {paths:?}");
    paths.pop().expect("one path")
}

fn icon_at(progress: f64) -> AnimatedIcon {
    AnimatedIcon::new(BAR, ConstantAnimation::new(progress))
}

/// Constant data passes its check, and run-time data is refused for each rule it breaks.
pub(crate) fn icon_data_is_checked() {
    assert_eq!(BAR.check(), Ok(()));
    let bar = |frames: Vec<[f64; 2]>| {
        IconPathBuf::new(
            vec![1.0; frames.len()],
            vec![IconCommandBuf::move_to(frames), IconCommandBuf::close()],
        )
    };
    let two = || bar(vec![[0.0, 0.0], [1.0, 1.0]]);
    let try_new = |times: Vec<f64>, paths: Vec<IconPathBuf>| {
        AnimatedIconData::try_new(24.0, times, Vec::new(), paths, false).err()
    };
    assert_eq!(try_new(vec![], vec![two()]), Some(IconDataError::Empty));
    assert_eq!(try_new(vec![0.0, 1.0], vec![]), Some(IconDataError::Empty));
    assert_eq!(
        try_new(vec![2.0, 3.0], vec![two()]),
        Some(IconDataError::Times),
        "a time line without 0 and 1"
    );
    assert_eq!(
        try_new(vec![0.0, 0.7, 0.5, 1.0], vec![bar(vec![[0.0, 0.0]; 4])]),
        Some(IconDataError::Times),
        "unordered times"
    );
    assert_eq!(
        try_new(vec![0.0, 0.5, 1.0], vec![two()]),
        Some(IconDataError::FrameCount { path: 0 }),
        "frames that do not match the times"
    );
    assert_eq!(
        try_new(vec![0.0, 1.0], vec![bar(vec![[0.0, f64::NAN], [1.0, 1.0]])]),
        Some(IconDataError::NonFinite { path: 0 })
    );
    assert_eq!(
        try_new(
            vec![0.0, 1.0],
            vec![IconPathBuf::new(
                vec![1.0, 1.5],
                vec![IconCommandBuf::move_to(vec![[0.0, 0.0]; 2])]
            )]
        ),
        Some(IconDataError::Opacity { path: 0 })
    );
    assert_eq!(
        try_new(
            vec![0.0, 1.0],
            vec![bar(vec![[-f64::MAX, 0.0], [f64::MAX, 0.0]])]
        ),
        None,
        "extreme but finite ends interpolate finitely through the middle"
    );
    assert_eq!(
        AnimatedIconData::try_new(
            24.0,
            vec![0.0, 1.0],
            vec![
                Cubic::new(0.4, 0.0, 0.2, 1.0),
                Cubic::new(0.4, 0.0, 0.2, 1.0)
            ],
            vec![two()],
            false
        )
        .err(),
        Some(IconDataError::Curves),
        "one curve per gap"
    );
    assert_eq!(
        AnimatedIconData::try_new(0.0, vec![0.0, 1.0], Vec::new(), vec![two()], false).err(),
        Some(IconDataError::ViewBox)
    );
}

/// The frame drawn at 0, 0.25, 0.5 and 1, scaled from the 24-unit view box to the icon's
/// 48 px; progress outside [0, 1] is clamped, and a progress that is not finite keeps the last
/// finite frame.
pub(crate) fn the_frame_follows_the_progress() {
    let corner = |laid: &LaidOut| drawn(laid).0[0];
    let at = |progress: f64| {
        let laid = mount(icon_at(progress).size(48.0));
        corner(&laid)
    };
    assert_eq!(at(0.0), Point::new(8.0, 8.0));
    assert_eq!(
        at(0.25),
        Point::new(8.0, 13.0),
        "a quarter of the way: half the first gap"
    );
    assert_eq!(at(0.5), Point::new(8.0, 18.0));
    assert_eq!(at(1.0), Point::new(8.0, 28.0));
    assert_eq!(at(1.5), at(1.0), "clamped above");
    assert_eq!(at(-1.0), at(0.0), "clamped below");
    assert_eq!(
        at(f64::NAN),
        at(0.0),
        "the first frame before any finite progress"
    );

    let controller = AnimationController::builder(Duration::from_millis(100)).build();
    controller.set_value(1.0).expect("no build is running");
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), AnimatedIcon::new(BAR, controller).size(48.0)),
        loose(200.0),
        vsync,
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(corner(&laid), Point::new(8.0, 28.0));
    let (_, color) = drawn(&laid);
    assert!(
        (color.opacity() - 0.5).abs() < 0.01,
        "the path's opacity at the last frame"
    );
}

/// The icon's size and color come from the ambient theme; data that matches the text
/// direction mirrors under right-to-left text.
pub(crate) fn theme_and_direction_shape_the_icon() {
    let red = Color::rgb(255, 0, 0);
    let laid = mount(IconTheme::new(
        IconThemeData {
            size: Some(24.0),
            color: Some(red),
            ..IconThemeData::default()
        },
        icon_at(0.0),
    ));
    let (points, color) = drawn(&laid);
    assert_eq!(points[0], Point::new(4.0, 4.0), "24 px: the view box as is");
    assert_eq!(color, red);

    let rtl = mount(Directionality::new(
        TextDirection::Rtl,
        icon_at(0.0).size(24.0),
    ));
    assert_eq!(
        drawn(&rtl).0[0],
        Point::new(20.0, 4.0),
        "mirrored about the centre"
    );
    const STILL: AnimatedIconData = AnimatedIconData::from_static(
        24.0,
        &[0.0, 1.0],
        &[],
        &[IconPath::new(
            &[1.0, 1.0],
            &[
                IconCommand::move_to(&[[4.0, 4.0], [4.0, 4.0]]),
                IconCommand::close(),
            ],
        )],
        false,
    );
    let unmirrored = mount(Directionality::new(
        TextDirection::Rtl,
        AnimatedIcon::new(STILL, ConstantAnimation::new(0.0)).size(24.0),
    ));
    assert_eq!(
        drawn(&unmirrored).0[0],
        Point::new(4.0, 4.0),
        "not mirrored"
    );
}

/// Under reduced motion a controller's run lands at its target on the next tick, and the icon
/// shows the target frame at once; without reduced motion it is part of the way.
pub(crate) fn reduced_motion_shows_the_target_frame() {
    let run = |reduce: bool| {
        let vsync = Vsync::new();
        let controller =
            AnimationController::builder(Duration::from_millis(400)).build_on(Some(&vsync));
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                AnimatedIcon::new(BAR, controller.controller().clone()).size(48.0),
            ),
            loose(200.0),
            vsync,
        );
        if reduce {
            laid.set_motion_override(MotionOverride::Reduce);
        }
        laid.pump_for(Duration::ZERO);
        let _ = controller.controller().forward();
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        let corner = drawn(&laid).0[0];
        drop(controller);
        corner
    };
    assert_eq!(run(true), Point::new(8.0, 28.0), "the target frame");
    let moving = run(false);
    assert!(
        moving.y > 8.0 && moving.y < 28.0,
        "part of the way: {moving:?}"
    );
}

/// The label lives on the icon as an image; the animation is not announced.
pub(crate) fn the_label_lives_on_the_icon() {
    let mut laid = mount(icon_at(0.5).semantic_label("Menu"));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Menu").expect("the label");
    assert_eq!(node.role(), Role::Image);
    assert_eq!(node.value(), None, "no progress is announced");
}
