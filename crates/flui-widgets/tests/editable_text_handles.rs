//! `EditableText` by touch: a tap shows a handle under the caret as the
//! field's `CaretHandle` says, a long press selects a word and shows a handle
//! under each end, dragging a handle moves its end, and a key, a mouse press,
//! the focus leaving or the caret handle's timeout takes them away. The
//! handles live in the app's overlay and follow their ends through layer
//! links, so every touch below lands where a handle is painted.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::focus::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_painting::typography::TextStyle;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerPress, PointerRelease, PointerSample,
};
use flui_platform::{EventTime, TargetPlatform};
use flui_testing::replay::{PointerScript, TouchScript};
use flui_view::{BoxedView, BuildContext, ViewExt};
use flui_widgets::{
    CaretHandle, Column, DefaultSelectionHandles, EditableText, Focus, HandleInfo, HandleSide,
    SelectionHandles, SizedBox, Text, TextEditingController, WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

const WORDS: &str = "alpha beta gamma";

/// Handles that note each one they build, as a square.
#[derive(Clone, Default)]
struct Marked(Rc<RefCell<Vec<HandleInfo>>>);

impl Marked {
    fn sides(&self) -> Vec<HandleSide> {
        self.0.borrow().iter().map(|info| info.side).collect()
    }

    fn line_height(&self) -> f64 {
        self.0
            .borrow()
            .last()
            .map(|info| info.line_height)
            .expect("a handle was built")
    }
}

impl SelectionHandles for Marked {
    fn build(&self, info: &HandleInfo, _ctx: &dyn BuildContext) -> BoxedView {
        self.0.borrow_mut().push(*info);
        SizedBox::new(Scene::HANDLE, Scene::HANDLE).boxed()
    }
}

fn style() -> TextStyle {
    TextStyle::default().with_font_size(16.0)
}
struct Scene {
    laid: LaidOut,
    controller: TextEditingController,
    field_focus: Rc<FocusNode>,
    elsewhere: Rc<FocusNode>,
    marked: Marked,
}

impl Scene {
    /// The side of the square handles.
    const HANDLE: f64 = 22.0;

    /// A field over [`WORDS`] in an app with an overlay, whose tap shows
    /// `caret` and whose handles are [`Marked`], beside a focusable box.
    fn over(caret: CaretHandle) -> Self {
        Self::with(|field| field.caret_handle(caret))
    }

    fn with(configure: impl FnOnce(EditableText) -> EditableText) -> Self {
        let controller = TextEditingController::with_text(WORDS);
        let field_focus = FocusNode::with_debug_label("field");
        let elsewhere = FocusNode::with_debug_label("elsewhere");
        let marked = Marked::default();
        let mut scene = Self {
            laid: lay_out(SizedBox::shrink(), tight(400.0, 300.0)),
            controller,
            field_focus,
            elsewhere,
            marked,
        };
        scene.rebuild(configure);
        scene
    }

    /// Builds the app again around the same controller, focus nodes and
    /// handles, so the field updates in place.
    fn rebuild(&mut self, configure: impl FnOnce(EditableText) -> EditableText) {
        let field = EditableText::new(self.controller.clone(), Rc::clone(&self.field_focus))
            .text_style(style())
            .handles(self.marked.clone());
        self.laid.pump_widget(WidgetsApp::new(Column::new((
            SizedBox::new(400.0, 40.0).child(configure(field)),
            Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.elsewhere)),
        ))));
        self.laid.pump();
    }

    fn editable(&self) -> flui_foundation::RenderId {
        self.laid.find_all_by_render_type("RenderEditable")[0]
    }

    /// The point `width` along the field's text, at the line's middle.
    fn along(&self, width: f64) -> Offset<f64> {
        let editable = self.editable();
        let height = self.laid.size(editable).height;
        self.laid.absolute_offset(editable) + Offset::new(width, height / 2.0)
    }

    /// The middle of the handle under the end `width` along the line: its
    /// top centre is at the bottom of the caret there.
    fn handle_at(&self, width: f64) -> Offset<f64> {
        self.laid.absolute_offset(self.editable())
            + Offset::new(width, self.marked.line_height() + Self::HANDLE / 2.0)
    }

    /// The handles in the overlay.
    fn handles(&self) -> usize {
        self.laid
            .find_all_by_render_type("RenderFollowerLayer")
            .len()
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }

    fn tap(&mut self, at: Offset<f64>) {
        self.laid.replay(&PointerScript::tap(at));
        self.settle();
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

    fn key(&mut self, code: Code, key: Key) {
        let event: KeyEvent = KeyEventBuilder::new(code)
            .with_key(key)
            .with_state(KeyState::Down)
            .with_modifiers(Modifiers::NONE)
            .build();
        let _handled = self.laid.focus_manager().dispatch_key_event(&event);
        self.settle();
    }

    /// A primary mouse click at `at`.
    fn click(&mut self, at: Offset<f64>) {
        let mouse = PointerInfo::new(
            PointerId::try_from(77_u64).expect("a nonzero id"),
            PointerKind::Mouse,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(0),
            PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite"),
        );
        self.laid
            .dispatch_pointer_event(&PointerEvent::Down(PointerPress::new(
                mouse,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )));
        self.laid
            .dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
                mouse,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )));
        self.settle();
    }
    fn selection(&self) -> std::ops::Range<usize> {
        self.controller.selection()
    }
}

/// The width of `words` in the field's style.
fn width_of(words: &str) -> f64 {
    let mut laid = lay_out(
        WidgetsApp::new(Column::new((Text::new(words).style(style()),))),
        tight(400.0, 300.0),
    );
    laid.pump();
    laid.size(laid.find_all_by_render_type("RenderParagraph")[0])
        .width
}

/// Where the text after `prefix` begins: a paragraph's width leaves out
/// trailing spaces, so this measures the prefix with a letter after it.
fn start_of(prefix: &str) -> f64 {
    width_of(&format!("{prefix}x")) - width_of("x")
}

/// A touch tap places the caret and shows a handle under it, built for the
/// collapsed side; dragging the handle moves the caret.
///
/// Red-check: show no handle after a tap (no follower in the overlay).
pub(crate) fn a_tap_shows_a_caret_handle_that_a_drag_moves() {
    let mut scene = Scene::over(CaretHandle::on_tap());
    let caret = start_of("alpha ");
    scene.tap(scene.along(caret));
    assert_eq!(scene.controller.caret_byte_offset(), 6);
    assert_eq!(scene.handles(), 1);
    assert_eq!(scene.marked.sides().last(), Some(&HandleSide::Collapsed));
    assert!(scene.field_focus.has_primary_focus());

    let from = scene.handle_at(caret);
    let to = from + Offset::new(start_of("alpha beta ") - caret, 0.0);
    scene.touch_drag(from, to);
    assert_eq!(scene.controller.caret_byte_offset(), 11);
    // The timer's run starts on the frame after it was started.
    scene.settle();
    assert_eq!(
        scene.handles(),
        1,
        "the handle stays with the caret it moved"
    );
}

/// A field whose caret handle is `NEVER`, as on iOS, shows none after a tap,
/// and a `DefaultSelectionHandles` above a field chooses its caret handle.
///
/// Red-check: ignore the field's `CaretHandle` (the `NEVER` tap shows one).
pub(crate) fn a_caret_handle_follows_the_field_or_its_default() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    scene.tap(scene.along(start_of("alpha ")));
    assert_eq!(scene.controller.caret_byte_offset(), 6);
    assert_eq!(scene.handles(), 0);

    assert_eq!(
        CaretHandle::for_platform(TargetPlatform::iOS),
        CaretHandle::NEVER
    );
    assert_eq!(
        CaretHandle::for_platform(TargetPlatform::Android).hides_after(),
        Some(Duration::from_secs(4))
    );

    let controller = TextEditingController::with_text(WORDS);
    let marked = Marked::default();
    let field = EditableText::new(controller.clone(), FocusNode::with_debug_label("field"))
        .text_style(style())
        .handles(marked);
    let app = WidgetsApp::new(Column::new((DefaultSelectionHandles::new(
        Marked::default(),
        SizedBox::new(400.0, 40.0).child(field),
    )
    .caret_handle(CaretHandle::NEVER),)));
    let mut laid = lay_out(app, tight(400.0, 300.0));
    laid.pump();
    let editable = laid.find_all_by_render_type("RenderEditable")[0];
    let at = laid.absolute_offset(editable) + Offset::new(start_of("alpha "), 5.0);
    laid.replay(&PointerScript::tap(at));
    for _ in 0..4 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(controller.caret_byte_offset(), 6);
    assert_eq!(
        laid.find_all_by_render_type("RenderFollowerLayer").len(),
        0,
        "the default's NEVER holds"
    );
}

/// A caret handle with a timeout goes once that time passes without a
/// touch, and one dragged meanwhile starts its time again.
///
/// Red-check: never start the timer (the handle stays).
pub(crate) fn a_caret_handle_hides_after_its_timeout() {
    let mut scene = Scene::over(CaretHandle::on_tap().hide_after(Duration::from_secs(1)));
    scene.tap(scene.along(start_of("alpha ")));
    assert_eq!(scene.handles(), 1);
    scene.laid.pump_for(Duration::from_millis(600));
    assert_eq!(scene.handles(), 1, "still within its time");
    scene.laid.pump_for(Duration::from_millis(600));
    scene.settle();
    assert_eq!(scene.handles(), 0, "its time without a touch is up");
}

/// A touch long press selects the word under it and shows a handle under
/// each end; dragging the end's handle moves the end, and the start's
/// handle the start, neither passing the other.
///
/// Red-check: let a long press place only the caret (no word, no handles).
pub(crate) fn a_long_press_selects_a_word_whose_handles_move_its_ends() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.selection(), 6..10);
    assert_eq!(scene.handles(), 2);
    let sides = scene.marked.sides();
    assert!(sides.contains(&HandleSide::Start) && sides.contains(&HandleSide::End));

    let end = width_of("alpha beta");
    let from = scene.handle_at(end);
    scene.touch_drag(from, from + Offset::new(width_of(WORDS) - end, 0.0));
    assert_eq!(scene.selection(), 6..16);

    let start = start_of("alpha ");
    let from = scene.handle_at(start);
    scene.touch_drag(from, from - Offset::new(start, 0.0));
    assert_eq!(scene.selection(), 0..16);
    assert_eq!(scene.handles(), 2);
}

/// A key, even one that leaves the selection alone, a mouse press or the
/// focus leaving takes the handles away.
///
/// Red-check: leave the handles on a key (the first case keeps two).
pub(crate) fn a_key_a_mouse_or_the_focus_leaving_hides_the_handles() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    let word = scene.along(width_of("alpha be"));
    scene.long_press(word);
    assert_eq!(scene.handles(), 2);
    scene.key(Code::ShiftLeft, Key::Named(NamedKey::Shift));
    assert_eq!(
        scene.selection(),
        6..10,
        "a key that leaves the selection alone"
    );
    assert_eq!(scene.handles(), 0, "a key");

    scene.long_press(word);
    assert_eq!(scene.handles(), 2);
    scene.click(scene.along(width_of("al")));
    assert_eq!(scene.handles(), 0, "a mouse press");

    scene.long_press(word);
    assert_eq!(scene.handles(), 2);
    let _ = scene.elsewhere.request_focus();
    scene.settle();
    assert_eq!(scene.handles(), 0, "the focus leaving");
}

/// A long press in an obscured field selects its whole text, not a word.
///
/// Red-check: select the word under the press in an obscured field.
pub(crate) fn a_long_press_in_an_obscured_field_selects_all_of_it() {
    let mut scene = Scene::with(|field| field.caret_handle(CaretHandle::NEVER).obscure_text(true));
    let editable = scene.editable();
    let width = scene.laid.size(editable).width;
    scene.long_press(scene.along(width.min(40.0)));
    assert_eq!(scene.selection(), 0..WORDS.len());
    assert_eq!(scene.handles(), 2);
}

/// An edit the handles didn't make takes them away, even one that leaves
/// the selection's offsets where they were.
///
/// Red-check: compare only the offsets (the handle survives the edit).
pub(crate) fn an_edit_that_keeps_the_offsets_still_hides_the_handles() {
    let mut scene = Scene::over(CaretHandle::on_tap());
    scene.tap(scene.along(width_of(WORDS) + 4.0));
    assert_eq!(scene.controller.caret_byte_offset(), WORDS.len());
    assert_eq!(scene.handles(), 1);
    let replacement = "ALPHA BETA GAMMA";
    assert_eq!(replacement.len(), WORDS.len());
    scene.controller.set_text(replacement);
    scene.settle();
    assert_eq!(scene.controller.caret_byte_offset(), WORDS.len());
    assert_eq!(scene.handles(), 0);
}

/// A finger resting on a caret handle keeps it past its timeout, and its
/// time starts again once the finger lifts.
///
/// Red-check: let the timer run while a finger rests on the handle.
pub(crate) fn a_finger_resting_on_the_caret_handle_keeps_it() {
    let mut scene = Scene::over(CaretHandle::on_tap().hide_after(Duration::from_secs(1)));
    let caret = start_of("alpha ");
    scene.tap(scene.along(caret));
    assert_eq!(scene.handles(), 1);
    let rest = TouchScript::new("rest on the caret handle")
        .down(1, scene.handle_at(caret))
        .wait(Duration::from_millis(500))
        .frame()
        .wait(Duration::from_millis(500))
        .frame()
        .wait(Duration::from_millis(500))
        .frame()
        .up(1)
        .build()
        .expect("a valid script");
    let mut while_resting = None;
    scene.laid.replay_with(&rest, |laid, step| {
        if step == 3 {
            while_resting = Some(laid.find_all_by_render_type("RenderFollowerLayer").len());
        }
    });
    assert_eq!(while_resting, Some(1), "the finger keeps it");
    scene.settle();
    scene.laid.pump_for(Duration::from_millis(600));
    assert_eq!(scene.handles(), 1, "its time starts again on the lift");
    scene.laid.pump_for(Duration::from_millis(600));
    scene.settle();
    assert_eq!(scene.handles(), 0, "its time without a touch is up");
}

/// A long press under way when the focus leaves selects nothing at its
/// deadline and shows no handles.
///
/// Red-check: only hide the handles when the focus leaves (the press
/// selects at its deadline).
pub(crate) fn the_focus_leaving_mid_press_cancels_the_long_press() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    let _ = scene.field_focus.request_focus();
    scene.settle();
    assert!(scene.field_focus.has_primary_focus());
    let press = TouchScript::new("press while the focus leaves")
        .down(1, scene.along(width_of("alpha be")))
        .wait(Duration::from_millis(1000))
        .up(1)
        .build()
        .expect("a valid script");
    let elsewhere = Rc::clone(&scene.elsewhere);
    scene.laid.replay_with(&press, |_laid, step| {
        if step == 0 {
            let _ = elsewhere.request_focus();
        }
    });
    scene.settle();
    assert!(!scene.field_focus.has_focus());
    assert!(scene.selection().is_empty(), "{:?}", scene.selection());
    assert_eq!(scene.handles(), 0);
}

/// A field given a new caret-handle timeout while its handle shows runs on
/// the new one: a handle that stops timing out stays past the old deadline,
/// and one that starts timing out goes once the new time passes.
///
/// Red-check: leave the timer alone when the policy changes (the handle goes
/// at the old deadline).
pub(crate) fn a_new_caret_handle_timeout_applies_to_the_shown_handle() {
    let timed = CaretHandle::on_tap().hide_after(Duration::from_secs(1));
    let mut scene = Scene::over(timed);
    scene.tap(scene.along(start_of("alpha ")));
    assert_eq!(scene.handles(), 1);
    scene.laid.pump_for(Duration::from_millis(600));
    scene.rebuild(|field| field.caret_handle(CaretHandle::on_tap()));
    scene.laid.pump_for(Duration::from_millis(1200));
    scene.settle();
    assert_eq!(
        scene.handles(),
        1,
        "an untimed handle outlives the old time"
    );

    scene.rebuild(|field| field.caret_handle(timed));
    scene.laid.pump_for(Duration::from_millis(600));
    assert_eq!(scene.handles(), 1, "within the new time");
    scene.laid.pump_for(Duration::from_millis(600));
    scene.settle();
    assert_eq!(scene.handles(), 0, "the new time is up");
}

/// A touch whose field loses focus before it lifts shows no handles on
/// the unfocused field.
///
/// Red-check: keep the contact when the focus leaves (its release shows the
/// caret handle).
pub(crate) fn the_focus_leaving_before_a_tap_lifts_shows_no_handles() {
    let mut scene = Scene::over(CaretHandle::on_tap());
    let tap = TouchScript::new("tap while the focus leaves")
        .down(1, scene.along(start_of("alpha ")))
        .wait(Duration::from_millis(50))
        .up(1)
        .build()
        .expect("a valid script");
    let elsewhere = Rc::clone(&scene.elsewhere);
    scene.laid.replay_with(&tap, |_laid, step| {
        if step == 0 {
            let _ = elsewhere.request_focus();
        }
    });
    scene.settle();
    assert!(!scene.field_focus.has_focus());
    assert_eq!(scene.handles(), 0);
}

/// A handle drag whose release lands past its last move ends though the
/// controller's listener panics on that last change: the timed caret handle
/// goes once its time is up, rather than staying held by a finished drag.
///
/// Red-check: let the last move's panic skip the drag's end (the handle
/// stays past its time).
pub(crate) fn a_panic_on_the_last_drag_move_still_ends_the_drag() {
    use flui_foundation::Listenable;
    use flui_platform::pointer::PointerMove;

    let mut scene = Scene::over(CaretHandle::on_tap().hide_after(Duration::from_secs(1)));
    let caret = start_of("alpha ");
    scene.tap(scene.along(caret));
    assert_eq!(scene.handles(), 1);

    let armed = Rc::new(std::cell::Cell::new(true));
    let fired = Rc::new(std::cell::Cell::new(false));
    let watched = scene.controller.clone();
    let listener = scene.controller.add_listener(Rc::new({
        let armed = Rc::clone(&armed);
        let fired = Rc::clone(&fired);
        move || {
            if armed.get() && watched.caret_byte_offset() == 11 {
                fired.set(true);
                panic!("a listener fails on the last change");
            }
        }
    }));
    let from = scene.handle_at(caret);
    let shift = |prefix: &str| from + Offset::new(start_of(prefix) - caret, 0.0);
    let info = PointerInfo::new(
        PointerId::try_from(5_u64).expect("a nonzero id"),
        PointerKind::Touch,
    );
    let held = PointerButtons::only(PointerButton::PRIMARY);
    let sample = |millis: u64, at: Offset<f64>| {
        PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite"),
        )
    };
    scene
        .laid
        .dispatch_pointer_event(&PointerEvent::Down(PointerPress::new(
            info,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample(0, from),
        )));
    for (millis, prefix) in [(16, "alpha bet"), (32, "alpha beta")] {
        scene
            .laid
            .dispatch_pointer_event(&PointerEvent::Move(PointerMove::new(
                info,
                held,
                sample(millis, shift(prefix)),
            )));
    }
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scene
            .laid
            .dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
                info,
                PointerButton::PRIMARY,
                held,
                sample(48, shift("alpha beta ")),
            )));
    }));
    // Whether the panic reaches here or the input path contains it, the
    // listener failed on the release's change.
    drop(released);
    assert!(fired.get(), "the release made the last change");
    armed.set(false);
    scene.controller.remove_listener(listener);
    assert_eq!(scene.controller.caret_byte_offset(), 11);
    // The timer's run starts on the frame after it was started.
    scene.settle();
    scene.laid.pump_for(Duration::from_millis(1200));
    scene.settle();
    assert_eq!(scene.handles(), 0, "the ended drag lets its time run out");
}

/// An edit made while a long press holds its word ends the press: lifting
/// after moving neither selects in the new text from the old word nor
/// shows handles.
///
/// Red-check: keep the pressed word across the edit (the move selects
/// from it and the release shows handles).
pub(crate) fn an_edit_during_a_long_press_ends_it() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    let press = TouchScript::new("long press across an edit")
        .down(1, scene.along(start_of("alpha be")))
        .wait(Duration::from_millis(1000))
        .frame()
        .move_to(1, scene.along(start_of("alpha beta ga")))
        .wait(Duration::from_millis(50))
        .up(1)
        .build()
        .expect("a valid script");
    let controller = scene.controller.clone();
    let after_edit = Rc::new(RefCell::new(None));
    let noted = Rc::clone(&after_edit);
    scene.laid.replay_with(&press, |_laid, step| {
        if step == 1 {
            assert_eq!(controller.selection(), 6..10, "the press selected beta");
            controller.set_text("one two three");
            *noted.borrow_mut() = Some(controller.selection());
        }
    });
    scene.settle();
    assert_eq!(scene.controller.text(), "one two three");
    assert_eq!(Some(scene.selection()), *after_edit.borrow());
    assert_eq!(scene.handles(), 0);
}

/// Shown handles follow a new line height once the field lays out again:
/// the handle a larger font gives is built for its taller line.
///
/// Red-check: never rebuild the handles after a layout (they keep the old
/// line height).
pub(crate) fn shown_handles_follow_a_new_line_height() {
    let mut scene = Scene::over(CaretHandle::NEVER);
    scene.long_press(scene.along(start_of("alpha be")));
    assert_eq!(scene.handles(), 2);
    let before = scene.marked.line_height();
    scene.rebuild(|field| {
        field
            .caret_handle(CaretHandle::NEVER)
            .text_style(TextStyle::default().with_font_size(32.0))
    });
    scene.settle();
    assert_eq!(scene.handles(), 2);
    let after = scene.marked.line_height();
    assert!(after > before * 1.5, "{before} then {after}");
}
