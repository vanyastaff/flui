//! A mounted [`Scrollable`](super::Scrollable)'s place in the nested scroll
//! channel (ADR-0209): the leaf that dispatches its own deltas, the
//! participant that takes remainders and fling residuals of the scrollables
//! inside it, and the runner of its own animations.
//!
//! The owner is `Rc` with `Cell` state. Unmounting, or a change of
//! controller, axis, orientation, overscroll behavior or parent chain,
//! retires it; a retirement on the same position hands the contact, the run
//! and the sequences in progress to the successor, so the motion goes on. A
//! new physics or snap policy is taken in place and shapes the next step
//! and release: a build that allocates its physics every time keeps its
//! owner, and the stops of a sheet change without cutting its drag or its
//! settle short. Animation continuations hold the owner that drives their
//! run weakly and check that their own run is still the current one.
//!
//! # Failure custody
//!
//! Every entry point owns one [`PanicRecovery`]: a participant that panics
//! while it is told about a sequence never skips the owner's own
//! bookkeeping (a run attached, a sequence ended). The first failure resumes
//! once the entry point's work is done (ADR-0127).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_animation::simulation::{SpringDescription, SpringSimulation};
use flui_animation::{Animation, AnimationController, AnimationRunFuture, Simulation};
use flui_foundation::geometry::{Axis, Point, Vec2};
use flui_foundation::panic::PanicRecovery;
use flui_interaction::gesture::GestureSettingsProvider;
use flui_interaction::{HoldEnd, InputSequenceId, RemainderHandoff};
use flui_platform_api::keyboard::Code;
use flui_platform_api::pointer::{DeviceId, PointerKind};
use flui_rendering::view::ScrollDirection;

use super::nested::presentation_device_pixel_ratio;
use super::nested::{
    Acceptance, ChainNode, Handoff, OverscrollBehavior, Participant, PlaneSource, ScrollCause,
    ScrollDevice, ScrollEndReason, ScrollSequence, ScrollStep, SequenceSource, SnapPolicy,
};
use super::scroll_controller::PendingScrollCommand;
use super::scroll_physics::rest_slop;
use super::{ScrollController, ScrollMetrics, SharedScrollPhysics};
use flui_interaction::drag::DragIdentity;

/// How many commands one service of an owner's command slot carries out
/// before a request that keeps requesting waits for the next notification.
const DEFERRED_ROUNDS: u32 = 8;

/// A shared snap policy, compared by identity like [`SharedScrollPhysics`].
pub type SharedSnapPolicy = Arc<dyn SnapPolicy>;

/// What every owner a [`Scrollable`](super::Scrollable) state creates reads
/// from its lifecycle: the arena that issues sequence ids and the gesture
/// presets that pick the remainder rule.
#[derive(Default)]
pub(crate) struct OwnerEnv {
    pub(crate) sequences: SequenceSource,
    pub(crate) settings: RefCell<GestureSettingsProvider>,
    /// Told of every offset change the owner makes, with its cause: an
    /// [`AnchoredDrag`](super::AnchoredDrag) tracks its anchors through it.
    pub(crate) motion: RefCell<Option<MotionHook>>,
    /// The scrollable's own plane: the presentation, whose device pixel
    /// ratio a settle the channel starts on this owner reads, and its box.
    pub(crate) plane: PlaneSource,
}

/// Hears an owner move its offset `from` one value `to` another, for `cause`.
pub(crate) type MotionHook = Rc<dyn Fn(f64, f64, ScrollCause)>;

/// Where `snap` rests a release from `metrics` at `velocity`, within the
/// extents: a target that is not finite is replaced by the extent it points
/// past, with a diagnostic.
pub(crate) fn snap_target(snap: &dyn SnapPolicy, metrics: &ScrollMetrics, velocity: f64) -> f64 {
    let target = snap.target(metrics, velocity);
    if target.is_finite() {
        target.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    } else {
        tracing::warn!(
            target,
            "a snap policy chose a target that is not finite; clamped"
        );
        if target > 0.0 {
            metrics.max_scroll_extent
        } else {
            metrics.min_scroll_extent
        }
    }
}

/// The spring a snap settles on `target` with, from `metrics` at
/// `velocity`; none when it is already there or the values admit no spring.
pub(crate) fn settle_spring(
    metrics: &ScrollMetrics,
    target: f64,
    velocity: f64,
) -> Option<SpringSimulation> {
    if !target.is_finite() || target == metrics.pixels {
        return None;
    }
    let spring = SpringDescription::with_damping_ratio(0.5, 100.0, 1.1).ok()?;
    SpringSimulation::new(
        spring,
        metrics.pixels,
        target,
        velocity,
        metrics.ballistic_tolerance()?,
    )
    .ok()
}

/// The configuration an owner is created for; a different one retires it,
/// except for the snap policy, which the owner takes in place.
pub(crate) struct OwnerConfig {
    pub(crate) controller: ScrollController,
    pub(crate) fling: AnimationController,
    pub(crate) physics: SharedScrollPhysics,
    pub(crate) snap: Option<SharedSnapPolicy>,
    pub(crate) axis: Axis,
    pub(crate) reversed: bool,
    pub(crate) overscroll: OverscrollBehavior,
    pub(crate) parent: Option<Rc<ChainNode>>,
}

/// One animation run of the owner's controller, and the cause its deltas
/// carry. Identity, not the run generation, tells a continuation whether it
/// is still current.
struct RunTag {
    /// The owner that drives the run now: the one that started it, or the
    /// successor a rebuild handed it to.
    owner: RefCell<Weak<ScrollOwner>>,
    cause: ScrollCause,
    /// The controller value of the previous tick.
    last: Cell<f64>,
    /// How far the leaf lags the run's own value: what participants took of
    /// its steps. A run that rests at its edge in its own value rests there
    /// for the chain once the lag is added back.
    deflected: Cell<f64>,
}

impl RunTag {
    fn new(owner: Weak<ScrollOwner>, cause: ScrollCause, start: f64) -> Self {
        Self {
            owner: RefCell::new(owner),
            cause,
            last: Cell::new(start),
            deflected: Cell::new(0.0),
        }
    }

    /// Whether the run, as the leaf and the participants shared it, reached
    /// `edge` from where the leaf stands: within half a device pixel, the
    /// slop a ballistic run rests within (ADR-0155), however long the list.
    fn reached(&self, pixels: f64, edge: f64, device_pixel_ratio: f64) -> bool {
        (pixels + self.deflected.get() - edge).abs() <= rest_slop(device_pixel_ratio)
    }
}

/// The sequence the owner's animation and contactless steps belong to, and
/// its position in that sequence's chain.
struct Driving {
    sequence: Rc<ScrollSequence>,
    position: usize,
    run: Option<Rc<RunTag>>,
}

/// What a release hands up the chain once its run rests at an extent:
/// the residual velocity, that extent, and the release's pixel ratio.
#[derive(Clone, Copy)]
struct Residual {
    velocity: f64,
    edge: f64,
    device_pixel_ratio: f64,
}

/// See the [module](self) documentation.
pub(crate) struct ScrollOwner {
    /// This owner, for the runs it starts.
    this: Weak<ScrollOwner>,
    controller: ScrollController,
    fling: AnimationController,
    /// Read at each step and release, never held across the policy's call:
    /// a rebuild with another physics value takes it in place.
    physics: RefCell<SharedScrollPhysics>,
    /// Read at each release, never held across the policy's call.
    snap: RefCell<Option<SharedSnapPolicy>>,
    axis: Axis,
    reversed: bool,
    overscroll: OverscrollBehavior,
    node: Rc<ChainNode>,
    env: Rc<OwnerEnv>,
    alive: Cell<bool>,
    /// Passes of the channel through this owner in progress (ADR-0209 §3):
    /// while nonzero, a programmatic request on its controller waits in the
    /// controller's command slot and user input dispatched from inside is
    /// refused.
    passes: Cell<u32>,
    /// The slot is being serviced: a request the service makes waits for
    /// the same loop.
    servicing: Cell<bool>,
    /// The live contact sequence: from a hold or a drag start until the
    /// contact lifts.
    contact: RefCell<Option<Rc<ScrollSequence>>>,
    driving: RefCell<Option<Driving>>,
    /// The key that holds the keyboard sequence open.
    keyboard: RefCell<Option<KeyHold>>,
    /// The drag autoscrolling this leaf, and the sequence its frames step.
    autoscroll: RefCell<Option<(DragIdentity, Rc<ScrollSequence>)>>,
}

/// The key whose press began a keyboard sequence; its repeats continue the
/// sequence and its release ends it.
#[derive(Clone)]
struct KeyHold {
    code: Code,
    sequence: Rc<ScrollSequence>,
}

impl std::fmt::Debug for ScrollOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollOwner")
            .field("axis", &self.axis)
            .field("reversed", &self.reversed)
            .field("alive", &self.alive.get())
            .finish_non_exhaustive()
    }
}

impl ScrollOwner {
    pub(crate) fn new(config: OwnerConfig, env: Rc<OwnerEnv>) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| Self {
            this: this.clone(),
            node: Rc::new(ChainNode::new(
                Participant::Scrollable(this.clone()),
                config.parent,
                env.plane.clone(),
            )),
            controller: config.controller,
            fling: config.fling,
            physics: RefCell::new(config.physics),
            snap: RefCell::new(config.snap),
            axis: config.axis,
            reversed: config.reversed,
            overscroll: config.overscroll,
            env,
            alive: Cell::new(true),
            passes: Cell::new(0),
            servicing: Cell::new(false),
            contact: RefCell::new(None),
            driving: RefCell::new(None),
            keyboard: RefCell::new(None),
            autoscroll: RefCell::new(None),
        })
    }

    /// Whether this owner was created for `config`. The physics and the
    /// snap policy are not part of it: the owner takes them in place.
    pub(crate) fn serves(&self, config: &OwnerConfig) -> bool {
        self.alive.get()
            && self
                .controller
                .position()
                .ptr_eq(&config.controller.position())
            && self.axis == config.axis
            && self.reversed == config.reversed
            && self.overscroll == config.overscroll
            && self.node.has_parent(config.parent.as_ref())
    }

    pub(crate) fn node(&self) -> &Rc<ChainNode> {
        &self.node
    }

    /// Settle the next release through `snap`; a run in progress keeps the
    /// target it was started for.
    pub(crate) fn set_snap(&self, snap: Option<SharedSnapPolicy>) {
        let outgoing = self.snap.replace(snap);
        // A user policy's destructor runs with no borrow held.
        drop(outgoing);
    }

    fn snap(&self) -> Option<SharedSnapPolicy> {
        self.snap.borrow().clone()
    }

    /// Bound the next step and shape the next release with `physics`; a
    /// run in progress keeps the simulation it was started with.
    pub(crate) fn set_physics(&self, physics: SharedScrollPhysics) {
        if Arc::ptr_eq(&self.physics.borrow(), &physics) {
            return;
        }
        let outgoing = self.physics.replace(physics);
        // A user physics' destructor runs with no borrow held.
        drop(outgoing);
    }

    fn physics(&self) -> SharedScrollPhysics {
        self.physics.borrow().clone()
    }

    /// Move the offset to `pixels` for `cause`, and tell the motion hook.
    fn publish(&self, pixels: f64, cause: ScrollCause) {
        let from = self.controller.pixels();
        self.controller.set_pixels(pixels);
        let to = self.controller.pixels();
        if from == to {
            return;
        }
        let hook = self.env.motion.borrow().clone();
        if let Some(hook) = hook {
            hook(from, to, cause);
        }
    }

    pub(crate) fn overscroll(&self) -> OverscrollBehavior {
        self.overscroll
    }

    pub(crate) fn axis(&self) -> Axis {
        self.axis
    }

    pub(crate) fn controller(&self) -> &ScrollController {
        &self.controller
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.get()
    }

    /// The device pixel ratio of the presentation this owner scrolls in.
    fn device_pixel_ratio(&self) -> f64 {
        presentation_device_pixel_ratio(self.env.plane.pipeline().as_ref())
    }

    /// A pass of the channel through this owner begins.
    pub(crate) fn enter_pass(&self) {
        self.passes.set(self.passes.get().saturating_add(1));
    }

    /// A pass of the channel through this owner returned.
    pub(crate) fn leave_pass(&self) {
        self.passes.set(self.passes.get().saturating_sub(1));
    }

    /// Whether a pass of the channel through this owner runs now.
    pub(crate) fn in_pass(&self) -> bool {
        self.passes.get() > 0
    }

    /// Run `body` as a pass through this owner alone: a participant this
    /// owner is behind (an `AnchoredDrag` as an ancestor) runs inside the
    /// pass of a chain this owner is not a link of.
    pub(crate) fn within_pass<R>(
        self: &Rc<Self>,
        recovery: &mut PanicRecovery,
        body: impl FnOnce(&mut PanicRecovery) -> R,
    ) -> R {
        struct Leave<'a>(&'a ScrollOwner);
        impl Drop for Leave<'_> {
            fn drop(&mut self) {
                self.0.leave_pass();
            }
        }
        self.enter_pass();
        let result = {
            let _leave = Leave(self);
            body(recovery)
        };
        self.service_deferred(recovery);
        result
    }

    /// Whether user input dispatched into this owner now must be refused:
    /// it came from inside a pass through it, which it would re-enter.
    fn refused(&self) -> bool {
        if !self.in_pass() {
            return false;
        }
        tracing::error!(
            "user input was dispatched from inside a scroll pass into the chain that runs it; \
             refused"
        );
        true
    }

    /// Service what the controller's command slot holds, once no pass
    /// through this owner runs: a jump, an animation or a cancel requested
    /// during one. A request the service itself makes (a listener that
    /// jumps on every change) is serviced by the same loop, up to
    /// [`DEFERRED_ROUNDS`] commands; past that the latest waits in the slot
    /// for the next notification, with a diagnostic, and is never dropped.
    pub(crate) fn service_deferred(self: &Rc<Self>, recovery: &mut PanicRecovery) {
        if !self.alive.get() || self.in_pass() || self.servicing.replace(true) {
            return;
        }
        struct Done<'a>(&'a Cell<bool>);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _done = Done(&self.servicing);
        for _ in 0..DEFERRED_ROUNDS {
            if !self.alive.get() {
                return;
            }
            let Some(command) = self.controller.take_pending_command() else {
                return;
            };
            self.service_command(command, recovery);
        }
        if self.controller.has_pending_command() {
            tracing::warn!(
                rounds = DEFERRED_ROUNDS,
                "scroll requests kept requesting scrolls; the latest waits for the next \
                 notification"
            );
        }
    }

    /// Close the owner for `successor`, the owner of the same scrollable's
    /// new configuration. On the same position, everything the owner
    /// accepted goes on as the successor's, under the sequences it began
    /// (each keeps the chain it snapshotted): a contact still down, the run
    /// in progress and the motion it drives, a held key, a drop autoscroll.
    /// A rebuild in the middle of a drag keeps the drag, and one in the
    /// middle of a fling keeps the fling and its bounce. Moving them runs no
    /// participant.
    pub(crate) fn retire_into(&self, successor: &ScrollOwner) {
        if self
            .controller
            .position()
            .ptr_eq(&successor.controller.position())
        {
            let contact = self.contact.borrow_mut().take();
            let driving = self.driving.borrow_mut().take();
            let keyboard = self.keyboard.borrow_mut().take();
            let autoscroll = self.autoscroll.borrow_mut().take();
            let sequences = contact
                .iter()
                .chain(driving.iter().map(|driving| &driving.sequence))
                .chain(keyboard.iter().map(|held| &held.sequence))
                .chain(autoscroll.iter().map(|(_, sequence)| sequence));
            for sequence in sequences {
                sequence.rebase_leaf(self, &successor.this);
            }
            // A sequence of a scrollable inside this one reaches the
            // successor through this node: the remainders it takes and the
            // settle it is owed follow the position.
            self.node.hand_to(&successor.node);
            if let Some(run) = driving.as_ref().and_then(|driving| driving.run.as_ref()) {
                *run.owner.borrow_mut() = successor.this.clone();
            }
            *successor.contact.borrow_mut() = contact;
            *successor.driving.borrow_mut() = driving;
            *successor.keyboard.borrow_mut() = keyboard;
            *successor.autoscroll.borrow_mut() = autoscroll;
        }
        self.retire();
    }

    /// Close the owner: no new run, remainder or residual reaches it, and
    /// the sequences it holds end [`Cancelled`](ScrollEndReason::Cancelled).
    pub(crate) fn retire(&self) {
        if !self.alive.replace(false) {
            return;
        }
        let mut recovery = PanicRecovery::new();
        // Every slot is emptied before a settle tells a participant: no
        // borrow is held while user code runs.
        let contact = self.contact.borrow_mut().take();
        let driving = self.driving.borrow_mut().take();
        let autoscroll = self.autoscroll.borrow_mut().take();
        let keyboard = self.keyboard.borrow_mut().take();
        if let Some((_, sequence)) = autoscroll {
            sequence.finish_input();
            sequence.set_end_reason(ScrollEndReason::Cancelled);
            Self::settle(&sequence, ScrollEndReason::Cancelled, &mut recovery);
        }
        if let Some(held) = keyboard {
            held.sequence.finish_input();
            held.sequence.set_end_reason(ScrollEndReason::Cancelled);
            Self::settle(&held.sequence, ScrollEndReason::Cancelled, &mut recovery);
        }
        if let Some(contact) = contact {
            contact.cancel_contact();
            Self::settle(&contact, ScrollEndReason::Cancelled, &mut recovery);
        }
        if let Some(driving) = driving {
            if driving.run.is_some() {
                driving.sequence.run_finished();
            }
            if !driving.sequence.has_runs() {
                driving
                    .sequence
                    .end(ScrollEndReason::Cancelled, &mut recovery);
            }
        }
        recovery.finish();
    }

    // -- Geometry -----------------------------------------------------------

    /// This owner's axis as a channel axis index (`0` is `x`).
    fn axis_index(&self) -> usize {
        match self.axis {
            Axis::Horizontal => 0,
            Axis::Vertical => 1,
        }
    }

    /// The part of a physical vector along this owner's offset.
    fn to_scroll(&self, physical: Vec2) -> f64 {
        let along = match self.axis {
            Axis::Vertical => physical.y,
            Axis::Horizontal => physical.x,
        };
        if self.reversed { -along } else { along }
    }

    /// A change of this owner's offset as a physical vector.
    fn to_physical(&self, scroll: f64) -> Vec2 {
        let along = if self.reversed { -scroll } else { scroll };
        match self.axis {
            Axis::Vertical => Vec2::new(0.0, along),
            Axis::Horizontal => Vec2::new(along, 0.0),
        }
    }

    /// The physical delta of a finger moving `finger` along this owner's
    /// axis: the content follows the finger, so the offset moves against it.
    pub(crate) fn finger_to_physical(&self, finger: f64) -> Vec2 {
        match self.axis {
            Axis::Vertical => Vec2::new(0.0, -finger),
            Axis::Horizontal => Vec2::new(-finger, 0.0),
        }
    }

    // -- Sequences ----------------------------------------------------------

    fn remainder_rule(&self, device: ScrollDevice) -> RemainderHandoff {
        match device {
            ScrollDevice::Pointer { kind, .. } => self
                .env
                .settings
                .borrow()
                .snapshot_for(kind)
                .remainder_handoff(),
            _ => RemainderHandoff::NextSequence,
        }
    }

    /// A fresh sequence id from the binding's issuer, for input no binding
    /// dispatch named.
    fn mint(&self) -> Option<InputSequenceId> {
        self.env.sequences.mint()
    }

    /// The sequence the binding is dispatching input of, or a fresh one.
    fn dispatched_or_mint(&self) -> Option<InputSequenceId> {
        self.env.sequences.dispatched_or_mint()
    }

    /// Replace the driving sequence. The outgoing one ends unless it is the
    /// incoming one, still runs elsewhere, or is a contact still down;
    /// a run cut short ends it [`Preempted`](ScrollEndReason::Preempted).
    fn drive(&self, incoming: Option<Driving>, recovery: &mut PanicRecovery) {
        let incoming_sequence = incoming
            .as_ref()
            .map(|driving| Rc::as_ptr(&driving.sequence));
        let outgoing = self.driving.replace(incoming);
        let Some(outgoing) = outgoing else {
            return;
        };
        if outgoing.run.is_some() {
            outgoing.sequence.run_finished();
        }
        let sequence = &outgoing.sequence;
        if incoming_sequence == Some(Rc::as_ptr(sequence))
            || sequence.is_ended()
            || sequence.has_runs()
            || (sequence.is_contact() && sequence.is_live())
        {
            return;
        }
        let reason = if outgoing.run.is_some() {
            ScrollEndReason::Preempted
        } else {
            sequence.end_reason()
        };
        sequence.end(reason, recovery);
    }

    /// New user input `incoming` reached this leaf: the fling it handed to a
    /// scrollable above, still running under the sequence the leaf drove,
    /// stops, and that sequence ends [`Preempted`](ScrollEndReason::Preempted)
    /// before the new one starts (ADR-0209 §4). The leaf may take the input
    /// whole, so no remainder would reach the ancestor to catch it.
    fn preempt_handoff(&self, incoming: Option<InputSequenceId>, recovery: &mut PanicRecovery) {
        let handed_off = self
            .driving
            .borrow()
            .as_ref()
            .filter(|driving| {
                driving.run.is_none()
                    && Some(driving.sequence.id()) != incoming
                    && driving.sequence.has_runs()
                    && !driving.sequence.is_ended()
            })
            .map(|driving| Rc::clone(&driving.sequence));
        if let Some(sequence) = handed_off {
            sequence.preempt_runs(recovery);
        }
    }

    /// Stop the run this owner drives if it belongs to `sequence`.
    pub(crate) fn stop_run_of(&self, sequence: &ScrollSequence, recovery: &mut PanicRecovery) {
        let drives = self.driving.borrow().as_ref().is_some_and(|driving| {
            driving.run.is_some() && std::ptr::eq(Rc::as_ptr(&driving.sequence), sequence)
        });
        if drives {
            self.stop_motion(recovery);
        }
    }

    /// End `sequence` once nothing drives it any more: no run, and no live
    /// input — a contact still down, a wheel burst that may go on.
    fn settle(sequence: &ScrollSequence, reason: ScrollEndReason, recovery: &mut PanicRecovery) {
        if !sequence.has_runs() && !sequence.is_live() {
            sequence.end(reason, recovery);
        }
    }

    /// Start (or continue) the contact sequence `id`: a press or a drag on
    /// this scrollable. It preempts whatever motion the owner drives.
    fn begin_contact_with(
        self: &Rc<Self>,
        id: Option<InputSequenceId>,
        device: ScrollDevice,
        recovery: &mut PanicRecovery,
    ) -> Option<Rc<ScrollSequence>> {
        let current = self.contact.borrow().clone();
        if let Some(current) = &current
            && Some(current.id()) == id
            && current.is_live()
        {
            return Some(Rc::clone(current));
        }
        let id = id.or_else(|| self.mint())?;
        self.preempt_handoff(Some(id), recovery);
        let sequence = ScrollSequence::new(self, id, device, true, self.remainder_rule(device));
        let outgoing = self.contact.replace(Some(Rc::clone(&sequence)));
        let was_running = self
            .driving
            .borrow()
            .as_ref()
            .is_some_and(|driving| driving.run.is_some());
        self.drive(None, recovery);
        if was_running {
            let _ = self.fling.stop();
        }
        if let Some(outgoing) = outgoing {
            outgoing.cancel_contact();
            Self::settle(&outgoing, ScrollEndReason::Cancelled, recovery);
        }
        sequence.start(recovery);
        Some(sequence)
    }

    /// A drag of this scrollable started: the contact's sequence, the one a
    /// press on the moving scrollable already began or a new one that
    /// preempts whatever motion the scrollable drives.
    pub(crate) fn begin_contact(
        self: &Rc<Self>,
        id: Option<InputSequenceId>,
        kind: PointerKind,
        device: Option<DeviceId>,
    ) {
        let mut recovery = PanicRecovery::new();
        let _ = self.begin_contact_with(id, ScrollDevice::Pointer { kind, device }, &mut recovery);
        recovery.finish();
    }

    /// The live contact sequence.
    pub(crate) fn contact(&self) -> Option<Rc<ScrollSequence>> {
        self.contact
            .borrow()
            .clone()
            .filter(|sequence| sequence.is_live())
    }

    /// Start (or continue) the contactless sequence `id` on this leaf: a
    /// wheel latch, an assistive action, a programmatic scroll.
    fn begin_contactless(
        self: &Rc<Self>,
        id: InputSequenceId,
        device: ScrollDevice,
        recovery: &mut PanicRecovery,
    ) -> Rc<ScrollSequence> {
        // An id is one sequence: input still naming one that already ended
        // (its latch lapsed) is no new sequence, and moves the leaf alone.
        if let Some(driving) = self.driving.borrow().as_ref()
            && driving.sequence.id() == id
            && driving.position == 0
        {
            return Rc::clone(&driving.sequence);
        }
        let sequence = ScrollSequence::new(self, id, device, false, self.remainder_rule(device));
        self.drive(
            Some(Driving {
                sequence: Rc::clone(&sequence),
                position: 0,
                run: None,
            }),
            recovery,
        );
        sequence.start(recovery);
        sequence
    }

    /// Whether live input drives this owner: a contact still down, a held
    /// key, a drop autoscroll. A programmatic scroll never ends it.
    fn driven_by_input(&self) -> bool {
        self.contact().is_some()
            || self
                .keyboard
                .borrow()
                .as_ref()
                .is_some_and(|held| !held.sequence.is_ended())
            || self
                .autoscroll
                .borrow()
                .as_ref()
                .is_some_and(|(_, sequence)| !sequence.is_ended())
    }

    /// A programmatic sequence: it replaces the motion the owner drives,
    /// but never live input (a contact still down, a held key, a drop
    /// autoscroll), which keeps its sequence.
    fn begin_programmatic(
        self: &Rc<Self>,
        id: InputSequenceId,
        recovery: &mut PanicRecovery,
    ) -> Rc<ScrollSequence> {
        let device = ScrollDevice::Programmatic;
        if self.driven_by_input() {
            let sequence =
                ScrollSequence::new(self, id, device, false, RemainderHandoff::NextSequence);
            sequence.start(recovery);
            return sequence;
        }
        self.begin_contactless(id, device, recovery)
    }

    // -- Steps --------------------------------------------------------------

    /// Dispatch a user delta of the live contact pointing at `focal` (this
    /// leaf's plane): `physical` through the chain, the leaf taking what its
    /// physics admits.
    pub(crate) fn drag(&self, physical: Vec2, focal: Point) {
        let Some(sequence) = self.contact() else {
            return;
        };
        if !self.alive.get() || self.refused() {
            return;
        }
        let mut recovery = PanicRecovery::new();
        let position = self.controller.position();
        sequence.aim(focal);
        sequence.dispatch(
            0,
            ScrollCause::UserInput,
            physical,
            |offer| {
                let delta = self.to_scroll(offer);
                if delta == 0.0 {
                    return Vec2::ZERO;
                }
                // The user's direction, for activity subscribers (a floating
                // header's snap decision): a growing offset scrolls in
                // reverse.
                position.set_user_scroll_direction(if delta > 0.0 {
                    ScrollDirection::Reverse
                } else {
                    ScrollDirection::Forward
                });
                let pixels = self.controller.pixels();
                let metrics = ScrollMetrics::from(&position);
                let target = self
                    .physics()
                    .apply_boundary_conditions(&metrics, pixels + delta);
                if !target.is_finite() {
                    return Vec2::ZERO;
                }
                self.publish(target, ScrollCause::UserInput);
                self.to_physical(target - pixels)
            },
            &mut recovery,
        );
        recovery.finish();
    }

    /// Dispatch a contactless step that moves this leaf to `target` and
    /// report what moved: a precise wheel packet, an assistive action or a
    /// jump.
    fn step_to(
        &self,
        sequence: &ScrollSequence,
        cause: ScrollCause,
        target: f64,
        recovery: &mut PanicRecovery,
    ) {
        if !self.alive.get() || (cause != ScrollCause::Programmatic && self.refused()) {
            return;
        }
        let pixels = self.controller.pixels();
        let available = self.to_physical(target - pixels);
        sequence.dispatch(
            0,
            cause,
            available,
            |offer| {
                let now = self.controller.pixels();
                let next = if offer == available && now == pixels {
                    target
                } else {
                    now + self.to_scroll(offer)
                };
                if !next.is_finite() {
                    return Vec2::ZERO;
                }
                self.publish(next, cause);
                self.to_physical(next - now)
            },
            recovery,
        );
    }

    /// A precise or unknown wheel packet moving this leaf to `target`, under
    /// the binding's latch for its source.
    pub(crate) fn wheel_step(&self, sequence: &ScrollSequence, cause: ScrollCause, target: f64) {
        let mut recovery = PanicRecovery::new();
        self.step_to(sequence, cause, target, &mut recovery);
        recovery.finish();
    }

    /// A wheel packet of `delta` (scroll coordinates) that cannot move this
    /// leaf, at its extent: when a participant connection sits between the
    /// leaf and the nearest enclosing scrollable, the packet still runs
    /// through the chain under the binding's latch for its source, the leaf
    /// taking nothing, so that connection gets its turn by the chain's own
    /// latch (ADR-0209 §5). Returns whether the chain took part of it: the
    /// leaf then claims the packet. Without such a connection nothing is
    /// dispatched, and the enclosing scrollable's own claim decides.
    pub(crate) fn wheel_at_extent(
        self: &Rc<Self>,
        device: ScrollDevice,
        cause: ScrollCause,
        delta: f64,
        focal: Point,
    ) -> bool {
        if !self.alive.get()
            || self.refused()
            || !self
                .node
                .parent()
                .is_some_and(|parent| parent.is_user_connection())
        {
            return false;
        }
        let Some(sequence) = self.wheel_sequence(device) else {
            return false;
        };
        sequence.aim(focal);
        let mut recovery = PanicRecovery::new();
        let consumed = sequence.dispatch(
            0,
            cause,
            self.to_physical(delta),
            |_| Vec2::ZERO,
            &mut recovery,
        );
        recovery.finish();
        consumed != Vec2::ZERO
    }

    /// A delta of the current run: the controller ticked.
    pub(crate) fn tick(&self) {
        let value = self.fling.value();
        let (sequence, position, run) = {
            let driving = self.driving.borrow();
            match driving.as_ref() {
                Some(Driving {
                    sequence,
                    position,
                    run: Some(run),
                }) => (Rc::clone(sequence), *position, Rc::clone(run)),
                // A value the owner did not start (a reset to the shown
                // pixels before a run) is not a scroll.
                _ => return,
            }
        };
        if !value.is_finite() {
            return;
        }
        let last = run.last.replace(value);
        let delta = value - last;
        if delta == 0.0 {
            return;
        }
        let mut recovery = PanicRecovery::new();
        let available = self.to_physical(delta);
        let before = self.controller.pixels();
        let consumed = sequence.dispatch(
            position,
            run.cause,
            available,
            |offer| {
                // The run is authoritative over its own leaf: a bouncing run
                // overscrolls on purpose. In step with the run, its value is
                // written exactly, so a run that ends at an extent rests on
                // it.
                let pixels = self.controller.pixels();
                let next = if offer == available && pixels == last {
                    value
                } else {
                    pixels + self.to_scroll(offer)
                };
                self.publish(next, run.cause);
                offer
            },
            &mut recovery,
        );
        // What the run moved that the leaf did not: participants took it.
        let lag = self.to_scroll(consumed) - (self.controller.pixels() - before);
        if lag.is_finite() {
            run.deflected.set(run.deflected.get() + lag);
        }
        recovery.finish();
    }

    /// A descendant's remainder reached this scrollable: take what fits
    /// within the extents (ADR-0209 §6a). Programmatic steps are the
    /// descendant's own. While this owner runs motion of its own (a settle
    /// spring, a fling, a snap), a user's remainder it can take catches it:
    /// the run stops, its sequence ends `Preempted`, and the owner moves by
    /// what it takes alone. A remainder it cannot take leaves the run going,
    /// and framework motion from below (`SideEffect`) takes nothing from it.
    pub(crate) fn take_remainder(&self, step: &ScrollStep, recovery: &mut PanicRecovery) -> Vec2 {
        if !self.alive.get() || step.cause == ScrollCause::Programmatic {
            return Vec2::ZERO;
        }
        if self.runs_motion() {
            let catches = matches!(step.cause, ScrollCause::UserInput | ScrollCause::Momentum);
            if !catches || self.within_extents(step.available).is_none() {
                return Vec2::ZERO;
            }
            self.stop_motion(recovery);
        }
        self.take_within_extents(step.available, step.cause)
    }

    /// Whether the owner drives a run of its own now.
    fn runs_motion(&self) -> bool {
        self.driving
            .borrow()
            .as_ref()
            .is_some_and(|driving| driving.run.is_some())
    }

    // -- Runs ---------------------------------------------------------------

    /// Make a run of `sequence` at `position` the one the controller's ticks
    /// belong to, from `start`. Call before starting or retargeting the
    /// controller, so the outgoing run's continuation sees it lost.
    fn attach_run(
        &self,
        sequence: &Rc<ScrollSequence>,
        position: usize,
        cause: ScrollCause,
        start: f64,
        recovery: &mut PanicRecovery,
    ) -> Rc<RunTag> {
        let run = Rc::new(RunTag::new(self.this.clone(), cause, start));
        sequence.run_started();
        self.drive(
            Some(Driving {
                sequence: Rc::clone(sequence),
                position,
                run: Some(Rc::clone(&run)),
            }),
            recovery,
        );
        run
    }

    /// Detach `run` if it is still current, returning its sequence and
    /// position.
    fn detach_run(&self, run: &Rc<RunTag>) -> Option<(Rc<ScrollSequence>, usize)> {
        let mut driving = self.driving.borrow_mut();
        let current = driving.as_mut()?;
        if !current
            .run
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, run))
        {
            return None;
        }
        current.run = None;
        current.sequence.run_finished();
        Some((Rc::clone(&current.sequence), current.position))
    }

    /// The run a release at `velocity` starts: a snap to the policy's
    /// target, or the physics' ballistic simulation without one.
    fn release_simulation(
        &self,
        snap: Option<&dyn SnapPolicy>,
        metrics: &ScrollMetrics,
        velocity: f64,
    ) -> Option<Box<dyn Simulation>> {
        let Some(snap) = snap else {
            return self
                .physics()
                .create_ballistic_simulation(metrics, velocity);
        };
        let target = snap_target(snap, metrics, velocity);
        Some(Box::new(settle_spring(metrics, target, velocity)?))
    }

    /// Start the run a release at `velocity` (offset units per second)
    /// starts, as `sequence`'s motion at `position`. Returns whether the
    /// release is handled: a run started, or reentrant user code took over.
    fn start_run(
        self: &Rc<Self>,
        sequence: &Rc<ScrollSequence>,
        position: usize,
        velocity: f64,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) -> bool {
        if !self.alive.get() {
            return false;
        }
        let scroll_position = self.controller.position();
        let metrics =
            ScrollMetrics::from(&scroll_position).with_device_pixel_ratio(device_pixel_ratio);
        let generation = self.fling.run_generation();
        let is_current = || {
            self.alive.get()
                && self.fling.run_generation() == generation
                && self.controller.pixels() == metrics.pixels
        };
        // Determine transfer policy before owning a returned simulation. A
        // failing custom callback must not unwind through its arbitrary Drop.
        let snap = self.snap();
        let residual = if snap.is_some() {
            None
        } else {
            self.physics().boundary_velocity(&metrics, velocity)
        };
        if !is_current() {
            return true;
        }
        let Some(simulation) = self.release_simulation(snap.as_deref(), &metrics, velocity) else {
            return false;
        };
        if !is_current() {
            return true;
        }
        scroll_position.set_is_scrolling(true);
        if !is_current() {
            return true;
        }
        scroll_position.set_user_scroll_direction(if velocity > 0.0 {
            ScrollDirection::Reverse
        } else {
            ScrollDirection::Forward
        });
        // Activity observers may jump, replace the owner or start another run.
        if !is_current() {
            return true;
        }
        let run = self.attach_run(
            sequence,
            position,
            ScrollCause::SideEffect,
            metrics.pixels,
            recovery,
        );
        let Ok(future) = self.fling.animate_with(simulation) else {
            let _ = self.detach_run(&run);
            return false;
        };
        let residual = residual.map(|velocity| Residual {
            velocity,
            edge: if velocity > 0.0 {
                metrics.max_scroll_extent
            } else {
                metrics.min_scroll_extent
            },
            device_pixel_ratio,
        });
        self.watch_run(&run, &future, residual);
        true
    }

    /// Settle what `run` started: hand a residual at the edge to the chain,
    /// or end the sequence once nothing else drives it. The owner that
    /// drives the run when it completes settles it.
    fn watch_run(&self, run: &Rc<RunTag>, future: &AnimationRunFuture, residual: Option<Residual>) {
        let run = Rc::clone(run);
        let generation = self.fling.run_generation();
        future.when_complete_or_cancel(move |result| {
            let owner = run.owner.borrow().upgrade();
            let Some(owner) = owner else {
                return;
            };
            let Some((sequence, position)) = owner.detach_run(&run) else {
                // A newer run or sequence took over, and accounted for this
                // one.
                return;
            };
            let mut recovery = PanicRecovery::new();
            let rested = result.is_ok();
            if rested && owner.alive.get() && owner.fling.run_generation() == generation {
                // A same-pixel jump still queues an authoritative Cancel
                // even though no position notify services it. The run's
                // completion was published before those pixel listeners,
                // so honor their accepted command before old handoff work.
                if let Some(command) = owner.controller.take_pending_command() {
                    owner.service_command(command, &mut recovery);
                    Self::settle(&sequence, sequence.end_reason(), &mut recovery);
                    recovery.finish();
                    return;
                }
                if let Some(residual) = residual
                    && run.reached(
                        owner.controller.pixels(),
                        residual.edge,
                        residual.device_pixel_ratio,
                    )
                    && sequence.hand_off(
                        position,
                        owner.to_physical(residual.velocity),
                        residual.device_pixel_ratio,
                        &mut recovery,
                    ) == Handoff::Running
                {
                    recovery.finish();
                    return;
                }
            }
            let reason = if rested {
                sequence.end_reason()
            } else {
                ScrollEndReason::Preempted
            };
            Self::settle(&sequence, reason, &mut recovery);
            recovery.finish();
        });
    }

    /// A residual from below: run it if this owner can move that way.
    pub(crate) fn accept_fling(
        self: &Rc<Self>,
        sequence: &Rc<ScrollSequence>,
        position: usize,
        physical: Vec2,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) -> Acceptance {
        if !self.alive.get() {
            return Acceptance::Declined;
        }
        let velocity = self.to_scroll(physical);
        if velocity == 0.0 || !velocity.is_finite() {
            return Acceptance::Declined;
        }
        let metrics = ScrollMetrics::from(&self.controller.position())
            .with_device_pixel_ratio(device_pixel_ratio);
        let generation = self.fling.run_generation();
        // Ask the owner's real boundary policy whether motion in this
        // direction is admitted. Bouncing at an extent remains willing; a
        // hard clamp at the same extent declines.
        let proposed = metrics.pixels + velocity.signum();
        let allowed = self.physics().apply_boundary_conditions(&metrics, proposed);
        if !self.alive.get()
            || self.fling.run_generation() != generation
            || self.controller.pixels() != metrics.pixels
        {
            return Acceptance::Invalidated;
        }
        if allowed.is_finite()
            && allowed != metrics.pixels
            && self.start_run(sequence, position, velocity, device_pixel_ratio, recovery)
        {
            return Acceptance::Running;
        }
        Acceptance::Declined
    }

    /// Settle this owner after a sequence of the scrollables inside it moved
    /// it through a connection: a release at `physical` velocity, or rest.
    /// The run belongs to sequence `id` in this owner's own chain, so the
    /// participants above both hear the sequence once. Its own live contact
    /// keeps the offset. Returns whether a run started.
    pub(crate) fn settle_from_inside(
        self: &Rc<Self>,
        id: InputSequenceId,
        device: ScrollDevice,
        physical: Vec2,
        device_pixel_ratio: f64,
    ) -> bool {
        let mut recovery = PanicRecovery::new();
        let started = self.settle_within(id, device, physical, device_pixel_ratio, &mut recovery);
        recovery.finish();
        started
    }

    /// The settle a sequence of a scrollable inside this one owes it for
    /// moving it (ADR-0209 §6a): [`settle_from_inside`](Self::settle_from_inside)
    /// at this owner's own device pixel ratio, within the failure custody of
    /// the pass that pays it.
    pub(crate) fn settle_owed(
        self: &Rc<Self>,
        id: InputSequenceId,
        device: ScrollDevice,
        physical: Vec2,
        recovery: &mut PanicRecovery,
    ) -> bool {
        self.settle_within(id, device, physical, self.device_pixel_ratio(), recovery)
    }

    fn settle_within(
        self: &Rc<Self>,
        id: InputSequenceId,
        device: ScrollDevice,
        physical: Vec2,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) -> bool {
        if !self.alive.get() || self.contact().is_some() {
            return false;
        }
        let sequence = self.begin_contactless(id, device, recovery);
        sequence.finish_input();
        let _ = self.fling.stop();
        let started = self.start_run(
            &sequence,
            0,
            self.to_scroll(physical),
            device_pixel_ratio,
            recovery,
        );
        if !started {
            Self::settle(&sequence, ScrollEndReason::Released, recovery);
        }
        started
    }

    // -- Wheel --------------------------------------------------------------

    /// The sequence a wheel or trackpad packet on this leaf belongs to: the
    /// binding's latch for its source.
    pub(crate) fn wheel_sequence(
        self: &Rc<Self>,
        device: ScrollDevice,
    ) -> Option<Rc<ScrollSequence>> {
        let id = self.dispatched_or_mint()?;
        let mut recovery = PanicRecovery::new();
        self.preempt_handoff(Some(id), &mut recovery);
        let sequence = self.begin_contactless(id, device, &mut recovery);
        recovery.finish();
        Some(sequence)
    }

    /// A wheel or trackpad packet reached this leaf, moving it or not: the
    /// sequence it leads for the packet's latch ends where the binding says
    /// the latch ends, at its idle deadline or with this packet (ADR-0209
    /// §5). Call once the packet is handled, so a last packet's delta is
    /// still a step of the sequence.
    pub(crate) fn wheel_latch(&self) {
        self.env.sequences.follow_dispatched_latch(|id| {
            self.driving
                .borrow()
                .as_ref()
                .filter(|driving| driving.position == 0 && driving.sequence.id() == id)
                .map(|driving| Rc::clone(&driving.sequence))
        });
    }

    /// Detach the run the owner drives before new input of `sequence`
    /// stops the controller: the same sequence goes on, another one was
    /// already preempted when `sequence` began.
    pub(crate) fn halt(&self, sequence: &Rc<ScrollSequence>) {
        let mut recovery = PanicRecovery::new();
        self.drive(
            Some(Driving {
                sequence: Rc::clone(sequence),
                position: 0,
                run: None,
            }),
            &mut recovery,
        );
        recovery.finish();
    }

    /// Run a notched wheel's eased motion to `target` under `sequence`, from
    /// the shown pixels the controller was reset to. Returns whether it
    /// moves.
    pub(crate) fn animate_wheel(
        self: &Rc<Self>,
        sequence: &Rc<ScrollSequence>,
        target: f64,
        duration: std::time::Duration,
    ) -> bool {
        let mut recovery = PanicRecovery::new();
        let run = self.attach_run(
            sequence,
            0,
            ScrollCause::UserInput,
            self.fling.value(),
            &mut recovery,
        );
        let started = self.fling.animate_to_curved(
            target,
            Some(duration),
            Rc::new(flui_animation::curves::EASE_OUT),
        );
        let moving = match started {
            Ok(future) if self.fling.status().is_directional() => {
                self.watch_run(&run, &future, None);
                true
            }
            _ => {
                let _ = self.detach_run(&run);
                false
            }
        };
        recovery.finish();
        moving
    }

    // -- Drag autoscroll ------------------------------------------------------

    /// One frame of `drag`'s edge autoscroll: `velocity` (logical pixels per
    /// second, physical axes) for `elapsed`, as a
    /// [`SideEffect`](ScrollCause::SideEffect) step of the drag's own
    /// contactless sequence on this leaf (ADR-0211 §8). Returns whether the
    /// drag still scrolls here: the leaf could move that way and the chain
    /// took part of the step (for a frame that carries no time, only the
    /// former).
    pub(crate) fn autoscroll(
        self: &Rc<Self>,
        drag: DragIdentity,
        device: ScrollDevice,
        velocity: Vec2,
        elapsed: std::time::Duration,
    ) -> bool {
        if !self.alive.get() || self.refused() {
            return false;
        }
        let direction = self.to_scroll(velocity);
        let pixels = self.controller.pixels();
        let open = if direction > 0.0 {
            pixels < self.controller.max_scroll_extent()
        } else {
            direction < 0.0 && pixels > self.controller.min_scroll_extent()
        };
        let mut recovery = PanicRecovery::new();
        let step = velocity * elapsed.as_secs_f64();
        // A leaf at its extent dispatches nothing: the scrollable around it
        // autoscrolls on its own if the drag is near its edge too.
        let scrolling = if !open || step == Vec2::ZERO || !step.x.is_finite() || !step.y.is_finite()
        {
            open
        } else {
            let sequence = self.autoscroll_sequence(drag, device, &mut recovery);
            let consumed = sequence.map_or(Vec2::ZERO, |sequence| {
                sequence.dispatch(
                    0,
                    ScrollCause::SideEffect,
                    step,
                    |offer| self.take_within_extents(offer, ScrollCause::SideEffect),
                    &mut recovery,
                )
            });
            consumed != Vec2::ZERO
        };
        if !scrolling {
            self.end_autoscroll_with(drag, &mut recovery);
        }
        recovery.finish();
        scrolling
    }

    /// `drag` stopped autoscrolling this leaf: its sequence ends
    /// [`Released`](ScrollEndReason::Released).
    pub(crate) fn end_autoscroll(&self, drag: DragIdentity) {
        let mut recovery = PanicRecovery::new();
        self.end_autoscroll_with(drag, &mut recovery);
        recovery.finish();
    }

    fn end_autoscroll_with(&self, drag: DragIdentity, recovery: &mut PanicRecovery) {
        let ending = {
            let mut autoscroll = self.autoscroll.borrow_mut();
            match autoscroll.as_ref() {
                Some((current, _)) if *current == drag => autoscroll.take(),
                _ => None,
            }
        };
        if let Some((_, sequence)) = ending {
            sequence.finish_input();
            Self::settle(&sequence, ScrollEndReason::Released, recovery);
        }
    }

    /// The sequence `drag`'s frames step: the one it began, until something
    /// else ended it, or a new one, which replaces whatever motion the leaf
    /// drives (another drag's autoscroll included).
    fn autoscroll_sequence(
        self: &Rc<Self>,
        drag: DragIdentity,
        device: ScrollDevice,
        recovery: &mut PanicRecovery,
    ) -> Option<Rc<ScrollSequence>> {
        let current = self
            .autoscroll
            .borrow()
            .as_ref()
            .filter(|(current, sequence)| *current == drag && !sequence.is_ended())
            .map(|(_, sequence)| Rc::clone(sequence));
        if let Some(sequence) = current {
            return Some(sequence);
        }
        let id = self.mint()?;
        let sequence = self.begin_contactless(id, device, recovery);
        let outgoing = self.autoscroll.replace(Some((drag, Rc::clone(&sequence))));
        if let Some((_, outgoing)) = outgoing {
            outgoing.finish_input();
            Self::settle(&outgoing, ScrollEndReason::Released, recovery);
        }
        Some(sequence)
    }

    /// Move this scrollable by `offer` for `cause`, as far as its extents
    /// admit, and report what moved. From past an extent (a bouncing list
    /// springing back) only motion toward the range is admitted: the offset
    /// never moves against `offer`, never farther than it, and never farther
    /// out.
    fn take_within_extents(&self, offer: Vec2, cause: ScrollCause) -> Vec2 {
        let Some(target) = self.within_extents(offer) else {
            return Vec2::ZERO;
        };
        let pixels = self.controller.pixels();
        self.publish(target, cause);
        self.to_physical(target - pixels)
    }

    /// Where [`take_within_extents`](Self::take_within_extents) would move
    /// the offset for `offer`; none when it would not move.
    fn within_extents(&self, offer: Vec2) -> Option<f64> {
        let delta = self.to_scroll(offer);
        let pixels = self.controller.pixels();
        if delta == 0.0 || !pixels.is_finite() {
            return None;
        }
        // Ordered whatever the extents are: `low <= pixels <= high`.
        let low = self.controller.min_scroll_extent().min(pixels);
        let high = self.controller.max_scroll_extent().max(pixels);
        let target = (pixels + delta).clamp(low, high);
        (target.is_finite() && target != pixels).then_some(target)
    }

    // -- Programmatic and assistive motion ------------------------------------

    /// A controller jump to `target`, as a programmatic step of its own
    /// sequence. Returns `false` when the owner cannot dispatch it. The
    /// controller asks only while no pass runs through this owner; a jump
    /// asked during one waits in its command slot.
    pub(crate) fn jump(self: &Rc<Self>, target: f64) -> bool {
        let mut recovery = PanicRecovery::new();
        let jumped = self.jump_within(target, &mut recovery);
        recovery.finish();
        jumped
    }

    fn jump_within(self: &Rc<Self>, target: f64, recovery: &mut PanicRecovery) -> bool {
        if !self.alive.get() {
            return false;
        }
        let Some(id) = self.mint() else {
            return false;
        };
        let sequence = self.begin_programmatic(id, recovery);
        sequence.finish_input();
        self.step_to(&sequence, ScrollCause::Programmatic, target, recovery);
        Self::settle(&sequence, ScrollEndReason::Released, recovery);
        true
    }

    /// Service what the controller queued: an `animate_to` runs as a
    /// programmatic sequence, a jump asked during a pass lands. Nothing
    /// while a pass through this owner runs: its end services the slot.
    pub(crate) fn service_pending_command(self: &Rc<Self>) {
        let mut recovery = PanicRecovery::new();
        self.service_deferred(&mut recovery);
        recovery.finish();
    }

    fn service_command(
        self: &Rc<Self>,
        command: PendingScrollCommand,
        recovery: &mut PanicRecovery,
    ) {
        if let PendingScrollCommand::JumpTo { target_pixels } = command {
            // The jump ends the run the owner drives before its own
            // sequence starts; live input goes on from the new offset.
            let _ = self.fling.stop();
            let target = target_pixels.clamp(
                self.controller.min_scroll_extent(),
                self.controller.max_scroll_extent(),
            );
            if !self.jump_within(target, recovery) {
                self.controller.set_pixels(target);
            }
            return;
        }
        if matches!(command, PendingScrollCommand::Cancel) {
            let _ = self.controller.service_command(command, &self.fling);
            return;
        }
        let Some(id) = self.mint() else {
            let _ = self.controller.service_command(command, &self.fling);
            return;
        };
        let sequence = self.begin_programmatic(id, recovery);
        sequence.finish_input();
        let run = self.attach_run(
            &sequence,
            0,
            ScrollCause::Programmatic,
            self.controller.pixels(),
            recovery,
        );
        match self.controller.service_command(command, &self.fling) {
            Some(future) => self.watch_run(&run, &future, None),
            None => {
                if self.detach_run(&run).is_some() {
                    Self::settle(&sequence, ScrollEndReason::Released, recovery);
                }
            }
        }
    }

    /// Stop the run the owner drives; its sequence ends
    /// [`Preempted`](ScrollEndReason::Preempted).
    fn stop_motion(&self, recovery: &mut PanicRecovery) {
        let running = self
            .driving
            .borrow()
            .as_ref()
            .is_some_and(|driving| driving.run.is_some());
        if running {
            self.drive(None, recovery);
        }
        let _ = self.fling.stop();
    }

    /// An assistive scroll action or a reveal moving this leaf to `target`
    /// (clamped to the extents), then settling through the physics as a
    /// release from rest would. A reveal asked while a pass through this
    /// owner runs is a request like a controller jump: it waits in the
    /// command slot and lands when the pass returns.
    pub(crate) fn move_to(
        self: &Rc<Self>,
        target: f64,
        cause: ScrollCause,
        device_pixel_ratio: f64,
    ) -> MoveOutcome {
        if !self.alive.get() || !target.is_finite() {
            return MoveOutcome::Unmoved;
        }
        if cause == ScrollCause::Programmatic && self.in_pass() {
            self.controller.jump_to(target);
            return MoveOutcome::Unmoved;
        }
        if self.refused() {
            return MoveOutcome::Unmoved;
        }
        let mut recovery = PanicRecovery::new();
        let outcome = self.move_to_with(target, cause, device_pixel_ratio, &mut recovery);
        recovery.finish();
        outcome
    }

    fn move_to_with(
        self: &Rc<Self>,
        target: f64,
        cause: ScrollCause,
        device_pixel_ratio: f64,
        recovery: &mut PanicRecovery,
    ) -> MoveOutcome {
        if cause == ScrollCause::Programmatic {
            // A reveal stops the motion even where the target already shows.
            self.stop_motion(recovery);
        }
        let position = self.controller.position();
        let target = target.clamp(position.min_scroll_extent(), position.max_scroll_extent());
        let old = self.controller.pixels();
        if target == old {
            return MoveOutcome::Unmoved;
        }
        // A reveal is its own sequence even inside a pointer dispatch; an
        // assistive action names the one its runtime began.
        let sequence = if cause == ScrollCause::Programmatic {
            let Some(id) = self.mint() else {
                return MoveOutcome::Unmoved;
            };
            self.begin_programmatic(id, recovery)
        } else {
            let Some(id) = self.dispatched_or_mint() else {
                return MoveOutcome::Unmoved;
            };
            self.preempt_handoff(Some(id), recovery);
            self.begin_contactless(id, ScrollDevice::Assistive, recovery)
        };
        sequence.finish_input();
        let _ = self.fling.stop();
        position.set_is_scrolling(true);
        position.set_user_scroll_direction(if target > old {
            ScrollDirection::Reverse
        } else {
            ScrollDirection::Forward
        });
        self.step_to(&sequence, cause, target, recovery);
        if self.start_run(&sequence, 0, 0.0, device_pixel_ratio, recovery) {
            MoveOutcome::Settling
        } else {
            Self::settle(&sequence, ScrollEndReason::Released, recovery);
            MoveOutcome::Moved
        }
    }

    // -- Keyboard -----------------------------------------------------------

    /// A key moved this scrollable to `target` (clamped to the extents):
    /// a press begins a keyboard sequence of its own, an auto-repeat of the
    /// same key continues it, and either way the step settles through the
    /// physics as a release from rest would. Returns what moved, so a
    /// scrollable at the end of its range lets the key bubble to the
    /// enclosing one.
    pub(crate) fn key_scroll(
        self: &Rc<Self>,
        code: Code,
        auto_repeat: bool,
        target: f64,
        device_pixel_ratio: f64,
    ) -> MoveOutcome {
        if !self.alive.get() || !target.is_finite() || self.refused() {
            return MoveOutcome::Unmoved;
        }
        let position = self.controller.position();
        let target = target.clamp(position.min_scroll_extent(), position.max_scroll_extent());
        let old = self.controller.pixels();
        if target == old {
            return MoveOutcome::Unmoved;
        }
        let mut recovery = PanicRecovery::new();
        let held = self.keyboard.borrow().clone();
        let sequence = match held {
            Some(held) if auto_repeat && held.code == code && !held.sequence.is_ended() => {
                held.sequence
            }
            previous => {
                if let Some(previous) = previous {
                    previous.sequence.finish_input();
                    Self::settle(&previous.sequence, ScrollEndReason::Released, &mut recovery);
                }
                let Some(id) = self.mint() else {
                    recovery.finish();
                    return MoveOutcome::Unmoved;
                };
                self.preempt_handoff(Some(id), &mut recovery);
                let sequence = self.begin_contactless(id, ScrollDevice::Keyboard, &mut recovery);
                *self.keyboard.borrow_mut() = Some(KeyHold {
                    code,
                    sequence: Rc::clone(&sequence),
                });
                sequence
            }
        };
        let _ = self.fling.stop();
        position.set_is_scrolling(true);
        position.set_user_scroll_direction(if target > old {
            ScrollDirection::Reverse
        } else {
            ScrollDirection::Forward
        });
        self.step_to(&sequence, ScrollCause::UserInput, target, &mut recovery);
        let outcome = if self.start_run(&sequence, 0, 0.0, device_pixel_ratio, &mut recovery) {
            MoveOutcome::Settling
        } else {
            MoveOutcome::Moved
        };
        recovery.finish();
        outcome
    }

    /// `code` was released: the keyboard sequence it held ends once its
    /// last run comes to rest.
    pub(crate) fn key_released(&self, code: Code) {
        let held = self.keyboard.borrow().clone();
        let Some(held) = held.filter(|held| held.code == code) else {
            return;
        };
        self.keyboard.borrow_mut().take();
        let mut recovery = PanicRecovery::new();
        held.sequence.finish_input();
        Self::settle(&held.sequence, ScrollEndReason::Released, &mut recovery);
        recovery.finish();
    }

    // -- Contacts -----------------------------------------------------------

    /// A press on this scrollable while it moves (ADR-0208 rule 4): the
    /// press's sequence takes over and stops the motion.
    pub(crate) fn hold(self: &Rc<Self>, id: Option<InputSequenceId>, kind: PointerKind) {
        let mut recovery = PanicRecovery::new();
        let device = ScrollDevice::Pointer { kind, device: None };
        if let Some(sequence) = self.begin_contact_with(id, device, &mut recovery) {
            sequence.set_held(true);
        }
        let _ = self.fling.stop();
        recovery.finish();
    }

    /// The hold ended. A lifted press settles the stopped position through
    /// the physics (a pager stopped between pages lands on one); a press
    /// another scrollable's drag took ends the sequence
    /// [`ArenaLost`](ScrollEndReason::ArenaLost) once this scrollable's own
    /// drag lost too. Returns whether a settle run started.
    pub(crate) fn hold_ended(self: &Rc<Self>, end: HoldEnd, device_pixel_ratio: f64) -> bool {
        let Some(sequence) = self.contact().filter(|sequence| sequence.is_held()) else {
            return false;
        };
        sequence.set_held(false);
        let mut recovery = PanicRecovery::new();
        match end {
            HoldEnd::Lifted => sequence.release(Vec2::ZERO, &mut recovery),
            // This scrollable's own drag may take the contact next and stop
            // the settle; until its drag lost too, the sequence stays live.
            HoldEnd::Yielded => {
                if sequence.is_drag_lost() {
                    sequence.cancel_contact();
                    sequence.set_end_reason(ScrollEndReason::ArenaLost);
                }
            }
            _ => sequence.cancel_contact(),
        }
        let started = self.start_run(&sequence, 0, 0.0, device_pixel_ratio, &mut recovery);
        if !started {
            Self::settle(&sequence, sequence.end_reason(), &mut recovery);
        }
        recovery.finish();
        started
    }

    /// This scrollable's drag lost the contact before it started.
    pub(crate) fn drag_lost(&self) {
        let Some(sequence) = self.contact() else {
            return;
        };
        sequence.set_drag_lost();
        if !sequence.is_held() {
            sequence.cancel_contact();
            sequence.set_end_reason(ScrollEndReason::ArenaLost);
            let mut recovery = PanicRecovery::new();
            Self::settle(&sequence, ScrollEndReason::ArenaLost, &mut recovery);
            recovery.finish();
        }
    }

    /// The drag of the live contact ended: a completed one releases at
    /// `physical` velocity and flings, a cancelled one settles from rest.
    /// Returns whether a run started.
    pub(crate) fn drag_ended(
        self: &Rc<Self>,
        completed: bool,
        physical: Vec2,
        device_pixel_ratio: f64,
    ) -> bool {
        let Some(sequence) = self.contact() else {
            return false;
        };
        let mut recovery = PanicRecovery::new();
        let velocity = if completed {
            sequence.release(physical, &mut recovery);
            // An ancestor that holds this axis settles with the velocity
            // instead; this leaf only comes to rest.
            if sequence.held_above(self.axis_index()) {
                0.0
            } else {
                self.to_scroll(physical)
            }
        } else {
            sequence.cancel_contact();
            0.0
        };
        let started = self.start_run(&sequence, 0, velocity, device_pixel_ratio, &mut recovery);
        if !started {
            Self::settle(&sequence, sequence.end_reason(), &mut recovery);
        }
        recovery.finish();
        started
    }
}

/// What an assistive move or a reveal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveOutcome {
    /// The target was the shown offset, or the move was refused.
    Unmoved,
    /// The offset moved and rests.
    Moved,
    /// The offset moved and a settle run started.
    Settling,
}
