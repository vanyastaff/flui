//! `SelectionArea` by touch: a long press selects a word and shows a handle
//! under each end, dragging a handle moves its end, and a tap, Esc or a mouse
//! selection hides them. The handles live in the app's overlay and follow
//! their ends through layer links, so every touch below lands where the
//! handle is painted.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_painting::typography::TextDirection;
use flui_platform::EventTime;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove,
    PointerPosition, PointerPress, PointerRelease, PointerSample,
};
use flui_testing::replay::{MouseScript, PointerScript};
use flui_view::{BoxedView, BuildContext, ViewExt};
use flui_widgets::{
    Column, DefaultSelectionHandles, Directionality, HandleInfo, HandleSide, SelectionArea,
    SelectionHandles, SizedBox, Text, WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

const WORDS: &str = "alpha beta gamma";

/// What `on_selection_changed` heard, in order.
type Heard = Rc<RefCell<Vec<Option<String>>>>;

/// Handles that note each one they build, as a square.
#[derive(Clone, Default)]
struct Marked(Rc<RefCell<Vec<HandleInfo>>>);

impl SelectionHandles for Marked {
    fn build(&self, info: &HandleInfo, _ctx: &dyn BuildContext) -> BoxedView {
        self.0.borrow_mut().push(*info);
        SizedBox::new(Scene::HANDLE, Scene::HANDLE).boxed()
    }
}

struct Scene {
    laid: LaidOut,
    heard: Heard,
}

impl Scene {
    /// The side of the square handles, as wide as the default dots.
    const HANDLE: f64 = 22.0;

    /// An area over [`WORDS`] in an app with an overlay, its handles
    /// built by `handles`.
    fn over(handles: Option<Marked>) -> Self {
        let heard: Heard = Rc::default();
        let area = Self::area(&heard, handles);
        Self::laid_out(WidgetsApp::new(area), heard)
    }

    fn laid_out(app: WidgetsApp, heard: Heard) -> Self {
        let mut laid = lay_out(app, tight(400.0, 300.0));
        laid.pump();
        Self { laid, heard }
    }

    fn area(heard: &Heard, handles: Option<Marked>) -> SelectionArea {
        let recorded = Rc::clone(heard);
        let area = SelectionArea::new(Column::new((Text::new(WORDS),))).on_selection_changed(
            move |_cx, selected| {
                recorded
                    .borrow_mut()
                    .push(selected.map(|selected| selected.text.clone()));
            },
        );
        match handles {
            Some(handles) => area.handles(handles),
            None => area,
        }
    }

    /// The text's paragraph.
    fn paragraph(&self) -> flui_foundation::RenderId {
        self.laid.find_all_by_render_type("RenderParagraph")[0]
    }

    /// The point `width` along the text's line, at the line's middle.
    fn along(&self, width: f64) -> Offset<f64> {
        let paragraph = self.paragraph();
        let height = self.laid.size(paragraph).height;
        self.laid.absolute_offset(paragraph) + Offset::new(width, height / 2.0)
    }

    /// The middle of the handle under the end `width` along the line: its
    /// top centre is at the bottom of the line.
    fn handle_at(&self, width: f64) -> Offset<f64> {
        let paragraph = self.paragraph();
        let height = self.laid.size(paragraph).height;
        self.laid.absolute_offset(paragraph) + Offset::new(width, height + Self::HANDLE / 2.0)
    }

    /// The handles in the overlay.
    fn handles(&self) -> usize {
        self.laid
            .find_all_by_render_type("RenderFollowerLayer")
            .len()
    }

    fn selected(&self) -> Option<String> {
        self.heard.borrow().last().cloned().flatten()
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }

    fn long_press(&mut self, at: Offset<f64>) {
        self.laid
            .replay(&PointerScript::long_press(at, Duration::from_millis(800)));
        self.settle();
    }

    fn touch_drag(&mut self, from: Offset<f64>, to: Offset<f64>) {
        self.laid.replay(&PointerScript::drag(
            from,
            to,
            12,
            Duration::from_millis(16),
        ));
        self.settle();
    }

    fn escape(&mut self) {
        let event: KeyEvent = KeyEventBuilder::new(Code::Escape)
            .with_key(Key::Named(NamedKey::Escape))
            .with_state(KeyState::Down)
            .with_modifiers(Modifiers::NONE)
            .build();
        let _handled = self.laid.focus_manager().dispatch_key_event(&event);
        self.settle();
    }

    /// A primary mouse click at `at` with Shift held.
    fn shift_click(&mut self, at: Offset<f64>) {
        let mouse = PointerInfo::new(
            PointerId::try_from(77_u64).expect("a nonzero id"),
            PointerKind::Mouse,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(0),
            PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite"),
        );
        let down = PointerEvent::Down(
            PointerPress::new(mouse, PointerButton::PRIMARY, PointerButtons::NONE, sample)
                .with_modifiers(Modifiers::SHIFT),
        );
        let up = PointerEvent::Up(PointerRelease::new(
            mouse,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample,
        ));
        self.laid.dispatch_pointer_event(&down);
        self.laid.dispatch_pointer_event(&up);
        self.settle();
    }

    /// One step of a touch contact by hand, `millis` into it, for a test
    /// whose release lands where no move reported.
    fn touch(&mut self, step: Touch, millis: u64, at: Offset<f64>) {
        let finger = PointerInfo::new(
            PointerId::try_from(79_u64).expect("a nonzero id"),
            PointerKind::Touch,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite"),
        );
        let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
        let event = match step {
            Touch::Down => PointerEvent::Down(PointerPress::new(
                finger,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            Touch::Move => PointerEvent::Move(PointerMove::new(finger, held, sample)),
            Touch::Up => PointerEvent::Up(PointerRelease::new(
                finger,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
        };
        self.laid.dispatch_pointer_event(&event);
    }
}

/// The laid-out width of `words` in the default style, the x of the caret
/// after them in [`WORDS`].
fn width_of(words: &str) -> f64 {
    let measured = Scene::laid_out(
        WidgetsApp::new(Column::new((Text::new(words),))),
        Rc::default(),
    );
    measured
        .laid
        .size(measured.laid.find_all_by_render_type("RenderParagraph")[0])
        .width
}

/// A touch long press selects the word under it and shows a handle under
/// each end, built for its side, in the area's direction and its line's
/// height; the paragraph publishes both ends.
///
/// Red-check: never show the handles after a long press (no handle is in
/// the overlay).
pub(crate) fn a_long_press_selects_a_word_and_shows_a_handle_under_each_end() {
    let marked = Marked::default();
    let mut scene = Scene::over(Some(marked.clone()));
    let beta = width_of("alpha be");
    scene.long_press(scene.along(beta));
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    assert_eq!(scene.handles(), 2);
    let built = marked.0.borrow().clone();
    let last = &built[built.len() - 2..];
    assert_eq!(
        last.iter().map(|info| info.side).collect::<Vec<_>>(),
        [HandleSide::Start, HandleSide::End]
    );
    let line = scene.laid.size(scene.paragraph()).height;
    for info in last {
        assert_eq!(info.text_direction, TextDirection::Ltr);
        assert!((info.line_height - line).abs() < 0.5, "{info:?} on {line}");
    }
    let published = scene
        .laid
        .render_property(scene.paragraph(), "selection_handles")
        .expect("the paragraph reports its handles");
    assert!(!published.contains("None"), "{published}");
}

/// Dragging a handle moves its end and leaves the other; an end never
/// passes the other.
///
/// Red-check: leave the handle's box deferring to its child (the square
/// paints nothing that takes a touch, so the drag reaches nothing); move
/// both ends with either handle (the end leaves no "beta").
pub(crate) fn dragging_a_handle_moves_its_end() {
    let mut scene = Scene::over(Some(Marked::default()));
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));

    let end = width_of("alpha beta");
    let past = width_of(WORDS) + 10.0;
    scene.touch_drag(
        scene.handle_at(end),
        scene.handle_at(end) + Offset::new(past - end, 0.0),
    );
    assert_eq!(scene.selected().as_deref(), Some("beta gamma"));

    let start = width_of("alpha ");
    scene.touch_drag(
        scene.handle_at(start),
        scene.handle_at(start) - Offset::new(start + 10.0, 0.0),
    );
    assert_eq!(scene.selected().as_deref(), Some(WORDS));

    // The start dragged past the end stops short of it.
    let start = 0.0;
    scene.touch_drag(
        scene.handle_at(start),
        scene.handle_at(start) + Offset::new(past, 0.0),
    );
    let kept = scene.selected().expect("still selected");
    assert!(!kept.is_empty() && WORDS.ends_with(&kept), "{kept:?}");
    assert_eq!(scene.handles(), 2);
}

/// A tap, Esc or a mouse selection hides the handles, a Shift+click too,
/// though it keeps a selection.
///
/// Red-check: keep the handles when the selection clears (Esc leaves them
/// in the overlay); keep them for a tap that selects (the Shift+click); keep
/// them for a mouse drag.
pub(crate) fn a_tap_esc_or_the_mouse_hides_the_handles() {
    let mut scene = Scene::over(None);
    let beta = scene.along(width_of("alpha be"));

    scene.long_press(beta);
    assert_eq!(scene.handles(), 2);
    scene.laid.replay(&PointerScript::tap(beta));
    scene.settle();
    assert_eq!((scene.selected(), scene.handles()), (None, 0), "a tap");

    scene.long_press(beta);
    assert_eq!(scene.handles(), 2);
    scene.shift_click(scene.along(width_of(WORDS) - 1.0));
    assert_eq!(
        (scene.selected().as_deref(), scene.handles()),
        (Some("beta gamma"), 0),
        "a Shift+click extends without handles"
    );

    scene.long_press(beta);
    assert_eq!(scene.handles(), 2);
    scene.escape();
    assert_eq!((scene.selected(), scene.handles()), (None, 0), "Esc");

    scene.long_press(beta);
    assert_eq!(scene.handles(), 2);
    let script = MouseScript::new("drag")
        .press(scene.along(1.0))
        .wait(Duration::from_millis(16))
        .move_to(scene.along(width_of("alpha")))
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("the drag releases");
    scene.laid.replay(&script);
    scene.settle();
    assert!(scene.selected().is_some(), "the mouse selected");
    assert_eq!(scene.handles(), 0, "a mouse selection");
}

/// The handles are the area's own, else the nearest default's, else dots;
/// without an overlay there are none, and the long press still selects.
///
/// Red-check: skip the default (the area without handles of its own builds
/// dots); take the default over the area's own.
pub(crate) fn handles_come_from_the_area_then_the_default_and_need_an_overlay() {
    let (own, default) = (Marked::default(), Marked::default());
    let app = |own: Option<Marked>, heard: &Heard| {
        let area = Scene::area(heard, own);
        let default = default.clone();
        WidgetsApp::new(DefaultSelectionHandles::new(default, area))
    };
    let at = width_of("alpha be");

    let heard: Heard = Rc::default();
    let mut scene = Scene::laid_out(app(Some(own.clone()), &heard), heard);
    scene.long_press(scene.along(at));
    assert_eq!(scene.handles(), 2);
    assert!(!own.0.borrow().is_empty() && default.0.borrow().is_empty());

    let heard: Heard = Rc::default();
    let mut scene = Scene::laid_out(app(None, &heard), heard);
    scene.long_press(scene.along(at));
    assert_eq!(scene.handles(), 2);
    assert!(!default.0.borrow().is_empty(), "the default builds them");

    let heard: Heard = Rc::default();
    let area = Scene::area(&heard, None);
    let mut scene = Scene::laid_out(
        WidgetsApp::with_builder(move |_ctx, _routes| area.clone().boxed()),
        heard,
    );
    scene.long_press(scene.along(at));
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    assert_eq!(scene.handles(), 0, "no overlay, no handles");
}

/// A listener that panics while a long press selects still leaves the word
/// selected with its handles shown.
///
/// Red-check: show the handles after the selection's failure is resumed
/// (no handle is in the overlay).
pub(crate) fn a_panicking_listener_still_shows_the_handles() {
    let heard: Heard = Rc::default();
    let recorded = Rc::clone(&heard);
    let area = SelectionArea::new(Column::new((Text::new(WORDS),)))
        .on_selection_changed(move |_cx, selected| {
            recorded
                .borrow_mut()
                .push(selected.map(|selected| selected.text.clone()));
            assert!(recorded.borrow().is_empty(), "the listener fails");
        })
        .handles(Marked::default());
    let mut scene = Scene::laid_out(WidgetsApp::new(area), heard);
    let at = scene.along(width_of("alpha be"));
    // Wherever the gesture layer contains the panic, the area has done its
    // own work first.
    let _contained =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.long_press(at)));
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    assert_eq!(scene.handles(), 2);
}

/// Shown handles rebuild when the area is given other handles.
///
/// Red-check: store the new look without rebuilding the shown handles (the
/// second handles never build).
pub(crate) fn shown_handles_rebuild_with_new_handles() {
    let (first, second) = (Marked::default(), Marked::default());
    let heard: Heard = Rc::default();
    let mut scene = Scene::laid_out(
        WidgetsApp::new(Scene::area(&heard, Some(first))),
        Rc::clone(&heard),
    );
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.handles(), 2);
    assert!(second.0.borrow().is_empty());

    scene
        .laid
        .pump_widget(WidgetsApp::new(Scene::area(&heard, Some(second.clone()))));
    scene.settle();
    assert_eq!(scene.handles(), 2);
    assert_eq!(second.0.borrow().len(), 2, "both handles rebuilt");
}

/// A start dragged to the end of the text before it selects nothing
/// there: its handle is published at the next text's start, and a drag
/// from that handle moves the end from there.
///
/// Red-check: resolve the dragged end where the selection keeps it (the
/// second drag, gripped against the first text's end, lands past "beta").
pub(crate) fn an_end_at_a_text_edge_drags_from_where_its_handle_is() {
    const NEXT: &str = "delta epsilon";
    let heard: Heard = Rc::default();
    let recorded = Rc::clone(&heard);
    let area = SelectionArea::new(Column::new((Text::new(WORDS), Text::new(NEXT))))
        .on_selection_changed(move |_cx, selected| {
            recorded
                .borrow_mut()
                .push(selected.map(|selected| selected.text.clone()));
        })
        .handles(Marked::default());
    let mut scene = Scene::laid_out(WidgetsApp::new(area), heard);
    // Each text's line origin and height, which no drag below changes.
    let lines: Vec<(Offset<f64>, f64)> = scene
        .laid
        .find_all_by_render_type("RenderParagraph")
        .into_iter()
        .map(|paragraph| {
            (
                scene.laid.absolute_offset(paragraph),
                scene.laid.size(paragraph).height,
            )
        })
        .collect();
    let middle = |index: usize, width: f64| {
        let (origin, height) = lines[index];
        origin + Offset::new(width, height / 2.0)
    };
    // The middle of the handle under the second text's start.
    let (origin, height) = lines[1];
    let second_start = origin + Offset::new(0.0, height + Scene::HANDLE / 2.0);

    scene.long_press(middle(1, width_of("del")));
    assert_eq!(scene.selected().as_deref(), Some("delta"));

    // Past the first text's end, on its line: the start selects nothing
    // of it.
    scene.touch_drag(second_start, middle(0, width_of(WORDS) + 4.0));
    let selected = scene.selected().expect("still selected");
    assert!(selected.trim_start().starts_with("delta"), "{selected:?}");

    // The handle is painted at the second text's start; dragged from
    // there by as much as "alpha beta"'s start lies from it, it lands
    // before "beta".
    let to = second_start + (middle(0, width_of("alpha ")) - middle(1, 0.0));
    scene.touch_drag(second_start, to);
    let selected = scene.selected().expect("still selected");
    assert!(
        selected.trim_start().starts_with("beta gamma"),
        "{selected:?}"
    );
}

/// An area in an app whose frame request fails while `armed` is set. An
/// element keeps the hook of its mount, so the hook is installed before
/// the app mounts.
fn with_failing_wake() -> (Scene, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let armed = Arc::new(AtomicBool::new(false));
    let mut laid = lay_out(SizedBox::new(1.0, 1.0), tight(400.0, 300.0));
    let arm = Arc::clone(&armed);
    laid.with_build_owner_mut(|owner| {
        owner.set_on_build_scheduled(move || {
            assert!(!arm.load(Ordering::SeqCst), "the frame request failed");
        });
    });
    let heard: Heard = Rc::default();
    laid.pump_widget(WidgetsApp::new(Scene::area(
        &heard,
        Some(Marked::default()),
    )));
    laid.pump();
    (Scene { laid, heard }, armed)
}

/// A frame request that fails while the handles follow a change still
/// lets the change commit and be heard: a handle drag moves its end (the
/// failure stops the drag after its first move), and
/// a tap clears.
///
/// Red-check: let a failing wake escape the handle update (the drag
/// leaves "beta"; the tap leaves the selection).
pub(crate) fn a_failing_wake_while_the_handles_follow_still_commits() {
    use std::sync::atomic::Ordering;

    let (mut scene, armed) = with_failing_wake();
    let beta = scene.along(width_of("alpha be"));
    scene.long_press(beta);
    assert_eq!(scene.handles(), 2);

    let end = width_of("alpha beta");
    let past = width_of(WORDS) + 10.0;
    let (from, to) = (
        scene.handle_at(end),
        scene.handle_at(end) + Offset::new(past - end, 0.0),
    );
    armed.store(true, Ordering::SeqCst);
    let _contained =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.touch_drag(from, to)));
    armed.store(false, Ordering::SeqCst);
    scene.settle();
    let dragged = scene.selected().expect("still selected");
    assert!(dragged.starts_with("beta g"), "the drag: {dragged:?}");

    armed.store(true, Ordering::SeqCst);
    let _contained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scene.laid.replay(&PointerScript::tap(beta));
    }));
    armed.store(false, Ordering::SeqCst);
    scene.settle();
    assert_eq!((scene.selected(), scene.handles()), (None, 0), "the tap");
}

/// An area unmounted while it shows its handles takes them out of the
/// overlay even when releasing its primary selection fails.
///
/// Red-check: release the primary selection outside the containment (the
/// handles stay in the overlay).
pub(crate) fn a_failing_primary_release_still_takes_the_handles_away() {
    let mut scene = Scene::over(Some(Marked::default()));
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.handles(), 2);
    let primary = scene.laid.primary_selection();
    assert!(primary.is_claimed());

    primary.fail_releases();
    let _failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scene.laid.pump_widget(WidgetsApp::new(Text::new(WORDS)));
    }));
    scene.settle();
    assert_eq!(scene.handles(), 0);
}

/// A step of a touch contact made by hand.
#[derive(Clone, Copy)]
enum Touch {
    Down,
    Move,
    Up,
}

/// A handle released where no update reported moves its end there.
///
/// Red-check: clear the grip on the release without applying its
/// position (the end stays where the last move left it).
pub(crate) fn a_handle_released_past_its_last_move_ends_there() {
    let mut scene = Scene::over(Some(Marked::default()));
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));

    let from = scene.handle_at(width_of("alpha beta"));
    let released = scene.handle_at(width_of(WORDS) + 10.0);
    scene.touch(Touch::Down, 0, from);
    scene.touch(Touch::Move, 16, from + Offset::new(20.0, 0.0));
    scene.touch(Touch::Move, 32, from + Offset::new(24.0, 0.0));
    scene.touch(Touch::Up, 48, released);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("beta gamma"));
}

/// Shown handles rebuild when the direction the area reads in changes,
/// though the handles stay the same.
///
/// Red-check: store the new direction without rebuilding the shown
/// handles (the handles keep the left-to-right direction).
pub(crate) fn shown_handles_rebuild_with_a_new_direction() {
    let marked = Marked::default();
    let heard: Heard = Rc::default();
    let area = Scene::area(&heard, Some(marked.clone()));
    let mut scene = Scene::laid_out(
        WidgetsApp::new(Directionality::new(TextDirection::Ltr, area.clone())),
        heard,
    );
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.handles(), 2);

    scene.laid.pump_widget(WidgetsApp::new(Directionality::new(
        TextDirection::Rtl,
        area,
    )));
    scene.settle();
    let built = marked.0.borrow().clone();
    let last = &built[built.len() - 2..];
    assert!(
        last.iter()
            .all(|info| info.text_direction == TextDirection::Rtl),
        "{last:?}"
    );
}
