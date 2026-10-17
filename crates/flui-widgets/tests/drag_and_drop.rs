//! `Draggable` and `DropTarget` on the drag-and-drop sessions (ADR-0211):
//! a drag within the window, its escalation to the operating system, Escape,
//! the cut and paste that reach the same drop from the keyboard and assistive
//! technology, a target or a source leaving mid-drag, a panicking drop, the
//! paste point in a right-to-left locale, the cursor in each state and
//! spring-loading.
//!
//! One fixture, a desk, holds them: two cards to drag at the top, then a
//! target that takes a move, one that takes a copy and one that declines.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use flui_interaction::drag::{
    AllowedActions, DragItem, DragOutcome, DragPoint, DragTrigger, SpringLoading,
};
use flui_interaction::events::{
    PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_painting::typography::TextDirection;
use flui_platform_api::data_transfer::{TransferActions, TransferFormat};
use flui_platform_api::drag::OsDragEnd;
use flui_platform_api::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform_api::{CursorIcon, EventTime, HapticFeedback, TargetPlatform};
use flui_testing::a11y::{Action, ActionData, ActionRequest, Role, TreeId};
use flui_view::StateHandle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{
    Alternative, ControlSpec, CopySelectionTextIntent, Directionality, Draggable, DropHover,
    DropTarget, GestureAlternatives, GestureCommand, InsertPosition, Overlay, OverlayEntry,
    OverlayHandle, PasteTextIntent, Positioned, SingleActivator, SizedBox, Stack, StackFit, Text,
};

use crate::common::{LaidOut, lay_out, tight};

/// What the desk's commands and drops change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Board {
    todo: Vec<&'static str>,
    done: Vec<&'static str>,
    copies: Vec<&'static str>,
    /// Whether the move target is mounted.
    done_shown: bool,
}

/// What a desk's callbacks recorded, and how a test configures it.
struct Shared {
    board: StateHandle<Board>,
    log: RefCell<Vec<String>>,
    hover: RefCell<BTreeMap<&'static str, DropHover>>,
    drops: RefCell<Vec<DragPoint>>,
    springs: Cell<u32>,
    allowed: Cell<AllowedActions>,
    trigger: Cell<DragTrigger>,
    direction: Cell<TextDirection>,
    panic_next_drop: Cell<bool>,
    spring: Cell<Option<SpringLoading>>,
}

impl Shared {
    fn note(&self, what: impl Into<String>) {
        self.log.borrow_mut().push(what.into());
    }

    fn hover(&self, target: &'static str) -> DropHover {
        self.hover.borrow().get(target).copied().unwrap_or_default()
    }

    fn board(&self) -> Board {
        self.board.with(Clone::clone)
    }
}

const ROW: f64 = 60.0;
const DONE: (f64, f64) = (140.0, 80.0);
const COPIES: (f64, f64) = (230.0, 80.0);
const LOCKED: (f64, f64) = (320.0, 80.0);

/// A point inside the target at `(top, height)`.
fn inside(target: (f64, f64)) -> (f64, f64) {
    (100.0, target.0 + target.1 / 2.0)
}

/// The desk: an overlay for the drag's feedback, holding the cards and the
/// targets.
#[derive(Clone)]
struct Desk {
    shared: Rc<Shared>,
}

impl View for Desk {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

struct DeskState {
    handle: OverlayHandle,
    _entry: OverlayEntry,
}

impl StatefulView for Desk {
    type State = DeskState;

    fn create_state(&self) -> DeskState {
        let handle = OverlayHandle::new();
        let shared = Rc::clone(&self.shared);
        let entry = OverlayEntry::new(move |_ctx| {
            DeskContent {
                shared: Rc::clone(&shared),
            }
            .into_view()
            .boxed()
        });
        handle.insert(&entry, &InsertPosition::Top);
        DeskState {
            handle,
            _entry: entry,
        }
    }
}

impl ViewState<Desk> for DeskState {
    fn build(&self, _view: &Desk, _ctx: &dyn BuildContext) -> impl IntoView {
        Overlay::new(self.handle.clone())
    }
}

#[derive(Clone)]
struct DeskContent {
    shared: Rc<Shared>,
}

impl View for DeskContent {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

struct DeskContentState {
    board: StateHandle<Board>,
}

impl StatefulView for DeskContent {
    type State = DeskContentState;

    fn create_state(&self) -> DeskContentState {
        DeskContentState {
            board: self.shared.board.clone(),
        }
    }
}

impl ViewState<DeskContent> for DeskContentState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.board.bind(ctx);
    }

    fn build(&self, view: &DeskContent, _ctx: &dyn BuildContext) -> impl IntoView {
        let shared = &view.shared;
        let board = self.board.with(Clone::clone);
        let mut children: Vec<BoxedView> = board
            .todo
            .iter()
            .enumerate()
            .map(|(index, name)| {
                #[expect(clippy::cast_precision_loss, reason = "two cards")]
                let top = index as f64 * ROW;
                Positioned::new(card(shared, name))
                    .left(0.0)
                    .top(top)
                    .width(400.0)
                    .height(ROW)
                    .into_view()
                    .boxed()
            })
            .collect();
        if board.done_shown {
            children.push(target_at(shared, "Done", DONE, TransferActions::MOVE));
        }
        children.push(target_at(shared, "Copies", COPIES, TransferActions::COPY));
        children.push(target_at(
            shared,
            "Locked",
            LOCKED,
            TransferActions::empty(),
        ));
        Directionality::new(
            shared.direction.get(),
            Stack::new(children).fit(StackFit::Expand),
        )
    }
}

/// A card: its cut and copy as visible controls beside it.
fn card(shared: &Rc<Shared>, name: &'static str) -> BoxedView {
    let cut = GestureCommand::new(format!("Cut {name}"), CopySelectionTextIntent::Cut);
    let copy = GestureCommand::new(format!("Copy {name}"), CopySelectionTextIntent::Copy);
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
        ControlSpec::new(cut, Text::new("x")).control(copy, Text::new("c")),
    ))
    .expect("each card's labels name it")
    .target(name);
    let (ended, started) = (Rc::clone(shared), Rc::clone(shared));
    Draggable::new(
        alternatives,
        move || vec![DragItem::new().with_value(name).with_text(name)],
        SizedBox::new(200.0, ROW).child(Text::new(name)),
    )
    .allowed_actions(shared.allowed.get())
    .trigger(shared.trigger.get())
    .feedback(|| Text::new("feedback").into_view().boxed())
    .child_when_dragging(move || Text::new(format!("{name} lifted")).into_view().boxed())
    .on_drag_started(move |_cx| started.note(format!("start {name}")))
    .on_drag_end(move |_cx, outcome: DragOutcome| {
        ended.note(format!("end {name} {outcome:?}"));
        if outcome
            == (DragOutcome::Dropped {
                action: TransferActions::MOVE,
            })
        {
            ended
                .board
                .update(|board| board.todo.retain(|card| *card != name));
        }
    })
    .into_view()
    .boxed()
}

/// A target at `(top, height)` taking a drop of a card with `takes`, or
/// declining with no actions.
fn target_at(
    shared: &Rc<Shared>,
    name: &'static str,
    (top, height): (f64, f64),
    takes: TransferActions,
) -> BoxedView {
    let paste = GestureCommand::new(format!("Paste into {name}"), PasteTextIntent);
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        paste,
        Text::new("v"),
    )))
    .expect("the target's label names it")
    .target(name);
    let (shown, dropped, sprung) = (Rc::clone(shared), Rc::clone(shared), Rc::clone(shared));
    let mut target = DropTarget::new(
        alternatives,
        move |drag| {
            drag.value::<&'static str>()
                .and(Some(takes))
                .filter(|takes| !takes.is_empty())
        },
        move |hover| {
            shown.hover.borrow_mut().insert(name, hover);
            SizedBox::new(200.0, height)
                .child(Text::new(name))
                .into_view()
                .boxed()
        },
    )
    .on_drop(move |_cx, drag, at, action| {
        assert!(
            !dropped.panic_next_drop.replace(false),
            "intentional drop panic"
        );
        let card = *drag
            .value::<&'static str>()
            .expect("a card's drag carries its name");
        dropped.note(format!("drop {card} into {name} {}", action.bits()));
        dropped.drops.borrow_mut().push(at);
        dropped.board.update(|board| match name {
            "Done" => board.done.push(card),
            _ => board.copies.push(card),
        });
    });
    if name == "Done"
        && let Some(policy) = shared.spring.get()
    {
        target = target.spring_loaded(policy, move |_cx, _drag| {
            sprung.springs.set(sprung.springs.get() + 1);
        });
    }
    Positioned::new(target)
        .left(0.0)
        .top(top)
        .width(400.0)
        .height(height)
        .into_view()
        .boxed()
}

fn shared() -> Rc<Shared> {
    Rc::new(Shared {
        board: StateHandle::new(initial_board()),
        log: RefCell::new(Vec::new()),
        hover: RefCell::new(BTreeMap::new()),
        drops: RefCell::new(Vec::new()),
        springs: Cell::new(0),
        allowed: Cell::new(AllowedActions::within(
            TransferActions::MOVE | TransferActions::COPY,
        )),
        trigger: Cell::new(DragTrigger::Immediate),
        direction: Cell::new(TextDirection::Ltr),
        panic_next_drop: Cell::new(false),
        spring: Cell::new(None),
    })
}

fn mount(shared: &Rc<Shared>) -> LaidOut {
    let mut laid = lay_out(
        Desk {
            shared: Rc::clone(shared),
        },
        tight(400.0, 400.0),
    );
    laid.pump();
    laid
}

/// Press on Alice's card with the mouse and move to `(x, y)`.
fn lift_alice_to(laid: &LaidOut, (x, y): (f64, f64)) {
    laid.dispatch_pointer_down(100.0, 30.0);
    laid.dispatch_pointer_move(100.0, 45.0);
    laid.dispatch_pointer_move(x, y);
}

// --- Keys and assistive technology ------------------------------------------------------------

/// The platform's command chord for `letter`: Cmd on Apple platforms, Ctrl elsewhere.
fn chord(letter: &str) -> SingleActivator {
    let activator = SingleActivator::character(letter);
    if matches!(
        TargetPlatform::current(),
        TargetPlatform::MacOS | TargetPlatform::iOS
    ) {
        activator.meta()
    } else {
        activator.control()
    }
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

fn escape(laid: &LaidOut) {
    press(laid, &SingleActivator::new(Key::Named(NamedKey::Escape)));
}

/// Tab until the row offering `label` as a custom action has focus.
fn focus_row(laid: &mut LaidOut, label: &str) {
    for _ in 0..32 {
        press(laid, &SingleActivator::named(NamedKey::Tab));
        laid.pump();
        let focused = laid.a11y_tree().and_then(|tree| {
            tree.focus().map(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .any(|action| action.description == label)
            })
        });
        if focused == Some(true) {
            return;
        }
    }
    panic!("no row offering {label:?} took focus");
}

/// Invoke the custom action labelled `label`, as assistive technology does.
fn custom_action(laid: &mut LaidOut, label: &str) {
    let (target, id) = laid
        .a11y_tree()
        .and_then(|tree| {
            tree.nodes().find_map(|node| {
                node.raw()
                    .custom_actions()
                    .iter()
                    .find(|action| action.description == label)
                    .map(|action| (node.id(), action.id))
            })
        })
        .unwrap_or_else(|| panic!("no node offers {label:?}"));
    laid.invoke_semantics_action(ActionRequest {
        action: Action::CustomAction,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::CustomAction(id)),
    })
    .expect("the custom action is delivered");
}

/// Press the visible control labelled `label` with a mouse.
fn press_control(laid: &mut LaidOut, label: &str) {
    let center = laid
        .a11y_tree()
        .and_then(|tree| {
            let node = tree.find_by_role_and_label(Role::Button, label).ok()?;
            let bounds = tree.window_bounds(&node)?;
            Some((
                f64::midpoint(bounds.x0, bounds.x1),
                f64::midpoint(bounds.y0, bounds.y1),
            ))
        })
        .unwrap_or_else(|| panic!("no control {label:?}"));
    laid.dispatch_pointer_down(center.0, center.1);
    laid.dispatch_pointer_up(center.0, center.1);
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..4 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// The board a pointer drag of Alice into Done leaves.
fn moved_alice() -> Board {
    Board {
        todo: vec!["Bob"],
        done: vec!["Alice"],
        copies: Vec::new(),
        done_shown: true,
    }
}

// --- Rows ---------------------------------------------------------------------------------------

/// A drag within the window: the feedback follows the pointer in the overlay,
/// the card shows it is lifted, the drop lands on the target that accepted
/// the last move, before the source hears that it moved, and the lift and
/// the drop play their haptics. Released over the target that declines, the
/// drag drops nothing.
pub(crate) fn a_drag_drops_on_the_target_that_accepted_its_last_move() {
    let shared = shared();
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(LOCKED));
    laid.pump();
    let feedback = laid
        .find_text("feedback")
        .expect("the feedback shows in the overlay");
    let at = laid.absolute_offset(feedback);
    // The card was grabbed 100 px from its left and 30 px from its top.
    assert_eq!((at.dx, at.dy), (0.0, LOCKED.0 + LOCKED.1 / 2.0 - 30.0));
    assert!(
        laid.find_text("Alice lifted").is_some(),
        "the card shows it is lifted"
    );
    assert_eq!(shared.hover("Locked"), DropHover::Declined);
    laid.dispatch_pointer_move(inside(DONE).0, inside(DONE).1);
    laid.pump();
    assert_eq!(
        shared.hover("Locked"),
        DropHover::Idle,
        "the declining target was left"
    );
    assert_eq!(
        shared.hover("Done"),
        DropHover::Accepting(TransferActions::MOVE)
    );
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    settle(&mut laid);
    assert_eq!(shared.board(), moved_alice());
    assert_eq!(
        *shared.log.borrow(),
        [
            "start Alice".to_owned(),
            format!("drop Alice into Done {}", TransferActions::MOVE.bits()),
            "end Alice Dropped { action: TransferActions(MOVE) }".to_owned(),
        ]
    );
    assert!(
        laid.find_text("feedback").is_none(),
        "the feedback left with the drop"
    );
    assert_eq!(shared.hover("Done"), DropHover::Idle);
    assert_eq!(
        laid.window().haptic_actuator().feedback(),
        [HapticFeedback::DragLift, HapticFeedback::Drop]
    );

    // Bob, released over the target that declines: nothing moves.
    laid.dispatch_pointer_down(100.0, 30.0);
    laid.dispatch_pointer_move(100.0, 45.0);
    laid.dispatch_pointer_move(inside(LOCKED).0, inside(LOCKED).1);
    laid.dispatch_pointer_up(inside(LOCKED).0, inside(LOCKED).1);
    settle(&mut laid);
    assert_eq!(shared.board(), moved_alice());
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Bob Cancelled")
    );
}

/// A drag that may reach outside the app becomes the operating system's drag
/// when the pointer leaves the window: the OS is offered the item's text and
/// the drag's identity, with every action the drag allows, the feedback
/// gives way to the OS's image, and the source hears how the OS ended it. A
/// drag that may not reach outside stays in its window and drops nothing
/// there.
pub(crate) fn a_drag_leaving_the_window_escalates_to_the_os_when_it_may_reach_it() {
    let shared = shared();
    shared.allowed.set(AllowedActions::new(
        TransferActions::MOVE,
        TransferActions::COPY,
    ));
    let mut laid = mount(&shared);
    laid.window()
        .os_drags()
        .end_with(OsDragEnd::Dropped(TransferActions::COPY));
    lift_alice_to(&laid, inside(DONE));
    laid.pump();
    assert!(laid.find_text("feedback").is_some());
    laid.dispatch_pointer_move(600.0, 30.0);
    settle(&mut laid);
    let started = laid.window().os_drags().started();
    assert_eq!(started.len(), 1, "one OS session started");
    assert_eq!(
        started[0].allowed,
        TransferActions::MOVE | TransferActions::COPY
    );
    assert_eq!(started[0].representations[0].format, TransferFormat::Text);
    assert_eq!(
        started[0].representations.len(),
        2,
        "the text, then the identity"
    );
    assert!(
        laid.find_text("feedback").is_none(),
        "the OS draws the drag now"
    );
    assert_eq!(
        shared.hover("Done"),
        DropHover::Idle,
        "leaving the window left the target"
    );
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Dropped { action: TransferActions(COPY) }")
    );
    laid.dispatch_pointer_up(600.0, 30.0);
    settle(&mut laid);
    assert_eq!(
        shared.board().todo,
        ["Alice", "Bob"],
        "a copy elsewhere keeps the card"
    );

    let shared = self::shared();
    let mut laid = mount(&shared);
    laid.window()
        .os_drags()
        .end_with(OsDragEnd::Dropped(TransferActions::COPY));
    lift_alice_to(&laid, (600.0, 30.0));
    laid.dispatch_pointer_up(600.0, 30.0);
    settle(&mut laid);
    assert!(
        laid.window().os_drags().started().is_empty(),
        "an in-app drag stays in its window"
    );
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Cancelled")
    );
}

/// Escape cancels a drag in its window before focus sees the key: the target
/// it was over is left, the source hears it was cancelled, and the release
/// that follows drops nothing.
pub(crate) fn escape_cancels_a_drag_in_its_window() {
    let shared = shared();
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    laid.pump();
    assert_eq!(
        shared.hover("Done"),
        DropHover::Accepting(TransferActions::MOVE)
    );
    escape(&laid);
    laid.pump();
    assert_eq!(shared.hover("Done"), DropHover::Idle);
    assert!(laid.find_text("feedback").is_none());
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Cancelled")
    );
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    settle(&mut laid);
    assert_eq!(shared.board().done, Vec::<&str>::new());
}

/// The keyboard reaches the same drop: the cut key on the focused card holds
/// it, the paste key on the focused target drops it there as a move, and
/// the board is the one a pointer drag leaves. A copy pastes as a copy and
/// keeps the card; Escape cancels a held cut.
pub(crate) fn cut_and_paste_by_keyboard_drop_like_a_drag() {
    let shared = shared();
    let mut laid = mount(&shared);
    laid.enable_semantics();
    laid.pump();
    focus_row(&mut laid, "Cut Alice");
    press(&laid, &chord("x"));
    laid.pump();
    assert!(
        laid.find_text("Alice lifted").is_some(),
        "a cut card shows it is lifted"
    );
    focus_row(&mut laid, "Paste into Done");
    press(&laid, &chord("v"));
    settle(&mut laid);
    assert_eq!(shared.board(), moved_alice());
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Dropped { action: TransferActions(MOVE) }")
    );

    focus_row(&mut laid, "Copy Bob");
    press(&laid, &chord("c"));
    focus_row(&mut laid, "Paste into Copies");
    press(&laid, &chord("v"));
    settle(&mut laid);
    assert_eq!(shared.board().copies, ["Bob"]);
    assert_eq!(shared.board().todo, ["Bob"], "a copy keeps the card");

    focus_row(&mut laid, "Cut Bob");
    press(&laid, &chord("x"));
    escape(&laid);
    focus_row(&mut laid, "Paste into Done");
    press(&laid, &chord("v"));
    settle(&mut laid);
    assert_eq!(shared.board().todo, ["Bob"], "Escape cancelled the cut");
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Bob Cancelled")
    );
}

/// A target that leaves the tree while a drag is over it takes no drop: the
/// release over the place it was drops nothing, and the source keeps its
/// card.
pub(crate) fn a_target_removed_mid_drag_takes_no_drop() {
    let shared = shared();
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    laid.pump();
    assert_eq!(
        shared.hover("Done"),
        DropHover::Accepting(TransferActions::MOVE)
    );
    shared.board.update(|board| board.done_shown = false);
    laid.pump();
    laid.dispatch_pointer_move(inside(DONE).0, inside(DONE).1 + 1.0);
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1 + 1.0);
    settle(&mut laid);
    assert!(
        !shared
            .log
            .borrow()
            .iter()
            .any(|line| line.starts_with("drop"))
    );
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Cancelled")
    );
    assert_eq!(shared.board().todo, ["Alice", "Bob"]);
}

/// A source that leaves the tree mid-drag cancels its drag: the target is
/// left, the feedback goes, `on_drag_end` hears it, and the release drops
/// nothing.
pub(crate) fn unmounting_the_source_mid_drag_cancels_it() {
    let shared = shared();
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    laid.pump();
    // Without keys a remaining card would take over the mounted draggable,
    // so every card goes.
    shared.board.update(|board| board.todo.clear());
    laid.pump();
    laid.pump();
    assert_eq!(shared.hover("Done"), DropHover::Idle, "the target was left");
    assert!(
        laid.find_text("feedback").is_none(),
        "the feedback left with the source"
    );
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Cancelled")
    );
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    settle(&mut laid);
    assert_eq!(shared.board().done, Vec::<&str>::new());
}

/// A drop callback that panics does not strand the drag: the panic reaches
/// the dispatch, the source hears the drag cancelled and keeps its card, the
/// feedback and the cursor are released, and the next drag drops.
pub(crate) fn a_panicking_drop_callback_ends_the_drag_cancelled_and_the_next_drops() {
    let shared = shared();
    shared.panic_next_drop.set(true);
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    }));
    assert!(
        released.is_err(),
        "the drop callback's panic reaches the dispatch"
    );
    settle(&mut laid);
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Cancelled")
    );
    assert!(laid.find_text("feedback").is_none());
    assert_ne!(
        laid.cursor(),
        CursorIcon::Grabbing,
        "the drag gave the cursor back"
    );
    assert_eq!(shared.board().todo, ["Alice", "Bob"]);

    lift_alice_to(&laid, inside(DONE));
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    settle(&mut laid);
    assert_eq!(shared.board(), moved_alice());
}

/// A paste drops at the target's reading-start top corner: its top-left, or
/// its top-right in a right-to-left locale.
pub(crate) fn a_paste_drops_at_the_targets_reading_start_corner() {
    for (direction, x) in [(TextDirection::Ltr, 0.0), (TextDirection::Rtl, 400.0)] {
        let shared = shared();
        shared.direction.set(direction);
        let mut laid = mount(&shared);
        laid.enable_semantics();
        laid.pump();
        custom_action(&mut laid, "Cut Alice");
        custom_action(&mut laid, "Paste into Done");
        settle(&mut laid);
        let drops = shared.drops.borrow().clone();
        assert_eq!(drops.len(), 1, "{direction:?}: the paste dropped");
        assert_eq!(drops[0].local.dy, 0.0, "{direction:?}: the top edge");
        assert_eq!(
            drops[0].local.dx > 0.0,
            direction == TextDirection::Rtl,
            "{direction:?}: the start edge"
        );
        assert_eq!(
            (drops[0].global.dx, drops[0].global.dy),
            (x, DONE.0),
            "{direction:?}"
        );
    }
}

/// A mouse drag shows on the cursor what a drop would do: the open hand over
/// a card at rest, the grabbing hand while it moves over nothing or over a
/// target that would move it, the copy badge over one that would copy it, and
/// refusal over one that declines; the release gives the cursor back. A
/// finger's drag leaves the cursor alone.
pub(crate) fn the_cursor_shows_what_a_drop_would_do() {
    let shared = shared();
    let mut laid = mount(&shared);
    laid.dispatch_pointer_hover(100.0, 30.0);
    assert_eq!(laid.cursor(), CursorIcon::Grab);
    laid.dispatch_pointer_down(100.0, 30.0);
    laid.dispatch_pointer_move(100.0, 45.0);
    laid.dispatch_pointer_move(300.0, 120.0);
    assert_eq!(laid.cursor(), CursorIcon::Grabbing, "over nothing");
    laid.dispatch_pointer_move(inside(DONE).0, inside(DONE).1);
    assert_eq!(
        laid.cursor(),
        CursorIcon::Grabbing,
        "over a target that would move it"
    );
    laid.dispatch_pointer_move(inside(COPIES).0, inside(COPIES).1);
    assert_eq!(
        laid.cursor(),
        CursorIcon::Copy,
        "over a target that would copy it"
    );
    laid.dispatch_pointer_move(inside(LOCKED).0, inside(LOCKED).1);
    assert_eq!(
        laid.cursor(),
        CursorIcon::NoDrop,
        "over a target that declines"
    );
    laid.dispatch_pointer_up(inside(LOCKED).0, inside(LOCKED).1);
    settle(&mut laid);
    assert_eq!(
        laid.cursor(),
        CursorIcon::Default,
        "the hovered regions have the cursor back"
    );

    let touch = |id: u64, (x, y): (f64, f64), phase: u8| {
        let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero contact");
        let position = flui_foundation::geometry::Offset::new(x, y);
        let event = match phase {
            0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
            1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
            _ => make_up_event_for_id(pointer, position, PointerKind::Touch),
        }
        .expect("finite contact");
        laid.dispatch_pointer_event(&event);
    };
    touch(40, (100.0, 30.0), 0);
    touch(40, (100.0, 60.0), 1);
    touch(40, inside(COPIES), 1);
    assert_eq!(
        laid.cursor(),
        CursorIcon::Default,
        "a finger has no cursor to change"
    );
    touch(40, inside(COPIES), 2);
    settle(&mut laid);
    assert_eq!(
        shared.board().copies,
        ["Alice"],
        "the finger's drag dropped"
    );
}

/// A target spring-loads once per hover: a drag resting over it past the
/// delay runs it once however long it stays, and again only after it leaves
/// and comes back.
pub(crate) fn spring_loading_fires_once_per_hover() {
    let shared = shared();
    shared.spring.set(Some(SpringLoading {
        delay: Duration::from_millis(300),
    }));
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(shared.springs.get(), 0, "not before the delay");
    for _ in 0..10 {
        laid.pump_for(Duration::from_millis(100));
    }
    assert_eq!(shared.springs.get(), 1, "once for the hover");
    laid.dispatch_pointer_move(inside(LOCKED).0, inside(LOCKED).1);
    laid.dispatch_pointer_move(inside(DONE).0, inside(DONE).1);
    for _ in 0..5 {
        laid.pump_for(Duration::from_millis(100));
    }
    assert_eq!(shared.springs.get(), 2, "once more for the new hover");
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
}

/// A long-press trigger lifts the card only after the hold: a finger that
/// moves at once starts no drag, one that holds past the long-press timeout
/// starts it through the hold's hand-off and drops like any drag.
pub(crate) fn a_long_press_trigger_lifts_after_the_hold() {
    let shared = shared();
    shared.trigger.set(DragTrigger::LongPress);
    let mut laid = mount(&shared);
    let touch = |laid: &LaidOut, id: u64, (x, y): (f64, f64), phase: u8| {
        let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero contact");
        let position = flui_foundation::geometry::Offset::new(x, y);
        let event = match phase {
            0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
            1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
            _ => make_up_event_for_id(pointer, position, PointerKind::Touch),
        }
        .expect("finite contact");
        laid.dispatch_pointer_event(&event);
    };
    touch(&laid, 41, (100.0, 30.0), 0);
    touch(&laid, 41, inside(DONE), 1);
    touch(&laid, 41, inside(DONE), 2);
    settle(&mut laid);
    assert!(
        shared.log.borrow().is_empty(),
        "a quick move starts no drag"
    );

    touch(&laid, 42, (100.0, 30.0), 0);
    laid.pump_for(Duration::from_secs(1));
    assert_eq!(
        *shared.log.borrow(),
        ["start Alice"],
        "the hold lifted the card"
    );
    touch(&laid, 42, (100.0, 60.0), 1);
    touch(&laid, 42, inside(DONE), 1);
    touch(&laid, 42, inside(DONE), 2);
    settle(&mut laid);
    assert_eq!(shared.board(), moved_alice());
}

/// Matrix row: `Draggable` and `DropTarget`. Alice reaches Done by the
/// pointer drag, by assistive technology's Cut and Paste custom actions, by
/// the cut and paste keys, and by the visible Cut and Paste controls, each
/// leaving the board the drag leaves; Escape after a cut moves nothing.
pub(crate) fn draggable_and_drop_target_by_every_path() {
    let reference = {
        let shared = shared();
        let mut laid = mount(&shared);
        lift_alice_to(&laid, inside(DONE));
        laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
        settle(&mut laid);
        shared.board()
    };
    assert_eq!(reference, moved_alice(), "the gesture moves Alice");
    type Path = fn(&mut LaidOut);
    let paths: [(&str, Path); 4] = [
        ("assistive technology", |laid| {
            custom_action(laid, "Cut Alice");
            custom_action(laid, "Paste into Done");
        }),
        ("keys", |laid| {
            focus_row(laid, "Cut Alice");
            press(laid, &chord("x"));
            focus_row(laid, "Paste into Done");
            press(laid, &chord("v"));
        }),
        ("visible controls", |laid| {
            press_control(laid, "Cut Alice");
            press_control(laid, "Paste into Done");
        }),
        ("Escape after a cut", |laid| {
            focus_row(laid, "Cut Alice");
            press(laid, &chord("x"));
            escape(laid);
            custom_action(laid, "Paste into Done");
        }),
    ];
    for (name, path) in paths {
        let shared = shared();
        let mut laid = mount(&shared);
        laid.enable_semantics();
        laid.pump();
        path(&mut laid);
        settle(&mut laid);
        let expected = if name == "Escape after a cut" {
            initial_board()
        } else {
            reference.clone()
        };
        assert_eq!(shared.board(), expected, "by {name}");
    }
}

/// The board before anything moves.
fn initial_board() -> Board {
    Board {
        todo: vec!["Alice", "Bob"],
        done: Vec::new(),
        copies: Vec::new(),
        done_shown: true,
    }
}

// --- Gesture policy ----------------------------------------------------------------------------

/// Alternatives for a draggable whose alternatives are not what the row is
/// about: a context menu nobody opens.
fn quiet_alternatives() -> GestureAlternatives {
    GestureAlternatives::new(Alternative::ContextMenu(flui_widgets::MenuSpec::new(
        GestureCommand::new("Cut", CopySelectionTextIntent::Cut),
        |_cx, _request| {},
    )))
    .expect("one labelled command")
}

fn quiet_draggable(started: Rc<Cell<u32>>, ended: Rc<Cell<u32>>) -> Draggable {
    Draggable::new(
        quiet_alternatives(),
        || vec![DragItem::new().with_value(7_u32)],
        flui_widgets::ColoredBox::new(flui_painting::styling::Color::RED),
    )
    .on_drag_started(move |_| started.set(started.get() + 1))
    .on_drag_end(move |_, _| ended.set(ended.get() + 1))
}

fn touch(laid: &LaidOut, id: u64, y: f64, phase: u8) {
    let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero touch");
    let position = flui_foundation::geometry::Offset::new(50.0, y);
    let event = match phase {
        0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
        1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
        2 => make_up_event_for_id(pointer, position, PointerKind::Touch),
        _ => unreachable!("scripted touch phase"),
    }
    .expect("finite touch fixture");
    laid.dispatch_pointer_event(&event);
}

pub(crate) fn draggable_reads_admission_profiles_and_retires_authored_owners() {
    crate::common::cases::run_cases(
        "mounted draggable policy",
        &[
            (
                "live admission snapshot",
                draggable_retains_down_profile_and_refreshes_next_contact,
            ),
            (
                "authored replacement",
                draggable_cancels_authored_replacement_before_stale_terminal,
            ),
        ],
    );
}

fn draggable_retains_down_profile_and_refreshes_next_contact() {
    use flui_interaction::{GestureSettings, GestureSettingsSource};
    use flui_widgets::GestureDetector;
    let profile = |slop| {
        GestureSettings::default()
            .try_with_touch_slop(slop)
            .expect("finite slop")
    };
    let source = GestureSettingsSource::new(profile(20.0));
    let starts = Rc::new(Cell::new(0));
    let laid = lay_out(
        crate::gesture_settings::SettingsScope::new(
            source.provider(),
            GestureDetector::new()
                .on_tap(|_| {})
                .child(quiet_draggable(Rc::clone(&starts), Rc::new(Cell::new(0)))),
        ),
        tight(400.0, 400.0),
    );
    touch(&laid, 1, 50.0, 0);
    source.replace(profile(100.0));
    touch(&laid, 1, 90.0, 1);
    assert_eq!(
        starts.get(),
        1,
        "active contact retains its admitted short slop"
    );
    touch(&laid, 1, 90.0, 2);
    touch(&laid, 2, 50.0, 0);
    touch(&laid, 2, 90.0, 1);
    assert_eq!(starts.get(), 1, "next contact reads the updated large slop");
    touch(&laid, 2, 170.0, 1);
    assert_eq!(
        starts.get(),
        2,
        "new threshold still permits a deliberate drag"
    );
    touch(&laid, 2, 170.0, 2);
}

fn draggable_cancels_authored_replacement_before_stale_terminal() {
    use flui_interaction::GestureSettings;
    use flui_widgets::GestureDetector;
    let threshold = Rc::new(Cell::new(20.0));
    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (profile, started, ended, remembered) = (
        threshold.clone(),
        starts.clone(),
        ends.clone(),
        signal.clone(),
    );
    let probe =
        crate::common::SignalProbe::new(move |crate::common::ProbeSignals { count, .. }| {
            remembered.set(Some(count));
            crate::gesture_settings::SettingsScope::new(
                GestureSettings::default()
                    .try_with_touch_slop(profile.get())
                    .expect("finite slop"),
                GestureDetector::new()
                    .on_tap(|_| {})
                    .child(quiet_draggable(started.clone(), ended.clone())),
            )
        });
    let mut laid = lay_out(probe.view(), tight(400.0, 400.0));
    touch(&laid, 1, 50.0, 0);
    touch(&laid, 1, 90.0, 1);
    assert_eq!(starts.get(), 1);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("publish equal authored draggable policy");
    laid.pump();
    assert_eq!(ends.get(), 0, "equal policy keeps the accepted drag");
    threshold.set(100.0);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
        .expect("replace authored draggable policy");
    laid.pump();
    assert_eq!(ends.get(), 1, "outgoing drag is cancelled once");
    touch(&laid, 1, 90.0, 2);
    assert_eq!(ends.get(), 1, "stale release cannot end a replacement");
    touch(&laid, 2, 50.0, 0);
    touch(&laid, 2, 90.0, 1);
    assert_eq!(starts.get(), 1, "replacement keeps the authored threshold");
    touch(&laid, 2, 170.0, 1);
    touch(&laid, 2, 170.0, 2);
    assert_eq!((starts.get(), ends.get()), (2, 2));
}

/// Unmounting a target releases its receiver from the owner lane, so what its
/// callbacks capture goes with the target rather than staying until the UI
/// runtime closes.
pub(crate) fn unmounting_a_target_releases_its_receiver() {
    let captured = Rc::new(());
    let witness = Rc::downgrade(&captured);
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        GestureCommand::new("Paste", PasteTextIntent),
        Text::new("v"),
    )))
    .expect("one labelled command");
    let target = DropTarget::new(
        alternatives,
        |_drag| None,
        |_hover| SizedBox::new(40.0, 40.0).into_view().boxed(),
    )
    .on_drop(move |_cx, _drag, _at, _action| {
        std::hint::black_box(&captured);
    });
    let mut laid = lay_out(target, tight(400.0, 400.0));
    assert!(
        witness.upgrade().is_some(),
        "the mounted target holds its callbacks"
    );
    laid.pump_widget(SizedBox::shrink());
    assert!(
        witness.upgrade().is_none(),
        "the unmounted target's receiver, and what its callbacks captured, leave the owner \
         lane with the target"
    );
}

/// A drag reports to the callbacks it started with: a draggable that a
/// rebuild hands another item mid-drag (an unkeyed list losing its first
/// card) keeps the drag of the first item, and the first item's callback
/// hears how it ended.
pub(crate) fn a_drag_reports_to_the_callbacks_it_started_with() {
    let shared = shared();
    let mut laid = mount(&shared);
    lift_alice_to(&laid, inside(DONE));
    laid.pump();
    shared
        .board
        .update(|board| board.todo.retain(|card| *card != "Alice"));
    laid.pump();
    laid.dispatch_pointer_up(inside(DONE).0, inside(DONE).1);
    settle(&mut laid);
    assert_eq!(
        shared.log.borrow().last().map(String::as_str),
        Some("end Alice Dropped { action: TransferActions(MOVE) }")
    );
    assert_eq!(shared.board(), moved_alice(), "Bob stays where he is");
}

/// While a drag rests near a list's edge the list scrolls under it, and the
/// target that scrolls under the resting pointer is entered, and the one that
/// scrolled away left, before the pointer moves again. The scrollable's
/// receiver is found through the row's sliver and viewport, in a fixed-extent
/// and in a lazily built list.
pub(crate) fn autoscroll_finds_the_target_scrolled_under_a_resting_pointer() {
    autoscroll_through(true);
    autoscroll_through(false);
}

fn autoscroll_through(fixed_extent: bool) {
    let hovers: Rc<RefCell<BTreeMap<usize, DropHover>>> = Rc::default();
    let row = {
        let hovers = Rc::clone(&hovers);
        move |index: usize| -> BoxedView {
            let hovers = Rc::clone(&hovers);
            let paste = GestureCommand::new(format!("Paste into {index}"), PasteTextIntent);
            let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
                ControlSpec::new(paste, Text::new("v")),
            ))
            .expect("one labelled command");
            DropTarget::new(
                alternatives,
                |drag| drag.value::<&'static str>().map(|_| TransferActions::MOVE),
                move |hover| {
                    hovers.borrow_mut().insert(index, hover);
                    SizedBox::new(200.0, 50.0).into_view().boxed()
                },
            )
            .into_view()
            .boxed()
        }
    };
    let row = Rc::new(row);
    let list = flui_widgets::Scrollable::new().viewport_builder(Rc::new(move |position| {
        let list = if fixed_extent {
            flui_widgets::ListView::new(50.0, (0..10).map(|index| row(index)).collect::<Vec<_>>())
        } else {
            let row = Rc::clone(&row);
            flui_widgets::ListView::builder(10, 50.0, move |index| (index < 10).then(|| row(index)))
        };
        list.position(position).into_view().boxed()
    }));
    let card = Draggable::new(
        quiet_alternatives(),
        || vec![DragItem::new().with_value("Alice")],
        SizedBox::new(200.0, ROW).child(Text::new("Alice")),
    );
    let mut laid = lay_out(
        Stack::new(vec![
            Positioned::new(card)
                .left(0.0)
                .top(0.0)
                .width(400.0)
                .height(ROW)
                .into_view()
                .boxed(),
            Positioned::new(list)
                .left(0.0)
                .top(ROW)
                .width(400.0)
                .height(200.0)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand),
        tight(400.0, 400.0),
    );
    laid.pump();
    // In the band along the list's bottom edge, over the fourth row.
    let resting = (100.0, ROW + 196.0);
    lift_alice_to(&laid, resting);
    laid.pump();
    assert_eq!(
        hovers.borrow().get(&3),
        Some(&DropHover::Accepting(TransferActions::MOVE))
    );
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
    }
    let entered: Vec<usize> = hovers
        .borrow()
        .iter()
        .filter(|(_, hover)| matches!(hover, DropHover::Accepting(_)))
        .map(|(index, _)| *index)
        .collect();
    assert_eq!(
        entered.len(),
        1,
        "one row is under the pointer: {entered:?}"
    );
    assert!(
        entered[0] > 3,
        "a later row scrolled under the resting pointer: {entered:?}"
    );
    laid.dispatch_pointer_up(resting.0, resting.1);
}
