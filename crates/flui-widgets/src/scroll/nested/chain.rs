//! The chain a scroll leaf snapshots once per sequence, and the passes that
//! walk it (ADR-0209 §1, §4–§7).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Offset, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_interaction::RemainderHandoff;
use flui_interaction::{InputSequenceId, Velocity};
use flui_view::{BoxedView, BuildContext, BuildContextExt, InheritedView, IntoView, View, ViewExt};
use smallvec::SmallVec;

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
    /// Sequences this node's connection was told started and not yet ended,
    /// each with how many snapshots hold it: two scrollables on one contact
    /// (a press holding both a moving list and the list moving it) put the
    /// same connection in two snapshots, which must hear it once.
    live: RefCell<SmallVec<[(InputSequenceId, u32); 2]>>,
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
            .field("live_sequences", &self.live.borrow().len())
            .finish()
    }
}

impl ChainNode {
    pub(crate) fn new(participant: Participant, parent: Option<Rc<ChainNode>>) -> Self {
        Self {
            participant,
            parent,
            live: RefCell::new(SmallVec::new()),
        }
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

/// One participant of a snapshot; a link closes when its participant
/// panicked, and is skipped from then on.
struct Link {
    node: Rc<ChainNode>,
    participant: Participant,
    closed: Cell<bool>,
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
    leaf: Weak<ScrollOwner>,
    /// Nearest first.
    links: SmallVec<[Link; 4]>,
    /// How many links a remainder or residual may reach: up to and
    /// including the nearest [`OverscrollBehavior::Contain`] scrollable.
    post_end: usize,
    handoff: RemainderHandoff,
    /// Per axis (`x`, `y`): the position the remainder is latched to.
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
    /// sequence lasts until its contact lifts; a contactless one until the
    /// next sequence reaches its leaf. Nobody is told until
    /// [`start`](Self::start).
    pub(crate) fn new(
        leaf: &Rc<ScrollOwner>,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        Self::snapshot(
            Rc::downgrade(leaf),
            leaf.overscroll(),
            leaf.node().parent(),
            id,
            device,
            contact,
            handoff,
        )
    }

    /// Snapshot the chain from `parent` up for sequence `id` of a leaf that
    /// is no scrollable (an `InteractiveViewer`): it consumes through the
    /// closure it passes to [`dispatch`](Self::dispatch) and hands its
    /// remainder to everything above.
    pub(crate) fn above(
        parent: Option<&Rc<ChainNode>>,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        Self::snapshot(
            Weak::new(),
            OverscrollBehavior::Auto,
            parent,
            id,
            device,
            contact,
            handoff,
        )
    }

    fn snapshot(
        leaf: Weak<ScrollOwner>,
        leaf_overscroll: OverscrollBehavior,
        parent: Option<&Rc<ChainNode>>,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        handoff: RemainderHandoff,
    ) -> Rc<Self> {
        let mut links = SmallVec::new();
        let mut post_end = None;
        if leaf_overscroll == OverscrollBehavior::Contain {
            post_end = Some(0);
        }
        let mut node = parent;
        while let Some(current) = node {
            if post_end.is_none()
                && let Participant::Scrollable(owner) = &current.participant
                && owner
                    .upgrade()
                    .is_some_and(|owner| owner.overscroll() == OverscrollBehavior::Contain)
            {
                post_end = Some(links.len() + 1);
            }
            links.push(Link {
                node: Rc::clone(current),
                participant: current.participant.clone(),
                closed: Cell::new(false),
            });
            node = current.parent();
        }
        let post_end = post_end.unwrap_or(links.len());
        Rc::new(Self {
            id,
            device,
            contact,
            leaf,
            links,
            post_end,
            handoff,
            latch: [Cell::new(None), Cell::new(None)],
            phase: Cell::new(Phase::Live),
            end_reason: Cell::new(ScrollEndReason::Released),
            runs: Cell::new(0),
            held: Cell::new(false),
            drag_lost: Cell::new(false),
        })
    }

    /// Tell every participant the sequence started.
    pub(crate) fn start(&self, recovery: &mut PanicRecovery) {
        for index in 0..self.links.len() {
            if !self.links[index].node.acquire(self.id) {
                continue;
            }
            self.call(index, recovery, |connection| {
                connection.sequence_started(self.id);
            });
        }
    }

    pub(crate) fn is_contact(&self) -> bool {
        self.contact
    }

    /// A one-shot contactless input (a jump, an animation command, an
    /// assistive action) delivered its step: the sequence ends once its run
    /// comes to rest. A wheel burst stays live until the next sequence.
    pub(crate) fn finish_input(&self) {
        if self.phase.get() == Phase::Live {
            self.phase.set(Phase::Released);
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

    fn fling_step(&self, velocity: Vec2) -> FlingStep {
        FlingStep {
            sequence: self.id,
            device: self.device,
            available: Velocity::new(Offset::new(velocity.x, velocity.y)),
        }
    }

    /// The last user contact lifted with `velocity`, before any run starts.
    pub(crate) fn release(&self, velocity: Vec2, recovery: &mut PanicRecovery) {
        if self.phase.get() != Phase::Live {
            return;
        }
        self.phase.set(Phase::Released);
        let step = self.fling_step(velocity);
        for index in 0..self.links.len() {
            self.call(index, recovery, |connection| connection.released(&step));
        }
    }

    /// Tell every participant the sequence is over. Once only.
    pub(crate) fn end(&self, reason: ScrollEndReason, recovery: &mut PanicRecovery) {
        if self.phase.replace(Phase::Ended) == Phase::Ended {
            return;
        }
        for index in 0..self.links.len() {
            if !self.links[index].node.release(self.id) {
                continue;
            }
            self.call(index, recovery, |connection| {
                connection.sequence_ended(self.id, reason);
            });
        }
    }

    /// Dispatch one delta from the runner at `runner`: participants above
    /// take their part root to leaf, `consume` takes the runner's, and the
    /// remainder goes back up within the latch. Returns what everyone
    /// consumed.
    pub(crate) fn dispatch(
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
        let mut remaining = available;
        let ancestors = runner.min(self.links.len())..self.links.len();
        for index in ancestors.rev() {
            if remaining == Vec2::ZERO {
                break;
            }
            let step = self.step(cause, remaining);
            let taken = match self.call(index, recovery, |connection| connection.pre_scroll(&step))
            {
                Called::Answered(taken) => clamp_consumed(taken, remaining, "pre_scroll"),
                Called::Panicked => remaining,
                Called::Absent => Vec2::ZERO,
            };
            remaining -= taken;
        }
        let offer = self.latch_mask(runner, remaining);
        let mut taken = Vec2::ZERO;
        if offer != Vec2::ZERO {
            taken = clamp_consumed(consume(offer), offer, "leaf");
        }
        remaining -= taken;
        let mut at_edge = [false; 2];
        for (axis, edge) in at_edge.iter_mut().enumerate() {
            let latched = self.latched(axis, runner);
            if component(taken, axis) != 0.0 {
                if latched.is_none() {
                    self.latch[axis].set(Some(runner));
                }
            } else if latched == Some(runner) && component(offer, axis) != 0.0 {
                *edge = true;
            }
        }
        let mut below = available - remaining;
        for index in runner..self.post_end {
            if remaining == Vec2::ZERO {
                break;
            }
            let position = index + 1;
            let offer = Vec2::new(
                self.post_offer(0, position, runner, at_edge[0], remaining.x),
                self.post_offer(1, position, runner, at_edge[1], remaining.y),
            );
            if offer == Vec2::ZERO {
                continue;
            }
            let step = self.step(cause, offer);
            let taken = match &self.links[index].participant {
                Participant::Connection(_) => {
                    match self.call(index, recovery, |connection| {
                        connection.post_scroll(&step, below)
                    }) {
                        Called::Answered(taken) => clamp_consumed(taken, offer, "post_scroll"),
                        Called::Panicked => offer,
                        Called::Absent => Vec2::ZERO,
                    }
                }
                Participant::Scrollable(owner) => owner
                    .upgrade()
                    .filter(|_| !self.links[index].closed.get())
                    .map_or(Vec2::ZERO, |owner| {
                        clamp_consumed(owner.take_remainder(&step), offer, "scrollable")
                    }),
            };
            remaining -= taken;
            below += taken;
            for (axis, edge) in at_edge.iter_mut().enumerate() {
                let latched = self.latched(axis, runner);
                if component(taken, axis) != 0.0 {
                    if latched.is_none() || *edge {
                        // The first consumer latches; past a latched edge,
                        // the remainder moves up, never back.
                        self.latch[axis].set(Some(position));
                        *edge = false;
                    }
                } else if latched == Some(position) && component(offer, axis) != 0.0 {
                    *edge = true;
                }
            }
        }
        available - remaining
    }

    /// Offer a fling residual of `velocity` to the participants above
    /// `from`, nearest first, projected onto each scrollable's axis.
    pub(crate) fn hand_off(
        self: &Rc<Self>,
        from: usize,
        velocity: Vec2,
        dpr: f64,
        recovery: &mut PanicRecovery,
    ) -> Handoff {
        let step = self.fling_step(velocity);
        let mut outcome = Handoff::Over;
        for index in from.min(self.post_end)..self.post_end {
            if self.links[index].closed.get() {
                continue;
            }
            match &self.links[index].participant {
                Participant::Connection(_) => {
                    if let Called::Answered(FlingHandoff::Accepted) =
                        self.call(index, recovery, |connection| connection.post_fling(&step))
                    {
                        break;
                    }
                }
                Participant::Scrollable(owner) => {
                    match owner
                        .upgrade()
                        .map(|owner| owner.accept_fling(self, index + 1, velocity, dpr, recovery))
                    {
                        Some(Acceptance::Running) => {
                            outcome = Handoff::Running;
                            break;
                        }
                        // Reentrant user code moved the owner while it was
                        // asked: the residual is stale for everyone above.
                        Some(Acceptance::Invalidated) => break,
                        Some(Acceptance::Declined) | None => {}
                    }
                }
            }
        }
        outcome
    }

    /// The release of a leaf whose remainder moved scrollables above it (a
    /// pager a photo's pan moved): each scrollable the latch holds on an axis
    /// settles from where it stands, with the release `velocity` on its
    /// axes, under the same sequence in its own chain. Returns, per axis
    /// (`x`, `y`), whether the leaf itself holds the latch: only there may it
    /// run motion of its own.
    pub(crate) fn settle_latched_ancestors(
        &self,
        velocity: Vec2,
        device_pixel_ratio: f64,
    ) -> [bool; 2] {
        let latched = [self.latch[0].get(), self.latch[1].get()];
        for axis in 0..2 {
            let Some(position) = latched[axis].filter(|position| *position > 0) else {
                continue;
            };
            if axis == 1 && latched[0] == Some(position) {
                // Settled with both axes above.
                continue;
            }
            let link = &self.links[position - 1];
            if link.closed.get() {
                continue;
            }
            let Participant::Scrollable(owner) = &link.participant else {
                // A connection settles itself from `released`.
                continue;
            };
            let Some(owner) = owner.upgrade() else {
                continue;
            };
            let along = |axis: usize, value: f64| {
                if latched[axis] == Some(position) {
                    value
                } else {
                    0.0
                }
            };
            let velocity = Vec2::new(along(0, velocity.x), along(1, velocity.y));
            let _ = owner.settle_from_inside(self.id, self.device, velocity, device_pixel_ratio);
        }
        self.leaf_holds()
    }

    /// Per axis (`x`, `y`), whether the leaf itself holds the latch.
    pub(crate) fn leaf_holds(&self) -> [bool; 2] {
        [self.latch[0].get(), self.latch[1].get()].map(|position| position == Some(0))
    }

    fn step(&self, cause: ScrollCause, available: Vec2) -> ScrollStep {
        ScrollStep {
            sequence: self.id,
            cause,
            device: self.device,
            available,
        }
    }

    /// The axis's latch as `runner` sees it: a participant below a runner
    /// that took over the motion no longer holds it.
    fn latched(&self, axis: usize, runner: usize) -> Option<usize> {
        self.latch[axis].get().map(|position| position.max(runner))
    }

    /// What the runner may take: nothing on an axis latched above it.
    fn latch_mask(&self, runner: usize, remaining: Vec2) -> Vec2 {
        let open = |axis: usize, value: f64| match self.latched(axis, runner) {
            Some(position) if position != runner => 0.0,
            _ => value,
        };
        Vec2::new(open(0, remaining.x), open(1, remaining.y))
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

    /// Call the user connection at link `index`, contained. A panic closes
    /// the link and delivers its `sequence_ended(Cancelled)`; the first
    /// failure stays in `recovery`. While the connection runs, a scroll
    /// dispatched into this chain is refused.
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
        let _refusing = RefuseDispatch::over(self);
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

    fn owners(&self) -> impl Iterator<Item = Rc<ScrollOwner>> + '_ {
        std::iter::once(self.leaf.upgrade())
            .chain(self.links.iter().map(|link| match &link.participant {
                Participant::Scrollable(owner) => owner.upgrade(),
                Participant::Connection(_) => None,
            }))
            .flatten()
    }
}

/// Every scrollable of a chain refuses dispatch while one of its user
/// connections runs: a scroll started from inside a participant would
/// re-enter the pass that called it.
struct RefuseDispatch<'a> {
    sequence: &'a ScrollSequence,
}

impl<'a> RefuseDispatch<'a> {
    fn over(sequence: &'a ScrollSequence) -> Self {
        for owner in sequence.owners() {
            owner.refuse_dispatch(true);
        }
        Self { sequence }
    }
}

impl Drop for RefuseDispatch<'_> {
    fn drop(&mut self) {
        for owner in self.sequence.owners() {
            owner.refuse_dispatch(false);
        }
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
    //! measures one owner's drag steps through a participant.

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use flui_animation::AnimationController;
    use flui_foundation::IdMint;
    use flui_foundation::geometry::{Axis, Vec2};
    use flui_interaction::InputSequenceId;
    use flui_platform_api::pointer::PointerKind;

    use super::{ChainNode, Participant};
    use crate::scroll::nested::{NestedScrollConnection, OverscrollBehavior, ScrollStep};
    use crate::scroll::scroll_owner::{OwnerConfig, OwnerEnv, ScrollOwner};
    use crate::scroll::{ClampingScrollPhysics, ScrollController};

    thread_local! {
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

    #[test]
    fn steady_state_dispatch_allocates_nothing() {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 100_000.0);
        let fling = AnimationController::builder(Duration::from_millis(1))
            .unbounded()
            .build_on(None);
        let connection: Rc<dyn NestedScrollConnection> = Rc::new(Nibbler);
        let parent = Rc::new(ChainNode::new(
            Participant::Connection(Rc::downgrade(&connection)),
            None,
        ));
        let owner = ScrollOwner::new(
            OwnerConfig {
                controller: controller.clone(),
                fling: fling.controller().clone(),
                physics: Arc::new(ClampingScrollPhysics::new()),
                snap: None,
                axis: Axis::Vertical,
                reversed: false,
                overscroll: OverscrollBehavior::Auto,
                parent: Some(parent),
            },
            Rc::new(OwnerEnv::default()),
        );
        let sequence = IdMint::<InputSequenceId>::new()
            .next()
            .expect("a fresh mint issues");
        owner.begin_contact(Some(sequence), PointerKind::Touch, None);
        for _ in 0..4 {
            owner.drag(Vec2::new(0.0, 3.0));
        }
        let control = allocations();
        drop(std::hint::black_box(Box::new(0_u8)));
        assert!(allocations() > control, "the allocator counts");
        let before = allocations();
        for _ in 0..64 {
            owner.drag(Vec2::new(0.0, 3.0));
        }
        assert_eq!(allocations() - before, 0, "a step allocates nothing");
        assert_eq!(controller.pixels(), 68.0 * 2.0, "each step moved the leaf");
    }
}
