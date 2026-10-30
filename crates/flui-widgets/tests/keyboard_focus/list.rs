//! A focusable list: one Tab stop, the arrow keys and Page Up/Down, Home and
//! End over its rows, and the row it focused kept built while the pointer
//! scrolls it away.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::KeyEventResult;
use flui_platform::keyboard::{Key, KeyRepeat, KeyState, NamedKey};
use flui_view::ViewExt;
use flui_widgets::{
    Column, Expanded, Focus, ListView, RawButton, ScrollController, Scrollable, SizedBox, Text,
};

use super::{press, tab};
use crate::common::{LaidOut, lay_out, tight};

const ROW: f64 = 40.0;

/// A button labelled `label`, one row tall.
fn button(label: String) -> flui_view::BoxedView {
    SizedBox::new(300.0, ROW)
        .child(RawButton::new(Text::new(label)).on_press(|_cx| {}))
        .boxed()
}

/// A `before` button, a list of `count` buttons in a 200 px
/// scrollable, and an `after` button; and how many arrow keys the list's
/// parent received.
fn list_app(
    count: usize,
    scroll: &ScrollController,
    focusable: bool,
) -> (LaidOut, Rc<Cell<usize>>) {
    let outer = Rc::new(Cell::new(0));
    let arrows = Rc::clone(&outer);
    let list = Scrollable::new()
        .controller(scroll.clone())
        .viewport_builder(Rc::new(move |position| {
            ListView::builder(count, ROW, move |index| {
                (index < count).then(|| button(format!("row{index}")))
            })
            .position(position)
            .focusable(focusable)
            .semantics_label("Notes")
            .boxed()
        }));
    let parent = Focus::new(list)
        .can_request_focus(false)
        .on_key_event(move |_cx, event| {
            let arrow = matches!(
                event.key,
                Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown)
            );
            if arrow && event.state() == KeyState::Down {
                arrows.set(arrows.get() + 1);
                return KeyEventResult::Handled;
            }
            KeyEventResult::Ignored
        });
    let mut app = lay_out(
        Column::new(vec![
            button("before".to_owned()),
            SizedBox::new(300.0, 200.0).child(parent).boxed(),
            button("after".to_owned()),
            Expanded::new(SizedBox::new(300.0, 0.0)).boxed(),
        ]),
        tight(300.0, 400.0),
    );
    app.enable_semantics();
    settle(&mut app);
    (app, outer)
}

fn settle(app: &mut LaidOut) {
    for _ in 0..4 {
        app.pump();
    }
}

/// The label of the node the accessibility tree reports focused.
fn focused(app: &LaidOut) -> Option<String> {
    let tree = app.a11y_tree().expect("semantics enabled");
    tree.focus()
        .and_then(|node| node.label().map(ToOwned::to_owned))
}

fn key(app: &mut LaidOut, named: NamedKey) -> bool {
    let manager = app.focus_manager();
    let handled = app.enter_owner_scope(|| press(&manager, &Key::Named(named)));
    settle(app);
    handled
}

fn tab_once(app: &mut LaidOut) {
    let manager = app.focus_manager();
    let _ = app.enter_owner_scope(|| tab(&manager));
    settle(app);
}

/// Tab stops once at the list, on its first row; the arrows, Page Down, End
/// and Home move between rows, building the ones out of the band; Tab leaves
/// the list, and Tab back returns to the row it left.
pub(crate) fn list_keyboard_navigation() {
    let scroll = ScrollController::new();
    let (mut app, outer) = list_app(10_000, &scroll, true);

    tab_once(&mut app);
    assert_eq!(focused(&app).as_deref(), Some("before"));
    tab_once(&mut app);
    assert_eq!(
        focused(&app).as_deref(),
        Some("row0"),
        "Tab enters on the first row"
    );

    assert!(key(&mut app, NamedKey::ArrowDown));
    assert_eq!(focused(&app).as_deref(), Some("row1"));
    assert!(key(&mut app, NamedKey::ArrowUp));
    assert_eq!(focused(&app).as_deref(), Some("row0"));
    assert!(key(&mut app, NamedKey::ArrowUp));
    assert_eq!(
        outer.get(),
        1,
        "the first row's up arrow goes to the list's parent"
    );
    assert_eq!(focused(&app).as_deref(), Some("row0"));

    // Five rows fit; Page Down first goes to the last fully visible one,
    // not a page past the focused row.
    assert!(key(&mut app, NamedKey::ArrowDown));
    assert!(key(&mut app, NamedKey::ArrowDown));
    assert_eq!(focused(&app).as_deref(), Some("row2"));
    assert!(key(&mut app, NamedKey::PageDown));
    assert_eq!(focused(&app).as_deref(), Some("row4"));
    assert!(key(&mut app, NamedKey::PageDown));
    let paged = focused(&app);
    assert!(
        matches!(paged.as_deref(), Some("row8")),
        "a second Page Down moves a page past the edge; got {paged:?}"
    );

    // Page Down held: each repeat moves on, and the list never builds more
    // rows than the same list scrolled there by the pointer, plus the one
    // pinned row.
    let baseline_scroll = ScrollController::new();
    let (mut baseline, _) = list_app(10_000, &baseline_scroll, false);

    let manager = app.focus_manager();
    let _ = app.enter_owner_scope(|| super::down(&manager, &Key::Named(NamedKey::PageDown)));
    app.pump();
    for _ in 0..30 {
        let repeat = super::key(
            KeyState::Down,
            Key::Named(NamedKey::PageDown),
            KeyRepeat::AutoRepeat,
        );
        let _ = app.enter_owner_scope(|| manager.dispatch_key_event(&repeat));
        app.pump();
        let built = app.count_elements_by_view_type::<RawButton>() - 2;
        baseline_scroll.jump_to(scroll.pixels());
        baseline.pump();
        baseline.pump();
        let bound = baseline.count_elements_by_view_type::<RawButton>() - 2 + 1;
        assert!(
            built <= bound,
            "{built} rows built; the pointer-scrolled list builds {bound} with the pinned row"
        );
    }
    super::up(&manager, &Key::Named(NamedKey::PageDown));
    settle(&mut app);
    let held = focused(&app).expect("a row holds focus");
    let reached: usize = held
        .strip_prefix("row")
        .and_then(|index| index.parse().ok())
        .expect("a row label");
    assert!(reached > 100, "held Page Down kept moving; reached {held}");

    assert!(key(&mut app, NamedKey::End));
    assert_eq!(
        focused(&app).as_deref(),
        Some("row9999"),
        "End builds and focuses the last row"
    );
    assert!(key(&mut app, NamedKey::ArrowDown));
    assert_eq!(
        outer.get(),
        2,
        "the last row's down arrow goes to the list's parent"
    );
    assert!(key(&mut app, NamedKey::Home));
    assert_eq!(focused(&app).as_deref(), Some("row0"));

    assert!(key(&mut app, NamedKey::ArrowDown));
    tab_once(&mut app);
    assert_eq!(
        focused(&app).as_deref(),
        Some("after"),
        "Tab leaves after one stop"
    );
    let manager = app.focus_manager();
    let shift_tab = app.enter_owner_scope(|| {
        super::chord(
            &manager,
            &Key::Named(NamedKey::Tab),
            flui_platform::keyboard::Modifiers::SHIFT,
        )
    });
    assert!(shift_tab);
    settle(&mut app);
    assert_eq!(
        focused(&app).as_deref(),
        Some("row1"),
        "Shift+Tab returns to the row the list remembers"
    );
}

/// The focused row stays focused, built and announced while the pointer
/// scrolls it far away, and the next arrow key moves from it.
pub(crate) fn focused_row_is_pinned_while_scrolled_away() {
    let scroll = ScrollController::new();
    let (mut app, _) = list_app(10_000, &scroll, true);
    tab_once(&mut app);
    tab_once(&mut app);
    assert!(key(&mut app, NamedKey::ArrowDown));
    assert_eq!(focused(&app).as_deref(), Some("row1"));

    scroll.jump_to(200_000.0);
    settle(&mut app);
    assert_eq!(
        scroll.pixels(),
        200_000.0,
        "the pointer's scroll stands: nothing scrolls the focused row back into view"
    );
    assert_eq!(
        focused(&app).as_deref(),
        Some("row1"),
        "the scrolled-away row keeps focus and its node"
    );

    assert!(key(&mut app, NamedKey::ArrowDown));
    assert_eq!(
        focused(&app).as_deref(),
        Some("row2"),
        "the arrow moves from the focused row, not from the viewport"
    );
}

/// The `before` and `after` buttons around an empty focusable list.
fn empty_app() -> LaidOut {
    let scroll = ScrollController::new();
    let list = Scrollable::new()
        .controller(scroll)
        .viewport_builder(Rc::new(move |position| {
            ListView::builder(0, ROW, |_| None)
                .position(position)
                .focusable(true)
                .semantics_label("Notes")
                .empty_label("No notes")
                .boxed()
        }));
    let mut app = lay_out(
        Column::new(vec![
            button("before".to_owned()),
            SizedBox::new(300.0, 200.0).child(list).boxed(),
            button("after".to_owned()),
            Expanded::new(SizedBox::new(300.0, 0.0)).boxed(),
        ]),
        tight(300.0, 400.0),
    );
    app.enable_semantics();
    settle(&mut app);
    app
}

/// An empty focusable list is still one Tab stop and announces its empty
/// label.
pub(crate) fn an_empty_list_is_a_tab_stop_with_its_empty_label() {
    let mut app = empty_app();
    tab_once(&mut app);
    tab_once(&mut app);
    assert_eq!(focused(&app).as_deref(), Some("No notes"));
    tab_once(&mut app);
    assert_eq!(focused(&app).as_deref(), Some("after"));
}

/// The list's name is on its list node, with no separate label node beside
/// it; an empty list's focus lands on that named list node.
pub(crate) fn the_list_node_carries_the_list_name() {
    use flui_testing::a11y::Role;

    let scroll = ScrollController::new();
    let (app, _) = list_app(10_000, &scroll, true);
    let tree = app.a11y_tree().expect("semantics enabled");
    let named: Vec<Role> = tree
        .nodes()
        .filter(|node| node.label() == Some("Notes"))
        .map(|node| node.role())
        .collect();
    assert_eq!(named, vec![Role::List], "{}", tree.describe());

    let mut empty = empty_app();
    tab_once(&mut empty);
    tab_once(&mut empty);
    let tree = empty.a11y_tree().expect("semantics enabled");
    let focus = tree.focus().expect("the empty list holds focus");
    assert_eq!(
        (focus.role(), focus.label()),
        (Role::List, Some("No notes")),
        "{}",
        tree.describe()
    );
    assert!(
        focus.supports_action(flui_testing::Action::Focus),
        "assistive technology can focus the empty list:\n{}",
        tree.describe()
    );
    assert_eq!(tree.find_all(Role::List).len(), 1, "{}", tree.describe());
}
