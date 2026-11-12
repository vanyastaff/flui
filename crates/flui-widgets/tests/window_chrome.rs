//! An application's own title bar: the regions its widgets mark reach the window's backend,
//! the press-taking widgets in the bar are cut out of its move area, nothing is marked while
//! the system draws the title bar, and the caption buttons act on the window (ADR-0286).

use flui_foundation::geometry::{DevicePoint, DeviceRect};
use flui_painting::styling::Color;
use flui_platform::input_regions::{
    CaptionButton, ChromeHit, DeviceInputRegion, InputRegionKind, ResizeEdge, chrome_hit,
};
use flui_platform::{AppTitleBar, PlatformWindow as _, WindowCommand, WindowFrame, WindowMode};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_view::{EventCx, IntoView, View};
use flui_widgets::{
    ColoredBox, GestureDetector, MouseRegion, Positioned, SizedBox, Stack, WindowCaptionButton,
    WindowMovePassThrough, WindowResizeArea, WindowTitleBar,
};

use crate::common::{LaidOut, lay_out, tight};

fn device(
    kind: InputRegionKind,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> DeviceInputRegion {
    DeviceInputRegion::new(
        kind,
        DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
    )
}

fn button(width: f64) -> impl View {
    GestureDetector::new()
        .on_tap(|_ctx: &mut EventCx<'_>| {})
        .child(SizedBox::new(width, 20.0).child(ColoredBox::new(Color::rgb(0, 0, 200))))
}

/// A 200×32 bar with a tappable button at x 10, a hover-only area at x 40, a tappable
/// decoration that passes its presses through at x 70 and a close button at x 170, over a
/// resize band along the window's top edge.
fn bar() -> impl View {
    Stack::new((
        WindowTitleBar::new().child(Stack::new((
            SizedBox::new(200.0, 32.0).child(ColoredBox::new(Color::rgb(230, 230, 230))),
            Positioned::new(button(20.0)).left(10.0).top(6.0),
            Positioned::new(MouseRegion::new().child(SizedBox::new(20.0, 20.0)))
                .left(40.0)
                .top(6.0),
            Positioned::new(WindowMovePassThrough::new().child(button(20.0)))
                .left(70.0)
                .top(6.0),
            Positioned::new(WindowCaptionButton::close(SizedBox::new(20.0, 20.0)))
                .left(170.0)
                .top(6.0),
        ))),
        Positioned::new(WindowResizeArea::new(ResizeEdge::Top).child(SizedBox::new(200.0, 4.0)))
            .left(0.0)
            .top(0.0),
    ))
}

fn with_frame(frame: WindowFrame) -> LaidOut {
    let mut laid = lay_out(SizedBox::new(200.0, 200.0), tight(200.0, 200.0));
    laid.window()
        .admit_and_apply(WindowCommand::SetFrame(frame))
        .expect("the headless window takes every frame");
    laid.pump();
    laid.pump_widget(bar());
    laid.pump();
    laid
}

fn last_sent(laid: &LaidOut) -> Vec<DeviceInputRegion> {
    laid.window()
        .input_region_updates()
        .last()
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn a_title_bar_moves_the_window_except_where_its_widgets_take_presses() {
    let laid = with_frame(WindowFrame::AppTitleBar(AppTitleBar::new()));
    let sent = last_sent(&laid);
    assert_eq!(
        sent,
        vec![
            device(InputRegionKind::WindowMove, 0, 0, 200, 32),
            device(InputRegionKind::WindowContent, 10, 6, 30, 26),
            device(
                InputRegionKind::CaptionButton(CaptionButton::Close),
                170,
                6,
                190,
                26
            ),
            device(InputRegionKind::WindowResize(ResizeEdge::Top), 0, 0, 200, 4),
        ]
    );
    let hit = |x, y| chrome_hit(&sent, DevicePoint::new(x, y));
    assert_eq!(hit(5, 20), ChromeHit::Move, "the bar's background");
    assert_eq!(hit(20, 20), ChromeHit::Content, "a tappable button");
    assert_eq!(hit(50, 20), ChromeHit::Move, "a hover-only area");
    assert_eq!(hit(80, 20), ChromeHit::Move, "a pass-through decoration");
    assert_eq!(
        hit(180, 20),
        ChromeHit::CaptionButton(CaptionButton::Close),
        "the close button"
    );
    assert_eq!(hit(100, 2), ChromeHit::Resize(ResizeEdge::Top), "the band");
}

pub(crate) fn nothing_is_marked_while_the_system_draws_the_title_bar() {
    let laid = with_frame(WindowFrame::System);
    assert_eq!(last_sent(&laid), Vec::new());
}

pub(crate) fn a_frame_change_marks_and_unmarks_the_bar() {
    let mut laid = with_frame(WindowFrame::System);
    laid.window()
        .admit_and_apply(WindowCommand::SetFrame(WindowFrame::None))
        .expect("the headless window takes every frame");
    laid.pump();
    laid.pump();
    assert_eq!(last_sent(&laid).len(), 4, "{:?}", last_sent(&laid));

    laid.window()
        .admit_and_apply(WindowCommand::SetFrame(WindowFrame::System))
        .expect("the headless window takes every frame");
    laid.pump();
    laid.pump();
    assert_eq!(last_sent(&laid), Vec::new());
}

pub(crate) fn a_maximized_window_is_not_resized_from_its_border() {
    let mut laid = with_frame(WindowFrame::None);
    laid.window()
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
        .expect("the headless window maximizes");
    laid.pump();
    laid.pump();
    assert!(
        !last_sent(&laid)
            .iter()
            .any(|region| matches!(region.kind(), InputRegionKind::WindowResize(_))),
        "{:?}",
        last_sent(&laid)
    );
}

fn caption(button: impl IntoView) -> impl View {
    Stack::new((
        SizedBox::new(200.0, 200.0),
        Positioned::new(button).left(0.0).top(0.0),
    ))
}

pub(crate) fn the_maximize_button_maximizes_and_restores() {
    let mut laid = lay_out(
        caption(WindowCaptionButton::maximize(SizedBox::new(20.0, 20.0))),
        tight(200.0, 200.0),
    );
    tap(&mut laid);
    laid.pump();
    assert_eq!(laid.window().state().mode, WindowMode::Maximized);
    tap(&mut laid);
    laid.pump();
    assert_eq!(laid.window().state().mode, WindowMode::Normal);
}

pub(crate) fn the_minimize_button_minimizes() {
    let mut laid = lay_out(
        caption(WindowCaptionButton::minimize(SizedBox::new(20.0, 20.0))),
        tight(200.0, 200.0),
    );
    tap(&mut laid);
    laid.pump();
    assert_eq!(laid.window().state().mode, WindowMode::Minimized);
}

pub(crate) fn the_close_button_asks_to_close() {
    let mut laid = lay_out(
        caption(WindowCaptionButton::close(SizedBox::new(20.0, 20.0))),
        tight(200.0, 200.0),
    );
    tap(&mut laid);
    laid.pump();
    assert_eq!(laid.window().close_requests(), 1);
}

pub(crate) fn caption_buttons_are_buttons_named_after_what_they_do() {
    let mut laid = lay_out(
        caption(WindowCaptionButton::maximize(SizedBox::new(20.0, 20.0))),
        tight(200.0, 200.0),
    );
    laid.enable_semantics();
    laid.pump();
    let named = |laid: &LaidOut, label: &str| {
        laid.a11y_tree()
            .expect("semantics enabled")
            .find_all_by_label(label)
            .len()
    };
    assert_eq!(named(&laid, "Maximize"), 1);
    tap(&mut laid);
    laid.pump();
    laid.pump();
    assert_eq!(named(&laid, "Restore"), 1);
}

/// A primary-button tap on the caption button at the window's top-left corner, then a frame.
fn tap(laid: &mut LaidOut) {
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    laid.tick();
}
