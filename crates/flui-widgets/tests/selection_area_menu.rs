//! The selection menu of a `SelectionArea`: the toolbar a touch selection
//! shows over the selection, and the popup a secondary click, the menu key
//! or assistive technology opens, each offering copy and select all, then
//! the commands a `DefaultSelectionControls` and the area add. The app is a
//! `WidgetsApp`, whose `PlainSelectionMenu` draws both, so every button and
//! item below is found and pressed as assistive technology reads it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_interaction::focus::FocusNode;
use flui_painting::typography::TextStyle;
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::pointer::PointerButton;
use flui_testing::a11y::{A11yTree, Action, ActionRequest, Role, TreeId};
use flui_testing::replay::{MouseScript, PointerScript};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    Actions, CallbackAction, Column, ContextMenuArea, DefaultSelectionControls, Focus,
    GestureCommand, Intent, SelectionArea, ShowContextMenuIntent, SizedBox, Text, WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

const WORDS: &str = "alpha beta gamma";
const READING: [&str; 2] = ["Copy", "Select all"];
/// The diameter of the default dot handles.
const HANDLE: f64 = 22.0;

/// Shout the selection, a command the app adds for every area.
#[derive(Clone, Copy, Debug)]
struct Shout;
impl Intent for Shout {}

/// Whisper the selection, a command one area adds.
#[derive(Clone, Copy, Debug)]
struct Whisper;
impl Intent for Whisper {}

fn style() -> TextStyle {
    TextStyle::default().with_font_size(16.0)
}

struct Scene {
    laid: LaidOut,
    /// A node outside the area, holding the focus somewhere else.
    elsewhere: Rc<FocusNode>,
    log: Rc<RefCell<Vec<&'static str>>>,
    /// What the area last reported selected.
    heard: Rc<RefCell<Option<String>>>,
    paragraph: RenderId,
}

impl Scene {
    /// An area over [`WORDS`] in a `WidgetsApp`, configured by `area`,
    /// under `controls` (the app's own when `None`), with `Actions` above
    /// answering [`Shout`], [`Whisper`] and the menu key into the log, the
    /// page put in `frame`. Semantics on.
    fn build(
        area: impl FnOnce(SelectionArea) -> SelectionArea,
        controls: Option<fn(DefaultSelectionControls) -> DefaultSelectionControls>,
        frame: impl FnOnce(BoxedView) -> BoxedView,
    ) -> Self {
        let elsewhere = FocusNode::with_debug_label("elsewhere");
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let heard: Rc<RefCell<Option<String>>> = Rc::default();
        let hears = Rc::clone(&heard);
        let area = area(
            SelectionArea::new(Column::new((Text::new(WORDS).style(style()),)))
                .on_selection_changed(move |_cx, change| {
                    *hears.borrow_mut() = change
                        .selected
                        .as_ref()
                        .map(|selected| selected.text.clone());
                }),
        );
        let content = Column::new((SizedBox::new(400.0, 40.0).child(area),)).boxed();
        let content = match controls {
            Some(controls) => controls(DefaultSelectionControls::new(content)).boxed(),
            None => content,
        };
        let (shout, whisper, menu) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
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
        // The area sits a toolbar's room below the top, so the toolbar goes
        // over the line.
        let room = Focus::new(SizedBox::new(400.0, 80.0)).focus_node(Rc::clone(&elsewhere));
        let app = WidgetsApp::new(frame(Column::new((room, home)).boxed()));
        let mut laid = lay_out(app, tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        let paragraph = laid.find_all_by_render_type("RenderParagraph")[0];
        Self {
            laid,
            elsewhere,
            log,
            heard,
            paragraph,
        }
    }

    fn with(area: impl FnOnce(SelectionArea) -> SelectionArea) -> Self {
        Self::build(area, None, |page| page)
    }

    fn tree(&self) -> A11yTree {
        self.laid.a11y_tree().expect("semantics enabled")
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }

    /// The point `width` along the text, at the line's middle.
    fn along(&self, width: f64) -> Offset<f64> {
        let height = self.laid.size(self.paragraph).height;
        self.laid.absolute_offset(self.paragraph) + Offset::new(width, height / 2.0)
    }

    /// The middle of the handle under the selection's end `width` along
    /// the line: its top centre is at the bottom of the line there.
    fn end_handle(&self, width: f64) -> Offset<f64> {
        let height = self.laid.size(self.paragraph).height;
        self.laid.absolute_offset(self.paragraph) + Offset::new(width, height + HANDLE / 2.0)
    }

    fn long_press(&mut self, at: Offset<f64>) {
        self.laid
            .replay(&PointerScript::long_press(at, Duration::from_millis(800)));
        self.settle();
    }

    fn clicks(&mut self, at: Offset<f64>, count: usize, button: PointerButton) {
        let mut script = MouseScript::new("clicks");
        for _ in 0..count {
            script = script
                .press_button(at, button)
                .wait(Duration::from_millis(16))
                .release()
                .wait(Duration::from_millis(40));
        }
        let script = script.build().expect("every click releases");
        self.laid.replay(&script);
        self.settle();
    }

    fn secondary_click(&mut self, at: Offset<f64>) {
        self.clicks(at, 1, PointerButton::SECONDARY);
    }

    /// Selects the word at `at` with a double click.
    fn select_word(&mut self, at: Offset<f64>) {
        self.clicks(at, 2, PointerButton::PRIMARY);
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

    /// What the area holds selected, as it last reported.
    fn selected(&self) -> Option<String> {
        self.heard.borrow().clone()
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

    fn tap(&mut self, at: Offset<f64>) {
        self.laid.replay(&PointerScript::tap(at));
        self.settle();
    }
}

/// The width of `words` in the area's style.
fn width_of(words: &str) -> f64 {
    let mut laid = lay_out(
        WidgetsApp::new(Column::new((Text::new(words).style(style()),))),
        tight(400.0, 300.0),
    );
    laid.pump();
    laid.size(laid.find_all_by_render_type("RenderParagraph")[0])
        .width
}

fn strings(labels: &[&str]) -> Vec<String> {
    labels
        .iter()
        .map(|label| (*label).to_owned())
        .collect::<Vec<String>>()
}

/// A long press selects a word and shows the toolbar over it with copy and
/// select all, then the app's and the area's commands; Copy writes the
/// selection and takes the toolbar away, leaving the selection.
///
/// Red-check: show no toolbar when the long press lifts (no toolbar node).
pub(crate) fn an_area_s_long_press_shows_its_toolbar() {
    let mut scene = Scene::build(
        |area| area.menu_command(GestureCommand::new("Whisper", Whisper)),
        Some(|controls| controls.command(GestureCommand::new("Shout", Shout))),
        |page| page,
    );
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    assert_eq!(
        scene.toolbar(),
        strings(&["Copy", "Select all", "Shout", "Whisper"])
    );
    let tree = scene.tree();
    let bar = tree.find(Role::Toolbar).expect("a toolbar shows");
    let bounds = tree.window_bounds(&bar).expect("the toolbar has bounds");
    let line_top = scene.laid.absolute_offset(scene.paragraph).dy;
    assert!(bounds.y1 <= line_top, "the toolbar is over the line");

    scene.tap(scene.center_of(Role::Button, "Whisper"));
    assert_eq!(*scene.log.borrow(), ["whisper"]);
    assert!(
        scene.toolbar().is_empty(),
        "a chosen item takes the toolbar"
    );
    assert_eq!(scene.selected().as_deref(), Some("beta"));

    scene.long_press(scene.along(width_of("alpha be")));
    scene.tap(scene.center_of(Role::Button, "Copy"));
    assert_eq!(scene.laid.clipboard().text().as_deref(), Some("beta"));
    assert_eq!(scene.toolbar(), Vec::<String>::new());
}

/// A secondary click on the selection opens the popup on it, and stays
/// open while the popup holds the focus; off the selection it clears the
/// selection on a host other than Apple's and offers select all.
///
/// Red-check: attach no secondary tap to the area (no popup); lay the
/// area's actions out afresh each build (the popup closes when the area,
/// losing the focus to it, rebuilds).
pub(crate) fn an_area_s_secondary_click_opens_its_popup() {
    let mut scene = Scene::with(|area| area);
    scene.select_word(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));

    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    assert_eq!(scene.popup(), strings(&READING));
    scene.tap(scene.center_of(Role::MenuItem, "Select all"));
    assert_eq!(scene.selected().as_deref(), Some(WORDS));
    assert!(scene.popup().is_empty(), "a chosen item closes the popup");

    // Past the double-click window, so the next press counts from one.
    scene.laid.pump_for(Duration::from_millis(500));
    scene.select_word(scene.along(width_of("alpha be")));
    assert_eq!(scene.selected().as_deref(), Some("beta"));
    scene.secondary_click(scene.along(width_of("alpha beta ga")));
    if cfg!(target_vendor = "apple") {
        assert_eq!(scene.selected().as_deref(), Some("gamma"));
        assert_eq!(scene.popup(), strings(&READING));
    } else {
        assert_eq!(
            scene.selected(),
            None,
            "a click off the selection clears it"
        );
        assert_eq!(scene.popup(), strings(&["Select all"]));
    }
    scene.key(NamedKey::Escape);
    assert!(scene.popup().is_empty(), "Escape closes the popup");
}

/// The menu key in an area that holds the focus, and assistive
/// technology's `ShowContextMenu` on its document, open the popup; the
/// area then holds the focus, which the popup gives back on closing.
///
/// Red-check: leave `ShowContextMenuIntent` unbound in the area (the key
/// reaches the enclosing answer); offer no `ShowContextMenu` action.
pub(crate) fn the_menu_key_and_assistive_technology_open_an_area_s_popup() {
    let mut scene = Scene::with(|area| area);
    scene.select_word(scene.along(width_of("alpha be")));
    scene.key(NamedKey::ContextMenu);
    assert_eq!(scene.popup(), strings(&READING));
    assert!(scene.log.borrow().is_empty(), "the area answers the key");
    scene.key(NamedKey::Escape);
    assert_eq!(scene.popup(), Vec::<String>::new());

    let _ = scene.elsewhere.request_focus();
    scene.settle();
    let tree = scene.tree();
    let document = tree.find(Role::Document).expect("the area's document");
    assert!(
        document.supports_action(Action::ShowContextMenu),
        "the area offers ShowContextMenu"
    );
    scene
        .laid
        .invoke_semantics_action(ActionRequest {
            action: Action::ShowContextMenu,
            target_tree: TreeId::ROOT,
            target_node: document.id(),
            data: None,
        })
        .expect("the area takes ShowContextMenu");
    scene.settle();
    assert_eq!(scene.popup(), strings(&READING));
    scene.key(NamedKey::Escape);
    assert!(!scene.elsewhere.has_focus(), "the menu focused the area");
    scene.key(NamedKey::ContextMenu);
    assert_eq!(scene.popup(), strings(&READING), "the area holds the focus");
}

/// A handle drag takes the toolbar away until it lets go, and a key takes
/// it.
///
/// Red-check: show nothing when a handle lets go (no toolbar after the
/// drag).
pub(crate) fn an_area_s_handle_drag_hides_its_toolbar_until_it_lets_go() {
    let mut scene = Scene::with(|area| area);
    scene.long_press(scene.along(width_of("alpha be")));
    assert_eq!(scene.toolbar(), strings(&READING));

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
    assert_eq!(scene.selected().as_deref(), Some("beta ga"));
    assert_eq!(scene.toolbar(), strings(&READING), "letting go shows it");

    scene.key(NamedKey::ArrowLeft);
    assert!(scene.toolbar().is_empty(), "a key takes it");
}

/// An area's secondary click wins over an enclosing `ContextMenuArea`, and
/// an area with no menu leaves the click and the menu key to it.
///
/// Red-check: attach the area's secondary tap whether or not it shows a
/// menu (the enclosing menu never opens).
pub(crate) fn an_enclosing_menu_answers_only_for_an_area_without_one() {
    let enclosing = |page: BoxedView| {
        ContextMenuArea::try_new(GestureCommand::new("Shout", Shout), [])
            .expect("one labelled command")
            .child(page)
            .boxed()
    };
    let mut scene = Scene::build(|area| area, None, enclosing);
    scene.select_word(scene.along(width_of("alpha be")));
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), strings(&READING));
    scene.key(NamedKey::Escape);

    let mut scene = Scene::build(SelectionArea::no_menu, None, enclosing);
    scene.select_word(scene.along(width_of("alpha be")));
    scene.secondary_click(scene.along(width_of("alpha be")));
    assert_eq!(scene.popup(), strings(&["Shout"]));
    scene.key(NamedKey::Escape);
    scene.long_press(scene.along(width_of("alpha be")));
    assert!(scene.toolbar().is_empty(), "no menu shows no toolbar");
}
