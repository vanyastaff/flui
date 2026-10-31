//! `SelectionController`: reads and moves a `SelectionArea`'s selection from
//! outside, refuses ends the area does not present, and reports every change
//! with what made it.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_rendering::semantics::{DocumentPosition, DocumentSelection, SemanticsAnchor};
use flui_testing::replay::MouseScript;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::prelude::{BuildContext, IntoView, StatelessView};
use flui_widgets::{
    Column, SelectError, SelectionArea, SelectionChangeCause, SelectionController, SelectionInput,
    SizedBox, Text, WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

const FIRST: &str = "Hello world";
const SECOND: &str = "Grüße";

/// What `on_selection_changed` heard, in order.
type Heard = Rc<RefCell<Vec<(Option<String>, SelectionChangeCause)>>>;

struct Scene {
    laid: LaidOut,
    heard: Heard,
    controller: SelectionController,
    /// How often the controller's listenable notified.
    notified: Rc<Cell<usize>>,
}

impl Scene {
    /// An area over [`FIRST`] above [`SECOND`] with a controller.
    fn new() -> Self {
        let heard: Heard = Rc::default();
        let controller = SelectionController::new();
        let notified = Rc::new(Cell::new(0));
        let counter = Rc::clone(&notified);
        controller
            .listenable()
            .add_listener(Rc::new(move || counter.set(counter.get() + 1)));
        let app = Self::app(two_texts(), &heard, &controller);
        let mut laid = lay_out(app, tight(400.0, 300.0));
        laid.pump();
        Self {
            laid,
            heard,
            controller,
            notified,
        }
    }

    fn app(content: BoxedView, heard: &Heard, controller: &SelectionController) -> WidgetsApp {
        let recorded = Rc::clone(heard);
        WidgetsApp::new(
            SelectionArea::new(content)
                .controller(controller.clone())
                .on_selection_changed(move |_cx, change| {
                    recorded.borrow_mut().push((
                        change
                            .selected
                            .as_ref()
                            .map(|selected| selected.text.clone()),
                        change.cause,
                    ));
                }),
        )
    }

    /// The same area over `content` instead, its state kept.
    fn replace(&mut self, content: BoxedView) {
        let app = Self::app(content, &self.heard, &self.controller);
        self.laid.pump_widget(app);
        self.settle();
    }

    /// The listener's last report.
    fn last(&self) -> Option<(Option<String>, SelectionChangeCause)> {
        self.heard.borrow().last().cloned()
    }

    /// The bytes of the paragraph showing `words` that paint highlighted.
    fn painted(&self, words: &str) -> Option<String> {
        let paragraph = self.paragraph(words);
        self.laid.render_property(paragraph, "selection")
    }

    fn paragraph(&self, words: &str) -> flui_foundation::RenderId {
        self.laid
            .find_all_by_render_type("RenderParagraph")
            .into_iter()
            .find(|&paragraph| {
                self.laid
                    .render_property(paragraph, "text")
                    .is_some_and(|text| text.contains(words))
            })
            .expect("a paragraph shows the words")
    }

    /// A point `fraction` of the way across the paragraph showing `words`.
    fn at(&self, words: &str, fraction: f64) -> Offset<f64> {
        let paragraph = self.paragraph(words);
        let size = self.laid.size(paragraph);
        self.laid.absolute_offset(paragraph) + Offset::new(size.width * fraction, size.height / 2.0)
    }

    fn drag(&mut self, from: Offset<f64>, to: Offset<f64>) {
        let script = MouseScript::new("drag")
            .press(from)
            .wait(Duration::from_millis(16))
            .move_to(from + (to - from) * 0.5)
            .wait(Duration::from_millis(16))
            .move_to(to)
            .release()
            .build()
            .expect("the drag releases");
        self.laid.replay(&script);
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

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }
}

fn two_texts() -> BoxedView {
    Column::new(vec![Text::new(FIRST).boxed(), Text::new(SECOND).boxed()]).boxed()
}

fn span(anchor: &SemanticsAnchor, from: usize, to: usize) -> DocumentSelection {
    DocumentSelection::new(
        DocumentPosition::new(anchor.clone(), from),
        DocumentPosition::new(anchor.clone(), to),
    )
}

/// A controller lists the texts in reading order, selects a range, every
/// text and nothing, and reads back what the area holds; the area's listener
/// hears each change as programmatic and the controller notifies after each,
/// and a selection the area already holds is no change.
pub(crate) fn a_controller_selects_reads_and_clears() {
    let mut scene = Scene::new();
    let texts = scene.controller.texts();
    assert_eq!(texts.len(), 2, "both texts are presented");
    assert_eq!(
        scene.controller.selection(),
        None,
        "nothing is selected yet"
    );

    let hello = span(&texts[0], 0, 5);
    scene
        .controller
        .select(hello.clone())
        .expect("a range of a presented text");
    scene.settle();
    assert_eq!(scene.controller.selection(), Some(hello.clone()));
    assert_eq!(
        scene
            .controller
            .selected_text()
            .map(|selected| selected.text),
        Some("Hello".to_owned())
    );
    assert_eq!(
        scene.painted(FIRST).as_deref(),
        Some("Some(0..5)"),
        "it paints"
    );
    assert_eq!(
        scene.last(),
        Some((Some("Hello".to_owned()), SelectionChangeCause::Programmatic))
    );
    assert_eq!(
        scene.notified.get(),
        1,
        "the controller notifies the change"
    );

    scene.controller.select(hello).expect("the same range");
    assert_eq!(
        scene.heard.borrow().len(),
        1,
        "the same selection is no change"
    );
    assert_eq!(scene.notified.get(), 1, "nor a notification");

    scene.controller.select_all().expect("attached");
    scene.settle();
    assert_eq!(
        scene.last(),
        Some((
            Some(format!("{FIRST}\n{SECOND}")),
            SelectionChangeCause::Programmatic
        )),
        "every text is selected whole, in reading order"
    );

    scene.controller.clear().expect("attached");
    scene.settle();
    assert_eq!(scene.controller.selection(), None);
    assert_eq!(
        scene.painted(FIRST).as_deref(),
        Some("None"),
        "nothing paints"
    );
    assert_eq!(
        scene.last(),
        Some((None, SelectionChangeCause::Programmatic))
    );
    assert_eq!(scene.notified.get(), 3);
}

/// A controller's selection does not take the focus, so the keys do not
/// reach the area until the user selects in it.
pub(crate) fn a_controller_selection_leaves_the_focus_where_it_is() {
    let mut scene = Scene::new();
    let first = scene.controller.texts()[0].clone();
    scene
        .controller
        .select(span(&first, 0, 5))
        .expect("a range");
    scene.settle();
    let extended = scene.key(
        Key::Named(NamedKey::ArrowRight),
        Code::ArrowRight,
        Modifiers::SHIFT,
    );
    assert!(!extended, "the unfocused area does not take Shift+Right");
    assert_eq!(scene.painted(FIRST).as_deref(), Some("Some(0..5)"));
}

/// Ends the area does not present are refused, and so are bytes past a
/// text's end or inside a character; the selection and the listener are
/// left alone. A controller given to no area, or to an area that left, is
/// not attached.
pub(crate) fn a_controller_refuses_ends_the_area_does_not_present() {
    let detached = SelectionController::new();
    assert_eq!(
        detached.select(span(&SemanticsAnchor::new(), 0, 0)),
        Err(SelectError::NotAttached)
    );
    assert_eq!(detached.select_all(), Err(SelectError::NotAttached));
    assert_eq!(detached.clear(), Err(SelectError::NotAttached));
    assert_eq!(detached.texts(), Vec::new());

    let mut scene = Scene::new();
    let texts = scene.controller.texts();
    let stranger = SemanticsAnchor::new();
    assert_eq!(
        scene.controller.select(DocumentSelection::new(
            DocumentPosition::new(texts[0].clone(), 0),
            DocumentPosition::new(stranger.clone(), 0),
        )),
        Err(SelectError::UnknownText(stranger))
    );
    // "ü" takes bytes 2..4 of SECOND.
    assert_eq!(
        scene.controller.select(span(&texts[1], 0, 3)),
        Err(SelectError::NotABoundary {
            text: texts[1].clone(),
            byte: 3,
        })
    );
    assert_eq!(
        scene.controller.select(span(&texts[0], 0, FIRST.len() + 1)),
        Err(SelectError::NotABoundary {
            text: texts[0].clone(),
            byte: FIRST.len() + 1,
        })
    );
    scene.settle();
    assert_eq!(scene.controller.selection(), None, "nothing was selected");
    assert!(
        scene.heard.borrow().is_empty(),
        "the listener heard nothing"
    );
    assert_eq!(scene.notified.get(), 0);

    scene
        .controller
        .select(span(&texts[1], 0, 4))
        .expect("a boundary after the two-byte character");
    scene
        .laid
        .pump_widget(WidgetsApp::new(SizedBox::new(10.0, 10.0)));
    scene.settle();
    assert_eq!(scene.controller.selection(), None, "the area left");
    assert_eq!(
        scene.controller.select(span(&texts[1], 0, 1)),
        Err(SelectError::NotAttached)
    );
}

/// The listener hears what made each change: a mouse drag, a key, a text
/// that left while holding an end, and a controller.
pub(crate) fn the_listener_hears_what_changed_the_selection() {
    let mut scene = Scene::new();
    scene.drag(scene.at(FIRST, 0.0), scene.at(SECOND, 1.0));
    assert_eq!(
        scene.last().map(|(_, cause)| cause),
        Some(SelectionChangeCause::User(SelectionInput::Pointer))
    );
    scene.key(
        Key::Named(NamedKey::ArrowLeft),
        Code::ArrowLeft,
        Modifiers::SHIFT,
    );
    assert_eq!(
        scene.last().map(|(_, cause)| cause),
        Some(SelectionChangeCause::User(SelectionInput::Keyboard))
    );
    scene.replace(Column::new(vec![Text::new(FIRST).boxed()]).boxed());
    assert_eq!(
        scene.last(),
        Some((Some(FIRST.to_owned()), SelectionChangeCause::TextRemoved)),
        "the end on the text that left moves onto the one that stays"
    );
    scene.controller.clear().expect("attached");
    assert_eq!(
        scene.last().map(|(_, cause)| cause),
        Some(SelectionChangeCause::Programmatic)
    );
    assert_eq!(
        scene.notified.get(),
        scene.heard.borrow().len(),
        "one notification per change"
    );
}

/// A listener that panics on a controller's change still lets the
/// controller notify; the panic reaches the caller afterwards.
pub(crate) fn a_panicking_listener_still_lets_the_controller_notify() {
    let controller = SelectionController::new();
    let notified = Rc::new(Cell::new(0));
    let counter = Rc::clone(&notified);
    controller
        .listenable()
        .add_listener(Rc::new(move || counter.set(counter.get() + 1)));
    let area = SelectionArea::new(two_texts())
        .controller(controller.clone())
        .on_selection_changed(|_cx, _change| -> () { panic!("the listener fails") });
    let mut laid = lay_out(WidgetsApp::new(area), tight(400.0, 300.0));
    laid.pump();
    let first = controller.texts()[0].clone();
    let failed = catch_unwind(AssertUnwindSafe(|| controller.select(span(&first, 0, 5))));
    assert!(failed.is_err(), "the listener's panic reaches the caller");
    assert_eq!(notified.get(), 1, "the controller still notified");
    assert_eq!(
        controller.selection(),
        Some(span(&first, 0, 5)),
        "it is committed"
    );
}

/// Calls `select_all` on its controller from `build`, keeping the answer.
#[derive(Clone, Debug, StatelessView)]
struct SelectsInBuild {
    controller: SelectionController,
    answer: Rc<RefCell<Option<Result<(), SelectError>>>>,
}

impl StatelessView for SelectsInBuild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        *self.answer.borrow_mut() = Some(self.controller.select_all());
        SizedBox::new(10.0, 10.0)
    }
}

/// A controller called while an element builds refuses before anything
/// changes: no selection, no report, no notification.
pub(crate) fn a_controller_refuses_a_change_during_a_build() {
    let mut scene = Scene::new();
    let answer = Rc::new(RefCell::new(None));
    scene.replace(
        Column::new(vec![
            Text::new(FIRST).boxed(),
            Text::new(SECOND).boxed(),
            SelectsInBuild {
                controller: scene.controller.clone(),
                answer: Rc::clone(&answer),
            }
            .boxed(),
        ])
        .boxed(),
    );
    assert!(
        matches!(*answer.borrow(), Some(Err(SelectError::DuringBuild { .. }))),
        "refused during the build, answered {:?}",
        answer.borrow()
    );
    assert_eq!(scene.controller.selection(), None);
    assert_eq!(*scene.heard.borrow(), Vec::new());
    assert_eq!(scene.notified.get(), 0);
}
