//! The selection menu of an `EditableText`: the toolbar a touch selection
//! shows over the selection, and the popup a secondary click, the menu key
//! or assistive technology opens, each offering what the field can do now
//! and the commands a `DefaultSelectionControls` and the field add. The app
//! is a `WidgetsApp`, whose `PlainSelectionMenu` draws both, so every
//! button and item below is found and pressed as assistive technology reads
//! it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::focus::FocusNode;
use flui_painting::typography::{TextDirection, TextStyle};
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::pointer::PointerButton;
use flui_testing::a11y::{A11yTree, Action, ActionRequest, Role, TreeId};
use flui_testing::replay::{MouseScript, PointerScript};
use flui_view::prelude::{Signal, SignalWriteExt};
use flui_view::{BoxedView, BuildContext, ViewExt};
use flui_widgets::{
    Actions, CallbackAction, CaretHandle, Column, ContextMenuArea, DefaultSelectionControls,
    Directionality, EditableText, Focus, GestureCommand, HandleInfo, Intent, LongPressSelection,
    ScrollController, ScrollView, SelectionHandles, SelectionMenuPolicy, ShowContextMenuIntent,
    SingleChildScrollView, SizedBox, Text, TextEditingController, WidgetsApp,
};

use crate::common::{LaidOut, SignalProbe, lay_out, tight};

const WORDS: &str = "alpha beta gamma";
const EDITING: [&str; 4] = ["Cut", "Copy", "Paste", "Select all"];
/// The side of the square handles.
const HANDLE: f64 = 22.0;
/// How far the toolbar stands over the line.
const TOOLBAR_GAP: f64 = 8.0;

/// Square handles that note the height of the line they hang from.
#[derive(Clone, Default)]
struct Square(Rc<Cell<f64>>);

impl SelectionHandles for Square {
    fn build(&self, info: &HandleInfo, _ctx: &dyn BuildContext) -> BoxedView {
        self.0.set(info.line_height);
        SizedBox::new(HANDLE, HANDLE).boxed()
    }
}

/// Shout the selection, a command the app adds for every field.
#[derive(Clone, Copy, Debug)]
struct Shout;
impl Intent for Shout {}

/// Whisper the selection, a command one field adds.
#[derive(Clone, Copy, Debug)]
struct Whisper;
impl Intent for Whisper {}

fn style() -> TextStyle {
    TextStyle::default().with_font_size(16.0)
}

struct Scene {
    laid: LaidOut,
    controller: TextEditingController,
    field_focus: Rc<FocusNode>,
    /// A node outside the field, holding the focus somewhere else.
    elsewhere: Rc<FocusNode>,
    log: Rc<RefCell<Vec<&'static str>>>,
    line_height: Rc<Cell<f64>>,
    /// Whether the field draws [`Square`] handles, a new look each build,
    /// rather than the app's, one look across rebuilds.
    square: bool,
}

impl Scene {
    /// A field over [`WORDS`] in a `WidgetsApp`, configured by `field`,
    /// under `controls` (the app's own when `None`), with `Actions` above
    /// answering [`Shout`] and [`Whisper`] into the log. Semantics on.
    fn with(
        field: impl FnOnce(EditableText) -> EditableText,
        controls: Option<fn(DefaultSelectionControls) -> DefaultSelectionControls>,
    ) -> Self {
        Self::framed(field, controls, |page| page)
    }

    /// [`Self::with`], the page put in `frame` before the app takes it.
    fn framed(
        field: impl FnOnce(EditableText) -> EditableText,
        controls: Option<fn(DefaultSelectionControls) -> DefaultSelectionControls>,
        frame: impl FnOnce(BoxedView) -> BoxedView,
    ) -> Self {
        Self::build(field, controls, frame, true)
    }

    /// The field with every toolbar trigger on and the app's handles, whose
    /// look stays the same when the field rebuilds.
    fn app_handles() -> Self {
        Self::build(
            |field| field.menu_policy(SelectionMenuPolicy::ALWAYS),
            None,
            |page| page,
            false,
        )
    }

    fn build(
        field: impl FnOnce(EditableText) -> EditableText,
        controls: Option<fn(DefaultSelectionControls) -> DefaultSelectionControls>,
        frame: impl FnOnce(BoxedView) -> BoxedView,
        square: bool,
    ) -> Self {
        let mut scene = Self {
            laid: lay_out(SizedBox::new(0.0, 0.0), tight(400.0, 300.0)),
            controller: TextEditingController::with_text(WORDS),
            field_focus: FocusNode::with_debug_label("field"),
            elsewhere: FocusNode::with_debug_label("elsewhere"),
            log: Rc::default(),
            line_height: Rc::default(),
            square,
        };
        scene.laid = lay_out(scene.app(field, controls, frame), tight(400.0, 300.0));
        scene.laid.enable_semantics();
        scene.laid.pump();
        scene
    }

    /// Builds the app again around the same controller, focus nodes and
    /// log, with the field configured by `field`, so it updates in place.
    fn rebuild(&mut self, field: impl FnOnce(EditableText) -> EditableText) {
        let app = self.app(field, None, |page| page);
        self.laid.pump_widget(app);
        self.settle();
    }

    fn app(
        &self,
        field: impl FnOnce(EditableText) -> EditableText,
        controls: Option<fn(DefaultSelectionControls) -> DefaultSelectionControls>,
        frame: impl FnOnce(BoxedView) -> BoxedView,
    ) -> WidgetsApp {
        let editable = EditableText::new(self.controller.clone(), Rc::clone(&self.field_focus))
            .text_style(style());
        let editable = field(if self.square {
            editable.handles(Square(Rc::clone(&self.line_height)))
        } else {
            editable
        });
        let content = Column::new((SizedBox::new(400.0, 40.0).child(editable),)).boxed();
        let content = match controls {
            Some(controls) => controls(DefaultSelectionControls::new(content)).boxed(),
            None => content,
        };
        let (shout, whisper, menu) = (
            Rc::clone(&self.log),
            Rc::clone(&self.log),
            Rc::clone(&self.log),
        );
        let home = Actions::new(content)
            .action(CallbackAction::new(move |_cx, _: &Shout| {
                shout.borrow_mut().push("shout");
            }))
            .action(CallbackAction::new(move |_cx, _: &Whisper| {
                whisper.borrow_mut().push("whisper");
            }))
            .action(CallbackAction::new(
                move |_cx, _: &ShowContextMenuIntent| {
                    menu.borrow_mut().push("menu");
                },
            ));
        // The field sits a toolbar's room below the top, so the toolbar
        // goes over the line.
        let room = Focus::new(SizedBox::new(400.0, 80.0)).focus_node(Rc::clone(&self.elsewhere));
        WidgetsApp::new(frame(Column::new((room, home)).boxed()))
    }

    /// The field with every toolbar trigger on, whatever the host.
    fn plain() -> Self {
        Self::with(|field| field.menu_policy(SelectionMenuPolicy::ALWAYS), None)
    }

    fn tree(&self) -> A11yTree {
        self.laid.a11y_tree().expect("semantics enabled")
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
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

    fn long_press(&mut self, at: Offset<f64>) {
        self.laid
            .replay(&PointerScript::long_press(at, Duration::from_millis(800)));
        self.settle();
    }

    fn tap(&mut self, at: Offset<f64>) {
        self.laid.replay(&PointerScript::tap(at));
        self.settle();
    }

    fn secondary_click(&mut self, at: Offset<f64>) {
        let click = MouseScript::new("secondary click")
            .press_button(at, PointerButton::SECONDARY)
            .wait(Duration::from_millis(16))
            .release()
            .build()
            .expect("the click releases its button");
        self.laid.replay(&click);
        self.settle();
    }

    fn click(&mut self, at: Offset<f64>) {
        let click = MouseScript::new("click")
            .press(at)
            .wait(Duration::from_millis(16))
            .release()
            .build()
            .expect("the click releases its button");
        self.laid.replay(&click);
        self.settle();
    }

    fn key(&mut self, key: NamedKey) {
        let manager = self.laid.focus_manager();
        for state in [KeyState::Down, KeyState::Up] {
            let event = KeyEvent::new(
                state,
                Key::Named(key),
                Code::Unidentified,
                EventTime::from_nanos(0),
            );
            let _ = manager.dispatch_key_event(&event);
        }
        self.settle();
    }

    /// The labels of the nodes with `role`, in tree order.
    fn labels(&self, role: Role) -> Vec<String> {
        self.tree()
            .nodes()
            .filter(|node| node.role() == role)
            .filter_map(|node| node.label().map(str::to_owned))
            .collect::<Vec<String>>()
    }

    /// The toolbar's buttons; empty while no toolbar shows.
    fn toolbar(&self) -> Vec<String> {
        if self.tree().find(Role::Toolbar).is_err() {
            return Vec::new();
        }
        self.labels(Role::Button)
    }

    /// The open popup's items; empty while none is open.
    fn popup(&self) -> Vec<String> {
        self.labels(Role::MenuItem)
    }

    /// The middle of the node with `role` and `label`.
    fn center_of(&self, role: Role, label: &str) -> Offset<f64> {
        let tree = self.tree();
        let node = tree
            .find_by_role_and_label(role, label)
            .unwrap_or_else(|error| panic!("{label} is shown: {error}"));
        let bounds = tree.window_bounds(&node).expect("the node has bounds");
        Offset::new(
            f64::midpoint(bounds.x0, bounds.x1),
            f64::midpoint(bounds.y0, bounds.y1),
        )
    }

    /// The middle of the handle under the selection's end `width` along
    /// the line: its top centre is at the bottom of the caret there.
    fn end_handle(&self, width: f64) -> Offset<f64> {
        self.laid.absolute_offset(self.editable())
            + Offset::new(width, self.line_height.get() + HANDLE / 2.0)
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

fn strings(labels: &[&str]) -> Vec<String> {
    labels
        .iter()
        .map(|label| (*label).to_owned())
        .collect::<Vec<String>>()
}

/// A long press selects a word and shows the toolbar over it with the
/// editing commands; Copy writes the selection to the clipboard and takes
/// the toolbar away, leaving the selection.
///
/// Red-check: show no toolbar after a long press (no toolbar node); place
/// it unclamped (it hangs off the left edge).
pub(crate) fn a_long_press_shows_the_toolbar_whose_copy_writes_the_clipboard() {
    let mut scene = Scene::plain();
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.controller.selection(), 6..10);
    assert_eq!(scene.toolbar(), strings(&EDITING));
    let tree = scene.tree();
    let bar = tree.find(Role::Toolbar).expect("a toolbar shows");
    let bar = tree.window_bounds(&bar).expect("the toolbar has bounds");
    let line_top = scene.laid.absolute_offset(scene.editable()).dy;
    assert!(
        (bar.y1 - (line_top - TOOLBAR_GAP)).abs() < 0.5,
        "the toolbar stands over the line: {bar:?}"
    );
    // Centred over the word, it would hang off the window's left edge.
    assert!(
        (bar.x0 - TOOLBAR_GAP).abs() < 0.5,
        "the toolbar is kept inside the window: {bar:?}"
    );

    scene.tap(scene.center_of(Role::Button, "Copy"));
    assert_eq!(scene.laid.clipboard().text().as_deref(), Some("beta"));
    assert!(
        scene.toolbar().is_empty(),
        "a chosen item takes the toolbar"
    );
    assert_eq!(
        scene.controller.selection(),
        6..10,
        "copy keeps the selection"
    );
}

/// The toolbar offers only what the field can do: an obscured field copies
/// and cuts nothing, so its toolbar has Paste and Select all.
///
/// Red-check: offer every item whatever its action says (Cut and Copy
/// show).
pub(crate) fn the_toolbar_offers_only_what_the_field_can_do() {
    let mut scene = Scene::with(|field| field.obscure_text(true), None);
    scene.long_press(scene.along(20.0));
    assert_eq!(scene.controller.selection(), 0..WORDS.len());
    assert_eq!(scene.toolbar(), strings(&["Paste", "Select all"]));
}

/// A secondary click on the selection keeps it and opens the popup with the
/// editing commands; one elsewhere moves the caret there first, so only
/// Paste and Select all are offered. Choosing Copy copies.
///
/// Red-check: let a secondary press start the selection drag (the click on
/// the selection collapses it, and Cut and Copy are missing).
pub(crate) fn a_secondary_click_opens_the_popup_on_the_selection() {
    let mut scene = Scene::plain();
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(6, 10);
    scene.settle();

    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.controller.selection(), 6..10);
    assert_eq!(scene.popup(), strings(&EDITING));
    scene.click(scene.center_of(Role::MenuItem, "Copy"));
    assert_eq!(scene.laid.clipboard().text().as_deref(), Some("beta"));
    assert!(scene.popup().is_empty(), "a chosen item closes the popup");

    scene.secondary_click(scene.along(width_of("alpha beta ga")));
    assert!(!scene.controller.has_selection(), "the caret moved there");
    assert_eq!(scene.popup(), strings(&["Paste", "Select all"]));
    scene.key(NamedKey::Escape);
    assert!(scene.popup().is_empty(), "Escape closes the popup");
}

/// The menu key in a focused field and assistive technology's
/// `ShowContextMenu` on it open the popup.
///
/// Red-check: leave `ShowContextMenuIntent` unbound in the field (the key
/// opens nothing).
pub(crate) fn the_menu_key_and_assistive_technology_open_the_popup() {
    let mut scene = Scene::plain();
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(0, 5);
    scene.settle();

    scene.key(NamedKey::ContextMenu);
    assert_eq!(scene.popup(), strings(&EDITING));
    assert!(scene.log.borrow().is_empty(), "the field answers the key");
    scene.key(NamedKey::Escape);
    assert_eq!(scene.popup(), Vec::<String>::new());

    // Assistive technology may open the menu of a field without the focus;
    // the field takes it, so paste and the edits act on it.
    let _ = scene.elsewhere.request_focus();
    scene.settle();
    assert!(!scene.field_focus.has_focus());
    let field = scene
        .tree()
        .nodes()
        .find(|node| node.role() == Role::TextInput)
        .expect("the field's node")
        .id();
    assert!(
        scene
            .tree()
            .nodes()
            .any(|node| node.id() == field && node.supports_action(Action::ShowContextMenu)),
        "the field offers ShowContextMenu"
    );
    scene
        .laid
        .invoke_semantics_action(ActionRequest {
            action: Action::ShowContextMenu,
            target_tree: TreeId::ROOT,
            target_node: field,
            data: None,
        })
        .expect("the field takes ShowContextMenu");
    scene.settle();
    assert_eq!(scene.popup(), strings(&EDITING));
    // The popup holds the focus while open and gives it back on closing.
    scene.key(NamedKey::Escape);
    assert!(scene.field_focus.has_focus(), "the menu focused the field");
}

/// Assistive technology's click on a toolbar button performs it and takes
/// the toolbar away, as a tap does.
///
/// Red-check: close the toolbar only from the button's tap (it stays).
pub(crate) fn an_assistive_click_on_the_toolbar_takes_it_away() {
    let mut scene = Scene::plain();
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&EDITING));

    let tree = scene.tree();
    let copy = tree
        .find_by_role_and_label(Role::Button, "Copy")
        .expect("the toolbar offers Copy")
        .id();
    scene
        .laid
        .invoke_semantics_action(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: copy,
            data: None,
        })
        .expect("the button takes Click");
    scene.settle();
    assert_eq!(scene.laid.clipboard().text().as_deref(), Some("beta"));
    assert_eq!(scene.toolbar(), Vec::<String>::new());
}

/// The toolbar moves with the selection when the page scrolls under it: a
/// tap where Copy went copies. Scrolled to the window's top, it is placed
/// again, under the line.
///
/// Red-check: place the toolbar once where the selection was (the tap
/// lands over it); hear only a leader's resize (the scrolled toolbar
/// leaves the window).
pub(crate) fn the_toolbar_follows_a_scroll() {
    let scroll = ScrollController::new();
    let page = scroll.clone();
    let mut scene = Scene::framed(
        |field| field.menu_policy(SelectionMenuPolicy::ALWAYS),
        None,
        move |content| {
            // Room over the field, so the scrolled toolbar still stands
            // over the line, and below it, so the page scrolls.
            let room = SizedBox::new(400.0, 100.0);
            SingleChildScrollView::new()
                .controller(page)
                .child(Column::new((room, content, SizedBox::new(400.0, 600.0))))
                .boxed()
        },
    );
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&EDITING));
    let copy = scene.center_of(Role::Button, "Copy");

    let rise = 60.0;
    scroll.jump_to(rise).expect("no build is running");
    scene.settle();
    scene.tap(copy - Offset::new(0.0, rise));
    assert_eq!(scene.laid.clipboard().text().as_deref(), Some("beta"));

    // Scrolled up to the window's top, the line leaves no room over it:
    // the toolbar is placed again, under the selection.
    scene.long_press(scene.along(width_of("alpha be")));
    scroll.jump_to(150.0).expect("no build is running");
    scene.settle();
    let tree = scene.tree();
    let bar = tree.find(Role::Toolbar).expect("a toolbar shows");
    let bar = tree.window_bounds(&bar).expect("the toolbar has bounds");
    let line = scene.laid.absolute_offset(scene.editable()).dy;
    assert!(bar.y0 > line, "the toolbar went under the line: {bar:?}");
    scene.tap(scene.center_of(Role::Button, "Select all"));
    assert_eq!(scene.controller.selection(), 0..WORDS.len());
}

/// A `DefaultSelectionControls` command and the field's own follow the
/// editing commands, and perform through the app's `Actions`.
///
/// Red-check: leave out the scope's commands (Shout is missing).
pub(crate) fn added_commands_follow_the_editing_commands() {
    let mut scene = Scene::with(
        |field| field.menu_command(GestureCommand::new("Whisper", Whisper)),
        Some(|controls| controls.command(GestureCommand::new("Shout", Shout))),
    );
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(
        scene.toolbar(),
        strings(&["Cut", "Copy", "Paste", "Select all", "Shout", "Whisper"])
    );
    scene.tap(scene.center_of(Role::Button, "Shout"));
    assert_eq!(*scene.log.borrow(), ["shout"]);
}

/// Dragging a handle takes the toolbar away and its release shows it again;
/// a key takes it away with the handles.
///
/// Red-check: keep the toolbar through a handle drag.
pub(crate) fn a_handle_drag_hides_the_toolbar_until_it_lets_go() {
    let mut scene = Scene::plain();
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&EDITING));

    let from = scene.end_handle(width_of("alpha beta"));
    let to = scene.end_handle(width_of("alpha beta ga"));
    let mut during = None;
    let drag = PointerScript::drag(from, to, 12, Duration::from_millis(16));
    let last = drag.steps().len() - 1;
    scene.laid.replay_with(&drag, |laid, index| {
        // The frame before the release has laid out the dragged handle.
        if index + 1 == last {
            let tree = laid.a11y_tree().expect("semantics enabled");
            during = Some(tree.find(Role::Toolbar).is_ok());
        }
    });
    scene.settle();
    assert_eq!(during, Some(false), "a drag takes the toolbar");
    assert_eq!(scene.controller.selection(), 6..13);
    assert_eq!(scene.toolbar(), strings(&EDITING), "letting go shows it");

    scene.key(NamedKey::ArrowLeft);
    assert!(scene.toolbar().is_empty(), "a key takes it");
}

/// A policy that shows the toolbar after no long press shows none, and a
/// field with no menu opens no popup though the app gives one, and leaves
/// the menu key to an enclosing answer.
///
/// Red-check: ignore the field's policy (the toolbar shows); bind the
/// menu key in a field with no menu (the enclosing answer is not heard).
pub(crate) fn a_policy_or_no_menu_keeps_the_menu_away() {
    let mut scene = Scene::with(|field| field.menu_policy(SelectionMenuPolicy::NEVER), None);
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.controller.selection(), 6..10);
    assert_eq!(scene.toolbar(), Vec::<String>::new());

    let mut scene = Scene::with(EditableText::no_menu, None);
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(6, 10);
    scene.settle();
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), Vec::<String>::new());
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), Vec::<String>::new());
    // The menu key goes to the enclosing answer the field would hide.
    scene.key(NamedKey::ContextMenu);
    assert_eq!(*scene.log.borrow(), ["menu"]);
}

/// A tap on the caret's handle shows the toolbar with what a caret allows,
/// paste and select all, and the next tap takes it away.
///
/// Red-check: let a lift from the caret's handle do nothing (no toolbar).
pub(crate) fn a_tap_on_the_caret_handle_toggles_the_toolbar() {
    let mut scene = Scene::with(
        |field| {
            field
                .caret_handle(CaretHandle::on_tap())
                .menu_policy(SelectionMenuPolicy::ALWAYS)
        },
        None,
    );
    let caret = start_of("alpha ");
    scene.tap(scene.along(caret));
    assert_eq!(scene.controller.caret_byte_offset(), 6);
    assert_eq!(
        scene.toolbar(),
        Vec::<String>::new(),
        "a tap shows no toolbar"
    );

    scene.tap(scene.end_handle(caret));
    assert_eq!(scene.toolbar(), strings(&["Paste", "Select all"]));
    scene.tap(scene.end_handle(caret));
    assert_eq!(scene.toolbar(), Vec::<String>::new());
}

/// A field rebuilt with another command while its toolbar shows offers it
/// at once.
///
/// Red-check: leave a shown toolbar as the field's last build left it
/// (Whisper is missing).
pub(crate) fn a_rebuilt_field_refreshes_its_shown_toolbar() {
    let mut scene = Scene::app_handles();
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&EDITING));

    scene.rebuild(|field| {
        field
            .menu_policy(SelectionMenuPolicy::ALWAYS)
            .menu_command(GestureCommand::new("Whisper", Whisper))
    });
    assert_eq!(
        scene.toolbar(),
        strings(&["Cut", "Copy", "Paste", "Select all", "Whisper"])
    );
    scene.tap(scene.center_of(Role::Button, "Whisper"));
    assert_eq!(*scene.log.borrow(), ["whisper"]);

    scene.rebuild(|field| field.menu_policy(SelectionMenuPolicy::ALWAYS).no_menu());
    assert_eq!(scene.toolbar(), Vec::<String>::new());
}

/// Commands wider together than the window leave the trailing ones behind
/// "More", which shows them in a column with "Back"; assistive technology
/// finds exactly the buttons that show, and choosing a hidden command
/// performs it and takes the toolbar away. A toolbar shown again starts
/// from its row.
///
/// Red-check: lay every button out (the last ones run past the window's
/// right edge); show the row's buttons under the column (Cut is still
/// announced).
pub(crate) fn a_toolbar_wider_than_the_window_moves_its_last_commands_behind_more() {
    let mut scene = Scene::with(
        |field| {
            field
                .menu_policy(SelectionMenuPolicy::ALWAYS)
                .menu_command(GestureCommand::new("Whisper to the far corner", Whisper))
        },
        Some(|controls| controls.command(GestureCommand::new("Shout across the room", Shout))),
    );
    let word = scene.along(width_of("alpha be"));
    scene.long_press(word);
    let row = scene.toolbar();
    assert_eq!(row.first().map(String::as_str), Some("Cut"));
    assert_eq!(row.last().map(String::as_str), Some("More"));
    assert!(
        !row.iter().any(|label| label == "Whisper to the far corner"),
        "the last command is behind More: {row:?}"
    );
    let inside = |scene: &Scene| {
        let tree = scene.tree();
        for label in scene.toolbar() {
            let node = tree
                .find_by_role_and_label(Role::Button, &label)
                .expect("the toolbar shows the button");
            let bounds = tree.window_bounds(&node).expect("the button has bounds");
            assert!(
                bounds.x0 >= 0.0 && bounds.x1 <= 400.0 && bounds.y0 >= 0.0,
                "{label} is inside the window: {bounds:?}"
            );
        }
    };
    inside(&scene);

    scene.tap(scene.center_of(Role::Button, "More"));
    let column = scene.toolbar();
    assert!(
        scene.center_of(Role::Button, "Back").dy
            < scene
                .center_of(Role::Button, "Whisper to the far corner")
                .dy,
        "Back heads the column: {column:?}"
    );
    assert!(
        column.contains(&"Whisper to the far corner".to_owned())
            && !column.contains(&"Cut".to_owned()),
        "the column holds what the row left over: {column:?}"
    );
    assert_eq!(
        column.len() + row.len(),
        8,
        "every command shows in one of them, beside More and Back"
    );
    inside(&scene);

    scene.tap(scene.center_of(Role::Button, "Back"));
    assert_eq!(scene.toolbar(), row);
    scene.tap(scene.center_of(Role::Button, "More"));
    scene.tap(scene.center_of(Role::Button, "Whisper to the far corner"));
    assert_eq!(*scene.log.borrow(), ["whisper"]);
    assert_eq!(scene.toolbar(), Vec::<String>::new());

    scene.long_press(word);
    assert_eq!(
        scene.toolbar(),
        row,
        "a toolbar shown again starts from its row"
    );
}

/// A field reading right to left lays its toolbar out from the right,
/// though the toolbar stands in an overlay above a left-to-right page.
///
/// Red-check: build the toolbar under the overlay's direction (Cut is left
/// of Copy).
pub(crate) fn the_toolbar_reads_in_the_field_s_direction() {
    let mut scene = Scene::framed(
        |field| field.menu_policy(SelectionMenuPolicy::ALWAYS),
        None,
        |page| Directionality::new(TextDirection::Rtl, page).boxed(),
    );
    scene.long_press(scene.along(390.0));
    assert_eq!(scene.toolbar(), strings(&EDITING));
    let (cut, copy) = (
        scene.center_of(Role::Button, "Cut"),
        scene.center_of(Role::Button, "Copy"),
    );
    assert!(cut.dx > copy.dx, "Cut starts the row from the right");
}

/// A field rebuilt with no menu while its popup is open closes the popup.
///
/// Red-check: close a replaced look's popup only at the next opening (it
/// stays open).
pub(crate) fn a_field_rebuilt_without_its_menu_closes_the_popup() {
    let mut scene = Scene::plain();
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(6, 10);
    scene.settle();
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), strings(&EDITING));

    scene.rebuild(EditableText::no_menu);
    assert_eq!(scene.popup(), Vec::<String>::new());
}

/// A secondary click in a field inside a `ContextMenuArea` opens the
/// field's popup alone: the field's tap wins the arena over the area's.
///
/// Red-check: open the field's popup on the raw press, with no tap of its
/// own in the arena (the area's menu opens over it).
pub(crate) fn a_field_s_secondary_click_wins_over_an_enclosing_menu() {
    let mut scene = Scene::framed(
        |field| field,
        None,
        |page| {
            ContextMenuArea::try_new(GestureCommand::new("Shout", Shout), [])
                .expect("one labelled command")
                .child(page)
                .boxed()
        },
    );
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(6, 10);
    scene.settle();
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), strings(&EDITING));
}

/// A field rebuilt with other commands while its popup is open closes the
/// popup, which offered the old ones. Only the field rebuilds, so the
/// app's look stays the same.
///
/// Red-check: close the open popup only when the look changes (it stays,
/// without Whisper).
pub(crate) fn a_field_rebuilt_with_other_commands_closes_the_popup() {
    let controller = TextEditingController::with_text(WORDS);
    let field_focus = FocusNode::with_debug_label("field");
    let whisper = Rc::new(Cell::new(false));
    let count: Rc<RefCell<Option<Signal<u32>>>> = Rc::default();
    let probe = {
        let (controller, field_focus) = (controller.clone(), Rc::clone(&field_focus));
        let (whisper, count) = (Rc::clone(&whisper), Rc::clone(&count));
        SignalProbe::new(move |signals| {
            count.replace(Some(signals.count));
            let field =
                EditableText::new(controller.clone(), Rc::clone(&field_focus)).text_style(style());
            let field = if whisper.get() {
                field.menu_command(GestureCommand::new("Whisper", Whisper))
            } else {
                field
            };
            SizedBox::new(400.0, 40.0).child(field)
        })
    };
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let home = Actions::new(Column::new((probe.view(),)))
        .action(CallbackAction::new(|_cx, _: &Whisper| {}));
    let page = Column::new((SizedBox::new(400.0, 80.0), home));
    let mut laid = lay_out(WidgetsApp::new(page), tight(400.0, 300.0));
    laid.enable_semantics();
    laid.pump();
    let mut scene = Scene {
        laid,
        controller,
        field_focus,
        elsewhere: FocusNode::with_debug_label("elsewhere"),
        log,
        line_height: Rc::default(),
        square: false,
    };
    let _ = scene.field_focus.request_focus();
    scene.controller.set_selection(6, 10);
    scene.settle();
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), strings(&EDITING));

    whisper.set(true);
    let signal = count.borrow().expect("the probe built");
    probe
        .write(|cx| signal.set(cx, 1))
        .expect("the probe's signal takes it");
    scene.settle();
    assert_eq!(scene.popup(), Vec::<String>::new());
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(
        scene.popup(),
        strings(&["Cut", "Copy", "Paste", "Select all", "Whisper"])
    );
}

/// A field rebuilt with no menu while its toolbar shows takes the toolbar
/// away, though its handles look the same. Only the field rebuilds.
///
/// Red-check: only show the toolbar again after a rebuild, without taking
/// it away first (the old toolbar stays).
pub(crate) fn a_field_rebuilt_without_its_menu_takes_its_toolbar_away() {
    let controller = TextEditingController::with_text(WORDS);
    let field_focus = FocusNode::with_debug_label("field");
    let off = Rc::new(Cell::new(false));
    let count: Rc<RefCell<Option<Signal<u32>>>> = Rc::default();
    let probe = {
        let (controller, field_focus) = (controller.clone(), Rc::clone(&field_focus));
        let (off, count) = (Rc::clone(&off), Rc::clone(&count));
        SignalProbe::new(move |signals| {
            count.replace(Some(signals.count));
            let field = EditableText::new(controller.clone(), Rc::clone(&field_focus))
                .text_style(style())
                .menu_policy(SelectionMenuPolicy::ALWAYS);
            let field = if off.get() { field.no_menu() } else { field };
            SizedBox::new(400.0, 40.0).child(field)
        })
    };
    let page = Column::new((SizedBox::new(400.0, 80.0), probe.view()));
    let mut laid = lay_out(WidgetsApp::new(page), tight(400.0, 300.0));
    laid.enable_semantics();
    laid.pump();
    let mut scene = Scene {
        laid,
        controller,
        field_focus,
        elsewhere: FocusNode::with_debug_label("elsewhere"),
        log: Rc::default(),
        line_height: Rc::default(),
        square: false,
    };
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&EDITING));

    off.set(true);
    let signal = count.borrow().expect("the probe built");
    probe
        .write(|cx| signal.set(cx, 1))
        .expect("the probe's signal takes it");
    scene.settle();
    assert_eq!(scene.toolbar(), Vec::<String>::new());
    assert_eq!(scene.controller.selection(), 6..10, "the selection stays");
}

/// Under [`LongPressSelection::Caret`], the caret a long press placed shows
/// the toolbar over it once the touch lifts, with what a caret allows,
/// though the field shows no handle for it; choosing an item performs it
/// and takes the toolbar away. A field that shows a caret handle shows
/// both, and a policy that shows nothing after a long press shows neither.
///
/// Red-check: show the toolbar after a long press only over a selection
/// (none over the caret); show it with a handle under the caret (one
/// follower too many).
pub(crate) fn a_caret_long_press_shows_the_toolbar_over_the_caret() {
    let followers = |scene: &Scene| {
        scene
            .laid
            .find_all_by_render_type("RenderFollowerLayer")
            .len()
    };
    let caret = |handle: CaretHandle, policy: SelectionMenuPolicy| {
        let mut scene = Scene::with(
            move |field| {
                field
                    .caret_handle(handle)
                    .long_press(LongPressSelection::Caret)
                    .menu_policy(policy)
            },
            None,
        );
        scene.long_press(scene.along(width_of("alpha be")));
        assert_eq!(
            scene.controller.selection(),
            8..8,
            "the press placed a caret"
        );
        scene
    };

    let mut bare = caret(CaretHandle::NEVER, SelectionMenuPolicy::ALWAYS);
    assert_eq!(bare.toolbar(), strings(&["Paste", "Select all"]));
    let handled = caret(CaretHandle::on_tap(), SelectionMenuPolicy::ALWAYS);
    assert_eq!(handled.toolbar(), strings(&["Paste", "Select all"]));
    assert_eq!(
        followers(&bare) + 1,
        followers(&handled),
        "no handle under the caret of a field that shows none"
    );

    bare.tap(bare.center_of(Role::Button, "Select all"));
    assert_eq!(bare.controller.selection(), 0..WORDS.len());
    assert_eq!(bare.toolbar(), Vec::<String>::new());

    let quiet = caret(CaretHandle::NEVER, SelectionMenuPolicy::NEVER);
    assert_eq!(quiet.toolbar(), Vec::<String>::new());
    assert_eq!(followers(&quiet), 0, "nothing follows the caret");
}
