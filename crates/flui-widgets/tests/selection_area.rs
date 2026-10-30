//! `SelectionArea`: one selection across the paragraphs beneath it, made with a mouse, touch and
//! the keyboard, copied, read and set through its controller, and kept consistent when a
//! paragraph holding an end unmounts.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Point};
use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform::Clipboard as _;
use flui_platform::EventTime;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerPress, PointerRelease, PointerSample,
};
use flui_testing::replay::{MouseScript, TouchScript};
use flui_view::prelude::*;
use flui_view::{BoxedView, StateHandle};
use flui_widgets::{
    AreaSelection, Column, SelectionArea, SelectionContainer, SelectionController, SelectionError,
    SelectionPoint, SizedBox, Text, WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

const FIRST: &str = "Hello world";
const SECOND: &str = "Second line";

/// What `on_selection_changed` heard, in order.
type Heard = Rc<RefCell<Vec<Option<String>>>>;

/// An area over two paragraphs (the second shown while `shown` is true), with `extra` after
/// them.
struct Scene {
    laid: LaidOut,
    controller: SelectionController,
    heard: Heard,
}

fn mount_with(shown: &StateHandle<bool>, extra: Option<BoxedView>) -> Scene {
    let controller = SelectionController::new();
    let heard: Heard = Rc::default();
    let recorded = Rc::clone(&heard);
    let mut children = vec![
        Text::new(FIRST).boxed(),
        Shown {
            shown: shown.clone(),
            child: Text::new(SECOND).boxed(),
        }
        .boxed(),
    ];
    children.extend(extra);
    let area = SelectionArea::new(Column::new(children))
        .controller(controller.clone())
        .on_selection_changed(move |_cx, content| {
            recorded
                .borrow_mut()
                .push(content.map(|content| content.plain_text.clone()));
        });
    let mut laid = lay_out(WidgetsApp::new(area), tight(400.0, 300.0));
    laid.pump();
    Scene {
        laid,
        controller,
        heard,
    }
}

fn mount() -> Scene {
    mount_with(&StateHandle::new(true), None)
}

impl Scene {
    /// The laid-out paragraphs of the page, in tree order.
    fn paragraphs(&self) -> Vec<RenderId> {
        self.laid.find_all_by_render_type("RenderParagraph")
    }

    /// A point `fraction` of the way across paragraph `index`, at its vertical middle.
    fn at(&self, index: usize, fraction: f64) -> Offset<f64> {
        let paragraph = self.paragraphs()[index];
        let origin = self.laid.absolute_offset(paragraph);
        let size = self.laid.size(paragraph);
        Offset::new(
            origin.dx + size.width * fraction,
            origin.dy + size.height / 2.0,
        )
    }

    fn selected(&self) -> Option<String> {
        self.controller.content().map(|content| content.plain_text)
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }

    fn drag(&mut self, from: Offset<f64>, to: Offset<f64>) {
        let mut script = MouseScript::new("drag").press(from);
        for step in 1..=8 {
            let t = f64::from(step) / 8.0;
            script = script
                .wait(Duration::from_millis(16))
                .move_to(from + (to - from) * t);
        }
        let script = script.release().build().expect("the drag releases");
        self.laid.replay(&script);
        self.settle();
    }

    /// `count` clicks at `at`, close enough together to count as one multi-click.
    fn clicks(&mut self, at: Offset<f64>, count: usize) {
        let mut script = MouseScript::new("clicks");
        for _ in 0..count {
            script = script
                .press(at)
                .wait(Duration::from_millis(16))
                .release()
                .wait(Duration::from_millis(40));
        }
        let script = script.build().expect("every click releases");
        self.laid.replay(&script);
        self.settle();
    }

    /// A primary click at `at` with Shift held.
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

    fn key(&mut self, key: Key, code: Code, modifiers: Modifiers) -> bool {
        let event: KeyEvent = KeyEventBuilder::new(code)
            .with_key(key)
            .with_state(KeyState::Down)
            .with_modifiers(modifiers)
            .build();
        let handled = self
            .laid
            .focus_manager()
            .dispatch_key_event(&event)
            .is_handled();
        self.settle();
        handled
    }
}

/// The platform's command modifier, as `DefaultFocusTraversal` binds it.
fn command() -> Modifiers {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

#[test]
fn a_drag_selects_across_paragraphs_and_a_click_clears() {
    let mut scene = mount();
    let (from, to) = (scene.at(0, 0.0), scene.at(1, 1.0));
    scene.drag(from + Offset::new(-2.0, 0.0), to + Offset::new(2.0, 0.0));
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
    let paragraphs = scene.paragraphs();
    assert_eq!(
        scene.laid.render_property(paragraphs[0], "selection"),
        Some(format!("{:?}", 0..FIRST.len()))
    );
    assert_eq!(
        scene.laid.render_property(paragraphs[1], "selection"),
        Some(format!("{:?}", 0..SECOND.len()))
    );

    // A drag back toward its start selects less; its start stays.
    let middle = scene.at(0, 0.5);
    scene.drag(from + Offset::new(-2.0, 0.0), middle);
    let partial = scene.selected().expect("part of the first paragraph");
    assert!(
        !partial.is_empty() && FIRST.starts_with(&partial) && partial.len() < FIRST.len(),
        "{partial:?}"
    );

    scene.clicks(scene.at(1, 0.5), 1);
    assert_eq!(scene.selected(), None);
    assert_eq!(
        scene
            .laid
            .render_property(scene.paragraphs()[0], "selection"),
        None
    );
    let heard = scene.heard.borrow();
    assert_eq!(
        heard.last(),
        Some(&None),
        "the clear is reported: {heard:?}"
    );
}

#[test]
fn multi_clicks_select_a_word_or_a_paragraph_and_shift_click_extends() {
    let mut scene = mount();
    scene.clicks(scene.at(0, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));
    scene.clicks(scene.at(1, 0.3), 3);
    assert_eq!(scene.selected().as_deref(), Some(SECOND));

    // From a word in the first paragraph, Shift+click past the second's end extends to it.
    scene.clicks(scene.at(0, 0.2), 2);
    scene.shift_click(scene.at(1, 1.0) + Offset::new(4.0, 0.0));
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
}

#[test]
fn keys_extend_select_everything_copy_and_clear() {
    let mut scene = mount();
    scene.clicks(scene.at(0, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));

    let cases = [
        (
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            Modifiers::SHIFT,
            "Hello ",
        ),
        (
            Key::Named(NamedKey::End),
            Code::End,
            Modifiers::SHIFT,
            "Hello world",
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            Modifiers::SHIFT,
            "Hello world\n",
        ),
        (
            Key::Named(NamedKey::ArrowLeft),
            Code::ArrowLeft,
            Modifiers::SHIFT,
            "Hello world",
        ),
    ];
    for (key, code, modifiers, expected) in cases {
        assert!(
            scene.key(key.clone(), code, modifiers),
            "{key:?} is handled"
        );
        assert_eq!(scene.selected().as_deref(), Some(expected), "after {key:?}");
    }

    let _ = scene.key(Key::character("a"), Code::KeyA, command());
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
    let _ = scene.key(Key::character("c"), Code::KeyC, command());
    assert_eq!(
        scene.laid.clipboard().read_text().as_deref(),
        Some("Hello world\nSecond line")
    );
    let _ = scene.key(Key::Named(NamedKey::Escape), Code::Escape, Modifiers::NONE);
    assert_eq!(scene.selected(), None);
}

#[test]
fn the_controller_selects_without_a_pointer_and_is_not_reported() {
    let scene = mount();
    let paragraphs = scene.controller.paragraphs();
    assert_eq!(paragraphs.len(), 2, "both paragraphs take part");

    scene.controller.select_all();
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );

    let selection = AreaSelection::new(
        SelectionPoint::new(paragraphs[1], 6),
        SelectionPoint::new(paragraphs[0], 6),
    );
    scene
        .controller
        .select(selection)
        .expect("current boundaries");
    assert_eq!(scene.selected().as_deref(), Some("world\nSecond"));
    assert_eq!(scene.controller.selection(), Some(selection));

    let past = AreaSelection::new(
        SelectionPoint::new(paragraphs[0], 0),
        SelectionPoint::new(paragraphs[0], FIRST.len() + 1),
    );
    assert_eq!(
        scene.controller.select(past),
        Err(SelectionError::NotABoundary {
            paragraph: paragraphs[0],
            offset: FIRST.len() + 1
        })
    );
    assert_eq!(
        scene.controller.selection(),
        Some(selection),
        "a refusal keeps it"
    );

    scene.controller.clear();
    assert_eq!(scene.selected(), None);
    assert!(
        scene.heard.borrow().is_empty(),
        "controller changes are not the user's: {:?}",
        scene.heard.borrow()
    );

    let detached = SelectionController::new();
    assert_eq!(detached.select(selection), Err(SelectionError::NotAttached));
}

#[test]
fn a_paragraph_that_leaves_with_an_end_moves_it_and_the_last_one_clears_once() {
    let shown = StateHandle::new(true);
    let mut scene = mount_with(&shown, None);
    let paragraphs = scene.controller.paragraphs();
    scene
        .controller
        .select(AreaSelection::new(
            SelectionPoint::new(paragraphs[0], 6),
            SelectionPoint::new(paragraphs[1], 6),
        ))
        .expect("current boundaries");

    shown.update(|shown| *shown = false);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("world"));
    assert_eq!(
        scene.heard.borrow().as_slice(),
        [Some("world".to_owned())],
        "the moved end is reported once"
    );

    // With the second paragraph gone and the selection only on it, nothing is left.
    let shown = StateHandle::new(true);
    let mut scene = mount_with(&shown, None);
    let paragraphs = scene.controller.paragraphs();
    scene
        .controller
        .select(AreaSelection::new(
            SelectionPoint::new(paragraphs[1], 0),
            SelectionPoint::new(paragraphs[1], 6),
        ))
        .expect("current boundaries");
    shown.update(|shown| *shown = false);
    scene.settle();
    assert_eq!(scene.selected(), None);
    assert_eq!(scene.heard.borrow().as_slice(), [None], "cleared once");
}

#[test]
fn a_disabled_container_keeps_its_text_out() {
    let extra = SelectionContainer::disabled(Text::new("Toolbar")).boxed();
    let scene = mount_with(&StateHandle::new(true), Some(extra));
    assert_eq!(scene.controller.paragraphs().len(), 2);
    scene.controller.select_all();
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
}

#[test]
fn a_long_press_selects_its_word_and_shows_handles() {
    let mut scene = mount();
    let press = TouchScript::new("long press")
        .down(1, scene.at(0, 0.8))
        .wait(Duration::from_secs(1))
        .up(1)
        .build()
        .expect("the press lifts its finger");
    scene.laid.replay(&press);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("world"));
    assert_eq!(
        scene
            .laid
            .find_all_by_render_type("RenderFollowerLayer")
            .len(),
        2,
        "two handles follow the ends"
    );
    assert_eq!(
        scene
            .laid
            .find_all_by_render_type("RenderLeaderLayer")
            .len(),
        2,
        "each end publishes a leader"
    );

    // The first tap dismisses the menu the long press opened and keeps the selection; the
    // next one clears it and takes the handles away.
    for (finger, expected) in [(2, Some("world")), (3, None)] {
        let tap = TouchScript::new("tap")
            .down(finger, Offset::new(390.0, 290.0))
            .wait(Duration::from_millis(30))
            .up(finger)
            .build()
            .expect("the tap lifts its finger");
        scene.laid.replay(&tap);
        scene.settle();
        assert_eq!(scene.selected().as_deref(), expected, "after tap {finger}");
    }
    assert_eq!(
        scene
            .laid
            .find_all_by_render_type("RenderFollowerLayer")
            .len(),
        0,
        "the handles are gone"
    );
}

/// Shows `child` while `shown` is true.
#[derive(Clone, StatefulView)]
struct Shown {
    shown: StateHandle<bool>,
    child: BoxedView,
}

struct ShownState {
    shown: StateHandle<bool>,
}

impl StatefulView for Shown {
    type State = ShownState;

    fn create_state(&self) -> Self::State {
        ShownState {
            shown: self.shown.clone(),
        }
    }
}

impl ViewState<Shown> for ShownState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shown.bind(ctx);
    }

    fn build(&self, view: &Shown, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.shown.with(|shown| *shown) {
            view.child.clone()
        } else {
            SizedBox::shrink().boxed()
        }
    }
}

impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shown")
    }
}

impl std::fmt::Debug for ShownState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ShownState")
    }
}

#[test]
fn a_handle_drags_its_end_past_the_other() {
    let mut scene = mount();
    let press = TouchScript::new("long press")
        .down(1, scene.at(0, 0.8))
        .wait(Duration::from_secs(1))
        .up(1)
        .build()
        .expect("the press lifts its finger");
    scene.laid.replay(&press);
    scene.settle();
    // Dismiss the menu the long press opened; the handles stay.
    let tap = TouchScript::new("dismiss")
        .down(2, Offset::new(390.0, 290.0))
        .wait(Duration::from_millis(30))
        .up(2)
        .build()
        .expect("the tap lifts its finger");
    scene.laid.replay(&tap);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("world"));

    // The start handle hangs below the leader at the start of "world".
    let leaders = scene.laid.find_all_by_render_type("RenderLeaderLayer");
    let start = leaders
        .iter()
        .copied()
        .min_by(|a, b| {
            let (a, b) = (
                scene.laid.absolute_offset(*a),
                scene.laid.absolute_offset(*b),
            );
            a.dx.total_cmp(&b.dx)
        })
        .expect("a start leader");
    let line = scene.laid.size(start).height;
    let grip = scene.laid.absolute_offset(start) + Offset::new(0.0, line + 14.0);
    let target = Offset::new(0.0, grip.dy);
    let mut drag = TouchScript::new("drag the start handle").down(3, grip);
    for step in 1..=8 {
        let t = f64::from(step) / 8.0;
        drag = drag
            .wait(Duration::from_millis(16))
            .move_to(3, grip + (target - grip) * t);
    }
    let drag = drag.up(3).build().expect("the drag lifts its finger");
    scene.laid.replay(&drag);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some(FIRST));
}

#[test]
fn a_finger_moving_on_after_a_long_press_extends_by_word() {
    let mut scene = mount();
    let from = scene.at(0, 0.8);
    let to = scene.at(1, 0.3);
    let mut script = TouchScript::new("long press and drag")
        .down(1, from)
        .wait(Duration::from_secs(1));
    for step in 1..=8 {
        let t = f64::from(step) / 8.0;
        script = script
            .wait(Duration::from_millis(16))
            .move_to(1, from + (to - from) * t);
    }
    let script = script.up(1).build().expect("the finger lifts");
    scene.laid.replay(&script);
    scene.settle();
    assert_eq!(scene.selected().as_deref(), Some("world\nSecond"));
}

#[test]
fn a_field_inside_the_area_keeps_its_own_select_all() {
    let controller = flui_widgets::TextEditingController::with_text("typed");
    let node = flui_interaction::focus::FocusNode::with_debug_label("field in area");
    let field = flui_widgets::EditableText::new(controller.clone(), Rc::clone(&node)).boxed();
    let mut scene = mount_with(&StateHandle::new(true), Some(field));
    assert_eq!(
        scene.controller.paragraphs().len(),
        2,
        "the field is not a paragraph"
    );
    let _ = node.request_focus();
    let _ = scene.key(Key::character("a"), Code::KeyA, command());
    assert_eq!(
        controller.selection(),
        0..5,
        "the field selected its own text"
    );
    assert_eq!(scene.selected(), None, "the area selected nothing");
}
