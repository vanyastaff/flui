//! What only the in-process connection has (ADR-0400 §4): a frame read through types, a
//! `Gesture` sent as it was built, a host with no agent, and a measured window over the tester's
//! own steps.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_interaction::pointer::HitTestBehavior;
use flui_objects::{RenderConstrainedBox, RenderPadding, RenderParagraph};
use flui_painting::{DrawOp, paint::Paint, styling::Color};
use flui_protocol::geometry::{LogicalPoint, LogicalRect};
use flui_rendering::delegates::{CustomPaintContext, CustomPainter};
use flui_semantics::SemanticsRole;
use flui_testing::FrameReport;
use flui_testing::perf::measure;
use flui_testing::scenario::gesture::{Gesture, GestureError, Pointer};
use flui_testing::scenario::{TestError, TestWindow, Tester, find};
use flui_testing::widgets::{SharedSignal, lay_out, tight};
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{
    Center, Column, CustomPaint, GestureDetector, Opacity, Padding, Row, Semantics, SizedBox, Text,
    Transform,
};

/// A group the test finds by `id`, around `child`.
fn group(id: &str, child: impl IntoView) -> BoxedView {
    Semantics::new()
        .container(true)
        .role(SemanticsRole::Group)
        .identifier(id)
        .child(child)
        .into_view()
        .boxed()
}

/// `rect` in the window's logical pixels, as the protocol reports one.
fn logical(rect: Rect) -> LogicalRect {
    LogicalRect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
}

/// A typed read at an element selects the render object of its type at or under it, and copies
/// its geometry out: its size, and its bounds in the window, which are the bounds the agent
/// reports for the element.
///
/// Fails if the selection starts above the element or below it, if the offset in the parent is
/// lost, if the bounds are not mapped into the window, or if the size is read off another node.
#[test]
fn a_typed_read_copies_the_geometry_of_the_render_object_under_the_element() {
    let mut t = Tester::mount(Center::new().child(group(
        "pad",
        Padding::all(10.0).child(SizedBox::new(40.0, 30.0)),
    )));
    let shown = t.rect(&find::id("pad"));
    let mut frame = t.frame();
    let padding = frame
        .geometry::<RenderPadding>(&find::id("pad"))
        .expect("one padding under the group");
    assert_eq!(padding.size, Some(Size::new(60.0, 50.0)));
    let bounds = padding.rect().expect("a laid-out box has bounds");
    assert_eq!(logical(bounds), shown);
    let child = frame
        .geometry::<RenderConstrainedBox>(&find::id("pad"))
        .expect("one sized box under the group");
    assert_eq!(child.size, Some(Size::new(40.0, 30.0)));
    assert_eq!(
        child.offset,
        Offset::new(10.0, 10.0),
        "the child sits past the padding in its parent"
    );
    assert_eq!(
        child.rect().map(logical),
        Some(LogicalRect::new(shown.x + 10.0, shown.y + 10.0, 40.0, 30.0)),
        "the child's bounds are placed in the window, past the padding"
    );
    t.finish();
}

/// A read of one render object names how many it found where it found another number: none at
/// the element, several in the frame; a read of all of them lists each in tree order.
///
/// Fails if a read of one takes the first of several, or passes on none, or if a read at an
/// element reads past it: the frame holds a padding, and none is at the element.
#[test]
fn a_typed_read_of_one_names_how_many_it_found() {
    let mut t = Tester::mount(Center::new().child(Column::new((
        Text::new("one"),
        group("two", Text::new("two")),
        Padding::all(4.0).child(Text::new("three")),
    ))));
    let mut frame = t.frame();
    assert_eq!(frame.render_all::<RenderParagraph>().len(), 3);
    let several = frame.only::<RenderParagraph>();
    assert!(
        matches!(
            several,
            Err(TestError::RenderObjectCount {
                found: 3,
                find: None,
                ..
            })
        ),
        "{several:?}"
    );
    drop(several);
    let none = frame.geometry::<RenderPadding>(&find::id("two"));
    assert!(
        matches!(
            none,
            Err(TestError::RenderObjectCount {
                found: 0,
                find: Some(_),
                ..
            })
        ),
        "{none:?}"
    );
    drop(none);
    t.finish();
}

/// `Frame::read` hands the one render object of its type to a function and returns the value it
/// copies out, here the text a paragraph lays out.
///
/// Fails if the read is handed another paragraph than the one under the element.
#[test]
fn a_read_copies_a_field_of_the_render_object_under_the_element() {
    let mut t = Tester::mount(Center::new().child(Row::new((
        group("first", Text::new("left")),
        group("second", Text::new("right")),
    ))));
    let shown = t
        .frame()
        .read::<RenderParagraph, String>(&find::id("second"), |paragraph| {
            paragraph
                .painter()
                .text()
                .map_or_default(flui_painting::typography::InlineSpan::to_plain_text)
        })
        .expect("one paragraph under the group");
    assert_eq!(shown, "right");
    t.finish();
}

/// Paints a rectangle of its colour over its whole size.
#[derive(Debug)]
struct Swatch(Color);

impl CustomPainter for Swatch {
    fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
        let size = ctx.size();
        ctx.canvas().draw_rect(
            Rect::from_origin_size(Point::ZERO, size),
            &Paint::fill(self.0),
        );
    }

    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false
    }

    fn repaint(&self) -> Option<Rc<dyn flui_foundation::Listenable>> {
        None
    }
}

/// The colours of the rectangles in `commands`, in paint order.
fn colors<'a>(commands: impl IntoIterator<Item = &'a flui_painting::DrawCommandBuf>) -> Vec<Color> {
    commands
        .into_iter()
        .filter_map(|command| match &command.op {
            DrawOp::Rect { paint, .. } => Some(paint.color),
            _ => None,
        })
        .collect()
}

/// The draw commands at an element are what it and the render objects under it painted, a
/// sibling's left out; the frame's draw list holds both, and the layers outline the scene.
///
/// Fails if the selection reads the whole scene, or misses the painter under the element.
#[test]
fn draw_commands_read_the_elements_painting_and_leave_a_siblings_out() {
    let red = Color::rgb(200, 0, 0);
    let blue = Color::rgb(0, 0, 200);
    let mut t = Tester::mount(
        Center::new().child(Row::new((
            group(
                "red",
                CustomPaint::new()
                    .painter(Swatch(red))
                    .size(Size::new(20.0, 20.0)),
            ),
            group(
                "blue",
                CustomPaint::new()
                    .painter(Swatch(blue))
                    .size(Size::new(20.0, 20.0)),
            ),
        ))),
    );
    let mut frame = t.frame();
    let at_blue = frame
        .draw_commands(&find::id("blue"))
        .expect("the group is a render object's");
    assert_eq!(colors(&at_blue), [blue]);
    let whole = frame.draw_list();
    assert_eq!(colors(&whole), [red, blue]);
    let layers = frame.layers();
    assert!(
        layers.kinds().any(|kind| kind == "Picture"),
        "the scene's outline:\n{layers}"
    );
    t.finish();
}

/// A long press held for a stated time: the long-press deadline runs on the test's clock, so a
/// press released before it is no long press and one held past it is.
///
/// Fails if a gesture's hold is not played as a pause between the press and the release.
#[test]
fn a_gesture_held_past_the_long_press_delay_is_a_long_press_and_one_held_short_is_not() {
    let fired = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&fired);
    let mut t = Tester::mount(
        Center::new().child(group(
            "press",
            GestureDetector::new()
                .on_long_press(move |_ctx| counted.set(counted.get() + 1))
                .behavior(HitTestBehavior::Opaque)
                .child(SizedBox::new(40.0, 30.0)),
        )),
    );
    let at = t.rect(&find::id("press")).center();
    let press = |hold| {
        Gesture::new()
            .pointer(Pointer::touch(at).down().hold(hold).up())
            .build()
    };
    t.perform(&press(Duration::from_millis(300)));
    assert_eq!(
        fired.get(),
        0,
        "a press released at 300 ms is no long press"
    );
    t.perform(&press(Duration::from_millis(550)));
    assert_eq!(fired.get(), 1, "a press held for 550 ms is a long press");
    t.finish();
}

/// A gesture refuses a press it leaves down and a release of a press it did not make, so no
/// contact stays with the window's input lease past the step; a key may stay down. A sequence
/// the protocol's plan refuses is refused by the builder, before any path plays it.
#[test]
fn a_gesture_refuses_an_unpaired_press_or_release() {
    let at = LogicalPoint::new(10.0, 10.0);
    assert!(matches!(
        Gesture::new()
            .pointer(
                Pointer::touch(at)
                    .down()
                    .move_by(5.0, 0.0, Duration::from_millis(32))
            )
            .try_build(),
        Err(GestureError::LeftDown { pointer: 0, .. })
    ));
    assert!(matches!(
        Gesture::new()
            .pointer(Pointer::mouse(at).down().up())
            .pointer(Pointer::mouse(at).up())
            .try_build(),
        Err(GestureError::NotPressed { pointer: 1, .. })
    ));
    assert!(
        Gesture::new()
            .pointer(Pointer::touch(at).down().cancel())
            .try_build()
            .is_ok(),
        "a cancel ends the contact"
    );
    let too_many = (0..=flui_protocol::input::MAX_SOURCES)
        .fold(Gesture::new(), |gesture, _| {
            gesture.pointer(Pointer::touch(at).down().up())
        })
        .try_build();
    assert!(
        matches!(too_many, Err(GestureError::Plan(_))),
        "a sequence the protocol's plan refuses is refused when it is built: {too_many:?}"
    );
}

/// Shows the number a test sets, as text.
#[derive(Clone, StatefulView)]
struct Shown {
    number: SharedSignal<u32>,
}

struct ShownState {
    number: SharedSignal<u32>,
}

impl StatefulView for Shown {
    type State = ShownState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ShownState {
            number: self.number.clone(),
        }
    }
}

impl ViewState<Shown> for ShownState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.number.bind(ctx);
    }

    fn build(&self, _view: &Shown, ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.number.get(ctx).to_string())
    }
}

/// What one changed frame of `Shown` costs on the widget harness: the frame's report, counted
/// inside a perf window as the perf scenarios count it. It is built on `lay_out`, the form
/// "Current forms" retires, on purpose: the perf scenarios that hold baselines still run there,
/// and this is the count their move to `Tester::measure` must keep.
fn harness_report() -> FrameReport {
    let number = SharedSignal::new(0);
    let mut laid = lay_out(
        Shown {
            number: number.clone(),
        },
        tight(800.0, 600.0),
    );
    let _idle = laid.pump_report(flui_testing::scenario::FRAME);
    number.set(1);
    let (report, _cost) = measure(|| laid.pump_report(flui_testing::scenario::FRAME));
    report
}

/// A tester with no agent measures what the widget harness's `pump_report` measures: the same
/// change of the same tree costs the same counts in each, the perf files' keys (ADR-0396), so a
/// scenario with no agent keeps its baseline when it moves to `Tester::measure`.
///
/// Fails if the frame's report is not kept for the pump the window measured, or if the tester's
/// pump does other work than the harness's frame.
#[test]
fn measure_unobserved_matches_pump_report() {
    let expected = harness_report();
    assert!(
        expected.build.builds_run > 0 && expected.pipeline.nodes_laid_out > 0,
        "the change rebuilds and lays out: {expected:?}"
    );

    let number = SharedSignal::new(0);
    let mut t = Tester::mount_unobserved(
        Shown {
            number: number.clone(),
        },
        TestWindow::default(),
    );
    t.pump();
    number.set(1);
    let _cost = t.measure(|t| t.pump());
    let measured = t.frame().report().clone();
    assert_eq!(measured.counters(), expected.counters());
    t.finish();
}

/// Keeps the handle of the window it is mounted in, so the test closes the window as the
/// application would.
#[derive(Clone, StatefulView)]
struct WindowKeeper(Rc<std::cell::RefCell<Option<flui_view::WindowHandle>>>);

struct WindowKept;

impl StatefulView for WindowKeeper {
    type State = WindowKept;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        *self.0.borrow_mut() = ctx.window();
        WindowKept
    }
}

impl ViewState<WindowKeeper> for WindowKept {
    fn build(&self, _view: &WindowKeeper, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("open")
    }
}

/// A pump after the application closed its last window runs no frame and reports none: the
/// frame in which the window closes and every pump after it return, they do not panic.
///
/// Fails if a pump reads the retired runtime, or if the report of a frame with no window keeps
/// an earlier frame's counts.
#[test]
fn a_pump_after_the_last_window_closed_runs_no_frame_and_reports_none() {
    let kept = Rc::new(std::cell::RefCell::new(None));
    let mut t = Tester::mount(WindowKeeper(Rc::clone(&kept)));
    t.pump();
    let window = kept
        .borrow()
        .clone()
        .expect("the view was built in a window");
    window.request_close().expect("the window is open");
    t.pump();
    t.pump_for(Duration::from_millis(50));
    assert_eq!(t.frame().report(), &FrameReport::default());
    t.finish_unchecked("the application closed its window");
}

/// Each draw command is placed in the window through every layer above it: an opacity layer and
/// the padding inside it offset one swatch, a transform layer scales the other, and each
/// command's rectangle, carried by its transform, covers the bounds the agent reports for its
/// element. The scene's outline names those layers, each under the one that holds it.
///
/// Fails if a command keeps the space of its layer, if an offset or a transform above it is
/// dropped or applied twice, or if the outline loses a layer or its depth.
#[test]
fn draw_commands_are_placed_in_the_window_through_the_layers_above_them() {
    let red = Color::rgb(200, 0, 0);
    let blue = Color::rgb(0, 0, 200);
    let swatch = |color| {
        CustomPaint::new()
            .painter(Swatch(color))
            .size(Size::new(20.0, 20.0))
    };
    let mut t = Tester::mount(
        Center::new().child(Column::new((
            Opacity::new(0.5)
                .child(Padding::only(30.0, 0.0, 0.0, 0.0).child(group("faded", swatch(red)))),
            Transform::scale(2.0, 2.0).child(group("scaled", swatch(blue))),
        ))),
    );
    let faded = t.rect(&find::id("faded"));
    let scaled = t.rect(&find::id("scaled"));
    assert_eq!(
        (scaled.width, scaled.height),
        (40.0, 40.0),
        "the agent reports the scaled bounds"
    );
    let mut frame = t.frame();
    for (id, shown) in [("faded", faded), ("scaled", scaled)] {
        let drawn = frame
            .draw_commands(&find::id(id))
            .expect("the group is a render object's");
        let rects: Vec<_> = drawn
            .iter()
            .filter_map(|command| match &command.op {
                DrawOp::Rect { rect, .. } => Some((command, rect)),
                _ => None,
            })
            .collect();
        let [(command, rect)] = rects[..] else {
            panic!("one rectangle under {id}: {drawn:?}");
        };
        assert_eq!(
            logical(command.transform.transform_rect(rect)),
            shown,
            "{id}'s rectangle is placed where the window shows its element"
        );
    }
    let layers = frame.layers();
    let outline: Vec<(usize, &str)> = layers.iter().collect();
    let depth_of = |kind: &str| {
        outline.iter().find(|(_, k)| *k == kind).map_or_else(
            || panic!("the outline has a {kind} layer:\n{layers}"),
            |(depth, _)| *depth,
        )
    };
    let pictures: Vec<usize> = outline
        .iter()
        .filter(|(_, kind)| *kind == "Picture")
        .map(|(depth, _)| *depth)
        .collect();
    assert!(
        pictures.len() >= 2
            && pictures
                .iter()
                .all(|depth| *depth > depth_of("Opacity").min(depth_of("Transform"))),
        "each swatch paints into a picture under its opacity or transform layer:\n{layers}"
    );
    t.finish();
}

/// Two contacts at once: the gesture plays both pointers' presses in the same tick, and each
/// lands on the detector under its own point.
///
/// Fails if the second pointer of a gesture is dropped, or if both play at the first one's point.
#[test]
fn a_gesture_with_two_contacts_taps_under_each_of_them() {
    let taps = Rc::new([Cell::new(0_u32), Cell::new(0_u32)]);
    let detector = |index: usize, id: &str| {
        let taps = Rc::clone(&taps);
        group(
            id,
            GestureDetector::new()
                .on_tap(move |_ctx| taps[index].set(taps[index].get() + 1))
                .behavior(HitTestBehavior::Opaque)
                .child(Text::new(id.to_owned())),
        )
    };
    let mut t = Tester::mount(Center::new().child(Row::new((
        detector(0, "left"),
        SizedBox::new(40.0, 30.0),
        detector(1, "right"),
    ))));
    let left = t.rect(&find::id("left")).center();
    let right = t.rect(&find::id("right")).center();
    t.perform(
        &Gesture::new()
            .pointer(Pointer::touch(left).down().up())
            .pointer(Pointer::touch(right).down().up())
            .build(),
    );
    assert_eq!([taps[0].get(), taps[1].get()], [1, 1]);
    t.finish();
}
