//! `SelectionArea` with a mouse and the keyboard: drags and clicks select,
//! Shift extends, keys move the moving end, select all, copy and clear, and a
//! blur dims the highlight. Semantics stays off, so every position comes from
//! where the texts are laid out.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Point};
use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform::EventTime;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove,
    PointerPosition, PointerPress, PointerRelease, PointerSample,
};
use flui_testing::replay::MouseScript;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{Column, SelectionArea, SizedBox, Text, WidgetsApp};

use crate::common::{LaidOut, lay_out, tight};

const FIRST: &str = "Hello world";
const SECOND: &str = "Second line";

/// What `on_selection_changed` heard, in order.
type Heard = Rc<RefCell<Vec<Option<String>>>>;

struct Scene {
    laid: LaidOut,
    heard: Heard,
}

impl Scene {
    /// An area over `content`, in an app that binds the platform shortcuts.
    fn over(content: impl Into<BoxedView>) -> Self {
        let heard: Heard = Rc::default();
        let mut laid = lay_out(Self::area(content, &heard), tight(400.0, 300.0));
        laid.pump();
        Self { laid, heard }
    }

    /// The app with an area over `content` that records into `heard`.
    fn area(content: impl Into<BoxedView>, heard: &Heard) -> WidgetsApp {
        let recorded = Rc::clone(heard);
        WidgetsApp::new(SelectionArea::new(content.into()).on_selection_changed(
            move |_cx, change| {
                recorded.borrow_mut().push(
                    change
                        .selected
                        .as_ref()
                        .map(|selected| selected.text.clone()),
                );
            },
        ))
    }

    /// The same area over `content` instead, its state kept.
    fn replace(&mut self, content: impl Into<BoxedView>) {
        let app = Self::area(content, &self.heard);
        self.laid.pump_widget(app);
        self.settle();
    }

    /// An area over [`FIRST`] above [`SECOND`].
    fn two_texts() -> Self {
        Self::over(Column::new((Text::new(FIRST), Text::new(SECOND))).boxed())
    }

    /// The laid-out paragraphs, in tree order.
    fn paragraphs(&self) -> Vec<RenderId> {
        self.laid.find_all_by_render_type("RenderParagraph")
    }

    /// The index in [`paragraphs`](Self::paragraphs) of the paragraph
    /// showing `words`.
    fn index_of(&self, words: &str) -> usize {
        self.paragraphs()
            .iter()
            .position(|&paragraph| {
                self.laid
                    .render_property(paragraph, "text")
                    .is_some_and(|text| text.contains(words))
            })
            .expect("a paragraph shows the words")
    }

    /// A point `fraction` of the way across paragraph `index`, at its
    /// vertical middle.
    fn at(&self, index: usize, fraction: f64) -> Offset<f64> {
        let paragraph = self.paragraphs()[index];
        let origin = self.laid.absolute_offset(paragraph);
        let size = self.laid.size(paragraph);
        Offset::new(
            origin.dx + size.width * fraction,
            origin.dy + size.height / 2.0,
        )
    }

    /// The selected text the listener last heard of.
    fn selected(&self) -> Option<String> {
        self.heard.borrow().last().cloned().flatten()
    }

    /// The part of paragraph `index` that paints highlighted.
    fn painted(&self, index: usize) -> Option<String> {
        self.laid
            .render_property(self.paragraphs()[index], "selection")
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

    /// `count` clicks at `at`, close enough together to count as one
    /// multi-click.
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

    /// One step of a mouse contact by hand, `millis` into it, for a test
    /// that changes the tree while the button is held.
    fn contact(&mut self, step: Contact, millis: u64, at: Offset<f64>) {
        let mouse = PointerInfo::new(
            PointerId::try_from(78_u64).expect("a nonzero id"),
            PointerKind::Mouse,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite"),
        );
        let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
        let event = match step {
            Contact::Press => PointerEvent::Down(PointerPress::new(
                mouse,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            Contact::Move => PointerEvent::Move(PointerMove::new(mouse, held, sample)),
            Contact::Release => PointerEvent::Up(PointerRelease::new(
                mouse,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
        };
        self.laid.dispatch_pointer_event(&event);
    }
}

/// A step of [`Scene::contact`].
#[derive(Clone, Copy)]
enum Contact {
    Press,
    Move,
    Release,
}

/// The platform's command modifier, as the app's shortcuts bind it.
fn command() -> Modifiers {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

/// A drag selects across texts and highlights them; a drag back toward its
/// start selects less and keeps the start; a single click clears, and the
/// clear is reported.
///
/// Red-check: never start a drag's selection (nothing is selected).
pub(crate) fn a_drag_selects_across_texts_and_a_click_clears() {
    let mut scene = Scene::two_texts();
    let (from, to) = (scene.at(0, 0.0), scene.at(1, 1.0));
    scene.drag(from + Offset::new(-2.0, 0.0), to + Offset::new(2.0, 0.0));
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
    assert_eq!(
        scene.painted(0),
        Some(format!("{:?}", Some(0..FIRST.len())))
    );
    assert_eq!(
        scene.painted(1),
        Some(format!("{:?}", Some(0..SECOND.len())))
    );

    let middle = scene.at(0, 0.5);
    scene.drag(from + Offset::new(-2.0, 0.0), middle);
    let partial = scene.selected().expect("part of the first text");
    assert!(
        !partial.is_empty() && FIRST.starts_with(&partial) && partial.len() < FIRST.len(),
        "{partial:?}"
    );

    scene.clicks(scene.at(1, 0.5), 1);
    assert_eq!(scene.painted(0), Some("None".to_owned()));
    assert_eq!(
        scene.heard.borrow().last(),
        Some(&None),
        "the clear is reported"
    );
}

/// A double click selects a word, a triple click a text, and Shift+click
/// extends from the anchor.
///
/// Red-check: select by character whatever the click count (the double
/// click selects nothing).
pub(crate) fn multi_clicks_select_a_word_or_a_text_and_shift_click_extends() {
    let mut scene = Scene::two_texts();
    scene.clicks(scene.at(0, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));
    scene.clicks(scene.at(1, 0.3), 3);
    assert_eq!(scene.selected().as_deref(), Some(SECOND));

    scene.clicks(scene.at(0, 0.2), 2);
    scene.shift_click(scene.at(1, 1.0) + Offset::new(4.0, 0.0));
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
}

/// Shift with the arrows and End moves the moving end, crossing into the
/// next text at an edge; the command chords select all and copy, and Esc
/// clears.
///
/// Red-check: ignore Shift+ArrowRight at a text's end (the selection stays
/// "Hello world").
pub(crate) fn keys_extend_select_everything_copy_and_clear() {
    let mut scene = Scene::two_texts();
    scene.clicks(scene.at(0, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));

    let cases = [
        (Key::Named(NamedKey::ArrowRight), Code::ArrowRight, "Hello "),
        (Key::Named(NamedKey::End), Code::End, "Hello world"),
        (
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            "Hello world\n",
        ),
        (
            Key::Named(NamedKey::ArrowLeft),
            Code::ArrowLeft,
            "Hello world",
        ),
    ];
    for (key, code, expected) in cases {
        assert!(
            scene.key(key.clone(), code, Modifiers::SHIFT),
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
        scene.laid.clipboard().text().as_deref(),
        Some("Hello world\nSecond line")
    );
    let _ = scene.key(Key::Named(NamedKey::Escape), Code::Escape, Modifiers::NONE);
    assert_eq!(scene.heard.borrow().last(), Some(&None));
    assert_eq!(scene.painted(0), Some("None".to_owned()));
}

/// A drag reads the texts where they are laid out even with semantics off:
/// in a column laid out upwards, dragging from the top text to the bottom
/// one selects them top first.
///
/// Red-check: order the texts by place in the render tree without semantics
/// (the drag selects "Beta" alone, ending before its start).
pub(crate) fn a_drag_without_semantics_reads_where_the_texts_are_laid_out() {
    use flui_objects::VerticalDirection;

    let mut scene = Scene::over(
        Column::new((Text::new("Alpha"), Text::new("Beta")))
            .vertical_direction(VerticalDirection::Up)
            .boxed(),
    );
    assert!(scene.laid.a11y_tree().is_none(), "semantics stays off");
    // Tree order: Alpha is paragraph 0, laid out below Beta.
    let (top, bottom) = (scene.at(1, 0.0), scene.at(0, 1.0));
    scene.drag(top + Offset::new(-2.0, 0.0), bottom + Offset::new(2.0, 0.0));
    assert_eq!(scene.selected().as_deref(), Some("Beta\nAlpha"));
}

/// A blur keeps the selection and dims its highlight; selecting again
/// brings the active colour back.
///
/// Red-check: always highlight with the active colour (the blurred area
/// still paints it).
pub(crate) fn a_blur_dims_the_highlight_and_keeps_the_selection() {
    use flui_widgets::SelectionStyle;

    let style = SelectionStyle::default();
    let mut scene = Scene::two_texts();
    scene.clicks(scene.at(0, 0.2), 2);
    let colour = |scene: &Scene| {
        scene
            .laid
            .render_property(scene.paragraphs()[0], "selection_color")
    };
    assert_eq!(
        colour(&scene),
        Some(format!("{:?}", style.active)),
        "focused"
    );

    scene.laid.focus_manager().unfocus();
    scene.settle();
    assert_eq!(
        colour(&scene),
        Some(format!("{:?}", style.inactive)),
        "blurred"
    );
    assert_eq!(
        scene.painted(0),
        Some(format!("{:?}", Some(0..5))),
        "the selection stays"
    );

    scene.clicks(scene.at(0, 0.8), 2);
    assert_eq!(
        colour(&scene),
        Some(format!("{:?}", style.active)),
        "focused again"
    );
}

/// A text that leaves while holding an end hands that end to the text it
/// leaves beside, keeping the selection's direction even when both ends
/// leave at once; once no selected text is left, the selection clears.
/// Each move is reported.
///
/// Red-check: never move a departed end (the listener keeps the old text);
/// leave a drag on the text it started on (the selection stops following
/// the pointer once that text leaves).
pub(crate) fn a_removed_text_hands_its_end_to_the_text_beside_it() {
    const THIRD: &str = "Third one";
    // One children type before and after, so the texts that stay keep their
    // elements and only the last one leaves.
    let texts = |words: &[&str]| {
        Column::new(
            words
                .iter()
                .map(|&words| Text::new(words).boxed())
                .collect::<Vec<BoxedView>>(),
        )
        .boxed()
    };
    let three = || texts(&[FIRST, SECOND, THIRD]);
    let two = || texts(&[FIRST, SECOND]);

    for backward in [false, true] {
        let mut scene = Scene::over(three());
        let (from, to) = (scene.at(0, 0.3), scene.at(2, 0.5));
        if backward {
            scene.drag(to, from);
        } else {
            scene.drag(from, to);
        }
        let selected = scene.selected().expect("a drag across three texts");
        let tail = selected.split('\n').next().expect("the first text's part");
        assert!(
            FIRST.ends_with(tail) && tail.len() < FIRST.len(),
            "{selected:?}"
        );

        scene.replace(two());
        let kept = format!("{tail}\n{SECOND}");
        assert_eq!(scene.selected(), Some(kept.clone()), "backward: {backward}");
        assert_eq!(
            scene.painted(1),
            Some(format!("{:?}", Some(0..SECOND.len())))
        );

        // The moving end stays the one the user moved: in the first text
        // when the drag ran backward, at the second text's end otherwise.
        assert!(scene.key(
            Key::Named(NamedKey::ArrowLeft),
            Code::ArrowLeft,
            Modifiers::SHIFT
        ));
        let moved = scene.selected().expect("still selected");
        if backward {
            assert_eq!(
                moved,
                format!("{}{kept}", &FIRST[FIRST.len() - tail.len() - 1..][..1])
            );
        } else {
            assert_eq!(moved, kept[..kept.len() - 1]);
        }

        scene.replace(SizedBox::shrink().boxed());
        assert_eq!(
            scene.heard.borrow().last(),
            Some(&None),
            "nothing selected is left"
        );
    }

    // Both ends leave at once: the middle text keeps the selection, still
    // backward, so the moving end is at its start. The outer texts sit in
    // columns of their own, so emptying those leaves the middle text's
    // element in place.
    let nested = |first: bool, third: bool| {
        let around = |words: &'static str, outer: bool| {
            let children: Vec<BoxedView> = if outer {
                vec![Text::new(words).boxed()]
            } else {
                Vec::new()
            };
            Column::new(children).boxed()
        };
        Column::new(vec![
            around(FIRST, first),
            Text::new(SECOND).boxed(),
            around(THIRD, third),
        ])
        .boxed()
    };
    let mut scene = Scene::over(nested(true, true));
    let (first, third) = (scene.index_of(FIRST), scene.index_of(THIRD));
    scene.drag(scene.at(third, 0.5), scene.at(first, 0.3));
    scene.replace(nested(false, false));
    assert_eq!(scene.selected().as_deref(), Some(SECOND));
    assert!(scene.key(
        Key::Named(NamedKey::ArrowRight),
        Code::ArrowRight,
        Modifiers::SHIFT
    ));
    assert_eq!(
        scene.selected().as_deref(),
        Some(&SECOND[1..]),
        "still backward"
    );

    // The text a drag started on leaves while the button is held: the drag
    // goes on from the end that stays and follows the pointer to release.
    let mut scene = Scene::over(nested(true, true));
    let (first, second) = (scene.index_of(FIRST), scene.index_of(SECOND));
    let (from, over) = (scene.at(first, 0.3), scene.at(second, 0.5));
    scene.contact(Contact::Press, 0, from);
    for step in 1..=4_u32 {
        scene.laid.pump_for(Duration::from_millis(16));
        let at = from + (over - from) * (f64::from(step) / 4.0);
        scene.contact(Contact::Move, u64::from(step) * 16, at);
    }
    scene.replace(nested(false, true));
    let release = scene.at(scene.index_of(THIRD), 1.0) + Offset::new(2.0, 0.0);
    scene.contact(Contact::Move, 160, release);
    scene.contact(Contact::Release, 176, release);
    scene.settle();
    assert_eq!(
        scene.selected(),
        Some(format!("{SECOND}\n{THIRD}")),
        "the drag follows the pointer after its first text left"
    );
}

/// A drag ends where the button is released, even when the release lies
/// past the last move.
///
/// Red-check: end a drag without applying its release position (the
/// selection stops at the last move, inside the first text).
pub(crate) fn a_drag_ends_where_it_is_released() {
    let mut scene = Scene::two_texts();
    let (from, middle) = (scene.at(0, 0.0) + Offset::new(-2.0, 0.0), scene.at(0, 0.5));
    let release = scene.at(1, 1.0) + Offset::new(2.0, 0.0);
    scene.contact(Contact::Press, 0, from);
    for step in 1..=4_u32 {
        let at = from + (middle - from) * (f64::from(step) / 4.0);
        scene.laid.pump_for(Duration::from_millis(16));
        scene.contact(Contact::Move, u64::from(step) * 16, at);
    }
    scene.contact(Contact::Release, 80, release);
    scene.settle();
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );
}

/// A click that selects nothing still makes the area the keys' target, and
/// the keys cross into the next text at the shown end of a text a line
/// limit cuts, never through its hidden tail.
///
/// Red-check: take the focus only for a nonempty selection (select all
/// after a single click is not handled); step the keys through the whole
/// text (Shift+Right at the shown end selects nothing more).
pub(crate) fn a_click_targets_the_keys_which_cross_at_the_shown_end() {
    let mut scene = Scene::two_texts();
    scene.clicks(scene.at(1, 0.5), 1);
    assert!(
        scene.key(Key::character("a"), Code::KeyA, command()),
        "select all is handled"
    );
    assert_eq!(
        scene.selected().as_deref(),
        Some("Hello world\nSecond line")
    );

    let mut scene = Scene::over(
        Column::new((Text::new("Shown\nhidden").max_lines(1), Text::new(SECOND))).boxed(),
    );
    scene.clicks(scene.at(0, 0.3), 2);
    assert_eq!(scene.selected().as_deref(), Some("Shown"));
    assert!(scene.key(
        Key::Named(NamedKey::ArrowRight),
        Code::ArrowRight,
        Modifiers::SHIFT
    ));
    assert_eq!(scene.selected().as_deref(), Some("Shown\n"));
    assert!(scene.key(
        Key::Named(NamedKey::ArrowLeft),
        Code::ArrowLeft,
        Modifiers::SHIFT
    ));
    assert_eq!(scene.selected().as_deref(), Some("Shown"));
}

/// A blur listener elsewhere that panics while the area takes the focus
/// back still lets the click's selection commit and be reported, and the
/// panic reaches the caller.
///
/// Red-check: request the focus outside the panic containment (the
/// Shift+click leaves the selection at the word).
pub(crate) fn a_panicking_blur_listener_keeps_the_clicked_selection() {
    use flui_interaction::focus::FocusNode;
    use flui_widgets::Focus;

    let armed = Rc::new(std::cell::Cell::new(false));
    let node = FocusNode::with_debug_label("elsewhere");
    let failing = Rc::clone(&armed);
    let elsewhere = Focus::new(SizedBox::new(10.0, 10.0))
        .focus_node(Rc::clone(&node))
        .on_focus_change(move |_cx, focused| {
            assert!(focused || !failing.get(), "the blur listener fails");
        });
    let mut scene = Scene::over(Column::new((elsewhere, Text::new(FIRST))).boxed());
    let paragraph = scene.index_of(FIRST);
    scene.clicks(scene.at(paragraph, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));

    let _outcome = node.request_focus();
    scene.settle();
    armed.set(true);
    let end = scene.at(paragraph, 1.0) + Offset::new(4.0, 0.0);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scene.shift_click(end)));
    assert!(
        caught.is_err(),
        "the blur listener's panic reaches the caller"
    );
    assert_eq!(scene.selected().as_deref(), Some(FIRST));
}

/// Consecutive Shift+Down presses keep to the column they started in across
/// a shorter line, and Shift+End at a soft wrap stays on the line it ended:
/// Shift+End again does not move, and Shift+Home returns to that line's
/// start.
///
/// Red-check: take each line step's column from the current caret (the
/// second step lands near the start of the third line); resolve a line end
/// downstream (the second Shift+End selects the next line); drop a line
/// step's hit affinity (Shift+Home after landing on a wrapped line's end
/// stays put); drop the pointer hit's affinity (Shift+Home after a drag to
/// that line's end stays put); keep the old memory when a key leaves the
/// ends where they were (Shift+Down after Shift+End on the short line keeps
/// the first line's column).
pub(crate) fn line_keys_keep_their_column_and_their_line() {
    let mut scene = Scene::over(Column::new((Text::new("abcdefghij\nx\nabcdefghij"),)).boxed());
    // Near the top left, on the first line.
    let corner =
        |scene: &Scene| scene.laid.absolute_offset(scene.paragraphs()[0]) + Offset::new(3.0, 3.0);
    scene.clicks(corner(&scene), 2);
    assert_eq!(scene.selected().as_deref(), Some("abcdefghij"));
    for _ in 0..2 {
        assert!(scene.key(
            Key::Named(NamedKey::ArrowDown),
            Code::ArrowDown,
            Modifiers::SHIFT
        ));
    }
    assert_eq!(
        scene.selected().as_deref(),
        Some("abcdefghij\nx\nabcdefghij")
    );
    // Shift+End on the short line keeps the ends but forgets the column.
    let mut scene = Scene::over(Column::new((Text::new("abcdefghij\nx\nabcdefghij"),)).boxed());
    scene.clicks(corner(&scene), 2);
    assert!(scene.key(
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
        Modifiers::SHIFT
    ));
    assert_eq!(scene.selected().as_deref(), Some("abcdefghij\nx"));
    assert!(scene.key(Key::Named(NamedKey::End), Code::End, Modifiers::SHIFT));
    assert!(scene.key(
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
        Modifiers::SHIFT
    ));
    assert_eq!(scene.selected().as_deref(), Some("abcdefghij\nx\na"));

    let mut scene = Scene::over(
        Column::new((SizedBox::width(60.0).child(Text::new("aaaa bbbb cccc dddd")),)).boxed(),
    );
    scene.clicks(corner(&scene), 2);
    assert_eq!(scene.selected().as_deref(), Some("aaaa"));
    assert!(scene.key(Key::Named(NamedKey::End), Code::End, Modifiers::SHIFT));
    let line = scene.selected().expect("the first line");
    assert!(line.starts_with("aaaa") && !line.contains('b'), "{line:?}");
    assert!(scene.key(Key::Named(NamedKey::End), Code::End, Modifiers::SHIFT));
    assert_eq!(scene.selected(), Some(line), "a second Shift+End stays");
    assert!(scene.key(Key::Named(NamedKey::Home), Code::Home, Modifiers::SHIFT));
    assert_eq!(
        scene.heard.borrow().last(),
        Some(&None),
        "back to the line's start"
    );

    // Shift+Down onto the end of a shorter wrapped line keeps the end on
    // that line, so Shift+Home returns to its start.
    // A box as wide as "aaaa" wraps "aaaa b aaaa" into three lines, the
    // middle one short.
    let measured = Scene::over(Column::new((Text::new("aaaa"),)).boxed());
    let width = measured.laid.size(measured.paragraphs()[0]).width + 1.0;
    let mut scene =
        Scene::over(Column::new((SizedBox::width(width).child(Text::new("aaaa b aaaa")),)).boxed());
    scene.clicks(corner(&scene), 2);
    assert!(scene.key(Key::Named(NamedKey::End), Code::End, Modifiers::SHIFT));
    let first = scene.selected().expect("the first line");
    assert!(scene.key(
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
        Modifiers::SHIFT
    ));
    let two = scene.selected().expect("two lines");
    assert_eq!(two, "aaaa b ", "{first:?} then {two:?}");
    assert!(scene.key(Key::Named(NamedKey::Home), Code::Home, Modifiers::SHIFT));
    assert_eq!(
        scene.selected(),
        Some(first.clone()),
        "back to the second line's start"
    );

    // A drag released past the end of the short line leaves its end on
    // that line too.
    let mut scene =
        Scene::over(Column::new((SizedBox::width(width).child(Text::new("aaaa b aaaa")),)).boxed());
    let paragraph = scene.paragraphs()[0];
    let height = scene.laid.size(paragraph).height;
    let past_short_line =
        scene.laid.absolute_offset(paragraph) + Offset::new(width - 1.0, height / 2.0);
    scene.drag(corner(&scene), past_short_line);
    assert_eq!(scene.selected().as_deref(), Some("aaaa b "));
    assert!(scene.key(Key::Named(NamedKey::Home), Code::Home, Modifiers::SHIFT));
    assert_eq!(
        scene.selected(),
        Some(first),
        "a dragged end goes back to the second line's start"
    );
}

/// The pointer finds the nearest text by distance on screen: a text scaled
/// down by an ancestor is near where it paints, not where its own
/// coordinates would put it.
///
/// Red-check: compare distances in each text's own coordinates (the double
/// click selects the unscaled text, which is farther on screen).
pub(crate) fn the_nearest_text_is_nearest_on_screen() {
    use flui_painting::Alignment;
    use flui_widgets::Transform;

    let mut scene = Scene::over(
        Column::new((
            Transform::scale(0.1, 0.1)
                .alignment(Alignment::TOP_LEFT)
                .child(Text::new("Tiny")),
            Text::new("Plain"),
        ))
        .boxed(),
    );
    let (tiny, plain) = (scene.index_of("Tiny"), scene.index_of("Plain"));
    let tiny_bottom = scene.laid.absolute_offset(scene.paragraphs()[tiny]).dy
        + scene.laid.size(scene.paragraphs()[tiny]).height * 0.1;
    let plain_top = scene.laid.absolute_offset(scene.paragraphs()[plain]).dy;
    let gap = plain_top - tiny_bottom;
    assert!(gap > 0.0, "the gap lies between the painted texts");
    scene.clicks(Offset::new(1.0, tiny_bottom + gap * 0.25), 2);
    assert_eq!(scene.selected().as_deref(), Some("Tiny"));
}

/// With a selection, Esc clears it; with none, Esc reaches the enclosing
/// dismiss, as a modal route's pop would be.
///
/// Red-check: declare the area's dismiss unconditionally (the second Esc
/// never reaches the enclosing action).
pub(crate) fn esc_clears_then_reaches_the_enclosing_dismiss() {
    use flui_widgets::{Actions, CallbackAction, DismissIntent};

    let dismissed = Rc::new(std::cell::Cell::new(0_u32));
    let counted = Rc::clone(&dismissed);
    let heard: Heard = Rc::default();
    let recorded = Rc::clone(&heard);
    let area = SelectionArea::new(Column::new((Text::new(FIRST),)).boxed()).on_selection_changed(
        move |_cx, change| {
            recorded.borrow_mut().push(
                change
                    .selected
                    .as_ref()
                    .map(|selected| selected.text.clone()),
            );
        },
    );
    let outer = Actions::new(area).action::<DismissIntent>(CallbackAction::new(
        move |_cx, _: &DismissIntent| counted.set(counted.get() + 1),
    ));
    let mut laid = lay_out(WidgetsApp::new(outer), tight(400.0, 300.0));
    laid.pump();
    let mut scene = Scene { laid, heard };

    scene.clicks(scene.at(0, 0.2), 2);
    assert_eq!(scene.selected().as_deref(), Some("Hello"));
    let _ = scene.key(Key::Named(NamedKey::Escape), Code::Escape, Modifiers::NONE);
    assert_eq!(
        scene.heard.borrow().last(),
        Some(&None),
        "the first Esc clears"
    );
    assert_eq!(dismissed.get(), 0, "the clear does not dismiss");
    let _ = scene.key(Key::Named(NamedKey::Escape), Code::Escape, Modifiers::NONE);
    assert_eq!(dismissed.get(), 1, "the second Esc dismisses");
}

/// A listener that panics while Shift+End commits at a soft wrap does not
/// cost the end its line: after the panic, Shift+Home still returns to that
/// line's start.
///
/// Red-check: store the keys' memory after the commit returns (the panic
/// skips it, and Shift+Home stays put).
pub(crate) fn a_panicking_listener_keeps_what_the_keys_remember() {
    let measured = Scene::over(Column::new((Text::new("aaaa"),)).boxed());
    let width = measured.laid.size(measured.paragraphs()[0]).width + 1.0;
    let armed = Rc::new(std::cell::Cell::new(false));
    let heard: Heard = Rc::default();
    let (recorded, failing) = (Rc::clone(&heard), Rc::clone(&armed));
    let area = SelectionArea::new(
        Column::new((SizedBox::width(width).child(Text::new("aaaa b aaaa")),)).boxed(),
    )
    .on_selection_changed(move |_cx, change| {
        recorded.borrow_mut().push(
            change
                .selected
                .as_ref()
                .map(|selected| selected.text.clone()),
        );
        assert!(!failing.get(), "the listener fails");
    });
    let mut laid = lay_out(WidgetsApp::new(area), tight(400.0, 300.0));
    laid.pump();
    let mut scene = Scene { laid, heard };
    let corner = scene.laid.absolute_offset(scene.paragraphs()[0]) + Offset::new(3.0, 3.0);
    scene.clicks(corner, 2);
    assert_eq!(scene.selected().as_deref(), Some("aaaa"));

    armed.set(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scene.key(Key::Named(NamedKey::End), Code::End, Modifiers::SHIFT)
    }));
    armed.set(false);
    assert!(caught.is_err(), "the listener's panic reaches the caller");
    assert_eq!(scene.selected().as_deref(), Some("aaaa "));
    assert!(scene.key(Key::Named(NamedKey::Home), Code::Home, Modifiers::SHIFT));
    assert_eq!(
        scene.heard.borrow().last(),
        Some(&None),
        "back to the line's start"
    );
}
