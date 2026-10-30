//! [`SwipeActions`] — a row whose actions a horizontal swipe reveals at its
//! leading and trailing edges.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use flui_animation::MotionPolicy;
use flui_foundation::geometry::Axis;
use flui_objects::{CrossAxisAlignment, StackFit};
use flui_painting::typography::TextDirection;
use flui_view::prelude::*;
use flui_view::{BoxedView, WriterSource};

use super::row::{
    ContactOrigin, OutWatch, RowGeometry, Stop, UncoverWatch, Uncovered, edge_strip,
    nothing_behind, return_to,
};
use crate::context_menu::ContextMenuArea;
use crate::interaction::{
    Alternative, CommandHandle, ContactAdmission, ControlPlacement, ControlSpec,
    GestureAlternatives, GestureAlternativesError, GestureCommand, MenuPresenter, MenuSpec,
};
use crate::localization::Directionality;
use crate::scroll::{AnchoredDrag, AnchoredDragController, DraggableAnchors};
use crate::{
    Center, ExcludeSemantics, Listener, MediaQuery, RawButton, Row, Semantics, SizedBox, Stack,
};

/// How far each revealed action reaches into the row, in logical pixels:
/// past the touch minimum target, so each is hit reliably at rest.
const ACTION_EXTENT: f64 = 80.0;

/// One action a [`SwipeActions`] row reveals: the command it performs and
/// what its button shows. The command's label names the button for
/// assistive technology, and is the row's custom action and menu item.
#[derive(Clone)]
pub struct SwipeAction {
    command: GestureCommand,
    content: BoxedView,
}

impl SwipeAction {
    /// An action performing `command`, its button showing `content`.
    #[must_use]
    pub fn new(command: GestureCommand, content: impl IntoView) -> Self {
        Self {
            command,
            content: content.into_view().boxed(),
        }
    }
}

impl fmt::Debug for SwipeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SwipeAction")
            .field(&self.command.label())
            .finish()
    }
}

/// An edge of a [`SwipeActions`] row, in reading order: mirrored in a
/// right-to-left locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SwipeEdge {
    /// The reading start, revealed by a swipe toward the reading end: the
    /// left edge under left-to-right text.
    Leading,
    /// The reading end, revealed by a swipe toward the reading start.
    Trailing,
}

/// How a [`SwipeActions`] row's actions are reached with one pointer and no
/// swipe (WCAG 2.2, 2.5.1): the [`Alternative`] it mounts.
#[derive(Clone)]
#[non_exhaustive]
pub enum SwipeAlternative {
    /// Every action as a control always shown beside the row, at
    /// `ControlPlacement`.
    VisibleControls(ControlPlacement),
    /// The theme-free menu of [`ContextMenuArea`], one item per action.
    ContextMenu,
    /// A context menu, one item per action, shown by the presenter: a design
    /// package's menu.
    PresentedMenu(MenuPresenter),
}

impl fmt::Debug for SwipeAlternative {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VisibleControls(placement) => {
                f.debug_tuple("VisibleControls").field(placement).finish()
            }
            Self::ContextMenu => f.write_str("ContextMenu"),
            Self::PresentedMenu(_) => f.write_str("PresentedMenu"),
        }
    }
}

/// Why a [`SwipeActions`] row could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SwipeActionsError {
    /// Neither edge has an action: there is nothing to reveal.
    #[error("a swipe actions row needs at least one action")]
    NoActions,
    /// The actions' commands cannot be told apart.
    #[error(transparent)]
    Alternatives(#[from] GestureAlternativesError),
}

/// Where the row's alternatives come from.
#[derive(Clone)]
enum Host {
    /// Alternatives the row mounts itself.
    Alternatives(GestureAlternatives),
    /// The theme-free context menu, which mounts the alternatives.
    Area(ContextMenuArea),
}

/// A row whose actions a horizontal swipe reveals: leading actions at the
/// reading start, trailing ones at the reading end.
///
/// ```rust,ignore
/// SwipeActions::try_new(
///     message_row,
///     [SwipeAction::new(GestureCommand::new("Pin Alice", Pin("Alice")), pin_icon)],
///     [
///         SwipeAction::new(GestureCommand::new("Delete Alice", Delete("Alice")), bin_icon),
///         SwipeAction::new(GestureCommand::new("Archive Alice", Archive("Alice")), box_icon),
///     ],
///     SwipeAlternative::ContextMenu,
/// )?
/// .full_swipe(SwipeEdge::Trailing)
/// .target("Alice")
/// ```
///
/// A swipe toward the reading start reveals the trailing actions, one toward
/// the reading end the leading ones; a release past halfway, or a fling,
/// leaves them shown, and a release short of it, a fling back or a cancelled
/// contact returns the row to where the contact found it. Each edge lists
/// its actions from the row's edge inward. Tapping a revealed action closes
/// the row and performs its command. With [`full_swipe`](Self::full_swipe), a
/// release far past the actions performs the edge's first action and the row
/// closes.
///
/// The actions are reached without the swipe too (ADR-0216): each is the
/// row's custom action for assistive technology and has its command's keys
/// while focus is in the row, which is a stop of focus traversal, and the
/// [`SwipeAlternative`] reaches them with one pointer. The commands resolve
/// through the [`Actions`](crate::Actions) chain where the row is mounted,
/// so every path has one outcome.
///
/// The swipe is an axial member of the arena: in a vertical list a
/// horizontal swipe moves the row and a vertical one scrolls the list. A
/// touch that goes down in the window's system-gesture strip at either side
/// belongs to the operating system's edge swipe and a navigator's swipe
/// back, never to the row. A [`VsyncScope`](crate::VsyncScope) above it runs
/// the settle.
#[derive(Clone, StatefulView)]
pub struct SwipeActions {
    child: BoxedView,
    leading: Rc<[SwipeAction]>,
    trailing: Rc<[SwipeAction]>,
    full_leading: bool,
    full_trailing: bool,
    host: Host,
}

impl SwipeActions {
    /// A row showing `child`, revealing `leading` and `trailing`, each from
    /// the row's edge inward, and reaching them with one pointer through
    /// `alternative`.
    ///
    /// The commands are ordered trailing first, then leading: the first is
    /// the alternatives' gesture command.
    ///
    /// # Errors
    ///
    /// [`SwipeActionsError::NoActions`] for no action on either edge;
    /// [`SwipeActionsError::Alternatives`] for an empty or repeated label.
    pub fn try_new(
        child: impl IntoView,
        leading: impl IntoIterator<Item = SwipeAction>,
        trailing: impl IntoIterator<Item = SwipeAction>,
        alternative: SwipeAlternative,
    ) -> Result<Self, SwipeActionsError> {
        let leading: Rc<[SwipeAction]> = leading.into_iter().collect();
        let trailing: Rc<[SwipeAction]> = trailing.into_iter().collect();
        let mut actions = trailing.iter().chain(leading.iter());
        let first = actions.next().ok_or(SwipeActionsError::NoActions)?;
        let more: Vec<&SwipeAction> = actions.collect();
        let host = match alternative {
            SwipeAlternative::VisibleControls(placement) => {
                let spec = more.iter().fold(
                    ControlSpec::new(first.command.clone(), first.content.clone()),
                    |spec, action| spec.control(action.command.clone(), action.content.clone()),
                );
                Host::Alternatives(GestureAlternatives::try_new(Alternative::VisibleControl(
                    spec.placement(placement),
                ))?)
            }
            SwipeAlternative::PresentedMenu(presenter) => {
                let menu = more.iter().fold(
                    MenuSpec::new(first.command.clone(), move |cx, request| {
                        presenter(cx, request);
                    }),
                    |spec, action| spec.item(action.command.clone()),
                );
                Host::Alternatives(GestureAlternatives::try_new(Alternative::ContextMenu(
                    menu,
                ))?)
            }
            SwipeAlternative::ContextMenu => Host::Area(ContextMenuArea::new(
                first.command.clone(),
                more.iter().map(|action| action.command.clone()),
            )?),
        };
        Ok(Self {
            child: child.into_view().boxed(),
            leading,
            trailing,
            full_leading: false,
            full_trailing: false,
            host,
        })
    }

    /// Let a release far past `edge`'s actions perform the edge's first
    /// action, as tapping it does. An edge without actions has no full swipe.
    #[must_use]
    pub fn full_swipe(mut self, edge: SwipeEdge) -> Self {
        match edge {
            SwipeEdge::Leading => self.full_leading = true,
            SwipeEdge::Trailing => self.full_trailing = true,
        }
        self
    }

    /// Name what the actions act on ("Alice"), for the labels the row makes
    /// itself, such as the menu control's.
    #[must_use]
    pub fn target(mut self, name: impl Into<String>) -> Self {
        self.host = match self.host {
            Host::Alternatives(alternatives) => Host::Alternatives(alternatives.target(name)),
            Host::Area(area) => Host::Area(area.target(name)),
        };
        self
    }

    /// The alternatives the row's actions have: their commands, in order,
    /// and the single-pointer alternative. For
    /// [`SwipeAlternative::ContextMenu`] the menu's presenter is the
    /// [`ContextMenuArea`] the row mounts.
    #[must_use]
    pub fn alternatives(&self) -> &GestureAlternatives {
        match &self.host {
            Host::Alternatives(alternatives) => alternatives,
            Host::Area(area) => area.alternatives(),
        }
    }

    fn anchors(&self) -> RowAnchors {
        RowAnchors {
            leading: reach(&self.leading),
            trailing: reach(&self.trailing),
            full_leading: self.full_leading,
            full_trailing: self.full_trailing,
        }
    }
}

impl fmt::Debug for SwipeActions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwipeActions")
            .field("leading", &self.leading)
            .field("trailing", &self.trailing)
            .field("full_leading", &self.full_leading)
            .field("full_trailing", &self.full_trailing)
            .finish_non_exhaustive()
    }
}

/// The stops a row's actions give it, before its extent is known.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowAnchors {
    /// How far the leading actions reach; zero without any.
    leading: f64,
    /// How far the trailing actions reach; zero without any.
    trailing: f64,
    full_leading: bool,
    full_trailing: bool,
}

impl RowAnchors {
    /// The stops of a row `extent` long: a full swipe's stop lies at the
    /// row's extent, beyond its actions by at least half an action.
    fn build(self, extent: Option<f64>) -> DraggableAnchors<Stop> {
        let mut anchors = DraggableAnchors::builder().at(Stop::Closed, 0.0);
        if self.leading > 0.0 {
            anchors = anchors.at(Stop::Leading, self.leading);
        }
        if self.trailing > 0.0 {
            anchors = anchors.at(Stop::Trailing, -self.trailing);
        }
        let out = extent.map(|extent| extent.min(DraggableAnchors::<Stop>::MAX_OFFSET));
        let beyond =
            |reach: f64| out.filter(|&out| reach > 0.0 && out >= reach + ACTION_EXTENT / 2.0);
        if self.full_leading
            && let Some(out) = beyond(self.leading)
        {
            anchors = anchors.at(Stop::LeadingOut, out);
        }
        if self.full_trailing
            && let Some(out) = beyond(self.trailing)
        {
            anchors = anchors.at(Stop::TrailingOut, -out);
        }
        anchors
            .build()
            .expect("BUG: a row's stops are finite, in range and apart")
    }
}

/// What a row's full swipes perform: the first action of each edge, as the
/// latest build resolved it.
#[derive(Default)]
struct FullSwipes {
    leading: Option<CommandHandle>,
    trailing: Option<CommandHandle>,
}

/// The state of a [`SwipeActions`] row: where it rests, its committed
/// extent and what its full swipes perform.
pub struct SwipeActionsState {
    controller: AnchoredDragController<Stop>,
    geometry: Rc<RowGeometry>,
    origin: Rc<ContactOrigin>,
    /// The stops last given to the controller, and the extent they took.
    applied: Rc<Cell<(RowAnchors, Option<f64>)>>,
    full: Rc<RefCell<FullSwipes>>,
    motion: Rc<Cell<Option<MotionPolicy>>>,
    watch: Option<OutWatch>,
    uncover: Option<UncoverWatch>,
}

impl fmt::Debug for SwipeActionsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwipeActionsState")
            .field("current", &self.controller.current())
            .field("offset", &self.controller.offset())
            .finish_non_exhaustive()
    }
}

impl StatefulView for SwipeActions {
    type State = SwipeActionsState;

    fn create_state(&self) -> Self::State {
        let anchors = self.anchors();
        SwipeActionsState {
            controller: AnchoredDragController::try_new(anchors.build(None), Stop::Closed)
                .expect("BUG: every row has a closed stop"),
            geometry: Rc::default(),
            origin: Rc::default(),
            applied: Rc::new(Cell::new((anchors, None))),
            full: Rc::default(),
            motion: Rc::default(),
            watch: None,
            uncover: None,
        }
    }
}

impl SwipeActionsState {
    /// Give the controller the stops of `anchors` at the row's committed
    /// extent, when either changed.
    fn reanchor(
        controller: &AnchoredDragController<Stop>,
        geometry: &RowGeometry,
        applied: &Cell<(RowAnchors, Option<f64>)>,
        anchors: Option<RowAnchors>,
    ) {
        let (last, last_extent) = applied.get();
        let anchors = anchors.unwrap_or(last);
        let extent = geometry.extent(Axis::Horizontal).or(last_extent);
        if (anchors, extent) != (last, last_extent) {
            applied.set((anchors, extent));
            controller.set_anchors(anchors.build(extent));
        }
    }

    fn full_swipe_performer(&self, writer: WriterSource) -> Rc<dyn Fn(Stop)> {
        let (controller, full, motion) = (
            self.controller.clone(),
            Rc::clone(&self.full),
            Rc::clone(&self.motion),
        );
        Rc::new(move |stop| {
            let handle = {
                let full = full.borrow();
                match stop {
                    Stop::LeadingOut => full.leading.clone(),
                    Stop::TrailingOut => full.trailing.clone(),
                    Stop::Closed | Stop::Leading | Stop::Trailing => None,
                }
            };
            // The row closes before the command runs: a command that panics,
            // or that removes the row, leaves no row standing open.
            return_to(&controller, Stop::Closed, motion.get());
            if let Some(handle) = handle {
                writer.write(|cx| {
                    let _ = handle.perform(cx);
                });
            }
        })
    }
}

impl ViewState<SwipeActions> for SwipeActionsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.geometry.acquire(ctx);
        let act = self.full_swipe_performer(ctx.writer_source());
        self.watch = Some(OutWatch::new(
            &self.controller,
            ctx.post_frame_handle(),
            act,
        ));
        self.uncover = Some(UncoverWatch::new(&self.controller, ctx.rebuild_handle()));
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.geometry.acquire(ctx);
    }

    fn did_update_view(&mut self, _old_view: &SwipeActions, new_view: &SwipeActions) {
        Self::reanchor(
            &self.controller,
            &self.geometry,
            &self.applied,
            Some(new_view.anchors()),
        );
    }

    fn dispose(&mut self) {
        if let Some(watch) = self.watch.take() {
            watch.retire();
        }
        if let Some(uncover) = self.uncover.take() {
            uncover.retire();
        }
        self.geometry.release();
    }

    fn build(&self, view: &SwipeActions, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        self.motion.set(MediaQuery::motion_of(ctx));
        let first =
            |actions: &[SwipeAction]| actions.first().map(|action| action.command.handle(ctx));
        let outgoing = self.full.replace(FullSwipes {
            leading: first(&view.leading),
            trailing: first(&view.trailing),
        });
        drop(outgoing);

        let controller = &self.controller;
        let (cancel, origin_cancel, motion) = (
            controller.clone(),
            Rc::clone(&self.origin),
            Rc::clone(&self.motion),
        );
        let drag = AnchoredDrag::inside(controller.clone(), Axis::Horizontal, view.child.clone())
            .admission(ContactAdmission::outside_system_edges(
                ctx,
                Axis::Horizontal,
            ))
            .on_cancelled(move || origin_cancel.cancelled(&cancel, motion.get()));
        let (down, origin_down) = (controller.clone(), Rc::clone(&self.origin));
        let (geometry, applied) = (Rc::clone(&self.geometry), Rc::clone(&self.applied));
        // Every Down reaches the row before its drag captures the contact.
        let row = Listener::new()
            .on_pointer_down(move |_cx, _dispatch| {
                Self::reanchor(&down, &geometry, &applied, None);
                origin_down.down(down.current());
            })
            .child(drag);

        // The uncovered edge's actions sit under the content, mounted while
        // it is uncovered: the content's slide reveals them without a
        // rebuild, and they are hit where the content moved away.
        let edge = Uncovered::at(controller.offset());
        let actions = match edge {
            Some(Uncovered::Leading) => &view.leading,
            Some(Uncovered::Trailing) => &view.trailing,
            None => &view.trailing[..0],
        };
        // The content keeps its place in the stack either way, so a contact
        // on it lives through the actions coming and going.
        let tray = match edge {
            Some(edge) if !actions.is_empty() => {
                let buttons = tray_buttons(actions, edge, ctx, controller, &self.motion);
                edge_strip(
                    Axis::Horizontal,
                    direction,
                    edge,
                    Some(reach(actions)),
                    Row::new(buttons)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .boxed(),
                )
            }
            _ => nothing_behind(),
        };
        let row = self
            .geometry
            .measure(Stack::new(vec![tray, row.boxed()]).fit(StackFit::Passthrough));
        match &view.host {
            Host::Alternatives(alternatives) => alternatives.clone().child(row).boxed(),
            Host::Area(area) => area.clone().child(row).boxed(),
        }
    }
}

/// How far `actions` reach into the row.
fn reach(actions: &[SwipeAction]) -> f64 {
    actions.iter().map(|_| ACTION_EXTENT).sum()
}

/// `actions`' buttons in reading order, each [`ACTION_EXTENT`] wide: the
/// leading edge's from the row's edge inward, the trailing edge's from
/// inside out to the row's edge.
fn tray_buttons(
    actions: &[SwipeAction],
    edge: Uncovered,
    ctx: &dyn BuildContext,
    controller: &AnchoredDragController<Stop>,
    motion: &Rc<Cell<Option<MotionPolicy>>>,
) -> Vec<BoxedView> {
    let button = |action: &SwipeAction| {
        let handle = action.command.handle(ctx);
        let (controller, motion) = (controller.clone(), Rc::clone(motion));
        let pressed = RawButton::new(
            Semantics::new()
                .label(action.command.label().to_owned())
                .child(ExcludeSemantics::new().child(Center::new().child(action.content.clone()))),
        )
        .on_press(move |cx| {
            // Closed first: a command that panics or removes the row leaves
            // no row standing open.
            return_to(&controller, Stop::Closed, motion.get());
            let _ = handle.perform(cx);
        });
        SizedBox::width(ACTION_EXTENT).child(pressed).boxed()
    };
    match edge {
        Uncovered::Leading => actions.iter().map(button).collect(),
        Uncovered::Trailing => actions.iter().rev().map(button).collect(),
    }
}
