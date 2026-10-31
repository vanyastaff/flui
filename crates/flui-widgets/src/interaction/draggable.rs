//! [`Draggable`] — the source of a drag-and-drop session (ADR-0211).
//!
//! A press on the child starts a session of the presentation's
//! drag-and-drop ([`LifecycleContext::drag_and_drop`]) when the arena gives
//! the contact to the draggable: at once for [`DragTrigger::Immediate`], or
//! after a long press for [`DragTrigger::LongPress`], whose hold hands its won
//! contact to the drag through a `Sequenced` composition (ADR-0208). The
//! session finds [`DropTarget`](crate::DropTarget)s by hit test as the pointer
//! moves, escalates to the operating system's drag when the pointer leaves the
//! window and the drag may reach beyond it, shows on a mouse's cursor what a
//! drop would do, and is cancelled by Escape.
//!
//! The drag's keyboard and assistive alternative is the clipboard's: the
//! platform's cut and copy keys ([`CopySelectionTextIntent`]) with the row
//! focused, or the cut and copy of its [`DragCommands`], which perform that intent,
//! hold the items for a paste into a drop target, which ends the session as a
//! drop would. Every path reports through [`on_drag_end`](Draggable::on_drag_end).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::drag::{
    AllowedActions, DragAndDrop, DragItem, DragObserver, DragOutcome, DragPoint, DragReach,
    DragSession, DragTrigger, MoveOutcome,
};
use flui_interaction::gesture::SequencedBranch;
use flui_interaction::{
    GestureRecognizer, HapticsHandle, HitTestHandle, LongPressGestureRecognizer, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragUpdateDetails,
    PointerEvent, PointerId, PointerKindSet,
};
use flui_platform::data_transfer::TransferActions;
use flui_platform::haptics::{HapticRequest, HapticTiming};
use flui_platform::pointer::PointerInfo;
use flui_platform::{CursorIcon, HapticFeedback};
use flui_view::element::ElementKind;
use flui_view::owner::{KeepAliveHandle, KeepAliveLease};
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome, RebuildHandle, WriterSource};

use super::actions::{Action, ActionOutcome, Actions, CopySelectionTextIntent};
use super::gesture_alternatives::{
    AlternativeKind, CommandAppearance, GestureAlternatives, GestureAlternativesError,
    MenuHoldScope,
};
use super::gesture_arena_scope::GesturePolicy;
use super::recognizer_attachment::RecognizerAttachment;
use crate::overlay::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::support::{EventCallback, ValueCallback, event_callback, value_callback};
use crate::{IgnorePointer, Listener, MouseRegion, Positioned, Stack, StackFit};

/// Where a drag's feedback sits relative to the pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DragAnchor {
    /// Where the child was grabbed: the feedback keeps the offset between the
    /// press and the child's corner, so it lifts from where it was.
    #[default]
    Child,
    /// At the pointer: the pointer holds the feedback's top-left corner.
    Pointer,
}

/// Builds a view on demand: the feedback, the child while dragged.
type ViewFn = Rc<dyn Fn() -> BoxedView>;

/// The drag's commands without a pointer path: a cut, and a copy when the
/// drag allows copying, and how one pointer reaches them.
///
/// The widget pairs each with its intent, [`CopySelectionTextIntent::Cut`]
/// and [`CopySelectionTextIntent::Copy`], so every alternative performs the
/// transfer the drag would.
#[derive(Clone, Debug)]
pub struct DragCommands {
    cut: CommandAppearance,
    copy: Option<CommandAppearance>,
    alternative: AlternativeKind,
}

impl DragCommands {
    /// A cut shown as `cut`, reached by a control beside the content.
    #[must_use]
    pub fn new(cut: CommandAppearance) -> Self {
        Self {
            cut,
            copy: None,
            alternative: AlternativeKind::default(),
        }
    }

    /// A copy too, shown as `copy`.
    #[must_use]
    pub fn copy(mut self, copy: CommandAppearance) -> Self {
        self.copy = Some(copy);
        self
    }

    /// Reach the commands with one pointer as `alternative` says; a control
    /// after the content by default.
    #[must_use]
    pub fn alternative(mut self, alternative: AlternativeKind) -> Self {
        self.alternative = alternative;
        self
    }

    fn alternatives(self) -> Result<GestureAlternatives, GestureAlternativesError> {
        GestureAlternatives::of_kind(
            self.alternative,
            self.cut.performing(CopySelectionTextIntent::Cut),
            self.copy
                .map(|copy| copy.performing(CopySelectionTextIntent::Copy)),
        )
    }
}

/// A widget that can be picked up and dropped on a
/// [`DropTarget`](crate::DropTarget), carrying the [`DragItem`]s its `items`
/// function makes for each drag.
///
/// Its [`DragCommands`] are the drag's keyboard, assistive and single-pointer
/// paths: a cut, and a copy, which this widget answers by holding the items
/// for a paste into a drop target. Their row is focusable, and the platform's
/// cut and copy keys reach the same intent while it is focused.
#[derive(Clone, StatefulView)]
pub struct Draggable {
    /// `None` only for the crate's own drag handles, whose commands the
    /// widget around them mounts.
    alternatives: Option<GestureAlternatives>,
    items: Rc<dyn Fn() -> Vec<DragItem>>,
    child: BoxedView,
    allowed: AllowedActions,
    trigger: DragTrigger,
    feedback: Option<ViewFn>,
    anchor: DragAnchor,
    child_when_dragging: Option<ViewFn>,
    on_drag_started: Option<EventCallback>,
    on_drag_end: Option<ValueCallback<DragOutcome>>,
    devices: PointerKindSet,
    axis: MultiDragAxis,
    reach: Option<DragReach>,
}

impl std::fmt::Debug for Draggable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Draggable")
            .field("allowed", &self.allowed)
            .field("trigger", &self.trigger)
            .field("anchor", &self.anchor)
            .finish_non_exhaustive()
    }
}

impl Draggable {
    /// A draggable `child` whose drags carry what `items` makes, reached
    /// without a pointer path through `commands`.
    ///
    /// It allows moving and copying within the app, and nothing outside it;
    /// see [`allowed_actions`](Self::allowed_actions).
    ///
    /// # Errors
    ///
    /// [`GestureAlternativesError::EmptyLabel`] for a command without a
    /// label, [`GestureAlternativesError::DuplicateLabel`] for a cut and a
    /// copy with one.
    pub fn try_new(
        commands: DragCommands,
        items: impl Fn() -> Vec<DragItem> + 'static,
        child: impl IntoView,
    ) -> Result<Self, GestureAlternativesError> {
        Ok(Self {
            alternatives: Some(commands.alternatives()?),
            items: Rc::new(items),
            child: child.into_view().boxed(),
            allowed: AllowedActions::within(TransferActions::MOVE | TransferActions::COPY),
            trigger: DragTrigger::default(),
            feedback: None,
            anchor: DragAnchor::default(),
            child_when_dragging: None,
            on_drag_started: None,
            on_drag_end: None,
            devices: PointerKindSet::all(),
            axis: MultiDragAxis::Free,
            reach: None,
        })
    }

    /// A drag handle without commands of its own: the widget around it mounts
    /// the alternatives its drag needs (a reorderable row's moves).
    pub(crate) fn bare(items: impl Fn() -> Vec<DragItem> + 'static, child: impl IntoView) -> Self {
        Self {
            alternatives: None,
            items: Rc::new(items),
            child: child.into_view().boxed(),
            allowed: AllowedActions::within(TransferActions::MOVE),
            trigger: DragTrigger::default(),
            feedback: None,
            anchor: DragAnchor::default(),
            child_when_dragging: None,
            on_drag_started: None,
            on_drag_end: None,
            devices: PointerKindSet::all(),
            axis: MultiDragAxis::Free,
            reach: None,
        }
    }

    /// Only contacts of `devices` start a drag; read when the draggable mounts.
    pub(crate) fn devices(mut self, devices: PointerKindSet) -> Self {
        self.devices = devices;
        self
    }

    /// The axis a contact must move along to start the drag, fixed at mount: a
    /// list's handle takes its list's axis, so the drag, deeper than the
    /// list's scroll on the same axis, wins the contact (ADR-0208 §2). The
    /// drag then follows the pointer freely.
    pub(crate) fn axis(mut self, axis: MultiDragAxis) -> Self {
        self.axis = axis;
        self
    }

    /// How far the drag may go, instead of what its allowed actions derive.
    pub(crate) fn reach(mut self, reach: DragReach) -> Self {
        self.reach = Some(reach);
        self
    }

    /// Name what is dragged ("Alice"), for the labels the alternatives make
    /// themselves, such as the menu control's.
    #[must_use]
    pub fn target(mut self, name: impl Into<String>) -> Self {
        self.alternatives = self
            .alternatives
            .map(|alternatives| alternatives.target(name));
        self
    }

    /// The alternatives the widget built from its [`DragCommands`]: the cut,
    /// then the copy, and the single-pointer alternative.
    #[must_use]
    pub fn alternatives(&self) -> &GestureAlternatives {
        self.alternatives
            .as_ref()
            .expect("BUG: every public constructor builds a draggable's alternatives")
    }

    /// What targets may do with the drag inside the app and outside it. A drag
    /// with actions outside the app escalates to the operating system's drag
    /// when the pointer leaves the window, and its items need a serialized
    /// representation for targets there.
    #[must_use]
    pub fn allowed_actions(mut self, allowed: AllowedActions) -> Self {
        self.allowed = allowed;
        self
    }

    /// What starts a drag from a press; [`DragTrigger::Immediate`] by default.
    /// Choose [`DragTrigger::LongPress`] inside something that scrolls under
    /// a finger.
    #[must_use]
    pub fn trigger(mut self, trigger: DragTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// The view shown under the pointer while the drag runs in the window, in
    /// the nearest [`Overlay`]. It takes no pointer input, so the targets
    /// beneath it are found. The operating system draws its own image once
    /// the drag leaves the window.
    #[must_use]
    pub fn feedback(mut self, feedback: impl Fn() -> BoxedView + 'static) -> Self {
        self.feedback = Some(Rc::new(feedback));
        self
    }

    /// Where the feedback sits relative to the pointer; [`DragAnchor::Child`]
    /// by default.
    #[must_use]
    pub fn anchor(mut self, anchor: DragAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// The view shown in place of the child while a drag of it, or a cut or
    /// copy of it waiting for a paste, is live.
    #[must_use]
    pub fn child_when_dragging(mut self, view: impl Fn() -> BoxedView + 'static) -> Self {
        self.child_when_dragging = Some(Rc::new(view));
        self
    }

    /// Called when a drag starts: the arena gave the contact to the drag, or a
    /// cut or copy holds the items.
    #[must_use]
    pub fn on_drag_started<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_started = Some(event_callback(callback));
        self
    }

    /// Called once when a drag ends: dropped with the action a target took
    /// (a source whose drag was moved removes the item), or cancelled.
    #[must_use]
    pub fn on_drag_end<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragOutcome) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drag_end = Some(value_callback(callback));
        self
    }
}

/// What the live drags of one draggable read, refreshed by each build.
#[derive(Clone)]
struct SourceConfig {
    items: Rc<dyn Fn() -> Vec<DragItem>>,
    allowed: AllowedActions,
    feedback: Option<ViewFn>,
    anchor: DragAnchor,
    on_drag_started: Option<EventCallback>,
    on_drag_end: Option<ValueCallback<DragOutcome>>,
    reach: Option<DragReach>,
}

impl SourceConfig {
    fn of(view: &Draggable) -> Self {
        Self {
            items: Rc::clone(&view.items),
            allowed: view.allowed,
            feedback: view.feedback.clone(),
            anchor: view.anchor,
            on_drag_started: view.on_drag_started.clone(),
            on_drag_end: view.on_drag_end.clone(),
            reach: view.reach,
        }
    }
}

/// The capabilities a draggable acquires in its lifecycle hooks.
#[derive(Default)]
struct Capabilities {
    drag_and_drop: Option<DragAndDrop>,
    hit_test: Option<HitTestHandle>,
    overlay: Option<OverlayHandle>,
    haptics: Option<HapticsHandle>,
    pipeline: Option<flui_rendering::pipeline::PipelineCell>,
    rebuild: Option<RebuildHandle>,
    writer: Option<WriterSource>,
    keep_alive: Option<KeepAliveHandle>,
}

/// One draggable as its drags, recognizers and actions share it. Owner-local;
/// no borrow of it is held while user code runs.
struct Source {
    config: RefCell<SourceConfig>,
    capabilities: RefCell<Capabilities>,
    /// Drags of this draggable that are live, held transfers included.
    active: Cell<usize>,
    /// The cut or copy waiting for a paste.
    held: RefCell<Option<DragSession>>,
    /// The contacts pressed on the child, as their Down reported them.
    pressed: RefCell<Vec<PointerInfo>>,
    /// Held while a drag is live, so a lazy list scrolling the dragged row
    /// out of its cache band keeps its state and its drags (ADR-0211).
    kept_alive: RefCell<Option<KeepAliveLease>>,
}

impl Source {
    fn rebuild(&self) {
        let rebuild = self.capabilities.borrow().rebuild.clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    fn write(&self, invoke: impl FnOnce(&mut EventCx<'_>)) {
        let writer = self.capabilities.borrow().writer.clone();
        if let Some(writer) = writer {
            writer.write(invoke);
        }
    }

    fn haptic(&self, feedback: HapticFeedback, timing: HapticTiming) {
        let haptics = self.capabilities.borrow().haptics.clone();
        if let Some(haptics) = haptics {
            let _ = haptics.perform(feedback, HapticRequest::new(timing));
        }
    }

    /// A drag of this draggable began.
    fn started(&self) {
        if self.active.replace(self.active.get() + 1) == 0 {
            let keep_alive = self.capabilities.borrow().keep_alive.clone();
            *self.kept_alive.borrow_mut() = keep_alive.map(|handle| handle.hold());
        }
        self.rebuild();
        let callback = self.config.borrow().on_drag_started.clone();
        if let Some(callback) = callback {
            self.write(|cx| callback(cx));
        }
    }

    /// A drag of this draggable ended with `outcome`; `callback` is the
    /// `on_drag_end` it started with.
    fn ended(&self, outcome: DragOutcome, callback: Option<&ValueCallback<DragOutcome>>) {
        let active = self.active.get().saturating_sub(1);
        self.active.set(active);
        if active == 0 {
            let released = self.kept_alive.borrow_mut().take();
            drop(released);
        }
        self.rebuild();
        // The drop is felt with the frame that shows it: the feedback gone
        // and the target's new contents (ADR-0210 §4).
        if matches!(outcome, DragOutcome::Dropped { .. }) {
            self.haptic(HapticFeedback::Drop, HapticTiming::FramePresented);
        }
        if let Some(callback) = callback {
            self.write(|cx| callback(cx, outcome));
        }
    }

    /// The arena gave `pointer`'s contact to the drag, pressed at `grab` in
    /// the child's space: start its session.
    fn start_contact(
        self: &Rc<Self>,
        pointer: PointerId,
        grab: Offset<f64>,
    ) -> Option<Rc<Contact>> {
        let (drag_and_drop, hit_test, overlay) = {
            let capabilities = self.capabilities.borrow();
            (
                capabilities.drag_and_drop.clone()?,
                capabilities.hit_test.clone(),
                capabilities.overlay.clone(),
            )
        };
        let config = self.config.borrow().clone();
        let info = self
            .pressed
            .borrow()
            .iter()
            .copied()
            .find(|info| info.id == pointer)
            .unwrap_or_else(|| PointerInfo::new(pointer, flui_interaction::PointerKind::Unknown));
        let anchor = match config.anchor {
            DragAnchor::Child => grab,
            DragAnchor::Pointer => Offset::ZERO,
        };
        let pipeline = self.capabilities.borrow().pipeline.clone();
        let feedback = overlay
            .zip(config.feedback.clone())
            .map(|(overlay, view)| Feedback::mount(&overlay, view, anchor, pipeline));
        let observer = Rc::new(SessionObserver {
            source: Rc::downgrade(self),
            feedback: feedback.clone(),
            on_drag_end: config.on_drag_end.clone(),
        });
        let mut start = drag_and_drop
            .drag((config.items)(), config.allowed)
            .pointer(info)
            .observer(observer);
        if let Some(reach) = config.reach {
            start = start.reach(reach);
        }
        if let Some(hit_test) = hit_test {
            start = start.hit_test(hit_test);
        }
        let session = match drag_and_drop.start(start) {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!(%error, "a drag could not start");
                if let Some(feedback) = &feedback {
                    feedback.remove();
                }
                return None;
            }
        };
        self.haptic(HapticFeedback::DragLift, HapticTiming::FramePresented);
        self.started();
        Some(Rc::new(Contact { session, feedback }))
    }

    /// Hold the items for a paste: a cut moves them, a copy copies them.
    fn hold(self: &Rc<Self>, action: TransferActions) -> ActionOutcome {
        let Some(drag_and_drop) = self.capabilities.borrow().drag_and_drop.clone() else {
            return ActionOutcome::NotPerformed;
        };
        let config = self.config.borrow().clone();
        let observer = Rc::new(SessionObserver {
            source: Rc::downgrade(self),
            feedback: None,
            on_drag_end: config.on_drag_end.clone(),
        });
        let start = drag_and_drop
            .drag((config.items)(), config.allowed)
            .observer(observer);
        match drag_and_drop.hold(start, action) {
            Ok(session) => {
                let replaced = self.held.borrow_mut().replace(session);
                drop(replaced);
                self.started();
                ActionOutcome::Performed
            }
            Err(error) => {
                tracing::debug!(%error, "the drag does not allow this transfer");
                ActionOutcome::NotPerformed
            }
        }
    }
}

/// Hears one session of a draggable.
///
/// The drag reports to the `on_drag_end` it started with: a draggable that a
/// rebuild gives another item mid-drag still tells the first item's callback
/// how the first item's drag ended.
struct SessionObserver {
    source: Weak<Source>,
    feedback: Option<Rc<Feedback>>,
    on_drag_end: Option<ValueCallback<DragOutcome>>,
}

impl DragObserver for SessionObserver {
    fn moved(&self, at: DragPoint) {
        if let Some(feedback) = &self.feedback {
            feedback.place(at.global);
        }
    }

    fn ended(&self, outcome: DragOutcome) {
        if let Some(feedback) = &self.feedback {
            feedback.remove();
        }
        if let Some(source) = self.source.upgrade() {
            source.ended(outcome, self.on_drag_end.as_ref());
        }
    }
}

/// One contact's drag: its session and the feedback that follows it.
struct Contact {
    session: DragSession,
    feedback: Option<Rc<Feedback>>,
}

impl MultiDragHandle for Contact {
    fn update(&self, details: MultiDragUpdateDetails) {
        if let Ok(MoveOutcome::Escalating | MoveOutcome::OsOwned) =
            self.session.pointer_moved(details.global_position, None)
            && let Some(feedback) = &self.feedback
        {
            // The operating system draws the drag outside the window.
            feedback.remove();
        }
    }

    fn end(&self, details: MultiDragEndDetails) {
        // A session the OS owns ends when the OS says; one already over
        // (cancelled by Escape) has nothing left to drop.
        let _ = self.session.pointer_released(details.global_position, None);
    }

    fn cancel(&self) {
        let _ = self.session.cancel();
    }
}

/// The feedback of one drag: an overlay entry placed where the pointer is.
struct Feedback {
    entry: OverlayEntry,
    place: Rc<FeedbackPlace>,
    removed: Cell<bool>,
}

/// Where a feedback entry paints, and how to tell it to move.
struct FeedbackPlace {
    /// The pointer's offset from the feedback's top-left corner.
    anchor: Offset<f64>,
    /// The pointer in the window's space. The entry resolves it into the
    /// overlay's space when it builds, once it knows its overlay node: a move
    /// before the entry's first build has no overlay to resolve against.
    pointer: Cell<Offset<f64>>,
    /// The render node the entry is laid out in, published by its anchor, and
    /// the tree that maps the window's space to it.
    overlay_node: Cell<Option<flui_foundation::RenderId>>,
    pipeline: Option<flui_rendering::pipeline::PipelineCell>,
    rebuild: RefCell<Option<RebuildHandle>>,
}

impl Feedback {
    fn mount(
        overlay: &OverlayHandle,
        view: ViewFn,
        anchor: Offset<f64>,
        pipeline: Option<flui_rendering::pipeline::PipelineCell>,
    ) -> Rc<Self> {
        let place = Rc::new(FeedbackPlace {
            anchor,
            pointer: Cell::new(Offset::ZERO),
            overlay_node: Cell::new(None),
            pipeline,
            rebuild: RefCell::new(None),
        });
        let entry = {
            let place = Rc::clone(&place);
            OverlayEntry::new(move |_ctx| {
                FeedbackAnchor {
                    view: Rc::clone(&view),
                    place: Rc::clone(&place),
                }
                .into_view()
                .boxed()
            })
        };
        overlay.insert(&entry, &InsertPosition::Top);
        Rc::new(Self {
            entry,
            place,
            removed: Cell::new(false),
        })
    }

    /// Put the feedback where the pointer is, `global` in the window's space.
    fn place(&self, global: Offset<f64>) {
        if self.removed.get() {
            return;
        }
        self.place.pointer.set(global);
        let rebuild = self.place.rebuild.borrow().clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    fn remove(&self) {
        if !self.removed.replace(true) {
            self.entry.remove();
        }
    }
}

impl FeedbackPlace {
    /// The feedback's top-left corner in the overlay's space.
    fn top_left(&self) -> Offset<f64> {
        self.pointer.get() - self.overlay_origin() - self.anchor
    }

    /// Where the overlay's origin is in the window: the feedback follows the
    /// pointer's window position wherever the overlay sits.
    fn overlay_origin(&self) -> Offset<f64> {
        let (Some(node), Some(pipeline)) = (self.overlay_node.get(), self.pipeline.as_ref()) else {
            return Offset::ZERO;
        };
        pipeline
            .try_with(|owner| owner.local_to_global(node, Point::new(0.0, 0.0), None))
            .flatten()
            .map_or(Offset::ZERO, |origin| Offset::new(origin.x, origin.y))
    }
}

/// The feedback entry's content: the feedback, positioned in a `Stack` of its
/// own, because the overlay's theater does not position its entries.
#[derive(Clone)]
struct FeedbackAnchor {
    view: ViewFn,
    place: Rc<FeedbackPlace>,
}

impl View for FeedbackAnchor {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for FeedbackAnchor {
    type State = FeedbackAnchorState;

    fn create_state(&self) -> Self::State {
        FeedbackAnchorState {
            place: Rc::clone(&self.place),
        }
    }
}

struct FeedbackAnchorState {
    place: Rc<FeedbackPlace>,
}

impl ViewState<FeedbackAnchor> for FeedbackAnchorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.place.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        // The nearest render object above the entry's content is the one the
        // entry is laid out in: the overlay's.
        self.place.overlay_node.set(ctx.find_render_object());
    }

    fn build(&self, view: &FeedbackAnchor, _ctx: &dyn BuildContext) -> impl IntoView {
        let at = view.place.top_left();
        Stack::new(vec![
            // The feedback sits under the pointer, where the drag hit-tests:
            // a hit-testable one would hide every target beneath it.
            Positioned::new(IgnorePointer::new().child((view.view)()))
                .left(at.dx)
                .top(at.dy)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand)
    }
}

/// Persistent state of a [`Draggable`]: its recognizers, which survive
/// rebuilds and are disposed on unmount, and the shared source its drags,
/// actions and callbacks read.
pub struct DraggableState {
    source: Rc<Source>,
    drag: Option<Rc<MultiDragGestureRecognizer>>,
    hold: Option<Rc<LongPressGestureRecognizer>>,
    /// The trigger the view asks for, and the scope's arena and settings.
    trigger: DragTrigger,
    /// The devices whose contacts start a drag, fixed at mount.
    devices: PointerKindSet,
    /// The axis a contact must move along to start a drag, fixed at mount.
    axis: MultiDragAxis,
    policy: Option<GesturePolicy>,
    /// The hold of an enclosing context menu, and whether the widget's own
    /// alternatives are a menu: a long-press drag succeeds a menu's long press
    /// instead of holding the contact itself.
    enclosing_hold: Option<Rc<SequencedBranch>>,
    own_menu: bool,
    /// The hold this widget composed for its own menu's long press.
    own_hold: Option<Rc<SequencedBranch>>,
    built_for: Option<BuiltFor>,
    drag_attachment: Rc<RecognizerAttachment<MultiDragGestureRecognizer>>,
    hold_attachment: Rc<RecognizerAttachment<LongPressGestureRecognizer>>,
}

impl std::fmt::Debug for DraggableState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraggableState")
            .field("active", &self.source.active.get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Draggable {
    type State = DraggableState;

    fn create_state(&self) -> Self::State {
        DraggableState {
            source: Rc::new(Source {
                config: RefCell::new(SourceConfig::of(self)),
                capabilities: RefCell::new(Capabilities::default()),
                active: Cell::new(0),
                held: RefCell::new(None),
                pressed: RefCell::new(Vec::new()),
                kept_alive: RefCell::new(None),
            }),
            drag: None,
            hold: None,
            trigger: self.trigger,
            devices: self.devices,
            axis: self.axis,
            policy: None,
            enclosing_hold: None,
            own_menu: self
                .alternatives
                .as_ref()
                .is_some_and(GestureAlternatives::is_menu),
            own_hold: None,
            built_for: None,
            drag_attachment: Rc::new(RecognizerAttachment::default()),
            hold_attachment: Rc::new(RecognizerAttachment::default()),
        }
    }
}

/// What a draggable's recognizers were built for.
struct BuiltFor {
    policy: GesturePolicy,
    trigger: DragTrigger,
    enclosing_hold: Option<Rc<SequencedBranch>>,
    own_menu: bool,
}

impl BuiltFor {
    /// Whether recognizers built for `self` serve `wanted` too.
    fn matches(&self, wanted: &Self) -> bool {
        let same_hold = match (&self.enclosing_hold, &wanted.enclosing_hold) {
            (None, None) => true,
            (Some(ours), Some(theirs)) => Rc::ptr_eq(ours, theirs),
            _ => false,
        };
        self.policy == wanted.policy
            && self.trigger == wanted.trigger
            && self.own_menu == wanted.own_menu
            && same_hold
    }
}

impl DraggableState {
    /// Acquire everything this widget reads from its context, in a lifecycle
    /// hook: never `build`, and never a gesture callback.
    fn acquire(&self, ctx: &dyn LifecycleContext) {
        let mut capabilities = self.source.capabilities.borrow_mut();
        capabilities.drag_and_drop = ctx.drag_and_drop();
        capabilities.hit_test = ctx.hit_test_handle();
        capabilities.overlay = Overlay::maybe_of(ctx);
        capabilities.haptics = ctx.haptics();
        capabilities.pipeline = ctx.pipeline_owner();
        capabilities.rebuild = Some(ctx.rebuild_handle());
        capabilities.writer = Some(ctx.writer_source());
        capabilities.keep_alive = Some(ctx.keep_alive_handle());
    }

    /// Build the recognizers for `trigger` under `settings`, retiring the ones
    /// they replace.
    fn rebuild_recognizers(&mut self) {
        let trigger = self.trigger;
        let Some(policy) = self.policy.clone() else {
            return;
        };
        let GesturePolicy { arena, settings } = policy.clone();
        let built_for = BuiltFor {
            policy,
            trigger,
            enclosing_hold: self.enclosing_hold.clone(),
            own_menu: self.own_menu,
        };
        if self
            .built_for
            .as_ref()
            .is_some_and(|built| built.matches(&built_for))
        {
            return;
        }
        let on_start = {
            let source = Rc::downgrade(&self.source);
            move |pointer: PointerId, grab: Offset<f64>| {
                let source = source.upgrade()?;
                source
                    .start_contact(pointer, grab)
                    .map(|contact| contact as Rc<dyn MultiDragHandle>)
            }
        };
        let mut own_hold = None;
        let (drag_arena, hold) = match (trigger, &self.enclosing_hold) {
            (DragTrigger::Immediate, _) => (arena, None),
            // A context menu's long press holds the contact and hands it to
            // the drag once it moves: the enclosing menu's, or this widget's
            // own, built by its alternatives on the hold composed here.
            (DragTrigger::LongPress, Some(menu)) => (menu.successor(), None),
            (DragTrigger::LongPress, None) if self.own_menu => match arena.sequenced() {
                Ok(branch) => {
                    let branch = Rc::new(branch);
                    let successor = branch.successor();
                    own_hold = Some(branch);
                    (successor, None)
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "a long-press drag could not compose as Sequenced; it starts at once"
                    );
                    (arena, None)
                }
            },
            (DragTrigger::LongPress, None) => match arena.sequenced() {
                Ok(branch) => {
                    let branch = Rc::new(branch);
                    let pressed = Rc::downgrade(&self.source);
                    let hand_off = Rc::clone(&branch);
                    let hold = LongPressGestureRecognizer::builder(branch.hold())
                        .settings(settings.clone())
                        .allowed_devices(self.devices)
                        .on_long_press_start(move |_| {
                            // The hold won one of the pressed contacts; only
                            // that one is handed to the drag.
                            let contacts: Vec<PointerId> = pressed
                                .upgrade()
                                .map(|source| {
                                    source.pressed.borrow().iter().map(|info| info.id).collect()
                                })
                                .unwrap_or_default();
                            for pointer in contacts {
                                if hand_off.hand_off(pointer) {
                                    break;
                                }
                            }
                        })
                        .build();
                    (branch.successor(), Some(hold))
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "a long-press drag could not compose as Sequenced; it starts at once"
                    );
                    (arena, None)
                }
            },
        };
        let drag = MultiDragGestureRecognizer::builder(drag_arena, self.axis)
            .settings(settings)
            .allowed_devices(self.devices)
            .on_start(on_start)
            .build();
        self.drag_attachment.attach(&drag);
        match &hold {
            Some(hold) => self.hold_attachment.attach(hold),
            None => self.hold_attachment.clear(),
        }
        let outgoing = (
            self.drag.replace(drag),
            std::mem::replace(&mut self.hold, hold),
        );
        self.own_hold = own_hold;
        self.built_for = Some(built_for);
        if let Some(drag) = outgoing.0 {
            let _ = drag.cancel();
        }
        if let Some(hold) = outgoing.1 {
            let _ = hold.cancel();
        }
    }
}

impl ViewState<Draggable> for DraggableState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    /// Re-acquires the capabilities and the overlay, and replaces the
    /// recognizers when the arena, the gesture policy or an enclosing menu's
    /// hold changed (ADR-0208).
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
        self.policy = Some(GesturePolicy::of(ctx));
        self.enclosing_hold = MenuHoldScope::of(ctx);
        self.rebuild_recognizers();
    }

    /// A new trigger, or alternatives that turn into a menu or out of one,
    /// replace the recognizers; a drag already running keeps going until the
    /// replaced ones cancel it.
    fn did_update_view(&mut self, _old_view: &Draggable, new_view: &Draggable) {
        let own_menu = new_view
            .alternatives
            .as_ref()
            .is_some_and(GestureAlternatives::is_menu);
        if new_view.trigger != self.trigger || own_menu != self.own_menu {
            self.trigger = new_view.trigger;
            self.own_menu = own_menu;
            if self.built_for.is_some() {
                self.rebuild_recognizers();
            }
        }
    }

    fn build(&self, view: &Draggable, _ctx: &dyn BuildContext) -> impl IntoView {
        let _prev = self.source.config.replace(SourceConfig::of(view));
        let content = if self.source.active.get() > 0
            && let Some(dragging) = &view.child_when_dragging
        {
            dragging()
        } else {
            view.child.clone()
        };
        let pressed = Rc::downgrade(&self.source);
        let lifted = Rc::downgrade(&self.source);
        let mut listener = Listener::new()
            .on_pointer_down(move |_cx, dispatch| {
                if let (Some(source), PointerEvent::Down(press)) =
                    (pressed.upgrade(), dispatch.local)
                {
                    let mut contacts = source.pressed.borrow_mut();
                    contacts.retain(|info| info.id != press.pointer.id);
                    contacts.push(press.pointer);
                }
            })
            .on_pointer_up(move |_cx, dispatch| forget_contact(&lifted, dispatch.local))
            .on_pointer_cancel({
                let source = Rc::downgrade(&self.source);
                move |_cx, dispatch| forget_contact(&source, dispatch.local)
            })
            .recognizer(&self.drag_attachment);
        if view.trigger == DragTrigger::LongPress {
            listener = listener.recognizer(&self.hold_attachment);
        }
        let transfers = DragTransfer {
            source: Rc::clone(&self.source),
        };
        let grab = MouseRegion::new()
            .cursor(CursorIcon::Grab)
            .child(listener.child(content));
        match &view.alternatives {
            Some(alternatives) => Actions::new(
                alternatives
                    .clone()
                    .with_hold(self.own_hold.clone())
                    .child(grab),
            )
            .action(transfers)
            .boxed(),
            None => grab.boxed(),
        }
    }

    /// Cancels every drag of this widget, its held transfer included: the
    /// targets entered get their leave and `on_drag_end` hears `Cancelled`.
    /// A drag the operating system runs continues until the OS ends it.
    ///
    /// A lazy list does not get here by scrolling a dragged row away: while
    /// a drag is live the row holds a keep-alive lease, so only removing the
    /// row (or its list) disposes it mid-drag.
    fn dispose(&mut self) {
        let released = self.source.kept_alive.borrow_mut().take();
        drop(released);
        self.drag_attachment.clear();
        self.hold_attachment.clear();
        let held = self.source.held.borrow_mut().take();
        drop(held);
        if let Some(drag) = self.drag.take() {
            let _ = drag.cancel();
        }
        if let Some(hold) = self.hold.take() {
            let _ = hold.cancel();
        }
    }
}

/// A Down's contact ended: forget it.
fn forget_contact(source: &Weak<Source>, event: &PointerEvent) {
    let Some(source) = source.upgrade() else {
        return;
    };
    let Some(pointer) = flui_interaction::PointerEventExt::pointer_id(event) else {
        return;
    };
    source
        .pressed
        .borrow_mut()
        .retain(|info| info.id != pointer);
}

/// Answers the cut and copy keys and commands by holding the items for a
/// paste.
struct DragTransfer {
    source: Rc<Source>,
}

impl Action<CopySelectionTextIntent> for DragTransfer {
    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &CopySelectionTextIntent) -> ActionOutcome {
        let action = match intent {
            CopySelectionTextIntent::Copy => TransferActions::COPY,
            CopySelectionTextIntent::Cut => TransferActions::MOVE,
        };
        self.source.hold(action)
    }
}
