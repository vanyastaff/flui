//! [`DropTarget`] — the receiving end of a drag-and-drop session (ADR-0211).
//!
//! A drop target registers one [`DropReceiver`] for its whole life and tags
//! its render node with the receiver's interaction-lane ticket, so a drag's
//! hit test finds it, whether the drag is a [`Draggable`](crate::Draggable)'s
//! in this window, another presentation's or another application's through
//! the operating system. It answers each enter and move with what a drop
//! would do, takes the drop only when it accepted the drag's last move, may
//! spring-load while a drag hovers, and leaves edge autoscroll to the
//! scrollable around it.
//!
//! The paste key ([`PasteTextIntent`]) with the target focused, or the paste
//! of its [`DropCommands`], which performs that intent, drops the
//! transfer a cut or copy holds, through the same receiver: the keyboard's
//! and assistive technology's drop.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::LocalPayloadTarget;
use flui_interaction::drag::{
    DragAndDrop, DragIdentity, DragPoint, DragView, DropReceiver, DropReceiverSlot, SpringLoading,
};
use flui_objects::RenderMetaData;
use flui_painting::typography::TextDirection;
use flui_platform::data_transfer::{DropFeedback, TransferActions};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{
    Child, EventCx, EventOutcome, RebuildHandle, RenderObjectContext, RenderView, WriterSource,
    impl_render_view,
};

use super::actions::{Action, ActionOutcome, Actions, PasteTextIntent};
use super::gesture_alternatives::{
    AlternativeKind, CommandAppearance, GestureAlternatives, GestureAlternativesError,
};
use crate::localization::Directionality;

/// Whether drags hover a [`DropTarget`], and what a drop would do: what its
/// builder shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DropHover {
    /// No drag is over the target.
    #[default]
    Idle,
    /// Drags are over the target and it would take none of them.
    Declined,
    /// A drag is over the target that a drop would take with this action.
    Accepting(TransferActions),
}

/// Answers whether a drop of the drag would be taken, and with which actions;
/// asked at every enter and move.
type AcceptFn = Rc<dyn Fn(&DragView<'_>) -> Option<TransferActions>>;
/// Called with the drop.
type DropFn = Rc<dyn Fn(&mut EventCx<'_>, &DragView<'_>, DragPoint, TransferActions)>;
/// Called when a hover spring-loads the target.
type SpringFn = Rc<dyn Fn(&mut EventCx<'_>, &DragView<'_>)>;

/// A widget that receives drops: from a [`Draggable`](crate::Draggable), from
/// another window of the app, or from another application.
///
/// `accept` answers, for each enter and move, the actions a drop would take
/// (`None` to decline); the drag's own allowance narrows the answer, and a
/// drop lands only on the target that accepted the last move. The builder
/// shows the target from its [`DropHover`].
///
/// Its [`DropCommands`] are the drop's keyboard, assistive and single-pointer
/// paths: a paste, which this widget answers by dropping the transfer a cut
/// or copy holds.
#[doc(alias = "DragTarget")]
#[derive(Clone, StatefulView)]
pub struct DropTarget {
    alternatives: GestureAlternatives,
    accept: AcceptFn,
    builder: Rc<dyn Fn(DropHover) -> BoxedView>,
    on_drop: Option<DropFn>,
    spring_loading: Option<SpringLoading>,
    on_spring_loaded: Option<SpringFn>,
}

impl std::fmt::Debug for DropTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropTarget")
            .field("spring_loading", &self.spring_loading)
            .finish_non_exhaustive()
    }
}

/// The drop's command without a pointer path: a paste, and how one pointer
/// reaches it.
///
/// The widget pairs it with [`PasteTextIntent`], so every alternative
/// performs the drop a pointer would.
#[derive(Clone, Debug)]
pub struct DropCommands {
    paste: CommandAppearance,
    alternative: AlternativeKind,
}

impl DropCommands {
    /// A paste shown as `paste`, reached by a control beside the content.
    #[must_use]
    pub fn new(paste: CommandAppearance) -> Self {
        Self {
            paste,
            alternative: AlternativeKind::default(),
        }
    }

    /// Reach the paste with one pointer as `alternative` says; a control
    /// after the content by default.
    #[must_use]
    pub fn alternative(mut self, alternative: AlternativeKind) -> Self {
        self.alternative = alternative;
        self
    }

    fn alternatives(self) -> Result<GestureAlternatives, GestureAlternativesError> {
        GestureAlternatives::of_kind(self.alternative, self.paste.performing(PasteTextIntent), [])
    }
}

impl DropTarget {
    /// A target answering each drag with `accept`, shown by `builder`, reached
    /// without a pointer path through `commands`.
    ///
    /// # Errors
    ///
    /// [`GestureAlternativesError::EmptyLabel`] for a paste without a label.
    pub fn new(
        commands: DropCommands,
        accept: impl Fn(&DragView<'_>) -> Option<TransferActions> + 'static,
        builder: impl Fn(DropHover) -> BoxedView + 'static,
    ) -> Result<Self, GestureAlternativesError> {
        Ok(Self {
            alternatives: commands.alternatives()?,
            accept: Rc::new(accept),
            builder: Rc::new(builder),
            on_drop: None,
            spring_loading: None,
            on_spring_loaded: None,
        })
    }

    /// Name what receives the drop ("Done"), for the labels the alternatives
    /// make themselves, such as the menu control's.
    #[must_use]
    pub fn target(mut self, name: impl Into<String>) -> Self {
        self.alternatives = self.alternatives.target(name);
        self
    }

    /// The alternatives the widget built from its [`DropCommands`].
    #[must_use]
    pub fn alternatives(&self) -> &GestureAlternatives {
        &self.alternatives
    }

    /// Called with a drop this target accepted: the drag, where it was
    /// dropped and the action it was dropped with. A paste is dropped at the
    /// target's reading-start top corner.
    #[must_use]
    pub fn on_drop<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &DragView<'_>, DragPoint, TransferActions) -> R + 'static,
        R: EventOutcome,
    {
        self.on_drop = Some(Rc::new(move |cx, drag, at, action| {
            callback(cx, drag, at, action).report();
        }));
        self
    }

    /// Spring-load: once a drag has hovered the target for `policy`'s delay,
    /// `callback` runs, once per hover (it opens a folder, switches a tab).
    #[must_use]
    pub fn spring_loaded<F, R>(mut self, policy: SpringLoading, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &DragView<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.spring_loading = Some(policy);
        self.on_spring_loaded = Some(Rc::new(move |cx, drag| callback(cx, drag).report()));
        self
    }
}

/// What the receiver reads of its view, refreshed by each build.
#[derive(Clone)]
struct TargetConfig {
    accept: AcceptFn,
    on_drop: Option<DropFn>,
    spring_loading: Option<SpringLoading>,
    on_spring_loaded: Option<SpringFn>,
}

impl TargetConfig {
    fn of(view: &DropTarget) -> Self {
        Self {
            accept: Rc::clone(&view.accept),
            on_drop: view.on_drop.clone(),
            spring_loading: view.spring_loading,
            on_spring_loaded: view.on_spring_loaded.clone(),
        }
    }
}

/// One drop target's receiver: one registration for the target's life, so
/// the drag machinery sees one receiver however often the target rebuilds.
struct TargetReceiver {
    config: RefCell<TargetConfig>,
    hovers: RefCell<Vec<(Option<DragIdentity>, Option<TransferActions>)>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    writer: RefCell<Option<WriterSource>>,
}

impl TargetReceiver {
    fn hover(&self) -> DropHover {
        let hovers = self.hovers.borrow();
        if let Some(action) = hovers.iter().find_map(|(_, accept)| *accept) {
            DropHover::Accepting(action)
        } else if hovers.is_empty() {
            DropHover::Idle
        } else {
            DropHover::Declined
        }
    }

    fn rebuild(&self) {
        let rebuild = self.rebuild.borrow().clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    fn write(&self, invoke: impl FnOnce(&mut EventCx<'_>)) {
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            writer.write(invoke);
        }
    }

    /// Ask `accept`, narrowed by what the drag allows here, and record the
    /// hover's standing.
    fn answer(&self, drag: &DragView<'_>) -> DropFeedback {
        let accept = Rc::clone(&self.config.borrow().accept);
        let answer = accept(drag)
            .map(|actions| actions & drag.allowed())
            .filter(|actions| !actions.is_empty());
        let key = drag.identity();
        let changed = {
            let mut hovers = self.hovers.borrow_mut();
            if let Some((_, standing)) = hovers.iter_mut().find(|(hovering, _)| *hovering == key) {
                std::mem::replace(standing, answer) != answer
            } else {
                hovers.push((key, answer));
                true
            }
        };
        if changed {
            self.rebuild();
        }
        DropFeedback { accept: answer }
    }

    fn forget(&self, drag: &DragView<'_>) {
        let key = drag.identity();
        self.hovers
            .borrow_mut()
            .retain(|(hovering, _)| *hovering != key);
        self.rebuild();
    }
}

impl DropReceiver for TargetReceiver {
    fn drag_entered(&self, drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        self.answer(drag)
    }

    fn drag_moved(&self, drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        self.answer(drag)
    }

    fn drag_left(&self, drag: &DragView<'_>) {
        self.forget(drag);
    }

    fn dropped(&self, drag: &DragView<'_>, at: DragPoint, action: TransferActions) {
        self.forget(drag);
        let on_drop = self.config.borrow().on_drop.clone();
        if let Some(on_drop) = on_drop {
            self.write(|cx| on_drop(cx, drag, at, action));
        }
    }

    fn spring_loading(&self) -> Option<SpringLoading> {
        self.config.borrow().spring_loading
    }

    fn spring_loaded(&self, drag: &DragView<'_>) {
        let callback = self.config.borrow().on_spring_loaded.clone();
        if let Some(callback) = callback {
            self.write(|cx| callback(cx, drag));
        }
    }
}

/// Where a paste drops: the target's node, the tree that knows its geometry,
/// the reading direction, and the ticket its receiver is registered under.
#[derive(Default)]
struct PastePlace {
    node: Cell<Option<flui_foundation::RenderId>>,
    pipeline: RefCell<Option<flui_rendering::pipeline::PipelineCell>>,
    direction: Cell<Option<TextDirection>>,
    ticket: Cell<Option<LocalPayloadTarget>>,
    drag_and_drop: RefCell<Option<DragAndDrop>>,
}

impl PastePlace {
    /// The target's reading-start top corner: its top-left, or its top-right
    /// in a right-to-left locale.
    fn point(&self) -> DragPoint {
        let geometry = self.node.get().zip(self.pipeline.borrow().clone());
        let rtl = self.direction.get() == Some(TextDirection::Rtl);
        let local = geometry
            .as_ref()
            .and_then(|(node, pipeline)| pipeline.try_with(|owner| owner.box_size(*node)).flatten())
            .map_or(Offset::ZERO, |size| {
                Offset::new(if rtl { size.width } else { 0.0 }, 0.0)
            });
        let global = geometry
            .and_then(|(node, pipeline)| {
                pipeline
                    .try_with(|owner| {
                        owner.local_to_global(node, Point::new(local.dx, local.dy), None)
                    })
                    .flatten()
            })
            .map_or(local, |point| Offset::new(point.x, point.y));
        DragPoint {
            global,
            local,
            time: None,
        }
    }
}

/// Persistent state of a [`DropTarget`]: its receiver, registered once, and
/// where a paste drops.
pub struct DropTargetState {
    receiver: Rc<TargetReceiver>,
    paste: Rc<PastePlace>,
}

impl std::fmt::Debug for DropTargetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropTargetState")
            .field("hover", &self.receiver.hover())
            .finish_non_exhaustive()
    }
}

impl StatefulView for DropTarget {
    type State = DropTargetState;

    fn create_state(&self) -> Self::State {
        DropTargetState {
            receiver: Rc::new(TargetReceiver {
                config: RefCell::new(TargetConfig::of(self)),
                hovers: RefCell::new(Vec::new()),
                rebuild: RefCell::new(None),
                writer: RefCell::new(None),
            }),
            paste: Rc::new(PastePlace::default()),
        }
    }
}

impl DropTargetState {
    fn acquire(&self, ctx: &dyn LifecycleContext) {
        *self.receiver.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        *self.receiver.writer.borrow_mut() = Some(ctx.writer_source());
        *self.paste.pipeline.borrow_mut() = ctx.pipeline_owner();
        *self.paste.drag_and_drop.borrow_mut() = ctx.drag_and_drop();
    }
}

impl ViewState<DropTarget> for DropTargetState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn build(&self, view: &DropTarget, ctx: &dyn BuildContext) -> impl IntoView {
        let _prev = self.receiver.config.replace(TargetConfig::of(view));
        self.paste.direction.set(Directionality::maybe_of(ctx));
        let paste = Paste {
            place: Rc::clone(&self.paste),
        };
        Actions::new(view.alternatives.clone().child(ReceiverAnchor {
            receiver: Rc::clone(&self.receiver),
            ticket: Rc::clone(&self.paste),
            child: Child::some(PasteProbe {
                place: Rc::clone(&self.paste),
                child: (view.builder)(self.receiver.hover()),
            }),
        }))
        .action(paste)
    }
}

/// Answers the paste key and command by dropping the held transfer here.
struct Paste {
    place: Rc<PastePlace>,
}

impl Action<PasteTextIntent> for Paste {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &PasteTextIntent) -> ActionOutcome {
        let drag_and_drop = self.place.drag_and_drop.borrow().clone();
        let (Some(drag_and_drop), Some(ticket)) = (drag_and_drop, self.place.ticket.get()) else {
            return ActionOutcome::NotPerformed;
        };
        match drag_and_drop.paste(ticket, self.place.point()) {
            Some(_) => ActionOutcome::Performed,
            None => ActionOutcome::NotPerformed,
        }
    }
}

/// Registers the receiver in the owner lane and tags its node with the
/// lane's ticket, so a drag's hit test finds it. Translucent, so targets
/// under it are found too.
#[derive(Clone)]
struct ReceiverAnchor {
    receiver: Rc<TargetReceiver>,
    ticket: Rc<PastePlace>,
    child: Child,
}

impl ReceiverAnchor {
    fn publish(&self, ctx: &RenderObjectContext<'_>, render_object: &mut RenderMetaData) {
        let slot = DropReceiverSlot(Rc::clone(&self.receiver) as Rc<dyn DropReceiver>);
        let payload: Rc<dyn Any> = Rc::new(slot);
        match ctx.register_local_payload(payload) {
            Ok(target) => {
                self.ticket.ticket.set(Some(target));
                render_object.set_shared_metadata(Some(Arc::new(target)));
            }
            Err(error) => tracing::debug!(
                ?error,
                "drop target mounted without an owner lane; drags cannot find it yet"
            ),
        }
    }
}

impl RenderView for ReceiverAnchor {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMetaData;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderMetaData::new();
        render_object.set_behavior(HitTestBehavior::Translucent);
        self.publish(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The receiver is the state's own for its whole life, so the ticket
        // registered at mount still names it.
        if render_object.metadata_as::<LocalPayloadTarget>().is_none() {
            self.publish(ctx, render_object);
        }
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.metadata_as::<LocalPayloadTarget>().copied()
            && let Err(error) = ctx.unregister_local_payload(target)
        {
            tracing::debug!(?error, "drop target receiver was already unregistered");
        }
        self.ticket.ticket.set(None);
        render_object.clear_metadata();
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ReceiverAnchor);

/// Learns the target's render node for a paste's geometry: mounted under the
/// anchor, the nearest render object above it is the anchor's.
#[derive(Clone)]
struct PasteProbe {
    place: Rc<PastePlace>,
    child: BoxedView,
}

impl View for PasteProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for PasteProbe {
    type State = PasteProbeState;

    fn create_state(&self) -> Self::State {
        PasteProbeState {
            place: Rc::clone(&self.place),
        }
    }
}

struct PasteProbeState {
    place: Rc<PastePlace>,
}

impl ViewState<PasteProbe> for PasteProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.place.node.set(ctx.find_render_object());
    }

    fn build(&self, view: &PasteProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}
