//! The chain a scroll leaf snapshots once per sequence, and the passes that
//! walk it (ADR-0209 §1, §4–§7).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Offset, Point, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_interaction::RemainderHandoff;
use flui_interaction::{GestureArena, InputSequenceId, Velocity};
use flui_platform::EventTime;
use flui_view::{BoxedView, BuildContext, BuildContextExt, InheritedView, IntoView, View, ViewExt};
use smallvec::SmallVec;

use super::idle_end::IdleEnd;
use super::plane::presentation_device_pixel_ratio;
use super::plane::{Plane, PlaneSource};
use super::{
    FlingHandoff, FlingStep, NestedScrollConnection, OverscrollBehavior, ScrollCause, ScrollDevice,
    ScrollEndReason, ScrollStep,
};
use crate::scroll::scroll_owner::ScrollOwner;

/// What sits at one place of a scroll chain.
#[derive(Clone)]
pub(crate) enum Participant {
    /// A [`NestedScroll`](super::NestedScroll)'s user connection.
    Connection(Weak<dyn NestedScrollConnection>),
    /// A scrollable: it takes remainders on its own axis and runs fling
    /// residuals it accepts.
    Scrollable(Weak<ScrollOwner>),
}

/// One place in the chain above a scroll leaf. A node is published to its
/// subtree and kept while its participant and parent stay the same, so its
/// identity is the chain's identity.
pub(crate) struct ChainNode {
    participant: Participant,
    parent: Option<Rc<ChainNode>>,
    /// The participant's own coordinate plane.
    plane: PlaneSource,
    /// Sequences this node's connection was told started and not yet ended,
    /// each with how many snapshots hold it: two scrollables on one contact
    /// (a press holding both a moving list and the list moving it) put the
    /// same connection in two snapshots, which must hear it once.
    live: RefCell<SmallVec<[(InputSequenceId, u32); 2]>>,
    /// The node of the owner a scrollable's owner retired into on the same
    /// position: a snapshot that holds this node reaches the successor
    /// through it.
    successor: RefCell<Option<Rc<ChainNode>>>,
}

impl std::fmt::Debug for ChainNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.participant {
            Participant::Connection(_) => "connection",
            Participant::Scrollable(_) => "scrollable",
        };
        f.debug_struct("ChainNode")
            .field("participant", &kind)
            .field("has_parent", &self.parent.is_some())
            .field("plane", &self.plane)
            .field("live_sequences", &self.live.borrow().len())
            .field("handed_on", &self.successor.borrow().is_some())
            .finish()
    }
}

impl ChainNode {
    /// A place for `participant` under `parent`, whose deltas are local to
    /// `plane`.
    pub(crate) fn new(
        participant: Participant,
        parent: Option<Rc<ChainNode>>,
        plane: PlaneSource,
    ) -> Self {
        Self {
            participant,
            parent,
            plane,
            live: RefCell::new(SmallVec::new()),
            successor: RefCell::new(None),
        }
    }

    pub(crate) fn plane(&self) -> &PlaneSource {
        &self.plane
    }

    /// The scrollable owner this node stands for now: its own while it
    /// serves, else the one it retired into on the same position. None for
    /// a connection, or once no owner serves the position.
    pub(crate) fn owner(&self) -> Option<Rc<ScrollOwner>> {
        let Participant::Scrollable(owner) = &self.participant else {
            return None;
        };
        if let Some(owner) = owner.upgrade().filter(|owner| owner.is_alive()) {
            return Some(owner);
        }
        let successor = self.successor.borrow().clone();
        successor?.owner()
    }

    /// This node's owner retired into the owner of `successor` on the same
    /// position.
    pub(crate) fn hand_to(&self, successor: &Rc<ChainNode>) {
        let outgoing = self.successor.replace(Some(Rc::clone(successor)));
        drop(outgoing);
    }

    /// A snapshot holds `sequence` now. True for the first of them: the
    /// connection is told the sequence started then.
    fn acquire(&self, sequence: InputSequenceId) -> bool {
        let mut live = self.live.borrow_mut();
        if let Some((_, holders)) = live.iter_mut().find(|(id, _)| *id == sequence) {
            *holders += 1;
            return false;
        }
        live.push((sequence, 1));
        true
    }

    /// A snapshot lets go of `sequence`. True for the last of them: the
    /// connection is told the sequence ended then.
    fn release(&self, sequence: InputSequenceId) -> bool {
        let mut live = self.live.borrow_mut();
        let Some(index) = live.iter().position(|(id, _)| *id == sequence) else {
            return false;
        };
        live[index].1 -= 1;
        if live[index].1 > 0 {
            return false;
        }
        live.swap_remove(index);
        true
    }

    /// Whether this node carries exactly `connection`.
    pub(crate) fn is_connection(&self, connection: &Weak<dyn NestedScrollConnection>) -> bool {
        matches!(&self.participant, Participant::Connection(own) if Weak::ptr_eq(own, connection))
    }

    /// Whether this node hangs under exactly `parent`.
    pub(crate) fn has_parent(&self, parent: Option<&Rc<ChainNode>>) -> bool {
        match (&self.parent, parent) {
            (None, None) => true,
            (Some(own), Some(parent)) => Rc::ptr_eq(own, parent),
            _ => false,
        }
    }

    pub(crate) fn parent(&self) -> Option<&Rc<ChainNode>> {
        self.parent.as_ref()
    }

    /// Whether this node is a user connection. As the parent of a
    /// scrollable's node, it says a connection sits between the scrollable
    /// and the nearest one enclosing it: no listener of its own asks it for
    /// a wheel packet, so the scrollable beneath offers the packet for it.
    pub(crate) fn is_user_connection(&self) -> bool {
        matches!(self.participant, Participant::Connection(_))
    }
}

/// Publishes the nearest chain node to a subtree.
#[derive(Clone, Debug)]
pub(crate) struct NestedScrollScope {
    node: Rc<ChainNode>,
    child: BoxedView,
}

impl NestedScrollScope {
    pub(crate) fn new(node: Rc<ChainNode>, child: impl IntoView) -> Self {
        Self {
            node,
            child: child.into_view().boxed(),
        }
    }

    /// The chain node above `ctx`, depending on it: a scrollable rebuilt
    /// under a new chain closes its old participation.
    pub(crate) fn parent_of(ctx: &dyn BuildContext) -> Option<Rc<ChainNode>> {
        ctx.depend_on::<Self, _>(|scope| Rc::clone(&scope.node))
    }
}

impl InheritedView for NestedScrollScope {
    type Data = Rc<ChainNode>;

    fn data(&self) -> &Self::Data {
        &self.node
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.node, &old.node)
    }
}

flui_view::impl_inherited_view!(NestedScrollScope);

/// The leaf a snapshot is taken for: the scrollable (none for a leaf that
/// is no scrollable), its plane and its remainder policy.
struct SnapshotLeaf {
    owner: Weak<ScrollOwner>,
    plane: PlaneSource,
    overscroll: OverscrollBehavior,
}

/// One participant of a snapshot; a link closes when its participant
/// panicked, and is skipped from then on.
struct Link {
    node: Rc<ChainNode>,
    participant: Participant,
    closed: Cell<bool>,
    /// A scrollable this sequence moved through the channel and has not
    /// settled yet: the sequence owes it a settle at its release, or at its
    /// end without one. Set before the scrollable publishes, so a failure
    /// while it does keeps the debt.
    owed: Cell<bool>,
    /// The participant's plane as last sampled, for a step a frame
    /// dispatches while its layout is checked out.
    plane: Cell<Option<Plane>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// A user contact is down, or a contactless step may still arrive.
    Live,
    /// The last user contact lifted; only runs remain.
    Released,
    /// Every participant has been told.
    Ended,
}

/// Where a fling residual went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handoff {
    /// A scrollable runs it under the same sequence.
    Running,
    /// Nobody runs it here any more: declined everywhere, or a connection
    /// took it over.
    Over,
}

/// A scrollable's answer to a fling residual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acceptance {
    /// It runs the residual.
    Running,
    /// Saturated toward the residual, orthogonal, or retired.
    Declined,
    /// Its own boundary probe moved it: the residual is stale.
    Invalidated,
}

/// The answer of one user callout.
enum Called<R> {
    Answered(R),
    /// The link is closed or its connection is gone.
    Absent,
    Panicked,
}

/// One input sequence as one leaf sees it: the chain snapshotted when it
/// started, its per-axis latch and its lifecycle.
///
/// Positions name a place in the chain: `0` is the leaf the sequence started
/// on, `i` is the `i`-th participant above it. A run a scrollable accepted
/// dispatches from that scrollable's position, so the chain above it is the
/// rest of the same snapshot.
pub(crate) struct ScrollSequence {
    id: InputSequenceId,
    device: ScrollDevice,
    contact: bool,
    /// The scrollable the sequence started on, or the successor a rebuild
    /// handed it to.
    leaf: RefCell<Weak<ScrollOwner>>,
    /// Nearest first.
    links: SmallVec<[Link; 4]>,
    /// How many links a remainder or residual may reach: up to and
    /// including the nearest [`OverscrollBehavior::Contain`] scrollable.
    post_end: usize,
    handoff: RemainderHandoff,
    /// Per axis (`x`, `y`) of the leaf's plane: the position the remainder is
    /// latched to. The leaf's plane, not the presentation's, so a motion along
    /// one of the leaf's axes latches that axis alone however the leaf is
    /// rotated.
    latch: [Cell<Option<usize>>; 2],
    phase: Cell<Phase>,
    /// What the sequence ends with once its last run comes to rest.
    end_reason: Cell<ScrollEndReason>,
    /// Runs driving the sequence now, on whichever scrollables took them.
    runs: Cell<u32>,
    /// A press on a moving scrollable holds this contact (ADR-0208 rule 4).
    held: Cell<bool>,
    /// The leaf's drag never started: the arena gave the contact away.
    drag_lost: Cell<bool>,
    /// A wheel or trackpad sequence's idle end, armed at its latch's lapse.
    idle_end: RefCell<Option<IdleEnd>>,
    /// The leaf's plane, and the plane it had when last sampled.
    leaf_plane: PlaneSource,
    leaf_sample: Cell<Option<Plane>>,
    /// Where the input points, in the presentation: the contact or the
    /// pointer, else the middle of the leaf. A run keeps its release's.
    focal: Cell<Option<Point>>,
    /// The presentation's pixel ratio at the sequence's latest user input:
    /// a run keeps its release's.
    device_pixel_ratio: Cell<f64>,
}

impl std::fmt::Debug for ScrollSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollSequence")
            .field("id", &self.id)
            .field("device", &self.device)
            .field("links", &self.links.len())
            .field("phase", &self.phase.get())
            .finish_non_exhaustive()
    }
}

impl ScrollSequence {
    /// Snapshot the chain above `leaf` for sequence `id`. A `contact`
    /// sequence lasts until its contact lifts; a contactless one until its
    /// input ends (a key's release, a wheel latch's lapse) or the next
    /// sequence reaches its leaf. Nobody is told until
    /// [`start`](Self::start).
    pub(crate) fn new(
        leaf: &Rc<ScrollOwner>,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        Self::snapshot(
            SnapshotLeaf {
                owner: Rc::downgrade(leaf),
                plane: leaf.node().plane().clone(),
                overscroll: leaf.overscroll(),
            },
            leaf.node().parent(),
            id,
            device,
            contact,
            handoff,
        )
    }

    /// Snapshot the chain from `parent` up for sequence `id` of a leaf that
    /// is no scrollable (an `InteractiveViewer`), whose deltas are local to
    /// `plane`: it consumes through the closure it passes to
    /// [`dispatch`](Self::dispatch) and hands its remainder to everything
    /// above.
    pub(crate) fn above(
        parent: Option<&Rc<ChainNode>>,
        plane: PlaneSource,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        Self::snapshot(
            SnapshotLeaf {
                owner: Weak::new(),
                plane,
                overscroll: OverscrollBehavior::Auto,
            },
            parent,
            id,
            device,
            contact,
            handoff,
        )
    }

    fn snapshot(
        leaf: SnapshotLeaf,
        parent: Option<&Rc<ChainNode>>,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        let mut links = SmallVec::new();
        let mut post_end = None;
        if leaf.overscroll == OverscrollBehavior::Contain {
            post_end = Some(0);
        }
        let mut node = parent;
        while let Some(current) = node {
            if post_end.is_none()
                && current
                    .owner()
                    .is_some_and(|owner| owner.overscroll() == OverscrollBehavior::Contain)
            {
                post_end = Some(links.len() + 1);
            }
            links.push(Link {
                node: Rc::clone(current),
                participant: current.participant.clone(),
                closed: Cell::new(false),
                owed: Cell::new(false),
                plane: Cell::new(None),
            });
            node = current.parent();
        }
        let post_end = post_end.unwrap_or(links.len());
        let device_pixel_ratio = presentation_device_pixel_ratio(leaf.plane.pipeline().as_ref());
        Rc::new(Self {
            id,
            device,
            contact,
            leaf: RefCell::new(leaf.owner),
            leaf_plane: leaf.plane,
            leaf_sample: Cell::new(None),
            focal: Cell::new(None),
            device_pixel_ratio: Cell::new(device_pixel_ratio),
            links,
            post_end,
            handoff,
            latch: [Cell::new(None), Cell::new(None)],
            phase: Cell::new(Phase::Live),
            end_reason: Cell::new(ScrollEndReason::Released),
            runs: Cell::new(0),
            held: Cell::new(false),
            drag_lost: Cell::new(false),
            idle_end: RefCell::new(None),
        })
    }

    /// Tell every participant the sequence started. A programmatic sequence
    /// first leaves out every participant that does not accept one: such a
    /// link is closed before anyone is told, so it hears nothing of it.
    pub(crate) fn start(&self, recovery: &mut PanicRecovery) {
        self.pass(recovery, |recovery| {
            for index in 0..self.links.len() {
                if self.device == ScrollDevice::Programmatic
                    && !self.accepts_programmatic(index, recovery)
                {
                    self.links[index].closed.set(true);
                    continue;
                }
                if !self.links[index].node.acquire(self.id) {
                    continue;
                }
                self.call(index, recovery, |connection| {
                    connection.sequence_started(self.id);
                });
            }
        });
    }

    /// Whether the participant at link `index` takes part in a programmatic
    /// sequence. A scrollable never does: it moves for a reveal or a jump
    /// in a sequence of its own (ADR-0209 §3). A connection says so itself;
    /// one that panics while it answers declines, and the first failure
    /// stays in `recovery`.
    fn accepts_programmatic(&self, index: usize, recovery: &mut PanicRecovery) -> bool {
        let Participant::Connection(connection) = &self.links[index].participant else {
            return false;
        };
        let Some(connection) = connection.upgrade() else {
            return false;
        };
        let mut answer = None;
        recovery.run(|| answer = Some(connection.accepts_programmatic()));
        if answer.is_none() {
            tracing::error!(
                sequence = self.id.get(),
                "a nested scroll participant panicked while asked whether it accepts \
                 programmatic scrolls; it takes no part in the sequence"
            );
        }
        recovery.retire(connection);
        answer.unwrap_or(false)
    }

    pub(crate) fn is_contact(&self) -> bool {
        self.contact
    }

    /// A one-shot contactless input (a jump, an animation command, an
    /// assistive action) delivered its step: the sequence ends once its run
    /// comes to rest. A wheel burst stays live until its latch lapses.
    pub(crate) fn finish_input(&self) {
        if self.phase.get() == Phase::Live {
            self.phase.set(Phase::Released);
        }
    }

    /// The binding's latch of this wheel or trackpad sequence lapses at
    /// `due` unless another packet moves it on: the idle end is armed for
    /// it on `arena`'s owner clock ([`idle_end`](super::idle_end)).
    pub(crate) fn lapse_at(self: &Rc<Self>, arena: &GestureArena, due: EventTime) {
        if self.phase.get() != Phase::Live {
            return;
        }
        let update = {
            let mut idle_end = self.idle_end.borrow_mut();
            if idle_end.as_ref().is_none_or(|armed| !armed.is_on(arena)) {
                // Dropped first: the old arena withdraws its registration
                // before the new one is made.
                drop(idle_end.take());
                *idle_end = IdleEnd::arm(arena, self);
            }
            let Some(timer) = idle_end.as_ref() else {
                return;
            };
            timer.move_to(due)
        };
        IdleEnd::publish(&update, due);
    }

    /// The latch of this contactless sequence lapsed, or its last packet
    /// was dispatched: no further input belongs to it, and it ends
    /// [`Released`](ScrollEndReason::Released) once its last run comes to
    /// rest (at once without one).
    pub(crate) fn lapse(&self, recovery: &mut PanicRecovery) {
        let timer = self.idle_end.borrow_mut().take();
        drop(timer);
        if self.phase.get() != Phase::Live {
            return;
        }
        self.finish_input();
        if !self.has_runs() {
            self.end(self.end_reason(), recovery);
        }
    }

    /// The contact was cancelled: no release, and the sequence ends
    /// [`Cancelled`](ScrollEndReason::Cancelled) after whatever run settles
    /// it.
    pub(crate) fn cancel_contact(&self) {
        if self.phase.get() == Phase::Live {
            self.phase.set(Phase::Released);
        }
        self.end_reason.set(ScrollEndReason::Cancelled);
    }

    pub(crate) fn id(&self) -> InputSequenceId {
        self.id
    }

    pub(crate) fn is_ended(&self) -> bool {
        self.phase.get() == Phase::Ended
    }

    /// A user contact still drives the sequence.
    pub(crate) fn is_live(&self) -> bool {
        self.phase.get() == Phase::Live
    }

    pub(crate) fn has_runs(&self) -> bool {
        self.runs.get() > 0
    }

    pub(crate) fn run_started(&self) {
        self.runs.set(self.runs.get().saturating_add(1));
    }

    pub(crate) fn run_finished(&self) {
        self.runs.set(self.runs.get().saturating_sub(1));
    }

    pub(crate) fn set_end_reason(&self, reason: ScrollEndReason) {
        self.end_reason.set(reason);
    }

    pub(crate) fn end_reason(&self) -> ScrollEndReason {
        self.end_reason.get()
    }

    pub(crate) fn set_held(&self, held: bool) {
        self.held.set(held);
    }

    pub(crate) fn is_held(&self) -> bool {
        self.held.get()
    }

    pub(crate) fn set_drag_lost(&self) {
        self.drag_lost.set(true);
    }

    pub(crate) fn is_drag_lost(&self) -> bool {
        self.drag_lost.get()
    }

    // -- Planes -------------------------------------------------------------

    /// The plane of the participant at `position` (`0` is the leaf),
    /// sampled now, or as last sampled while a frame holds the layout; the
    /// presentation's own until it was ever sampled.
    fn plane_at(&self, position: usize) -> Plane {
        let (source, sample) = match position.checked_sub(1) {
            None => (&self.leaf_plane, &self.leaf_sample),
            Some(index) => {
                let link = &self.links[index];
                (link.node.plane(), &link.plane)
            }
        };
        if let Some(plane) = source.sample() {
            sample.set(Some(plane));
            return plane;
        }
        sample.get().unwrap_or(Plane::ROOT)
    }

    /// Where the input points, in the presentation: the middle of the leaf
    /// until it pointed anywhere.
    fn focal(&self) -> Point {
        if let Some(focal) = self.focal.get() {
            return focal;
        }
        let focal = self
            .plane_at(0)
            .centre()
            .unwrap_or_else(|| Point::new(0.0, 0.0));
        self.focal.set(Some(focal));
        focal
    }

    /// The input points at `local`, in the leaf's plane, from its next step
    /// on: a contact or a wheel's pointer.
    pub(crate) fn aim(&self, local: Point) {
        if !local.is_finite() {
            return;
        }
        let root = self.plane_at(0).to_root_point(local).unwrap_or(local);
        self.focal.set(Some(root));
    }

    /// The presentation's pixel ratio, read again at user input; a run and
    /// a programmatic step keep the one of the input before them.
    fn device_pixel_ratio(&self, cause: ScrollCause, leaf: &Plane) -> f64 {
        if matches!(cause, ScrollCause::UserInput | ScrollCause::Momentum)
            && let Some(ratio) = leaf.device_pixel_ratio()
        {
            self.device_pixel_ratio.set(ratio);
        }
        self.device_pixel_ratio.get()
    }

    fn fling_step(
        &self,
        velocity: Vec2,
        plane: &Plane,
        focal: Point,
        device_pixel_ratio: f64,
    ) -> FlingStep {
        FlingStep {
            sequence: self.id,
            device: self.device,
            available: Velocity::new(Offset::new(velocity.x, velocity.y)),
            focal_point: plane.to_local_point(focal).unwrap_or(focal),
            device_pixel_ratio,
        }
    }

    /// The last user contact lifted with `velocity` (the leaf's plane),
    /// before any run starts: every participant hears it in its own plane,
    /// then every scrollable the sequence moved settles under the same
    /// sequence id, with the release velocity on the axes whose latch it
    /// holds and from rest on the others. The leaf runs motion only on the
    /// axes no ancestor holds ([`held_above`](Self::held_above)).
    pub(crate) fn release(&self, velocity: Vec2, recovery: &mut PanicRecovery) {
        if self.phase.get() != Phase::Live {
            return;
        }
        self.phase.set(Phase::Released);
        let focal = self.focal();
        let leaf = self.plane_at(0);
        let device_pixel_ratio = self.device_pixel_ratio(ScrollCause::UserInput, &leaf);
        // A plane that cannot be crossed releases nothing up the chain.
        let velocity = leaf.to_root_velocity(velocity, focal).unwrap_or(Vec2::ZERO);
        self.pass(recovery, |recovery| {
            for index in 0..self.links.len() {
                let plane = self.plane_at(index + 1);
                let local = plane
                    .to_local_velocity(velocity, focal)
                    .unwrap_or(Vec2::ZERO);
                let step = self.fling_step(local, &plane, focal, device_pixel_ratio);
                self.call(index, recovery, |connection| connection.released(&step));
            }
            self.settle_owed(velocity, recovery);
        });
    }

    /// Tell every participant the sequence is over. Once only. A scrollable
    /// the sequence moved and its release did not settle (none came, or the
    /// sequence was cut short) settles from rest first.
    pub(crate) fn end(&self, reason: ScrollEndReason, recovery: &mut PanicRecovery) {
        if self.phase.replace(Phase::Ended) == Phase::Ended {
            return;
        }
        // Nothing lapses for an ended sequence.
        let timer = self.idle_end.borrow_mut().take();
        drop(timer);
        self.pass(recovery, |recovery| {
            self.settle_owed(Vec2::ZERO, recovery);
            for index in 0..self.links.len() {
                if !self.links[index].node.release(self.id) {
                    continue;
                }
                self.call(index, recovery, |connection| {
                    connection.sequence_ended(self.id, reason);
                });
            }
        });
    }

    /// Settle every scrollable this sequence moved and still owes a settle,
    /// through the owner that serves it now: `velocity` (the presentation's
    /// plane) on the axes whose latch it holds, rest on the others, each in
    /// its own plane. Its own live contact settles it instead. A connection
    /// settles itself from `released`.
    fn settle_owed(&self, velocity: Vec2, recovery: &mut PanicRecovery) {
        for (index, link) in self.links.iter().enumerate() {
            if !link.owed.replace(false) {
                continue;
            }
            let Some(owner) = link.node.owner() else {
                continue;
            };
            let position = Some(index + 1);
            let focal = self.focal();
            // The latch is in the leaf's plane: the velocity crosses into it,
            // keeps the axes this participant holds, and goes on to its own.
            let leaf = self.plane_at(0);
            let held = leaf.to_local_velocity(velocity, focal).unwrap_or(velocity);
            let along = |axis: usize, value: f64| {
                if self.latch[axis].get() == position {
                    value
                } else {
                    0.0
                }
            };
            let held = Vec2::new(along(0, held.x), along(1, held.y));
            let velocity = leaf.to_root_velocity(held, focal).unwrap_or(held);
            let velocity = self
                .plane_at(index + 1)
                .to_local_velocity(velocity, focal)
                .unwrap_or(Vec2::ZERO);
            let _ = owner.settle_owed(self.id, self.device, velocity, recovery);
        }
    }

    /// Dispatch one delta from the runner at `runner`: participants above
    /// take their part root to leaf, `consume` takes the runner's, and the
    /// remainder goes back up within the latch. Returns what everyone
    /// consumed.
    ///
    /// A delta that is not finite is dropped here, before anyone sees it:
    /// finite inputs still make one (a jump across an extent near `f64::MAX`
    /// overflows the difference), and every source dispatches through here,
    /// so no participant or leaf is offered one, no answer is clamped against
    /// a NaN bound and nothing publishes a non-finite offset.
    pub(crate) fn dispatch(
        &self,
        runner: usize,
        cause: ScrollCause,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
        recovery: &mut PanicRecovery,
    ) -> Vec2 {
        if !(available.x.is_finite() && available.y.is_finite()) {
            tracing::warn!(
                available.x,
                available.y,
                "a scroll step that is not finite was dropped"
            );
            return Vec2::ZERO;
        }
        self.pass(recovery, |recovery| {
            self.dispatch_within(runner, cause, available, consume, recovery)
        })
    }

    fn dispatch_within(
        &self,
        runner: usize,
        cause: ScrollCause,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
        recovery: &mut PanicRecovery,
    ) -> Vec2 {
        if self.is_ended() {
            return consume(available);
        }
        let runner = runner.min(self.links.len());
        // Every plane once per step, the runner's at `runner`.
        let planes: SmallVec<[Plane; 5]> = (0..=self.links.len())
            .map(|position| self.plane_at(position))
            .collect();
        let focal = self.focal();
        let device_pixel_ratio = self.device_pixel_ratio(cause, &planes[0]);
        let plane = planes[runner];
        // The step travels in the presentation's plane; each participant
        // is offered it in its own.
        // A runner whose plane maps no way back (a singular transform) could
        // be offered no part of its own step: it takes the step alone.
        let Some(total) = plane
            .to_root_delta(available, focal)
            .filter(|total| plane.to_local_delta(*total, focal).is_some())
        else {
            tracing::debug!(
                sequence = self.id.get(),
                "a scroll runner's plane cannot be crossed; the step reaches no participant"
            );
            return clamp_consumed(consume(available), available, "leaf");
        };
        let mut remaining = total;
        for index in (runner..self.links.len()).rev() {
            if remaining == Vec2::ZERO {
                break;
            }
            let plane = planes[index + 1];
            let Some(offer) = plane
                .to_local_delta(remaining, focal)
                .filter(|offer| *offer != Vec2::ZERO)
            else {
                continue;
            };
            let step = self.step(cause, offer, &plane, focal, device_pixel_ratio);
            let taken = match self.call(index, recovery, |connection| connection.pre_scroll(&step))
            {
                Called::Answered(taken) => {
                    let taken = clamp_consumed(taken, offer, "pre_scroll");
                    root_taken(&plane, taken, offer, remaining, focal)
                }
                Called::Panicked => remaining,
                Called::Absent => Vec2::ZERO,
            };
            remaining = rest_of(remaining - taken, total);
        }
        let frame = LatchFrame {
            plane: planes[0],
            focal,
        };
        let offer = self.latch_mask(&frame, runner, remaining);
        let mut taken = Vec2::ZERO;
        if let Some(local) = plane
            .to_local_delta(offer, focal)
            .filter(|local| *local != Vec2::ZERO)
        {
            let local_taken = clamp_consumed(consume(local), local, "leaf");
            taken = root_taken(&plane, local_taken, local, offer, focal);
        }
        remaining = rest_of(remaining - taken, total);
        let mut at_edge = [false; 2];
        let (taken_latch, offer_latch) = (frame.local(taken), frame.local(offer));
        for (axis, edge) in at_edge.iter_mut().enumerate() {
            let latched = self.latched(axis, runner);
            if component(taken_latch, axis) != 0.0 {
                if latched.is_none() {
                    self.latch[axis].set(Some(runner));
                }
            } else if latched == Some(runner) && component(offer_latch, axis) != 0.0 {
                *edge = true;
            }
        }
        let mut below = total - remaining;
        for index in runner..self.post_end {
            if remaining == Vec2::ZERO {
                break;
            }
            let position = index + 1;
            let offer = self.post_offers(&frame, position, runner, at_edge, remaining);
            let plane = planes[position];
            let Some(local) = plane
                .to_local_delta(offer, focal)
                .filter(|local| *local != Vec2::ZERO)
            else {
                continue;
            };
            let step = self.step(cause, local, &plane, focal, device_pixel_ratio);
            let local_taken = match &self.links[index].participant {
                Participant::Connection(_) => {
                    let below = plane.to_local_delta(below, focal).unwrap_or(Vec2::ZERO);
                    match self.call(index, recovery, |connection| {
                        connection.post_scroll(&step, below)
                    }) {
                        Called::Answered(taken) => clamp_consumed(taken, local, "post_scroll"),
                        Called::Panicked => local,
                        Called::Absent => Vec2::ZERO,
                    }
                }
                Participant::Scrollable(_) => {
                    let link = &self.links[index];
                    match link.node.owner().filter(|_| !link.closed.get()) {
                        Some(owner) => {
                            // Owed before the owner publishes: a listener
                            // that fails while it does keeps the debt.
                            let owed = link.owed.replace(true);
                            let taken = clamp_consumed(
                                owner.take_remainder(&step, recovery),
                                local,
                                "scrollable",
                            );
                            if taken == Vec2::ZERO && !owed {
                                link.owed.set(false);
                            }
                            taken
                        }
                        None => Vec2::ZERO,
                    }
                }
            };
            let taken = root_taken(&plane, local_taken, local, offer, focal);
            remaining = rest_of(remaining - taken, total);
            below += taken;
            let (taken_latch, offer_latch) = (frame.local(taken), frame.local(offer));
            for (axis, edge) in at_edge.iter_mut().enumerate() {
                let latched = self.latched(axis, runner);
                if component(taken_latch, axis) != 0.0 {
                    if latched.is_none() || *edge {
                        // The first consumer latches; past a latched edge,
                        // the remainder moves up, never back.
                        self.latch[axis].set(Some(position));
                        *edge = false;
                    }
                } else if latched == Some(position) && component(offer_latch, axis) != 0.0 {
                    *edge = true;
                }
            }
        }
        // What everyone took, as the runner sees it.
        plane
            .to_local_delta(total - remaining, focal)
            .unwrap_or(Vec2::ZERO)
    }

    /// Offer a fling residual of `velocity` (the plane of the runner at
    /// `from`) to the participants above it, nearest first, each in its own
    /// plane and projected onto each scrollable's axis.
    pub(crate) fn hand_off(
        self: &Rc<Self>,
        from: usize,
        velocity: Vec2,
        dpr: f64,
        recovery: &mut PanicRecovery,
    ) -> Handoff {
        let focal = self.focal();
        let Some(velocity) = self
            .plane_at(from.min(self.links.len()))
            .to_root_velocity(velocity, focal)
        else {
            return Handoff::Over;
        };
        self.pass(recovery, |recovery| {
            for index in from.min(self.post_end)..self.post_end {
                if self.links[index].closed.get() {
                    continue;
                }
                let plane = self.plane_at(index + 1);
                // A plane that cannot be crossed declines.
                let Some(local) = plane.to_local_velocity(velocity, focal) else {
                    continue;
                };
                match &self.links[index].participant {
                    Participant::Connection(_) => {
                        let step = self.fling_step(local, &plane, focal, dpr);
                        if let Called::Answered(FlingHandoff::Accepted) =
                            self.call(index, recovery, |connection| connection.post_fling(&step))
                        {
                            break;
                        }
                    }
                    Participant::Scrollable(_) => {
                        match self.links[index]
                            .node
                            .owner()
                            .map(|owner| owner.accept_fling(self, index + 1, local, dpr, recovery))
                        {
                            Some(Acceptance::Running) => return Handoff::Running,
                            // Reentrant user code moved the owner while it
                            // was asked: the residual is stale for everyone
                            // above.
                            Some(Acceptance::Invalidated) => break,
                            Some(Acceptance::Declined) | None => {}
                        }
                    }
                }
            }
            Handoff::Over
        })
    }

    /// Per axis of the leaf's own plane (`x`, `y`), whether the leaf itself
    /// holds the latch.
    pub(crate) fn leaf_holds(&self) -> [bool; 2] {
        [0, 1].map(|axis| self.latch[axis].get() == Some(0))
    }

    /// Whether a participant above the leaf holds the latch on the leaf's
    /// own `axis` (`0` is `x`): the leaf then runs no motion of its own on
    /// that axis, which settles where the latch is.
    pub(crate) fn held_above(&self, axis: usize) -> bool {
        self.latch[axis].get().is_some_and(|position| position > 0)
    }

    /// The step a participant in `plane` is offered: `available` in that
    /// plane, the presentation's `focal` point mapped into it.
    fn step(
        &self,
        cause: ScrollCause,
        available: Vec2,
        plane: &Plane,
        focal: Point,
        device_pixel_ratio: f64,
    ) -> ScrollStep {
        ScrollStep {
            sequence: self.id,
            cause,
            device: self.device,
            contact: self.contact,
            available,
            focal_point: plane.to_local_point(focal).unwrap_or(focal),
            device_pixel_ratio,
        }
    }

    /// The axis's latch as `runner` sees it: a participant below a runner
    /// that took over the motion no longer holds it.
    fn latched(&self, axis: usize, runner: usize) -> Option<usize> {
        self.latch[axis].get().map(|position| position.max(runner))
    }

    /// What the runner may take: nothing on an axis latched above it.
    fn latch_mask(&self, frame: &LatchFrame, runner: usize, remaining: Vec2) -> Vec2 {
        let local = frame.local(remaining);
        let open = |axis: usize, value: f64| match self.latched(axis, runner) {
            Some(position) if position != runner => 0.0,
            _ => value,
        };
        let open = Vec2::new(open(0, local.x), open(1, local.y));
        if open == local {
            remaining
        } else {
            frame.root(open)
        }
    }

    /// What the participant at `position` is offered of `remaining`: each
    /// axis of the latch's frame by [`post_offer`](Self::post_offer).
    fn post_offers(
        &self,
        frame: &LatchFrame,
        position: usize,
        runner: usize,
        at_edge: [bool; 2],
        remaining: Vec2,
    ) -> Vec2 {
        let local = frame.local(remaining);
        let offered = Vec2::new(
            self.post_offer(0, position, runner, at_edge[0], local.x),
            self.post_offer(1, position, runner, at_edge[1], local.y),
        );
        if offered == local {
            remaining
        } else {
            frame.root(offered)
        }
    }

    /// What the participant at `position` is offered on `axis`.
    fn post_offer(
        &self,
        axis: usize,
        position: usize,
        runner: usize,
        at_edge: bool,
        remaining: f64,
    ) -> f64 {
        if remaining == 0.0 {
            return 0.0;
        }
        match self.latched(axis, runner) {
            None => remaining,
            Some(latched) if latched == position => remaining,
            Some(latched)
                if at_edge
                    && latched < position
                    && self.handoff == RemainderHandoff::AtEdgeWithinSequence =>
            {
                remaining
            }
            Some(_) => 0.0,
        }
    }

    /// Run one pass of the sequence through its chain (a dispatch, a
    /// hand-off, a release, its start or end): while it runs, every
    /// scrollable of the chain defers programmatic requests and refuses user
    /// input from inside it. When the outermost pass on a scrollable returns,
    /// the scrollable services what was requested meanwhile, within
    /// `recovery`, before this returns.
    fn pass<R>(
        &self,
        recovery: &mut PanicRecovery,
        body: impl FnOnce(&mut PanicRecovery) -> R,
    ) -> R {
        let scope = PassScope::enter(self.owners());
        let result = body(recovery);
        scope.exit(recovery);
        result
    }

    /// Call the user connection at link `index`, contained. A panic closes
    /// the link and delivers its `sequence_ended(Cancelled)`; the first
    /// failure stays in `recovery`. The connection runs inside a pass, so a
    /// scroll it requests waits for the pass to return.
    fn call<R>(
        &self,
        index: usize,
        recovery: &mut PanicRecovery,
        callout: impl FnOnce(&dyn NestedScrollConnection) -> R,
    ) -> Called<R> {
        let link = &self.links[index];
        if link.closed.get() {
            return Called::Absent;
        }
        let Participant::Connection(connection) = &link.participant else {
            return Called::Absent;
        };
        let Some(connection) = connection.upgrade() else {
            return Called::Absent;
        };
        let mut answer = None;
        recovery.run(|| answer = Some(callout(&*connection)));
        let called = if let Some(answer) = answer {
            Called::Answered(answer)
        } else {
            link.closed.set(true);
            tracing::error!(
                sequence = self.id.get(),
                "a nested scroll participant panicked; it consumed the whole step and \
                 leaves the sequence"
            );
            if self.phase.get() != Phase::Ended {
                recovery.run(|| {
                    connection.sequence_ended(self.id, ScrollEndReason::Cancelled);
                });
            }
            Called::Panicked
        };
        recovery.retire(connection);
        called
    }

    /// New input on the leaf cancels what this sequence handed off: every
    /// scrollable of the chain stops the run it drives for it, and the
    /// last one to stop ends the sequence
    /// [`Preempted`](ScrollEndReason::Preempted).
    pub(crate) fn preempt_runs(&self, recovery: &mut PanicRecovery) {
        for owner in self.owners() {
            owner.stop_run_of(self, recovery);
        }
    }

    /// The leaf `retired` handed the sequence to `successor`: its passes
    /// count on the successor from now on. A sequence of another leaf (one
    /// `retired` runs a residual of) keeps its own.
    pub(crate) fn rebase_leaf(&self, retired: &ScrollOwner, successor: &Weak<ScrollOwner>) {
        let mut leaf = self.leaf.borrow_mut();
        if std::ptr::eq(leaf.as_ptr(), retired) {
            *leaf = successor.clone();
        }
    }

    /// The scrollables of the chain: the leaf and every scrollable link,
    /// each through the owner that serves it now.
    fn owners(&self) -> impl Iterator<Item = Rc<ScrollOwner>> + '_ {
        let leaf = self.leaf.borrow().upgrade();
        std::iter::once(leaf)
            .chain(self.links.iter().map(|link| link.node.owner()))
            .flatten()
    }
}

/// The scrollables one pass runs through, counted in for its length
/// ([`ScrollOwner::enter_pass`]). Leaving it on an unwind only counts them
/// out; the normal [`exit`](Self::exit) also lets each scrollable whose
/// last pass this was service what was requested during it.
struct PassScope {
    owners: SmallVec<[Rc<ScrollOwner>; 5]>,
}

impl PassScope {
    fn enter(owners: impl Iterator<Item = Rc<ScrollOwner>>) -> Self {
        let owners: SmallVec<[Rc<ScrollOwner>; 5]> = owners.collect();
        for owner in &owners {
            owner.enter_pass();
        }
        Self { owners }
    }

    fn exit(mut self, recovery: &mut PanicRecovery) {
        let owners = std::mem::take(&mut self.owners);
        for owner in &owners {
            owner.leave_pass();
        }
        for owner in &owners {
            owner.service_deferred(recovery);
        }
    }
}

impl Drop for PassScope {
    fn drop(&mut self) {
        for owner in &self.owners {
            owner.leave_pass();
        }
    }
}

/// What a participant in `plane` that took `taken` of its `offer` (the
/// presentation's `root_offer`) took in the presentation's plane: all of
/// it exactly when it took all, so no rounding is left over.
fn root_taken(plane: &Plane, taken: Vec2, offer: Vec2, root_offer: Vec2, focal: Point) -> Vec2 {
    if taken == offer {
        return root_offer;
    }
    plane.to_root_delta(taken, focal).unwrap_or(Vec2::ZERO)
}

/// `left` of a step of `total`, with the rounding of a conversion, far
/// below the step's own scale, read as nothing left.
fn rest_of(left: Vec2, total: Vec2) -> Vec2 {
    let scale = total.x.abs().max(total.y.abs()) * 1e-12;
    let clear = |value: f64| if value.abs() <= scale { 0.0 } else { value };
    Vec2::new(clear(left.x), clear(left.y))
}

/// The frame a sequence's latch lives in: the leaf's plane at the step's
/// focal point. A remainder crosses into it to be masked per axis and back,
/// so the latch follows the leaf's own axes under a rotated or scaled
/// subtree. A plane that cannot be crossed leaves the vector as it is, and
/// the latch then reads the presentation's axes.
struct LatchFrame {
    plane: Plane,
    focal: Point,
}

impl LatchFrame {
    fn local(&self, root: Vec2) -> Vec2 {
        self.plane.to_local_delta(root, self.focal).unwrap_or(root)
    }

    fn root(&self, local: Vec2) -> Vec2 {
        self.plane.to_root_delta(local, self.focal).unwrap_or(local)
    }
}

fn component(vector: Vec2, axis: usize) -> f64 {
    if axis == 0 { vector.x } else { vector.y }
}

/// Bring a participant's answer within what it was offered, per axis: not
/// finite, pointing the other way or larger than offered is clamped, with a
/// diagnostic.
fn clamp_consumed(taken: Vec2, offered: Vec2, hook: &'static str) -> Vec2 {
    let clamped = Vec2::new(
        clamp_axis(taken.x, offered.x),
        clamp_axis(taken.y, offered.y),
    );
    let changed = |clamped: f64, taken: f64| taken.is_nan() || clamped != taken;
    if changed(clamped.x, taken.x) || changed(clamped.y, taken.y) {
        tracing::warn!(
            hook,
            taken.x,
            taken.y,
            offered.x,
            offered.y,
            "a nested scroll participant consumed outside what it was offered; clamped"
        );
    }
    clamped
}

fn clamp_axis(taken: f64, offered: f64) -> f64 {
    if taken == 0.0 || offered == 0.0 || taken.is_nan() {
        return 0.0;
    }
    if offered > 0.0 {
        taken.clamp(0.0, offered)
    } else {
        taken.clamp(offered, 0.0)
    }
}

#[cfg(test)]
mod tests {
    //! Steady-state dispatch allocates nothing (ADR-0209 §7). The chain and
    //! the owner are private seams; a thread-local counting allocator
    //! measures one owner's drag steps through a chain of participants.

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use flui_animation::AnimationController;
    use flui_foundation::IdMint;
    use flui_foundation::geometry::{Axis, Point, Vec2};
    use flui_interaction::{GestureArena, InputSequenceId, RemainderHandoff};
    use flui_platform::EventTime;
    use flui_platform::pointer::PointerKind;

    use super::{ChainNode, Participant, ScrollSequence};
    use crate::scroll::nested::{
        NestedScrollConnection, OverscrollBehavior, ScrollDevice, ScrollStep,
    };
    use crate::scroll::scroll_owner::{OwnerConfig, OwnerEnv, ScrollOwner};
    use crate::scroll::{ClampingScrollPhysics, ScrollController};

    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    struct Counting;

    // SAFETY: every operation forwards the caller's exact pointer and layout
    // to `System`, which owns the allocations and their failure semantics.
    #[expect(unsafe_code, reason = "a counting allocator wraps System")]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
            // SAFETY: the caller's valid layout, forwarded unchanged.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
            // SAFETY: the caller's valid layout, forwarded unchanged.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
            // SAFETY: `ptr` came from System with `layout`; both and `size`
            // are the caller's valid arguments, forwarded unchanged.
            unsafe { System.realloc(ptr, layout, size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: System allocated `ptr` with `layout`.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    fn allocations() -> usize {
        ALLOCATIONS.with(Cell::get)
    }

    /// Takes a pixel of every step first and leaves the rest.
    struct Nibbler;

    impl NestedScrollConnection for Nibbler {
        fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
            Vec2::new(0.0, step.available.y.signum())
        }
    }

    /// A vertical scroll owner over `max` pixels of content, below `parent`,
    /// with the fling controller that must outlive it.
    fn owner(
        max: f64,
        parent: Option<Rc<ChainNode>>,
    ) -> (Rc<ScrollOwner>, ScrollController, impl Sized) {
        let controller = ScrollController::new();
        controller
            .update_dimensions(300.0, 0.0, max)
            .expect("ordered extents");
        let fling = AnimationController::builder(Duration::from_millis(1))
            .unbounded()
            .build_on(None);
        let owner = ScrollOwner::new(
            OwnerConfig {
                controller: controller.clone(),
                fling: fling.controller().clone(),
                physics: Rc::new(ClampingScrollPhysics::new()),
                snap: None,
                axis: Axis::Vertical,
                reversed: false,
                overscroll: OverscrollBehavior::Auto,
                parent,
            },
            Rc::new(OwnerEnv::default()),
        );
        (owner, controller, fling)
    }

    fn node(participant: Participant, parent: Option<Rc<ChainNode>>) -> Rc<ChainNode> {
        Rc::new(ChainNode::new(
            participant,
            parent,
            super::PlaneSource::default(),
        ))
    }

    /// A presentation that swaps its gesture arena mid-sequence takes the
    /// wheel sequence's idle deadline with it: the next packet arms it on the
    /// new arena and the old one withdraws it, so the sequence still ends at
    /// its latch's lapse instead of with the next sequence.
    #[test]
    fn idle_deadline_follows_the_arena_a_sequence_is_dispatched_on() {
        let sequence = ScrollSequence::above(
            None,
            super::PlaneSource::default(),
            IdMint::<InputSequenceId>::new()
                .next()
                .expect("a fresh mint issues"),
            ScrollDevice::Pointer {
                kind: PointerKind::Mouse,
                device: None,
            },
            false,
            RemainderHandoff::NextSequence,
        );
        let (old, new) = (GestureArena::new(), GestureArena::new());
        let first = EventTime::from_nanos(100_000_000);
        let second = EventTime::from_nanos(200_000_000);

        sequence.lapse_at(&old, first);
        assert_eq!(old.next_deadline(), Some(first), "armed on the first arena");

        sequence.lapse_at(&old, second);
        assert_eq!(old.next_deadline(), Some(second), "a packet moves it on");

        sequence.lapse_at(&new, second);
        assert_eq!(new.next_deadline(), Some(second), "the new arena has it");
        assert_eq!(old.next_deadline(), None, "and the old one has let go");
    }

    /// A drag step through three links above the leaf (a connection, a
    /// scrollable, a connection) allocates nothing: while the leaf moves and
    /// the step's remainder stops there, and once the leaf is at its edge and
    /// the remainder climbs the chain to the outer scrollable.
    #[test]
    fn steady_state_dispatch_through_several_links_allocates_nothing() {
        const STEPS: u32 = 64;
        // Each connection takes a pixel of every 3 px step before the leaf:
        // the leaf moves 1 px, or, with no room, leaves it to the outer list.
        for (leaf_extent, leaf_moves) in [(100_000.0, true), (0.0, false)] {
            let top: Rc<dyn NestedScrollConnection> = Rc::new(Nibbler);
            let near: Rc<dyn NestedScrollConnection> = Rc::new(Nibbler);
            let (outer, outer_controller, _outer_fling) = owner(100_000.0, None);
            let chain = node(
                Participant::Connection(Rc::downgrade(&near)),
                Some(node(
                    Participant::Scrollable(Rc::downgrade(&outer)),
                    Some(node(Participant::Connection(Rc::downgrade(&top)), None)),
                )),
            );
            let (leaf, leaf_controller, _leaf_fling) = owner(leaf_extent, Some(chain));
            let sequence = IdMint::<InputSequenceId>::new()
                .next()
                .expect("a fresh mint issues");
            leaf.begin_contact(Some(sequence), PointerKind::Touch, None);
            for _ in 0..4 {
                leaf.drag(Vec2::new(0.0, 3.0), Point::new(0.0, 0.0));
            }
            let moved = |controller: &ScrollController| controller.pixels();
            let (leaf_before, outer_before) = (moved(&leaf_controller), moved(&outer_controller));
            let control = allocations();
            drop(std::hint::black_box(Box::new(0_u8)));
            assert!(allocations() > control, "the allocator counts");
            let before = allocations();
            for _ in 0..STEPS {
                leaf.drag(Vec2::new(0.0, 3.0), Point::new(0.0, 0.0));
            }
            assert_eq!(
                allocations() - before,
                0,
                "leaf extent {leaf_extent}: a step allocates nothing"
            );
            let (leaf_moved, outer_moved) = (
                moved(&leaf_controller) - leaf_before,
                moved(&outer_controller) - outer_before,
            );
            let expected = if leaf_moves {
                (f64::from(STEPS), 0.0)
            } else {
                (0.0, f64::from(STEPS))
            };
            assert_eq!(
                (leaf_moved, outer_moved),
                expected,
                "leaf extent {leaf_extent}: the step's pixels went where the chain sends them"
            );
        }
    }
}
