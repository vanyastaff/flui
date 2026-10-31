//! `ReorderableList`: a row moved by its handle and by a touch long press,
//! reported once with its index counted after removal, the drags that move
//! nothing, the preview while a drag runs, the move commands on keys and
//! assistive technology with their announcement, a dragged row that scrolls
//! out of the built band, and the gesture alternatives across every path.
//!
//! One fixture, a shelf, holds them: an overlay for the drag's feedback over
//! a list of named rows the app moves in its model.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::events::{
    PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, KeyEvent, KeyState, NamedKey};
use flui_testing::a11y::{A11yTree, Action, ActionData, ActionRequest, NodeId, TreeId};
use flui_testing::replay::{MouseScript, PointerScript};
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};
use flui_view::StateHandle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{
    Alternative, GestureAlternatives, GestureCommand, MenuSpec, Navigator, NavigatorHandle,
    PageRoute, ReorderCause, ReorderIntent, ReorderPreview, ReorderableList, SelectionArea,
    SingleActivator, SizedBox, Text, Urgency,
};

use crate::common::{LaidOut, lay_out, tight};

/// A row's height; the list is 400 wide, and the default handle sits at
/// each row's leading end.
const ROW: f64 = 50.0;
/// Where the default handle of a row is pressed: inside the touch minimum
/// square at the row's start.
const HANDLE_X: f64 = 24.0;
/// Where a row's content is pressed.
const CONTENT_X: f64 = 200.0;

/// The middle of row `index`, at `x`.
fn row_at(index: usize, x: f64) -> (f64, f64) {
    #[expect(clippy::cast_precision_loss, reason = "a few hundred rows")]
    let top = index as f64 * ROW;
    (x, top + ROW / 2.0)
}

/// What the shelf's moves recorded.
#[derive(Default)]
struct Log {
    moves: RefCell<Vec<(usize, usize, ReorderCause)>>,
    previews: RefCell<Vec<ReorderPreview>>,
    /// The texts selected in rows, when the rows are selectable.
    selected: RefCell<Vec<Option<String>>>,
    /// Whether each row's text sits in a `SelectionArea`.
    selectable: std::cell::Cell<bool>,
    /// The rows the item builder built, by index.
    built: RefCell<Vec<usize>>,
    /// Whether the list has a feedback builder, and the rows it built.
    with_feedback: std::cell::Cell<bool>,
    fed: RefCell<Vec<usize>>,
}

/// The shelf: its model, what it recorded and whether the list is shown.
#[derive(Clone)]
struct Shelf {
    items: StateHandle<Vec<String>>,
    shown: StateHandle<bool>,
    /// Whether the list scrolls horizontally.
    horizontal: StateHandle<bool>,
    log: Rc<Log>,
}

impl Shelf {
    fn new(items: &[&str]) -> Self {
        Self {
            items: StateHandle::new(items.iter().map(|item| (*item).to_owned()).collect()),
            shown: StateHandle::new(true),
            horizontal: StateHandle::new(false),
            log: Rc::default(),
        }
    }

    /// `count` rows named `0`, `1`, ….
    fn numbered(count: usize) -> Self {
        let names: Vec<String> = (0..count).map(|index| index.to_string()).collect();
        Self {
            items: StateHandle::new(names),
            shown: StateHandle::new(true),
            horizontal: StateHandle::new(false),
            log: Rc::default(),
        }
    }

    fn items(&self) -> Vec<String> {
        self.items.with(Clone::clone)
    }

    fn moves(&self) -> Vec<(usize, usize, ReorderCause)> {
        self.log.moves.borrow().clone()
    }

    fn mount(&self) -> LaidOut {
        let mut laid = lay_out(Desk(self.clone()), tight(400.0, 200.0));
        laid.pump();
        laid
    }
}

/// The navigator whose overlay shows the drag's feedback and the rows'
/// menus, over the shelf's list.
#[derive(Clone)]
struct Desk(Shelf);

impl View for Desk {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Desk {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let navigator = NavigatorHandle::new();
        let shelf = self.0.clone();
        navigator.seed_initial(PageRoute::<()>::new(move |_, _, _| {
            ShelfList(shelf.clone()).into_view().boxed()
        }));
        Navigator::new(navigator)
    }
}

/// The list over the shelf's model, rebuilt when the model moves.
#[derive(Clone)]
struct ShelfList(Shelf);

impl View for ShelfList {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

struct ShelfListState {
    items: StateHandle<Vec<String>>,
    shown: StateHandle<bool>,
    /// Whether the list scrolls horizontally.
    horizontal: StateHandle<bool>,
}

impl StatefulView for ShelfList {
    type State = ShelfListState;

    fn create_state(&self) -> ShelfListState {
        ShelfListState {
            items: self.0.items.clone(),
            shown: self.0.shown.clone(),
            horizontal: self.0.horizontal.clone(),
        }
    }
}

impl ViewState<ShelfList> for ShelfListState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.items.bind(ctx);
        self.shown.bind(ctx);
        self.horizontal.bind(ctx);
    }

    fn build(&self, view: &ShelfList, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.shown.with(|shown| *shown) {
            return SizedBox::shrink().into_view().boxed();
        }
        let shelf = &view.0;
        let horizontal = self.horizontal.with(|horizontal| *horizontal);
        // A row is `ROW` along the list, the rest of the cross axis beside its handle.
        let (width, height) = if horizontal {
            (ROW, 152.0)
        } else {
            (352.0, ROW)
        };
        let count = self.items.with(Vec::len);
        let (keys, rows, labels, model) = (
            self.items.clone(),
            self.items.clone(),
            self.items.clone(),
            self.items.clone(),
        );
        let (fed, with_feedback) = (Rc::clone(&shelf.log), shelf.log.with_feedback.get());
        let (moved, previewed, selecting) = (
            Rc::clone(&shelf.log),
            Rc::clone(&shelf.log),
            Rc::clone(&shelf.log),
        );
        let list = ReorderableList::builder(
            count,
            move |index| keys.with(|items| items[index].clone()),
            move |item| {
                selecting.built.borrow_mut().push(item.index());
                let name = rows.with(|items| items[item.index()].clone());
                let text = Text::new(name);
                if !selecting.selectable.get() {
                    return SizedBox::new(width, height).child(text).into_view().boxed();
                }
                let record = Rc::clone(&selecting);
                let area = SelectionArea::new(text).on_selection_changed(move |_cx, change| {
                    record.selected.borrow_mut().push(
                        change
                            .selected
                            .as_ref()
                            .map(|selected| selected.text.clone()),
                    );
                });
                SizedBox::new(width, height).child(area).into_view().boxed()
            },
            move |_cx, change| {
                moved
                    .moves
                    .borrow_mut()
                    .push((change.from, change.to, change.cause));
                model.update(|items| {
                    let item = items.remove(change.from);
                    items.insert(change.to, item);
                });
            },
        )
        .item_label(move |index| labels.with(|items| items[index].clone()))
        .on_reorder_preview(move |_cx, preview| previewed.previews.borrow_mut().push(preview))
        .item_extent_estimate(ROW)
        .scroll_direction(if horizontal {
            Axis::Horizontal
        } else {
            Axis::Vertical
        });
        let list = if with_feedback {
            list.feedback(move |item| {
                fed.fed.borrow_mut().push(item.index());
                SizedBox::new(120.0, ROW)
            })
        } else {
            list
        };
        list.into_view().boxed()
    }
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..8 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// Press with the mouse at `from`, move past the slop, then to `to`.
fn mouse_drag(laid: &LaidOut, from: (f64, f64), to: (f64, f64)) {
    laid.dispatch_pointer_down(from.0, from.1);
    laid.dispatch_pointer_move(from.0, from.1 + 15.0);
    laid.dispatch_pointer_move(to.0, to.1);
}

fn touch(laid: &LaidOut, id: u64, (x, y): (f64, f64), phase: u8) {
    let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero contact");
    let position = Offset::new(x, y);
    let event = match phase {
        0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
        1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
        _ => make_up_event_for_id(pointer, position, PointerKind::Touch),
    }
    .expect("finite contact");
    laid.dispatch_pointer_event(&event);
}

fn press(laid: &LaidOut, activator: &SingleActivator) {
    for state in [KeyState::Down, KeyState::Up] {
        laid.dispatch_key(
            KeyEvent::new(
                state,
                activator.trigger().clone(),
                Code::Unidentified,
                EventTime::from_nanos(0),
            )
            .with_modifiers(activator.modifiers()),
        );
    }
}

/// Whether `node` or a node beneath it is labelled `row`.
fn names_row(tree: &A11yTree, node: NodeId, row: &str) -> bool {
    let Some(found) = tree.nodes().find(|candidate| candidate.id() == node) else {
        return false;
    };
    found.label() == Some(row)
        || found
            .child_ids()
            .iter()
            .any(|child| names_row(tree, *child, row))
}

/// The node of the row named `row` offering `label`, and the action's id.
fn row_action(laid: &LaidOut, row: &str, label: &str) -> Option<(NodeId, i32)> {
    let tree = laid.a11y_tree()?;
    tree.nodes().find_map(|node| {
        let action = node
            .raw()
            .custom_actions()
            .iter()
            .find(|action| action.description == label)?;
        names_row(&tree, node.id(), row).then_some((node.id(), action.id))
    })
}

/// Invoke `label` on the row named `row`, as assistive technology does.
fn custom_action(laid: &mut LaidOut, row: &str, label: &str) {
    let (target, id) =
        row_action(laid, row, label).unwrap_or_else(|| panic!("row {row:?} offers no {label:?}"));
    laid.invoke_semantics_action(ActionRequest {
        action: Action::CustomAction,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::CustomAction(id)),
    })
    .expect("the custom action is delivered");
}

/// Tab into the list, whose one Tab stop is its remembered row, then walk
/// its rows with the arrow keys until the row named `row` offering `label`
/// has focus.
fn focus_row(laid: &mut LaidOut, row: &str, label: &str) {
    press(laid, &SingleActivator::named(NamedKey::Tab));
    laid.pump();
    for _ in 0..32 {
        let focused = row_action(laid, row, label).is_some_and(|(node, _)| {
            laid.a11y_tree()
                .and_then(|tree| tree.focus().map(|focus| focus.id() == node))
                .unwrap_or(false)
        });
        if focused {
            return;
        }
        press(laid, &SingleActivator::named(NamedKey::ArrowDown));
        laid.pump();
    }
    panic!("row {row:?} offering {label:?} never took focus");
}

fn polite(laid: &LaidOut) -> Option<String> {
    laid.a11y_tree()
        .expect("semantics enabled")
        .announcement(Urgency::Polite)
        .map(str::to_owned)
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

// --- Rows ---------------------------------------------------------------------------------------

/// A drag by a row's handle reports the move once, with `to` counted after
/// the item left `from`: "a" dropped on "d" goes last, where "d" was. A drag
/// back onto its own row moves nothing.
pub(crate) fn a_handle_drag_reports_one_move_counted_after_removal() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(3, HANDLE_X));
    laid.pump();
    laid.dispatch_pointer_up(row_at(3, HANDLE_X).0, row_at(3, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(0, 3, ReorderCause::Drag)]);
    assert_eq!(shelf.items(), strings(&["b", "c", "d", "a"]));

    mouse_drag(&laid, row_at(1, HANDLE_X), row_at(1, HANDLE_X));
    laid.dispatch_pointer_up(row_at(1, HANDLE_X).0, row_at(1, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(
        shelf.moves().len(),
        1,
        "a row dropped on itself does not move"
    );
}

/// The mouse drags a row only by its handle; a touch drags it by a long
/// press anywhere on the row, and a quick touch move does not.
pub(crate) fn a_touch_long_press_drags_a_row_and_a_mouse_needs_the_handle() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, CONTENT_X), row_at(2, CONTENT_X));
    laid.dispatch_pointer_up(row_at(2, CONTENT_X).0, row_at(2, CONTENT_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "the mouse drags only the handle");

    touch(&laid, 41, row_at(0, CONTENT_X), 0);
    touch(&laid, 41, row_at(2, CONTENT_X), 1);
    touch(&laid, 41, row_at(2, CONTENT_X), 2);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "a quick touch move drags nothing");

    touch(&laid, 42, row_at(0, CONTENT_X), 0);
    laid.pump_for(Duration::from_secs(1));
    touch(&laid, 42, (CONTENT_X, 40.0), 1);
    touch(&laid, 42, row_at(2, CONTENT_X), 1);
    laid.pump();
    touch(&laid, 42, row_at(2, CONTENT_X), 2);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(0, 2, ReorderCause::Drag)]);
    assert_eq!(shelf.items(), strings(&["b", "c", "a", "d"]));
}

/// A drag that does not drop moves nothing: Escape, the contact cancelled,
/// and the list unmounted under it.
pub(crate) fn a_drag_that_does_not_drop_moves_nothing() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(2, HANDLE_X));
    laid.pump();
    press(&laid, &SingleActivator::named(NamedKey::Escape));
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "Escape cancels the drag");

    let pointer = flui_interaction::PointerId::try_from(9).expect("nonzero contact");
    let at = |index: usize| {
        let (x, y) = row_at(index, HANDLE_X);
        Offset::new(x, y)
    };
    let down = make_down_event_for_id(pointer, at(0), PointerKind::Mouse).expect("finite");
    laid.dispatch_pointer_event(&down);
    for index in [1, 2] {
        let moved = make_move_event_for_id(pointer, at(index), PointerKind::Mouse).expect("finite");
        laid.dispatch_pointer_event(&moved);
    }
    laid.pump();
    laid.dispatch_pointer_event(&flui_interaction::events::make_cancel_event_for_id(
        pointer,
        PointerKind::Mouse,
    ));
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "a cancelled contact drops nothing");

    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(2, HANDLE_X));
    laid.pump();
    shelf.shown.update(|shown| *shown = false);
    laid.pump();
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "an unmounted list drops nothing");
    assert_eq!(shelf.items(), strings(&["a", "b", "c", "d"]));
}

/// While a drag runs, the preview reports each row it moves over, once per
/// change, from the dragged item's index: none between two rows, "over none"
/// once the drag leaves the list, and once more when a drag ends over a row.
pub(crate) fn the_preview_follows_the_drag_over_the_rows() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let seen = || -> Vec<(usize, Option<usize>)> {
        shelf
            .log
            .previews
            .borrow()
            .iter()
            .map(|preview| (preview.from, preview.over))
            .collect()
    };
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(1, HANDLE_X));
    laid.pump();
    laid.dispatch_pointer_move(row_at(1, HANDLE_X).0, row_at(1, HANDLE_X).1 + 2.0);
    laid.dispatch_pointer_move(row_at(3, HANDLE_X).0, row_at(3, HANDLE_X).1);
    laid.pump();
    assert_eq!(seen(), [(0, Some(0)), (0, Some(1)), (0, Some(3))]);
    laid.dispatch_pointer_move(450.0, row_at(1, HANDLE_X).1);
    laid.pump();
    assert_eq!(
        seen(),
        [(0, Some(0)), (0, Some(1)), (0, Some(3)), (0, None)],
        "a drag that left the list is over no row"
    );
    laid.dispatch_pointer_move(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    laid.pump();
    press(&laid, &SingleActivator::named(NamedKey::Escape));
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(
        seen(),
        [
            (0, Some(0)),
            (0, Some(1)),
            (0, Some(3)),
            (0, None),
            (0, Some(2)),
            (0, None)
        ],
        "a cancelled drag ends over no row"
    );
    assert_eq!(shelf.moves(), []);
}

/// A drag shows the list's feedback under the pointer, the row's label by
/// default; the item builder builds each row once, so what a row owns alone
/// (a field's focus node, a controller) is never mounted twice.
pub(crate) fn a_drag_shows_the_feedback_and_never_a_second_copy_of_the_row() {
    let built = |shelf: &Shelf| shelf.log.built.borrow().iter().filter(|i| **i == 0).count();
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    let before = built(&shelf);
    assert_eq!(before, 1, "a row is built once");
    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(2, HANDLE_X));
    laid.pump();
    assert_eq!(built(&shelf), before, "the drag builds no copy of the row");
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);

    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    shelf.log.with_feedback.set(true);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(1, HANDLE_X), row_at(3, HANDLE_X));
    laid.pump();
    assert!(
        shelf.log.fed.borrow().contains(&1),
        "the dragged row's feedback comes from the list's builder"
    );
    laid.dispatch_pointer_up(row_at(3, HANDLE_X).0, row_at(3, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(1, 3, ReorderCause::Drag)]);
}

/// A list turned horizontal after it mounted drags its rows by a handle
/// moved along the new axis: the handles' recognizers follow the list's axis.
pub(crate) fn a_list_that_turns_horizontal_drags_along_its_new_axis() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    shelf.horizontal.update(|horizontal| *horizontal = true);
    settle(&mut laid);
    // A horizontal row is a column `ROW` wide, its handle band on top.
    let handle = |index: f64| (index * ROW + ROW / 2.0, HANDLE_X);
    laid.dispatch_pointer_down(handle(0.0).0, handle(0.0).1);
    laid.dispatch_pointer_move(handle(0.0).0 + 15.0, handle(0.0).1);
    laid.dispatch_pointer_move(handle(3.0).0, handle(3.0).1);
    laid.pump();
    laid.dispatch_pointer_up(handle(3.0).0, handle(3.0).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(0, 3, ReorderCause::Drag)]);
}

/// The move commands: Alt+Down with focus in a row and assistive
/// technology's "Move to the end" move it as a command, and the move is
/// announced with the row's name and new position. A row offers only the
/// moves that apply to it.
pub(crate) fn the_move_commands_move_a_row_and_announce_it() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    laid.enable_semantics();
    laid.pump();
    assert!(
        row_action(&laid, "a", "Move up").is_none(),
        "the first row has no Move up"
    );
    assert!(
        row_action(&laid, "d", "Move down").is_none(),
        "the last row has no Move down"
    );

    focus_row(&mut laid, "b", "Move down");
    press(&laid, &SingleActivator::named(NamedKey::ArrowDown).alt());
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(1, 2, ReorderCause::Command)]);
    assert_eq!(shelf.items(), strings(&["a", "c", "b", "d"]));
    assert_eq!(polite(&laid).as_deref(), Some("Moved b to position 3 of 4"));

    custom_action(&mut laid, "a", "Move to the end");
    settle(&mut laid);
    assert_eq!(shelf.moves()[1], (0, 3, ReorderCause::Command));
    assert_eq!(shelf.items(), strings(&["c", "b", "d", "a"]));
    assert_eq!(polite(&laid).as_deref(), Some("Moved a to position 4 of 4"));
}

/// A dragged row scrolled out of the built band keeps its drag: resting at
/// the list's bottom edge scrolls it, and the drop far down moves the row
/// there.
pub(crate) fn a_dragged_row_scrolled_out_of_the_band_still_drops() {
    let shelf = Shelf::numbered(200);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, HANDLE_X), (HANDLE_X, 196.0));
    for _ in 0..240 {
        laid.pump_for(Duration::from_millis(16));
    }
    laid.dispatch_pointer_move(HANDLE_X, 100.0);
    laid.pump();
    laid.dispatch_pointer_up(HANDLE_X, 100.0);
    settle(&mut laid);
    let moves = shelf.moves();
    assert_eq!(moves.len(), 1, "the drag outlived its row: {moves:?}");
    let (from, to, cause) = moves[0];
    assert_eq!((from, cause), (0, ReorderCause::Drag));
    assert!(to > 20, "the row dropped far down the list, at {to}");
}

/// A row and the selectable text inside it both want a touch long press: the
/// deeper one wins, so the long press selects a word and moves nothing, and
/// the row's handle still drags it.
pub(crate) fn a_long_press_on_selectable_text_selects_and_the_handle_still_drags() {
    let shelf = Shelf::new(&["alpha beta", "b", "c"]);
    shelf.log.selectable.set(true);
    let mut laid = shelf.mount();
    let (_, y) = row_at(0, 0.0);
    laid.replay(&PointerScript::long_press(
        Offset::new(HANDLE_X + 30.0, y),
        Duration::from_millis(800),
    ));
    settle(&mut laid);
    assert_eq!(shelf.moves(), [], "the long press moved the row");
    assert!(
        shelf
            .log
            .selected
            .borrow()
            .iter()
            .flatten()
            .any(|text| text == "alpha"),
        "the long press selected the word under it: {:?}",
        shelf.log.selected.borrow()
    );

    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(2, HANDLE_X));
    laid.pump();
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(0, 2, ReorderCause::Drag)]);
}

/// "Move to the end" in a long lazy list keeps focus on the moved row: the
/// list brings it into view at its new place and focus follows its key.
pub(crate) fn moving_a_row_to_the_end_of_a_long_list_keeps_its_focus() {
    let shelf = Shelf::numbered(10_000);
    let mut laid = shelf.mount();
    laid.enable_semantics();
    laid.pump();
    focus_row(&mut laid, "0", "Move to the end");
    press(&laid, &SingleActivator::named(NamedKey::End).alt());
    settle(&mut laid);
    assert_eq!(shelf.moves(), [(0, 9_999, ReorderCause::Command)]);
    let focused = laid
        .a11y_tree()
        .and_then(|tree| tree.focus().map(|node| names_row(&tree, node.id(), "0")));
    assert_eq!(focused, Some(true), "focus stayed on the moved row");
}

// --- Input-path matrix --------------------------------------------------------------------------

/// The middle row's alternatives as `assert_gesture_alternatives` reads
/// them: its four moves, in a menu.
fn middle_row_alternatives() -> GestureAlternatives {
    let menu = MenuSpec::new(
        GestureCommand::new("Move to the start", ReorderIntent::Start),
        |_cx, _request| {},
    )
    .item(GestureCommand::new("Move up", ReorderIntent::Up))
    .item(GestureCommand::new("Move down", ReorderIntent::Down))
    .item(GestureCommand::new("Move to the end", ReorderIntent::End));
    GestureAlternatives::try_new(Alternative::ContextMenu(menu)).expect("distinct moves")
}

/// The gesture of "Move to the start": the middle row's handle dragged onto
/// the first row.
fn drag_the_middle_row_to_the_start() -> PointerScript {
    let (x, y) = row_at(1, HANDLE_X);
    let (_, top) = row_at(0, HANDLE_X);
    MouseScript::new("the middle row's handle onto the first row")
        .press(Offset::new(x, y))
        .wait(Duration::from_millis(16))
        .move_to(Offset::new(x, y - 15.0))
        .wait(Duration::from_millis(16))
        .move_to(Offset::new(x, top))
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("the drag releases its button")
}

/// Matrix row: `ReorderableList`. Each move of a row is reached by the
/// handle drag, assistive technology's action, its Alt key, and the row's
/// menu opened by every path a menu has, each leaving the same model and
/// announcement.
pub(crate) fn reorderable_list() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "ReorderableList",
        tight(400.0, 200.0),
        drag_the_middle_row_to_the_start(),
        || {
            let shelf = Shelf::new(&["a", "b", "c"]);
            let items = shelf.items.clone();
            AlternativesSubject::new(Desk(shelf), &middle_row_alternatives(), move || {
                items.with(Clone::clone)
            })
        },
    ));
}
