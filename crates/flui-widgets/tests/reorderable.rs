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

use flui_foundation::geometry::Offset;
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
    PageRoute, ReorderCause, ReorderIntent, ReorderPreview, ReorderableList, SingleActivator,
    SizedBox, Text, Urgency,
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
}

/// The shelf: its model, what it recorded and whether the list is shown.
#[derive(Clone)]
struct Shelf {
    items: StateHandle<Vec<String>>,
    shown: StateHandle<bool>,
    log: Rc<Log>,
}

impl Shelf {
    fn new(items: &[&str]) -> Self {
        Self {
            items: StateHandle::new(items.iter().map(|item| (*item).to_owned()).collect()),
            shown: StateHandle::new(true),
            log: Rc::default(),
        }
    }

    /// `count` rows named `0`, `1`, ….
    fn numbered(count: usize) -> Self {
        let names: Vec<String> = (0..count).map(|index| index.to_string()).collect();
        Self {
            items: StateHandle::new(names),
            shown: StateHandle::new(true),
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
}

impl StatefulView for ShelfList {
    type State = ShelfListState;

    fn create_state(&self) -> ShelfListState {
        ShelfListState {
            items: self.0.items.clone(),
            shown: self.0.shown.clone(),
        }
    }
}

impl ViewState<ShelfList> for ShelfListState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.items.bind(ctx);
        self.shown.bind(ctx);
    }

    fn build(&self, view: &ShelfList, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.shown.with(|shown| *shown) {
            return SizedBox::shrink().into_view().boxed();
        }
        let shelf = &view.0;
        let count = self.items.with(Vec::len);
        let (keys, rows, labels, model) = (
            self.items.clone(),
            self.items.clone(),
            self.items.clone(),
            self.items.clone(),
        );
        let (moved, previewed) = (Rc::clone(&shelf.log), Rc::clone(&shelf.log));
        ReorderableList::builder(
            count,
            move |index| keys.with(|items| items[index].clone()),
            move |item| {
                let name = rows.with(|items| items[item.index()].clone());
                SizedBox::new(352.0, ROW).child(Text::new(name))
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
        .into_view()
        .boxed()
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

/// Tab until the focused node is the row named `row` offering `label`.
fn focus_row(laid: &mut LaidOut, row: &str, label: &str) {
    for _ in 0..32 {
        press(laid, &SingleActivator::named(NamedKey::Tab));
        laid.pump();
        let focused = row_action(laid, row, label).is_some_and(|(node, _)| {
            laid.a11y_tree()
                .and_then(|tree| tree.focus().map(|focus| focus.id() == node))
                .unwrap_or(false)
        });
        if focused {
            return;
        }
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
    assert!(shelf.moves().is_empty(), "the mouse drags only the handle");

    touch(&laid, 41, row_at(0, CONTENT_X), 0);
    touch(&laid, 41, row_at(2, CONTENT_X), 1);
    touch(&laid, 41, row_at(2, CONTENT_X), 2);
    settle(&mut laid);
    assert!(shelf.moves().is_empty(), "a quick touch move drags nothing");

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
    assert!(shelf.moves().is_empty(), "Escape cancels the drag");

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
    assert!(
        shelf.moves().is_empty(),
        "a cancelled contact drops nothing"
    );

    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(2, HANDLE_X));
    laid.pump();
    shelf.shown.update(|shown| *shown = false);
    laid.pump();
    laid.dispatch_pointer_up(row_at(2, HANDLE_X).0, row_at(2, HANDLE_X).1);
    settle(&mut laid);
    assert!(shelf.moves().is_empty(), "an unmounted list drops nothing");
    assert_eq!(shelf.items(), strings(&["a", "b", "c", "d"]));
}

/// While a drag runs, the preview reports each row it moves over, once per
/// change, from the dragged item's index.
pub(crate) fn the_preview_follows_the_drag_over_the_rows() {
    let shelf = Shelf::new(&["a", "b", "c", "d"]);
    let mut laid = shelf.mount();
    mouse_drag(&laid, row_at(0, HANDLE_X), row_at(1, HANDLE_X));
    laid.pump();
    laid.dispatch_pointer_move(row_at(1, HANDLE_X).0, row_at(1, HANDLE_X).1 + 2.0);
    laid.dispatch_pointer_move(row_at(3, HANDLE_X).0, row_at(3, HANDLE_X).1);
    laid.pump();
    press(&laid, &SingleActivator::named(NamedKey::Escape));
    laid.dispatch_pointer_up(row_at(3, HANDLE_X).0, row_at(3, HANDLE_X).1);
    settle(&mut laid);
    let seen: Vec<(usize, Option<usize>)> = shelf
        .log
        .previews
        .borrow()
        .iter()
        .map(|preview| (preview.from, preview.over))
        .collect();
    assert_eq!(seen, [(0, Some(0)), (0, Some(1)), (0, Some(3))]);
    assert!(shelf.moves().is_empty());
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
