//! `ContextMenuArea`: the theme-free menu every context-menu trigger opens.
//! Where it opens for each trigger (flipping at the window's edges and
//! mirrored right-to-left), the keys that move inside it, what closes it and
//! where the focus goes then, a command that panics, and what assistive
//! technology reads, and the menu's long press handing its contact to a drag.
//! Its rows of the input-path matrix sit with the other behavioural widgets'
//! in `gesture_alternatives`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::drag::{DragItem, DragTrigger};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::pointer::PointerButton;
use flui_testing::a11y::{A11yRect, A11yTree, Action, ActionRequest, Role, TreeId};
use flui_testing::replay::{MouseScript, PointerScript, TouchScript};
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};
use flui_view::prelude::*;
use flui_view::{BoxedView, StateHandle};
use flui_widgets::{
    Actions, Alternative, AlternativeKind, CallbackAction, ColoredBox, CommandAppearance,
    ContextMenuArea, Directionality, DragCommands, Draggable, GestureAlternatives, GestureCommand,
    Intent, MenuSpec, Navigator, NavigatorHandle, Padding, PageRoute, Positioned, SizedBox, Stack,
};

use crate::common::{LaidOut, lay_out, tight};

/// Delete the message.
#[derive(Clone, Copy, Debug)]
struct Delete;
impl Intent for Delete {}

/// Archive the message.
#[derive(Clone, Copy, Debug)]
struct Archive;
impl Intent for Archive {}

/// Pin the message; its action panics in a scene that asks for it.
#[derive(Clone, Copy, Debug)]
struct Pin;
impl Intent for Pin {}

const DELETE: &str = "Delete Alice";
const ARCHIVE: &str = "Archive Alice";
const PIN: &str = "Pin Alice";
const ROW: (f64, f64) = (300.0, 50.0);

/// What the commands did, in order.
type Log = Rc<RefCell<Vec<&'static str>>>;

/// How a scene is laid out.
#[derive(Clone, Copy)]
struct Scene {
    direction: TextDirection,
    /// Where the row's top-left corner is in the page.
    row: Offset<f64>,
    /// How far the navigator is inset from the window on each side.
    inset: (f64, f64),
    /// Whether Pin's action panics.
    pin_panics: bool,
}

impl Scene {
    const fn new() -> Self {
        Self {
            direction: TextDirection::Ltr,
            row: Offset::new(0.0, 0.0),
            inset: (0.0, 0.0),
            pin_panics: false,
        }
    }

    const fn rtl(mut self) -> Self {
        self.direction = TextDirection::Rtl;
        self
    }

    const fn row_at(mut self, x: f64, y: f64) -> Self {
        self.row = Offset::new(x, y);
        self
    }

    const fn inset(mut self, horizontal: f64, vertical: f64) -> Self {
        self.inset = (horizontal, vertical);
        self
    }

    const fn pin_panics(mut self) -> Self {
        self.pin_panics = true;
        self
    }

    /// Alice's area, with Delete, Archive and Pin.
    fn area() -> ContextMenuArea {
        ContextMenuArea::try_new(
            GestureCommand::new(DELETE, Delete),
            [
                GestureCommand::new(ARCHIVE, Archive),
                GestureCommand::new(PIN, Pin),
            ],
        )
        .expect("three labelled commands")
        .target("Alice")
        .child(SizedBox::new(ROW.0, ROW.1))
    }

    /// The alternatives the area mounts, as `assert_gesture_alternatives`
    /// reads them.
    fn alternatives() -> GestureAlternatives {
        GestureAlternatives::try_new(Alternative::ContextMenu(
            MenuSpec::new(GestureCommand::new(DELETE, Delete), |_cx, _request| {})
                .item(GestureCommand::new(ARCHIVE, Archive))
                .item(GestureCommand::new(PIN, Pin)),
        ))
        .expect("three labelled commands")
        .target("Alice")
    }

    /// The scene's view, with the area shown while `shown` is true, and the
    /// log its commands write.
    fn view(self, shown: &StateHandle<bool>) -> (BoxedView, Log) {
        let log: Log = Rc::default();
        let navigator = NavigatorHandle::new();
        let (page_log, shown) = (Rc::clone(&log), shown.clone());
        navigator.seed_initial(PageRoute::<()>::new(move |_, _, _| {
            let row = Shown {
                shown: shown.clone(),
                child: Self::area().boxed(),
            };
            let (delete, archive, pin) = (
                Rc::clone(&page_log),
                Rc::clone(&page_log),
                Rc::clone(&page_log),
            );
            let pin_panics = self.pin_panics;
            Directionality::new(
                self.direction,
                Actions::new(Stack::new(vec![
                    Positioned::new(row)
                        .left(self.row.dx)
                        .top(self.row.dy)
                        .boxed(),
                ]))
                .action(CallbackAction::new(move |_cx, _: &Delete| {
                    delete.borrow_mut().push(DELETE);
                }))
                .action(CallbackAction::new(move |_cx, _: &Archive| {
                    archive.borrow_mut().push(ARCHIVE);
                }))
                .action(CallbackAction::new(move |_cx, _: &Pin| {
                    assert!(!pin_panics, "pinning failed");
                    pin.borrow_mut().push(PIN);
                })),
            )
            .boxed()
        }));
        let view = Padding::symmetric(self.inset.0, self.inset.1)
            .child(Navigator::new(navigator))
            .boxed();
        (view, log)
    }

    /// Alice's area as the home of a `WidgetsApp`, whose navigator shows the
    /// menu, mounted like [`Self::mount`].
    fn mount_as_app_home() -> Mounted {
        let log: Log = Rc::default();
        let delete = Rc::clone(&log);
        let home =
            Actions::new(Self::area()).action(CallbackAction::new(move |_cx, _: &Delete| {
                delete.borrow_mut().push(DELETE);
            }));
        let mut laid = lay_out(flui_widgets::WidgetsApp::new(home), tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        Mounted {
            laid,
            log,
            shown: StateHandle::new(true),
        }
    }

    /// The scene mounted in a 400 × 300 window, semantics on.
    fn mount(self) -> Mounted {
        let shown = StateHandle::new(true);
        let (view, log) = self.view(&shown);
        let mut laid = lay_out(view, tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        Mounted { laid, log, shown }
    }
}

/// A mounted scene.
struct Mounted {
    laid: LaidOut,
    log: Log,
    shown: StateHandle<bool>,
}

impl Mounted {
    fn tree(&self) -> A11yTree {
        self.laid.a11y_tree().expect("semantics enabled")
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.laid.pump_for(Duration::from_millis(16));
        }
    }

    fn replay(&mut self, script: &PointerScript) {
        self.laid.replay(script);
        self.settle();
    }

    fn secondary_click(&mut self, at: Offset<f64>) {
        let click = MouseScript::new("secondary click")
            .press_button(at, PointerButton::SECONDARY)
            .wait(Duration::from_millis(16))
            .release()
            .build()
            .expect("the click releases its button");
        self.replay(&click);
    }

    fn click(&mut self, at: Offset<f64>) {
        let click = MouseScript::new("click")
            .press(at)
            .wait(Duration::from_millis(16))
            .release()
            .build()
            .expect("the click releases its button");
        self.replay(&click);
    }

    fn long_press(&mut self, at: Offset<f64>) {
        let press = TouchScript::new("long press")
            .down(1, at)
            .wait(Duration::from_secs(1))
            .up(1)
            .build()
            .expect("the press lifts its finger");
        self.replay(&press);
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

    /// Tab until the row has focus.
    fn focus_row(&mut self) {
        for _ in 0..8 {
            self.key(NamedKey::Tab);
            if self.focus_is_on_row() {
                return;
            }
        }
        panic!("Tab never reaches the row:\n{}", self.tree().describe());
    }

    /// Open the menu through the alternatives' `ShowContextMenu`.
    fn show_context_menu(&mut self) {
        let row = {
            let tree = self.tree();
            tree.nodes()
                .find(|node| node.supports_action(Action::ShowContextMenu))
                .expect("the row offers ShowContextMenu")
                .id()
        };
        self.laid
            .invoke_semantics_action(ActionRequest {
                action: Action::ShowContextMenu,
                target_tree: TreeId::ROOT,
                target_node: row,
                data: None,
            })
            .expect("the row takes ShowContextMenu");
        self.settle();
    }

    fn focus_is_on_row(&self) -> bool {
        self.tree().focus().is_some_and(|node| {
            node.raw()
                .custom_actions()
                .iter()
                .any(|action| action.description == DELETE)
        })
    }

    fn focused_label(&self) -> Option<String> {
        self.tree()
            .focus()
            .and_then(|node| node.label().map(str::to_owned))
    }

    /// The open menu's window bounds; `None` while no menu is open.
    fn menu(&self) -> Option<A11yRect> {
        let tree = self.tree();
        let menu = tree.find(Role::Menu).ok()?;
        tree.window_bounds(&menu)
    }

    fn item(&self, label: &str) -> A11yRect {
        let tree = self.tree();
        let item = tree
            .find_by_role_and_label(Role::MenuItem, label)
            .unwrap_or_else(|error| panic!("{label} is an item of the open menu: {error}"));
        tree.window_bounds(&item).expect("the item has bounds")
    }

    fn item_center(&self, label: &str) -> Offset<f64> {
        let bounds = self.item(label);
        Offset::new(
            f64::midpoint(bounds.x0, bounds.x1),
            f64::midpoint(bounds.y0, bounds.y1),
        )
    }

    /// The left and right window edges of the painted text `label`, and the
    /// direction it is laid out in.
    fn text(&self, label: &str) -> (f64, f64, Option<String>) {
        let mut pending = vec![self.laid.root()];
        while let Some(id) = pending.pop() {
            if self.laid.render_node(id).get::<&str>("text") == Ok(label) {
                let left = self.laid.absolute_offset(id).dx;
                let direction = self
                    .laid
                    .render_node(id)
                    .get::<flui_foundation::inspect::Variant<'_>>("text_direction")
                    .ok()
                    .map(|variant| variant.name.to_owned());
                return (left, left + self.laid.size(id).width, direction);
            }
            pending.extend(self.laid.children(id));
        }
        panic!("no text {label:?} is painted");
    }

    /// Whether assistive technology reaches the row.
    fn row_is_reachable(&self) -> bool {
        self.tree().nodes().any(|node| {
            node.raw()
                .custom_actions()
                .iter()
                .any(|action| action.description == DELETE)
        })
    }
}

// ============================================================================
// Placement
// ============================================================================

/// How a placement row opens the menu.
#[derive(Clone, Copy, Debug)]
enum Trigger {
    SecondaryClick(f64, f64),
    LongPress(f64, f64),
    /// The row's `ShowContextMenu`, which carries no position.
    Assistive,
}

/// Which edges of the menu a placement row pins, in window coordinates.
#[derive(Clone, Copy, Debug)]
struct Edges {
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

const fn edges(
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
) -> Edges {
    Edges {
        left,
        right,
        top,
        bottom,
    }
}

/// Each trigger opens the menu where it points: at the pointer, growing
/// towards the trailing side and down; below the row from its leading edge
/// when no pointer opened it; flipped across that point at a window edge
/// without room; mirrored right-to-left; and placed in the overlay's own
/// space when the navigator does not start at the window's corner.
pub(crate) fn the_menu_opens_where_its_trigger_points() {
    let rows: [(&str, Scene, Trigger, Edges); 9] = [
        (
            "a secondary click",
            Scene::new(),
            Trigger::SecondaryClick(150.0, 25.0),
            edges(Some(150.0), None, Some(25.0), None),
        ),
        (
            "a long press",
            Scene::new(),
            Trigger::LongPress(150.0, 25.0),
            edges(Some(150.0), None, Some(25.0), None),
        ),
        (
            "a click by the right edge flips left",
            Scene::new().row_at(100.0, 0.0),
            Trigger::SecondaryClick(390.0, 25.0),
            edges(None, Some(390.0), Some(25.0), None),
        ),
        (
            "a click by the bottom edge flips up",
            Scene::new().row_at(0.0, 250.0),
            Trigger::SecondaryClick(150.0, 290.0),
            edges(Some(150.0), None, None, Some(290.0)),
        ),
        (
            "right-to-left grows leftwards",
            Scene::new().rtl(),
            Trigger::SecondaryClick(250.0, 25.0),
            edges(None, Some(250.0), Some(25.0), None),
        ),
        (
            "right-to-left by the left edge flips right",
            Scene::new().rtl(),
            Trigger::SecondaryClick(10.0, 25.0),
            edges(Some(10.0), None, Some(25.0), None),
        ),
        (
            "assistive technology opens it below the row",
            Scene::new().row_at(40.0, 20.0),
            Trigger::Assistive,
            edges(Some(40.0), None, Some(70.0), None),
        ),
        (
            "right-to-left, below the row from its right edge",
            Scene::new().rtl().row_at(40.0, 20.0),
            Trigger::Assistive,
            edges(None, Some(340.0), Some(70.0), None),
        ),
        (
            "an inset navigator",
            Scene::new().inset(40.0, 30.0).row_at(10.0, 10.0),
            Trigger::SecondaryClick(150.0, 60.0),
            edges(Some(150.0), None, Some(60.0), None),
        ),
    ];
    for (name, scene, trigger, expected) in rows {
        let mut mounted = scene.mount();
        match trigger {
            Trigger::SecondaryClick(x, y) => mounted.secondary_click(Offset::new(x, y)),
            Trigger::LongPress(x, y) => mounted.long_press(Offset::new(x, y)),
            Trigger::Assistive => mounted.show_context_menu(),
        }
        let menu = mounted
            .menu()
            .unwrap_or_else(|| panic!("{name}: the menu opens:\n{}", mounted.tree().describe()));
        let close = |actual: f64, expected: Option<f64>| {
            expected.is_none_or(|expected| (actual - expected).abs() < 0.5)
        };
        assert!(
            close(menu.x0, expected.left)
                && close(menu.x1, expected.right)
                && close(menu.y0, expected.top)
                && close(menu.y1, expected.bottom),
            "{name}: the menu is at {menu:?}, expected {expected:?}"
        );
        assert!(
            menu.x0 >= -0.5 && menu.x1 <= 400.5 && menu.y0 >= -0.5 && menu.y1 <= 300.5,
            "{name}: the menu stays inside the window: {menu:?}"
        );
    }
}

// ============================================================================
// Keys inside the menu
// ============================================================================

/// The menu takes the focus on its first item; Down and Up move between the
/// items and wrap at either end, Home and End go to the first and the last,
/// Tab stays inside, and Enter chooses the focused item.
pub(crate) fn arrow_keys_home_and_end_move_between_items() {
    let mut mounted = Scene::new().mount();
    mounted.secondary_click(Offset::new(150.0, 25.0));
    let steps = [
        (None, DELETE),
        (Some(NamedKey::ArrowDown), ARCHIVE),
        (Some(NamedKey::ArrowDown), PIN),
        (Some(NamedKey::ArrowDown), DELETE),
        (Some(NamedKey::ArrowUp), PIN),
        (Some(NamedKey::Home), DELETE),
        (Some(NamedKey::End), PIN),
        (Some(NamedKey::Tab), DELETE),
        (Some(NamedKey::ArrowDown), ARCHIVE),
    ];
    for (key, expected) in steps {
        if let Some(key) = key {
            mounted.key(key);
        }
        assert_eq!(
            mounted.focused_label().as_deref(),
            Some(expected),
            "after {key:?}"
        );
    }
    mounted.key(NamedKey::Enter);
    assert_eq!(*mounted.log.borrow(), [ARCHIVE], "Enter chooses Archive");
    assert!(mounted.menu().is_none(), "choosing an item closes the menu");
}

// ============================================================================
// Closing
// ============================================================================

/// Escape and a tap outside the menu each close it, choose nothing, and give
/// the focus back to the row it was on: whether the menu opened from a click
/// or the menu key, and whether the row is on a page of its own navigator or
/// on a `WidgetsApp`'s home, where the focus used to be lost.
pub(crate) fn escape_and_an_outside_tap_close_the_menu_and_restore_focus() {
    for (name, in_app, by_key, close) in [
        ("Escape", false, false, None),
        (
            "a tap outside",
            false,
            false,
            Some(Offset::new(380.0, 280.0)),
        ),
        ("the menu key, then Escape", false, true, None),
        ("app home: Escape", true, false, None),
        ("app home: the menu key, then Escape", true, true, None),
    ] {
        let mut mounted = if in_app {
            Scene::mount_as_app_home()
        } else {
            Scene::new().mount()
        };
        mounted.focus_row();
        if by_key {
            mounted.key(NamedKey::ContextMenu);
        } else {
            mounted.secondary_click(Offset::new(150.0, 25.0));
        }
        assert!(mounted.menu().is_some(), "{name}: the menu opens");
        assert_eq!(
            mounted.focused_label().as_deref(),
            Some(DELETE),
            "{name}: the menu takes the focus"
        );
        match close {
            None => mounted.key(NamedKey::Escape),
            Some(at) => mounted.click(at),
        }
        assert!(mounted.menu().is_none(), "{name}: the menu closes");
        assert!(
            mounted.focus_is_on_row(),
            "{name}: the focus is back on the row:\n{}",
            mounted.tree().describe()
        );
        assert!(mounted.log.borrow().is_empty(), "{name}: chose nothing");
    }
}

/// Unmounting the area closes its menu, and the navigator goes on taking
/// routes afterwards.
pub(crate) fn the_menu_closes_when_its_area_unmounts() {
    let mut mounted = Scene::new().mount();
    mounted.secondary_click(Offset::new(150.0, 25.0));
    assert!(mounted.menu().is_some(), "the menu opens");
    mounted.shown.update(|shown| *shown = false);
    mounted.settle();
    assert!(
        mounted.menu().is_none(),
        "the menu goes with its area:\n{}",
        mounted.tree().describe()
    );
    mounted.shown.update(|shown| *shown = true);
    mounted.settle();
    mounted.secondary_click(Offset::new(150.0, 25.0));
    let archive = mounted.item_center(ARCHIVE);
    mounted.click(archive);
    assert_eq!(
        *mounted.log.borrow(),
        [ARCHIVE],
        "a remounted area's menu works"
    );
}

/// Without a navigator to show the menu in, a request is a `flui::a11y`
/// warning, not a silent no-op.
pub(crate) fn a_missing_navigator_is_reported() {
    let ((), log) = flui_testing::log_capture::capture(|| {
        let mut laid = lay_out(Scene::area(), tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        let row = {
            let tree = laid.a11y_tree().expect("semantics enabled");
            tree.nodes()
                .find(|node| node.supports_action(Action::ShowContextMenu))
                .expect("the row offers ShowContextMenu")
                .id()
        };
        laid.invoke_semantics_action(ActionRequest {
            action: Action::ShowContextMenu,
            target_tree: TreeId::ROOT,
            target_node: row,
            data: None,
        })
        .expect("the row takes ShowContextMenu");
        laid.pump();
    });
    assert!(
        log.records()
            .iter()
            .any(|record| record.target == "flui::a11y" && record.message.contains("no Navigator")),
        "the request is reported:\n{log}"
    );
}

/// The menu reads in the area's direction, which the overlay it is shown in
/// does not inherit: each label is laid out in it and starts at its item's
/// leading edge.
pub(crate) fn items_read_in_the_area_direction() {
    for (direction, scene) in [
        (TextDirection::Ltr, Scene::new()),
        (TextDirection::Rtl, Scene::new().rtl()),
    ] {
        let mut mounted = scene.mount();
        mounted.secondary_click(Offset::new(150.0, 25.0));
        let item = mounted.item(DELETE);
        let (left, right, laid_out_in) = mounted.text(DELETE);
        assert_eq!(
            laid_out_in.as_deref(),
            Some(format!("{direction:?}").as_str()),
            "the label is laid out {direction:?}"
        );
        let (start_gap, end_gap) = match direction {
            TextDirection::Ltr => (left - item.x0, item.x1 - right),
            TextDirection::Rtl => (item.x1 - right, left - item.x0),
        };
        assert!(
            start_gap < end_gap,
            "{direction:?}: the label starts at the item's {direction:?} start: \
             text {left}..{right} in item {item:?}"
        );
    }
}

/// The menu's labels stay readable on its surface whatever text colour the
/// app sets around it: under white text, the items are not painted white on
/// the menu's white.
pub(crate) fn the_menu_labels_contrast_with_its_surface_under_any_text_colour() {
    use flui_painting::display_list::DrawOp;
    use flui_painting::typography::TextStyle;
    use flui_widgets::DefaultTextStyle;

    let shown = StateHandle::new(true);
    let (view, log) = Scene::new().view(&shown);
    let mut laid = lay_out(
        DefaultTextStyle::new(TextStyle::default().with_color(Color::WHITE), view),
        tight(400.0, 300.0),
    );
    laid.pump();
    let mut mounted = Mounted { laid, log, shown };
    mounted.secondary_click(Offset::new(150.0, 25.0));
    mounted.laid.pump();
    let label_colours: Vec<Color> = mounted
        .laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Paragraph { color, .. } => Some(color),
            _ => None,
        })
        .collect();
    assert_eq!(label_colours.len(), 3, "the three items are painted");
    assert!(
        label_colours.iter().all(|colour| *colour != Color::WHITE),
        "no label is painted white on the white menu: {label_colours:?}"
    );
}

/// A command that panics leaves no menu behind: the menu closed before the
/// command ran, the panic reaches the caller, and the next menu works.
pub(crate) fn a_panicking_item_leaves_no_menu_open() {
    let mut mounted = Scene::new().pin_panics().mount();
    mounted.secondary_click(Offset::new(150.0, 25.0));
    let pin = mounted.item_center(PIN);
    let raised = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mounted.click(pin)));
    let payload = raised.expect_err("the command's panic reaches the caller");
    let text = flui_foundation::panic::payload_text(&*payload).map(str::to_owned);
    flui_foundation::panic::retain_opaque_payload(payload);
    assert_eq!(text.as_deref(), Some("pinning failed"));
    mounted.settle();
    assert!(mounted.menu().is_none(), "no menu is left open");
    mounted.secondary_click(Offset::new(150.0, 25.0));
    let delete = mounted.item_center(DELETE);
    mounted.click(delete);
    assert_eq!(*mounted.log.borrow(), [DELETE], "the next menu works");
}

// ============================================================================
// Hand-off to a drag
// ============================================================================

/// What a menu-to-drag scene recorded, in order: the menu opening and
/// closing, and the drag starting.
type Events = Rc<RefCell<Vec<&'static str>>>;

/// A card dragged by a long press, logging its start to `events`; its own
/// commands open a menu that logs to `events` when `own_menu`.
fn long_press_card(events: &Events, own_menu: bool) -> Draggable {
    let mut commands = DragCommands::new(CommandAppearance::new(
        "Cut card",
        SizedBox::new(10.0, 10.0),
    ));
    if own_menu {
        let opened = Rc::clone(events);
        commands = commands.alternative(AlternativeKind::context_menu(move |_cx, _request| {
            opened.borrow_mut().push("open");
        }));
    }
    let started = Rc::clone(events);
    Draggable::try_new(
        commands,
        || vec![DragItem::new().with_value("card")],
        SizedBox::new(ROW.0, ROW.1).child(ColoredBox::new(Color::RED)),
    )
    .expect("one labelled command")
    .trigger(DragTrigger::LongPress)
    .on_drag_started(move |_| started.borrow_mut().push("drag"))
}

/// `content` as a `WidgetsApp`'s home, in a 400 × 300 window.
fn mount_home(content: impl IntoView) -> LaidOut {
    let mut laid = lay_out(flui_widgets::WidgetsApp::new(content), tight(400.0, 300.0));
    laid.pump();
    laid
}

/// A finger held past the long-press timeout, then moved past the slop.
fn hold_then_drag(at: Offset<f64>) -> PointerScript {
    let mut script = TouchScript::new("hold, then drag")
        .down(1, at)
        .wait(Duration::from_secs(1));
    for dx in [1.0, 2.0, 60.0, 80.0] {
        script = script
            .move_to(1, at + Offset::new(dx, 0.0))
            .wait(Duration::from_millis(16));
    }
    script.up(1).build().expect("the press lifts its finger")
}

/// A menu's long press around a long-press drag is the drag's hold, not a
/// second one: the press opens the menu, and once the finger moves past the
/// slop the menu closes and the drag takes the contact, with the page's
/// elements, and so the drag's gesture, kept through the menu's opening. A
/// long press that does not move keeps the menu and starts no drag. A card
/// whose own commands open a menu hands over the same way.
pub(crate) fn a_held_press_moved_past_the_slop_closes_the_menu_and_drags() {
    let at = Offset::new(150.0, 25.0);
    let area = |events: &Events| {
        ContextMenuArea::try_new(GestureCommand::new(DELETE, Delete), [])
            .expect("one labelled command")
            .child(long_press_card(events, false))
    };
    let still = TouchScript::new("long press")
        .down(1, at)
        .wait(Duration::from_secs(1))
        .up(1)
        .build()
        .expect("the press lifts its finger");
    // Whether the area's menu is open at the end; a card with its own menu
    // (`None`) logs its opening instead.
    let rows: [(&str, &PointerScript, &[&str], Option<bool>); 3] = [
        ("around: moved", &hold_then_drag(at), &["drag"], Some(false)),
        ("around: still", &still, &[], Some(true)),
        (
            "own menu: moved",
            &hold_then_drag(at),
            &["open", "drag"],
            None,
        ),
    ];
    for (name, script, expected, menu_open) in rows {
        let events = Events::default();
        let mut laid = if menu_open.is_none() {
            mount_home(long_press_card(&events, true))
        } else {
            mount_home(area(&events))
        };
        laid.enable_semantics();
        laid.pump();
        laid.replay(script);
        for _ in 0..4 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(*events.borrow(), expected, "{name}");
        if let Some(open) = menu_open {
            let tree = laid.a11y_tree().expect("semantics enabled");
            assert_eq!(tree.find(Role::Menu).is_ok(), open, "{name}: menu open");
        }
    }
}

// ============================================================================
// Assistive technology
// ============================================================================

/// Assistive technology reads the open menu as a menu of menu items, in
/// order, each offering a click and as wide as the menu and at least the
/// minimum target high, with the page below hidden from it while
/// the menu is open; a click on an item performs its command.
pub(crate) fn assistive_technology_reads_a_menu_of_menu_items() {
    let mut mounted = Scene::new().mount();
    assert!(mounted.row_is_reachable(), "closed, the row is reachable");
    mounted.show_context_menu();
    let (items, archive) = {
        let tree = mounted.tree();
        let menu = tree.find(Role::Menu).expect("the menu is published");
        let min_target = flui_interaction::GestureSettings::default().min_target();
        let (mut items, mut widths) = (Vec::new(), Vec::new());
        let mut pending: Vec<_> = menu.child_ids().iter().rev().copied().collect();
        while let Some(id) = pending.pop() {
            let node = tree
                .nodes()
                .find(|node| node.id() == id)
                .expect("a child is in the tree");
            if node.role() == Role::MenuItem {
                assert!(
                    node.supports_action(Action::Click),
                    "{:?} offers a click",
                    node.label()
                );
                let bounds = tree.window_bounds(&node).expect("an item has bounds");
                assert!(
                    bounds.y1 - bounds.y0 >= min_target - 0.5,
                    "{:?} is at least the {min_target} minimum target high: {bounds:?}",
                    node.label()
                );
                widths.push((bounds.x0, bounds.x1));
                items.push((node.id(), node.label().map(str::to_owned)));
            } else {
                pending.extend(node.child_ids().iter().rev());
            }
        }
        assert!(
            widths.windows(2).all(|pair| pair[0] == pair[1]),
            "the items span the menu's width alike: {widths:?}"
        );
        let archive = items
            .iter()
            .find(|(_, label)| label.as_deref() == Some(ARCHIVE))
            .map(|(id, _)| *id)
            .expect("Archive is an item");
        (
            items
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>(),
            archive,
        )
    };
    assert_eq!(
        items,
        [
            Some(DELETE.to_owned()),
            Some(ARCHIVE.to_owned()),
            Some(PIN.to_owned())
        ],
        "the items in order"
    );
    assert!(
        !mounted.row_is_reachable(),
        "open, the menu is all assistive technology reaches"
    );
    mounted
        .laid
        .invoke_semantics_action(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: archive,
            data: None,
        })
        .expect("the item takes a click");
    mounted.settle();
    assert_eq!(*mounted.log.borrow(), [ARCHIVE]);
    assert!(mounted.menu().is_none(), "choosing closes the menu");
    assert!(mounted.row_is_reachable(), "and the row is reachable again");
}

// ============================================================================
// Input-path matrix
// ============================================================================

/// A fresh scene's area as an `assert_gesture_alternatives` subject.
fn subject(scene: Scene) -> AlternativesSubject<Vec<&'static str>> {
    let shown = StateHandle::new(true);
    let (view, log) = scene.view(&shown);
    AlternativesSubject::new(view, &Scene::alternatives(), move || log.borrow().clone())
}

/// The area's own gesture: a secondary click on the row, then a click on the
/// menu's first item, which the click put at `item`.
fn click_the_first_item(row: Offset<f64>, item: Offset<f64>) -> PointerScript {
    MouseScript::new("secondary click, then the first item")
        .press_button(row, PointerButton::SECONDARY)
        .wait(Duration::from_millis(16))
        .release()
        .wait(Duration::from_millis(100))
        .move_to(item)
        .press(item)
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("both clicks release their buttons")
}

/// Matrix row: `ContextMenuArea`. Delete, Archive and Pin are each reached
/// by a secondary click, a long press, the menu keys, assistive technology's
/// `ShowContextMenu` and the control focus reveals, then chosen by assistive
/// technology and by a click, and each is the row's own custom action.
pub(crate) fn context_menu_area() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "ContextMenuArea",
        tight(400.0, 300.0),
        click_the_first_item(Offset::new(150.0, 25.0), Offset::new(170.0, 40.0)),
        || subject(Scene::new()),
    ));
}

/// Matrix row: `ContextMenuArea` in a right-to-left locale, whose menu grows
/// leftwards from the pointer.
pub(crate) fn context_menu_area_right_to_left() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "ContextMenuArea / right-to-left",
        tight(400.0, 300.0),
        click_the_first_item(Offset::new(150.0, 25.0), Offset::new(130.0, 40.0)),
        || subject(Scene::new().rtl()),
    ));
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
