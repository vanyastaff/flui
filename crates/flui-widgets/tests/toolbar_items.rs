//! `ToolbarItems`: a toolbar's commands in one row, the trailing ones that
//! don't fit behind an overflow button, driven by its own controller or one
//! a design package holds. Buttons are found and pressed as assistive
//! technology reads them.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use flui_foundation::geometry::Offset;
use flui_interaction::focus::FocusNode;
use flui_painting::Alignment;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_testing::a11y::{A11yTree, Role};
use flui_testing::replay::PointerScript;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::interaction::GestureDetector;
use flui_widgets::{
    Align, Focus, Semantics, SemanticsRole, SizedBox, ToolbarItems, ToolbarItemsController,
    WidgetsApp,
};

use crate::common::{LaidOut, lay_out, tight};

/// A button `width` wide, heard as `label`, that runs `tap`.
fn button(label: &str, width: f64, tap: impl Fn() + 'static) -> BoxedView {
    Semantics::new()
        .container(true)
        .role(SemanticsRole::Button)
        .label(label.to_owned())
        .child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move |_ctx| tap())
                .child(SizedBox::new(width, 20.0)),
        )
        .boxed()
}

/// `count` items 40 wide, "a", "b", …, that do nothing.
fn items(count: usize) -> Vec<BoxedView> {
    ["a", "b", "c", "d"][..count]
        .iter()
        .map(|label| button(label, 40.0, || {}))
        .collect::<Vec<BoxedView>>()
}

/// `toolbar` at the top left of a window 100 wide.
fn app(toolbar: ToolbarItems) -> WidgetsApp {
    WidgetsApp::new(Align::new(Alignment::TOP_LEFT).child(toolbar))
}

fn mount(toolbar: ToolbarItems) -> LaidOut {
    let mut laid = lay_out(app(toolbar), tight(100.0, 200.0));
    laid.enable_semantics();
    laid.pump();
    laid
}

fn tree(laid: &LaidOut) -> A11yTree {
    laid.a11y_tree().expect("semantics enabled")
}

/// The buttons that show, in tree order.
fn shown(laid: &LaidOut) -> Vec<String> {
    tree(laid)
        .nodes()
        .filter(|node| node.role() == Role::Button)
        .filter_map(|node| node.label().map(str::to_owned))
        .collect::<Vec<String>>()
}

fn tap(laid: &mut LaidOut, label: &str) {
    let tree = tree(laid);
    let node = tree
        .find_by_role_and_label(Role::Button, label)
        .unwrap_or_else(|error| panic!("{label} shows: {error}"));
    let bounds = tree.window_bounds(&node).expect("the button has bounds");
    let at = Offset::new(
        f64::midpoint(bounds.x0, bounds.x1),
        f64::midpoint(bounds.y0, bounds.y1),
    );
    laid.replay(&PointerScript::tap(at));
    laid.pump();
}

/// A design package's toolbar holds its controller: the controller names
/// the items the row left out, the package's own button offers them (here
/// it notes them, as its popup would), and expanding through the controller
/// shows them in the column.
///
/// Red-check: report no split (`hidden` stays empty); listen to no
/// controller given (the column never shows).
pub(crate) fn a_toolbar_s_controller_expands_it_and_names_what_it_hides() {
    let controller = ToolbarItemsController::new();
    let offered: Rc<RefCell<Vec<Range<usize>>>> = Rc::default();
    let noted = Rc::clone(&offered);
    let toolbar = ToolbarItems::new(items(3))
        .controller(controller.clone())
        .overflow_button(move |controller, _ctx| {
            let (controller, noted) = (controller.clone(), Rc::clone(&noted));
            button("Popup", 20.0, move || {
                noted.borrow_mut().push(controller.hidden());
            })
        });
    let mut laid = mount(toolbar);
    assert_eq!(controller.hidden(), 2..3);
    assert_eq!(shown(&laid), ["a", "b", "Popup"]);

    tap(&mut laid, "Popup");
    assert_eq!(offered.borrow().as_slice(), &[Range { start: 2, end: 3 }]);
    assert!(
        !controller.is_expanded(),
        "the package's button chose a popup"
    );

    controller.set_expanded(true);
    laid.pump();
    assert_eq!(shown(&laid), ["c", "Popup"]);
    controller.toggle();
    laid.pump();
    assert_eq!(shown(&laid), ["a", "b", "Popup"]);
}

/// Without a controller the plain button shows the left-over items and
/// takes them back; a toolbar rebuilt with another number of items starts
/// from its row, and one whose items all fit shows no button.
///
/// Red-check: keep the expanded state across the change of items (the
/// column still shows).
pub(crate) fn a_toolbar_s_own_controller_starts_from_the_row_when_its_items_change() {
    let mut laid = mount(ToolbarItems::new(items(3)));
    let row = shown(&laid);
    assert_eq!(row.first().map(String::as_str), Some("a"));
    assert_eq!(row.last().map(String::as_str), Some("More"));
    assert!(!row.contains(&"c".to_owned()), "c is behind More: {row:?}");

    tap(&mut laid, "More");
    let column = shown(&laid);
    assert!(
        column.contains(&"c".to_owned()) && column.contains(&"Back".to_owned()),
        "the column shows c and Back: {column:?}"
    );
    assert!(
        !column.contains(&"a".to_owned()),
        "a stays in the row: {column:?}"
    );

    laid.pump_widget(app(ToolbarItems::new(items(4))));
    laid.pump();
    let row = shown(&laid);
    assert_eq!(row.last().map(String::as_str), Some("More"));
    assert!(row.contains(&"a".to_owned()), "the row is back: {row:?}");

    laid.pump_widget(app(ToolbarItems::new(items(2))));
    laid.pump();
    assert_eq!(shown(&laid), ["a", "b"]);
}

/// Nothing in a toolbar takes the focus, so a field under it keeps the
/// focus and its input session, and no hidden item can hold it.
///
/// Red-check: build the row without `ExcludeFocus` (the item's node takes
/// the focus).
pub(crate) fn nothing_in_a_toolbar_takes_the_focus() {
    let node = FocusNode::with_debug_label("item");
    let focusable = Focus::new(SizedBox::new(40.0, 20.0))
        .focus_node(Rc::clone(&node))
        .boxed();
    let mut laid = mount(ToolbarItems::new(vec![focusable]));
    let _outcome = node.request_focus();
    laid.pump();
    assert!(!node.has_focus(), "the toolbar's item takes no focus");
}
