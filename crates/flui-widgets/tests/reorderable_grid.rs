//! `ReorderableGrid`: a tile moved by its handle any way and by a touch long
//! press, the moves along a row and across rows with their announcement, row
//! moves that follow the rows the grid lays out, in a right-to-left locale
//! too, and the gesture alternatives across every path.
//!
//! One fixture, a board, holds them: an overlay for the drag's feedback over
//! a grid of named tiles the app moves in its model, at most `TILE` wide.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::events::{
    PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_painting::Alignment;
use flui_painting::typography::TextDirection;
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, KeyEvent, KeyState, NamedKey};
use flui_testing::a11y::{A11yTree, Action, ActionData, ActionRequest, NodeId, TreeId};
use flui_testing::replay::{MouseScript, PointerScript};
use flui_testing::widgets::SharedSignal;
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{
    Align, Alternative, Directionality, GestureAlternatives, GestureCommand, MenuSpec, Navigator,
    NavigatorHandle, PageRoute, ReorderCause, ReorderIntent, ReorderableGrid, SingleActivator,
    SizedBox, SliverGridDelegateWithMaxCrossAxisExtent, Text, Urgency,
};

use crate::common::{LaidOut, lay_out, tight};

/// A tile's side: the grid puts as many tiles in a row as fit at this width.
const TILE: f64 = 100.0;
/// Where a tile's default handle is pressed, down from the tile's top: inside
/// the touch minimum square centred in the band above its content.
const HANDLE_Y: f64 = 24.0;
/// Where a tile's content is pressed, down from the tile's top.
const CONTENT_Y: f64 = 74.0;

/// The point `dy` down from the top of tile `index` of a vertical grid
/// `stride` tiles wide, in the middle of its width.
fn tile_at(index: usize, stride: usize, dy: f64) -> (f64, f64) {
    #[expect(clippy::cast_precision_loss, reason = "a few dozen tiles")]
    let (column, row) = ((index % stride) as f64, (index / stride) as f64);
    (column * TILE + TILE / 2.0, row * TILE + dy)
}

/// The board: its model, the moves it recorded, and the grid's width, axis
/// and reading direction.
#[derive(Clone)]
struct Board {
    items: SharedSignal<Vec<String>>,
    width: SharedSignal<f64>,
    horizontal: SharedSignal<bool>,
    rtl: SharedSignal<bool>,
    moves: Rc<RefCell<Vec<(usize, usize, ReorderCause)>>>,
}

impl Board {
    /// `count` tiles named `0`, `1`, ….
    fn numbered(count: usize) -> Self {
        Self::new(
            &(0..count)
                .map(|index| index.to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn new(names: &[impl AsRef<str>]) -> Self {
        Self {
            items: SharedSignal::new(
                names
                    .iter()
                    .map(|name| name.as_ref().to_owned())
                    .collect::<Vec<String>>(),
            ),
            width: SharedSignal::new(400.0),
            horizontal: SharedSignal::new(false),
            rtl: SharedSignal::new(false),
            moves: Rc::default(),
        }
    }

    fn items(&self) -> Vec<String> {
        self.items.value()
    }

    fn moves(&self) -> Vec<(usize, usize, ReorderCause)> {
        self.moves.borrow().clone()
    }

    fn mount(&self) -> LaidOut {
        let mut laid = lay_out(Desk(self.clone()), tight(400.0, 300.0));
        laid.enable_semantics();
        laid.pump();
        laid
    }
}

/// The navigator whose overlay shows the drag's feedback and the tiles'
/// menus, over the board's grid.
#[derive(Clone)]
struct Desk(Board);

impl View for Desk {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

struct DeskState {
    navigator: NavigatorHandle,
    rtl: SharedSignal<bool>,
}

impl StatefulView for Desk {
    type State = DeskState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> DeskState {
        let navigator = NavigatorHandle::new();
        let board = self.0.clone();
        navigator.seed_initial(PageRoute::<()>::new(move |_, _, _| {
            BoardGrid(board.clone()).into_view().boxed()
        }));
        DeskState {
            navigator,
            rtl: self.0.rtl.clone(),
        }
    }
}

impl ViewState<Desk> for DeskState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.rtl.bind(ctx);
    }

    fn build(&self, _view: &Desk, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = if self.rtl.get(ctx) {
            TextDirection::Rtl
        } else {
            TextDirection::Ltr
        };
        Directionality::new(direction, Navigator::new(self.navigator.clone()))
    }
}

/// The grid over the board's model, rebuilt when the model moves.
#[derive(Clone)]
struct BoardGrid(Board);

impl View for BoardGrid {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

struct BoardGridState {
    items: SharedSignal<Vec<String>>,
    width: SharedSignal<f64>,
    horizontal: SharedSignal<bool>,
}

impl StatefulView for BoardGrid {
    type State = BoardGridState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> BoardGridState {
        BoardGridState {
            items: self.0.items.clone(),
            width: self.0.width.clone(),
            horizontal: self.0.horizontal.clone(),
        }
    }
}

impl ViewState<BoardGrid> for BoardGridState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.items.bind(ctx);
        self.width.bind(ctx);
        self.horizontal.bind(ctx);
    }

    fn build(&self, view: &BoardGrid, ctx: &dyn BuildContext) -> impl IntoView {
        let items = Rc::new(self.items.get(ctx));
        let count = items.len();
        let (keys, tiles, labels) = (Rc::clone(&items), Rc::clone(&items), items);
        let model = self.items.clone();
        let moved = Rc::clone(&view.0.moves);
        let grid = ReorderableGrid::builder(
            SliverGridDelegateWithMaxCrossAxisExtent::new(TILE),
            count,
            move |index| keys[index].clone(),
            move |tile| Text::new(tiles[tile.index()].clone()),
            move |ctx, change| {
                moved
                    .borrow_mut()
                    .push((change.from, change.to, change.cause));
                model
                    .update_in(ctx, |items| {
                        let item = items.remove(change.from);
                        items.insert(change.to, item);
                    })
                    .expect("the board's signal is alive while its grid is mounted");
            },
        )
        .item_label(move |index| labels[index].clone())
        .scroll_direction(if self.horizontal.get(ctx) {
            Axis::Horizontal
        } else {
            Axis::Vertical
        });
        let width = self.width.get(ctx);
        Align::new(Alignment::TOP_LEFT).child(SizedBox::new(width, 300.0).child(grid))
    }
}

/// Pump past every motion a move starts: the rows' 250 ms slides.
fn settle(laid: &mut LaidOut) {
    for _ in 0..24 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// Press with the mouse at `from`, move past the slop toward `to`, then to
/// `to`, and release there.
fn mouse_drag(laid: &mut LaidOut, from: (f64, f64), to: (f64, f64)) {
    laid.dispatch_pointer_down(from.0, from.1);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy).max(1.0);
    laid.dispatch_pointer_move(from.0 + 20.0 * dx / length, from.1 + 20.0 * dy / length);
    laid.dispatch_pointer_move(to.0, to.1);
    laid.pump();
    laid.dispatch_pointer_up(to.0, to.1);
    settle(laid);
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

fn press(laid: &mut LaidOut, activator: &SingleActivator) {
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
    settle(laid);
}

fn alt(named: NamedKey) -> SingleActivator {
    SingleActivator::named(named).alt()
}

/// Whether `node` or a node beneath it is labelled `tile`.
fn names_tile(tree: &A11yTree, node: NodeId, tile: &str) -> bool {
    let Some(found) = tree.nodes().find(|candidate| candidate.id() == node) else {
        return false;
    };
    found.label() == Some(tile)
        || found
            .child_ids()
            .iter()
            .any(|child| names_tile(tree, *child, tile))
}

/// The node of the tile named `tile` that carries its moves, with the id of
/// its action `label` when it offers that move.
fn tile_node(laid: &LaidOut, tile: &str) -> Option<(NodeId, Vec<(String, i32)>)> {
    let tree = laid.a11y_tree()?;
    tree.nodes().find_map(|node| {
        let actions = node.raw().custom_actions();
        let moves = actions
            .iter()
            .filter(|action| action.description.starts_with("Move"))
            .map(|action| (action.description.clone(), action.id))
            .collect::<Vec<_>>();
        (!moves.is_empty() && names_tile(&tree, node.id(), tile)).then(|| (node.id(), moves))
    })
}

/// The moves the tile named `tile` offers assistive technology, in order.
fn offered(laid: &LaidOut, tile: &str) -> Vec<String> {
    tile_node(laid, tile)
        .map(|(_, moves)| moves.into_iter().map(|(label, _)| label).collect())
        .unwrap_or_default()
}

/// Invoke `label` on the tile named `tile`, as assistive technology does.
fn custom_action(laid: &mut LaidOut, tile: &str, label: &str) {
    let (target, id) = tile_node(laid, tile)
        .and_then(|(node, moves)| {
            moves
                .into_iter()
                .find(|(offered, _)| offered == label)
                .map(|(_, id)| (node, id))
        })
        .unwrap_or_else(|| panic!("tile {tile:?} offers no {label:?}"));
    laid.invoke_semantics_action(ActionRequest {
        action: Action::CustomAction,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::CustomAction(id)),
    })
    .expect("the custom action is delivered");
    settle(laid);
}

fn focused(laid: &LaidOut) -> Option<NodeId> {
    laid.a11y_tree()?.focus().map(|node| node.id())
}

/// Tab into the grid, whose one Tab stop is its remembered tile, then walk
/// its rows with the arrow keys until the tile named `tile` has focus.
fn focus_tile(laid: &mut LaidOut, tile: &str) {
    press(laid, &SingleActivator::named(NamedKey::Tab));
    let mut along = NamedKey::ArrowRight;
    for _ in 0..16 {
        for _ in 0..16 {
            if tile_node(laid, tile).map(|(node, _)| node) == focused(laid) {
                return;
            }
            let before = focused(laid);
            press(laid, &SingleActivator::named(along));
            if focused(laid) == before {
                break;
            }
        }
        let before = focused(laid);
        press(laid, &SingleActivator::named(NamedKey::ArrowDown));
        assert_ne!(focused(laid), before, "tile {tile:?} never took focus");
        along = match along {
            NamedKey::ArrowRight => NamedKey::ArrowLeft,
            _ => NamedKey::ArrowRight,
        };
    }
    panic!("tile {tile:?} never took focus");
}

fn polite(laid: &LaidOut) -> Option<String> {
    laid.a11y_tree()
        .expect("semantics enabled")
        .announcement(Urgency::Polite)
        .map(str::to_owned)
}

fn names(items: &[&str]) -> Vec<String> {
    items
        .iter()
        .map(|item| (*item).to_owned())
        .collect::<Vec<String>>()
}

// --- Tiles --------------------------------------------------------------------------------------

/// A tile's handle drags it any way: across a row, diagonally and straight
/// down, the drag beats the grid's own scroll; the drop reports one move with
/// `to` counted after the item left `from`. A touch long press on the tile's
/// content drags it too, and a mouse there does not.
pub(crate) fn a_tile_dragged_by_its_handle_moves_onto_another_tile() {
    // Four rows of four tiles: the 300-high grid scrolls.
    let board = Board::numbered(16);
    let mut laid = board.mount();
    mouse_drag(&mut laid, tile_at(0, 4, HANDLE_Y), tile_at(6, 4, HANDLE_Y));
    assert_eq!(board.moves(), [(0, 6, ReorderCause::Drag)], "diagonally");
    mouse_drag(&mut laid, tile_at(1, 4, HANDLE_Y), tile_at(5, 4, HANDLE_Y));
    assert_eq!(
        board.moves()[1..],
        [(1, 5, ReorderCause::Drag)],
        "straight down, against the grid's scroll"
    );
    mouse_drag(&mut laid, tile_at(3, 4, HANDLE_Y), tile_at(2, 4, HANDLE_Y));
    assert_eq!(
        board.moves()[2..],
        [(3, 2, ReorderCause::Drag)],
        "along a row"
    );

    mouse_drag(
        &mut laid,
        tile_at(8, 4, CONTENT_Y),
        tile_at(9, 4, CONTENT_Y),
    );
    assert_eq!(board.moves().len(), 3, "the mouse drags only the handle");
    touch(&laid, 51, tile_at(8, 4, CONTENT_Y), 0);
    laid.pump_for(Duration::from_secs(1));
    touch(&laid, 51, tile_at(9, 4, CONTENT_Y - 10.0), 1);
    touch(&laid, 51, tile_at(9, 4, CONTENT_Y), 1);
    laid.pump();
    touch(&laid, 51, tile_at(9, 4, CONTENT_Y), 2);
    settle(&mut laid);
    assert_eq!(board.moves()[3..], [(8, 9, ReorderCause::Drag)]);
}

/// The moves of a tile go along its row and by a whole row: the arrows on its
/// row move it to the next tile in reading order, from a row's end to the
/// next row's start, Up and Down to the tile above or below, and a row down
/// onto a short last row lands on the last tile. A tile offers
/// only the moves that apply to it, each move starts from where the model has
/// the tile now, and each is announced with the tile's new position.
pub(crate) fn the_grid_moves_go_along_a_row_and_by_a_row() {
    // Four tiles a row: 0 1 2 3 / 4 5 6.
    let board = Board::numbered(7);
    let mut laid = board.mount();
    assert_eq!(
        offered(&laid, "0"),
        ["Move down", "Move right", "Move to the end"]
    );
    assert_eq!(
        offered(&laid, "5"),
        [
            "Move to the start",
            "Move up",
            "Move left",
            "Move right",
            "Move to the end"
        ]
    );
    assert_eq!(
        offered(&laid, "3"),
        [
            "Move to the start",
            "Move down",
            "Move left",
            "Move right",
            "Move to the end"
        ],
        "a tile past the end of the short row below still moves down, and \
         Right at a row's end moves it to the next row"
    );

    focus_tile(&mut laid, "1");
    press(&mut laid, &alt(NamedKey::ArrowRight));
    assert_eq!(board.moves(), [(1, 2, ReorderCause::Command)]);
    assert_eq!(polite(&laid).as_deref(), Some("Moved 1 to position 3 of 7"));
    press(&mut laid, &alt(NamedKey::ArrowDown));
    press(&mut laid, &alt(NamedKey::ArrowUp));
    press(&mut laid, &alt(NamedKey::ArrowLeft));
    assert_eq!(
        board.moves()[1..],
        [
            (2, 6, ReorderCause::Command),
            (6, 2, ReorderCause::Command),
            (2, 1, ReorderCause::Command)
        ],
        "each move starts from where the tile is now"
    );
    assert_eq!(board.items(), names(&["0", "1", "2", "3", "4", "5", "6"]));

    custom_action(&mut laid, "3", "Move down");
    assert_eq!(board.moves()[4..], [(3, 6, ReorderCause::Command)]);
    assert_eq!(board.items(), names(&["0", "1", "2", "4", "5", "6", "3"]));
    assert_eq!(polite(&laid).as_deref(), Some("Moved 3 to position 7 of 7"));
}

/// The row moves go by the rows the grid lays out now: a grid narrowed from
/// four tiles a row to two moves a tile down by two. A horizontal grid's rows
/// run down the screen, so Left and Right move a tile by a row and Up and Down
/// along it; in a right-to-left locale its rows run from the right, and Left
/// moves a tile forward.
pub(crate) fn the_row_moves_follow_the_rows_the_grid_lays_out() {
    let board = Board::numbered(8);
    let mut laid = board.mount();
    focus_tile(&mut laid, "0");
    press(&mut laid, &alt(NamedKey::ArrowDown));
    assert_eq!(board.moves(), [(0, 4, ReorderCause::Command)]);
    board.width.update(|width| *width = 2.0 * TILE);
    settle(&mut laid);
    custom_action(&mut laid, "0", "Move down");
    assert_eq!(
        board.moves()[1..],
        [(4, 6, ReorderCause::Command)],
        "two tiles a row now"
    );

    // Three tiles a column, down the 300-high grid.
    for (rtl, forward_row) in [(false, NamedKey::ArrowRight), (true, NamedKey::ArrowLeft)] {
        let board = Board::numbered(8);
        board.horizontal.update(|horizontal| *horizontal = true);
        board.rtl.update(|value| *value = rtl);
        let mut laid = board.mount();
        custom_action(&mut laid, "0", if rtl { "Move left" } else { "Move right" });
        assert_eq!(
            board.moves(),
            [(0, 3, ReorderCause::Command)],
            "rtl {rtl}: {forward_row:?} moves a tile a row forward"
        );
        custom_action(&mut laid, "0", "Move down");
        assert_eq!(
            board.moves()[1..],
            [(3, 4, ReorderCause::Command)],
            "rtl {rtl}: Down moves a tile along its row"
        );
        assert!(
            !offered(&laid, "1").contains(&"Move up".to_owned()),
            "rtl {rtl}: the first tile of a row has no Move up"
        );
    }
}

// --- Input-path matrix --------------------------------------------------------------------------

/// The moves as `assert_gesture_alternatives` reads them: every move a tile
/// of a two-row grid has, in a menu.
fn tile_alternatives() -> GestureAlternatives {
    let menu = MenuSpec::new(
        GestureCommand::new("Move to the start", ReorderIntent::Start),
        |_ctx, _request| {},
    )
    .item(GestureCommand::new("Move up", ReorderIntent::Up))
    .item(GestureCommand::new("Move down", ReorderIntent::Down))
    .item(GestureCommand::new("Move left", ReorderIntent::Left))
    .item(GestureCommand::new("Move right", ReorderIntent::Right))
    .item(GestureCommand::new("Move to the end", ReorderIntent::End));
    GestureAlternatives::try_new(Alternative::ContextMenu(menu)).expect("distinct moves")
}

/// The gesture of "Move to the start": the second tile's handle dragged onto
/// the first tile.
fn drag_the_second_tile_to_the_start() -> PointerScript {
    let (x, y) = tile_at(1, 4, HANDLE_Y);
    let (first, _) = tile_at(0, 4, HANDLE_Y);
    MouseScript::new("the second tile's handle onto the first tile")
        .press(Offset::new(x, y))
        .wait(Duration::from_millis(16))
        .move_to(Offset::new(x - 20.0, y))
        .wait(Duration::from_millis(16))
        .move_to(Offset::new(first, y))
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("the drag releases its button")
}

/// Matrix row: `ReorderableGrid`. Each move of a tile is reached by the
/// handle drag, assistive technology's action, its Alt key, and the tile's
/// menu opened by every path a menu has, each leaving the same model and
/// announcement.
pub(crate) fn reorderable_grid() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "ReorderableGrid",
        tight(400.0, 300.0),
        drag_the_second_tile_to_the_start(),
        || {
            // 0 1 2 3 / 4 5: every move is offered by some tile.
            let board = Board::new(&["a", "b", "c", "d", "e", "f"]);
            let items = board.items.clone();
            AlternativesSubject::new(Desk(board), &tile_alternatives(), move || items.value())
        },
    ));
}

/// A dragged tile's gap moves in reading order: the tiles between make room,
/// the tile at a row's start sliding up to the end of the row above over
/// 250 ms, and a drop puts the tile where its gap was.
pub(crate) fn a_dragged_tile_makes_room_across_the_rows() {
    let board = Board::numbered(8);
    let mut laid = board.mount();
    let four = |laid: &LaidOut| laid.absolute_offset(laid.find_text("4").expect("tile 4"));
    let from = four(&laid);
    let (start, target) = (tile_at(0, 4, HANDLE_Y), tile_at(5, 4, CONTENT_Y));
    laid.dispatch_pointer_down(start.0, start.1);
    laid.dispatch_pointer_move(start.0 + 14.0, start.1 + 14.0);
    laid.dispatch_pointer_move(target.0, target.1);
    laid.pump();
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(125));
    let mid = four(&laid);
    settle(&mut laid);
    let to = four(&laid);
    assert_eq!(
        to,
        from + Offset::new(3.0 * TILE, -TILE),
        "to the end of the row above"
    );
    assert!(
        mid.dx > from.dx && mid.dx < to.dx && mid.dy < from.dy && mid.dy > to.dy,
        "on its way: {mid:?} from {from:?} to {to:?}"
    );
    laid.dispatch_pointer_up(target.0, target.1);
    settle(&mut laid);
    assert_eq!(board.moves(), [(0, 5, ReorderCause::Drag)]);
}
