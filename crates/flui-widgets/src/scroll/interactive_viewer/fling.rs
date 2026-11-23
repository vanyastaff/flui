//! The viewer's inertia: the friction run of a release velocity and the spring
//! that returns an overshoot to its bounds.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_animation::{Animation, AnimationController, FrictionSimulation, Tolerance};
use flui_foundation::Listenable;
use flui_foundation::geometry::{Offset, Rect, Vec2};
use flui_foundation::panic::FirstPanic;
use flui_scheduler::PostFrameHandle;

use super::viewer_core::ViewerCore;
use super::{Overlay, Travel};
use crate::scroll::nested::{ScrollCause, ScrollEndReason, ScrollSequence};

/// What a run of the viewer's motion controller drives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Motion {
    /// Inertia: a friction run of the focal velocity; its value is the
    /// distance travelled.
    Friction,
    /// The overlay shown at a release coming back; the run's value is the
    /// fraction of it left.
    Release(Overlay),
    /// A fling the boundary stopped at `velocity` (viewport pixels per
    /// second): the scene overshoots past the bound and comes back; the
    /// run's value is the overshoot in pixels.
    Overshoot(Offset<f64>),
}

/// The viewer's own motion: its inertia, its steps dispatched under the
/// sequence that released it, and its snap-backs.
#[derive(Debug)]
pub(super) struct FocalFling {
    pub(super) controller: AnimationController,
    pub(super) run: RefCell<Option<Rc<FocalRun>>>,
    pub(super) clock_bound: Cell<bool>,
    pub(super) closed: Cell<bool>,
}

impl FocalFling {
    pub(super) fn new(controller: AnimationController) -> Self {
        Self {
            controller,
            run: RefCell::new(None),
            clock_bound: Cell::new(false),
            closed: Cell::new(false),
        }
    }

    /// Stop the run; its sequence ends for `reason`.
    pub(super) fn stop(&self, reason: ScrollEndReason) {
        let outgoing = self.run.borrow_mut().take();
        if let Some(run) = outgoing {
            run.finish(reason);
        }
        let _ = self.controller.stop();
    }

    /// Stop inertia, not a snap-back: a press that turns out a tap leaves
    /// the scene coming back inside its bounds.
    pub(super) fn stop_friction(&self) {
        let coasting = self
            .run
            .borrow()
            .as_ref()
            .is_some_and(|run| run.motion == Motion::Friction);
        if coasting {
            self.stop(ScrollEndReason::Preempted);
        }
    }

    /// Whether the run can start; otherwise its sequence ends at once.
    pub(super) fn admits(&self, sequence: Option<&Rc<ScrollSequence>>) -> bool {
        if !self.closed.get() && self.clock_bound.get() {
            return true;
        }
        if let Some(sequence) = sequence {
            let mut failure = FirstPanic::new();
            sequence.end(ScrollEndReason::Released, &mut failure);
            failure.resume();
        }
        false
    }

    /// Coast at `velocity` (viewport pixels per second) as `sequence`'s
    /// motion; a native pinch's run has no sequence. A run that cannot start
    /// ends the sequence at once. A boundary that stops the run turns it
    /// into an overshoot.
    pub(super) fn start(
        &self,
        core: &Rc<ViewerCore>,
        velocity: Offset<f64>,
        sequence: Option<Rc<ScrollSequence>>,
        post_frame: Option<PostFrameHandle>,
    ) {
        self.stop(ScrollEndReason::Preempted);
        let speed = velocity.dx.hypot(velocity.dy);
        let geometry = core.geometry();
        let simulation = FrictionSimulation::try_new(0.1, 0.0, speed, Tolerance::DEFAULT);
        let (Some(post_frame), Some((viewport, boundary)), Ok(simulation), true) = (
            post_frame,
            geometry,
            simulation,
            speed.is_finite() && speed > 0.0,
        ) else {
            if let Some(sequence) = sequence {
                let mut failure = FirstPanic::new();
                sequence.end(ScrollEndReason::Released, &mut failure);
                failure.resume();
            }
            return;
        };
        if !self.admits(sequence.as_ref()) {
            return;
        }
        // Ten percent of the release speed remains after one second. The
        // library simulation supplies finite admission and its rest threshold.
        let direction = velocity / speed;
        let live = Rc::new(Cell::new(true));
        let blocked = Rc::new(Cell::new(None));
        let travelled = Rc::new(Cell::new(0.0_f64));
        let animation = self.controller.clone();
        let weak = Rc::downgrade(core);
        let (step_sequence, step_live, step_blocked) =
            (sequence.clone(), Rc::clone(&live), Rc::clone(&blocked));
        let listener = self.controller.listen(Rc::new(move || {
            if !step_live.get() {
                return;
            }
            let Some(core) = weak.upgrade() else {
                return;
            };
            let distance = animation.value();
            if !distance.is_finite() {
                return;
            }
            let step = direction * (distance - travelled.replace(distance));
            if step == Offset::ZERO {
                return;
            }
            let available = Vec2::new(-step.dx, -step.dy);
            let consume = |offer| core.consume(offer, [false, false]);
            let consumed = match &step_sequence {
                Some(sequence) => {
                    let mut failure = FirstPanic::new();
                    let consumed = sequence.dispatch(
                        0,
                        ScrollCause::SideEffect,
                        available,
                        consume,
                        &mut failure,
                    );
                    failure.resume();
                    consumed
                }
                None => consume(available),
            };
            // An axis the boundary stopped overshoots with what the run
            // still carried on it.
            let stopped =
                |offered: f64, taken: f64| offered != 0.0 && taken.abs() < offered.abs() - 1e-9;
            let speed = animation.velocity();
            let overshoot = Offset::new(
                if stopped(available.x, consumed.x) {
                    direction.dx * speed
                } else {
                    0.0
                },
                if stopped(available.y, consumed.y) {
                    direction.dy * speed
                } else {
                    0.0
                },
            );
            if overshoot != Offset::ZERO && overshoot.is_finite() {
                step_blocked.set(Some(overshoot));
                let _ = animation.stop();
            } else if consumed == Vec2::ZERO {
                let _ = animation.stop();
            }
        }));
        let run = Rc::new(FocalRun {
            listener: std::cell::Cell::new(Some(listener)),
            viewport,
            boundary,
            core: Rc::downgrade(core),
            post_frame,
            animation: self.controller.clone(),
            live,
            sequence,
            motion: Motion::Friction,
            blocked,
        });
        *self.run.borrow_mut() = Some(Rc::clone(&run));
        let _ = self.controller.animate_with(simulation);
        run.schedule();
    }

    /// Bring the scene back inside its bounds: the overlay a release left,
    /// or the overshoot of a fling the boundary stopped. Without a clock the
    /// scene is back at once. The run is `sequence`'s last.
    pub(super) fn snap_back(
        &self,
        core: &Rc<ViewerCore>,
        motion: Motion,
        sequence: Option<Rc<ScrollSequence>>,
        post_frame: Option<PostFrameHandle>,
    ) {
        self.stop(ScrollEndReason::Preempted);
        let simulation = match motion {
            Motion::Release(_) => super::stretch::release_run(),
            Motion::Overshoot(velocity) => {
                super::stretch::overshoot_run(velocity.dx.hypot(velocity.dy))
            }
            Motion::Friction => return,
        };
        let rest = || {
            core.overlay.set(Overlay::default());
            core.travel.set(Travel::default());
            core.publish(core.base());
        };
        let (Some(post_frame), Some((viewport, boundary)), Ok(simulation)) =
            (post_frame, core.geometry(), simulation)
        else {
            rest();
            if let Some(sequence) = sequence {
                let mut failure = FirstPanic::new();
                sequence.end(ScrollEndReason::Released, &mut failure);
                failure.resume();
            }
            return;
        };
        if !self.admits(sequence.as_ref()) {
            rest();
            return;
        }
        let live = Rc::new(Cell::new(true));
        let animation = self.controller.clone();
        let weak = Rc::downgrade(core);
        let step_live = Rc::clone(&live);
        let listener = self.controller.listen(Rc::new(move || {
            if !step_live.get() {
                return;
            }
            let Some(core) = weak.upgrade() else {
                return;
            };
            let value = animation.value();
            if !value.is_finite() {
                return;
            }
            let overlay = match motion {
                Motion::Release(overlay) => overlay.scaled(value.clamp(0.0, 1.0)),
                Motion::Overshoot(velocity) => {
                    let speed = velocity.dx.hypot(velocity.dy);
                    Overlay {
                        pan: velocity / speed * value.max(0.0),
                        ..Overlay::default()
                    }
                }
                Motion::Friction => return,
            };
            core.overlay.set(overlay);
            core.publish(core.base());
        }));
        let run = Rc::new(FocalRun {
            listener: std::cell::Cell::new(Some(listener)),
            viewport,
            boundary,
            core: Rc::downgrade(core),
            post_frame,
            animation: self.controller.clone(),
            live,
            sequence,
            motion,
            blocked: Rc::new(Cell::new(None)),
        });
        *self.run.borrow_mut() = Some(Rc::clone(&run));
        let _ = self.controller.animate_with(simulation);
        run.schedule();
    }

    pub(super) fn close(&self) {
        self.closed.set(true);
        self.clock_bound.set(false);
        self.stop(ScrollEndReason::Cancelled);
    }
}

/// Completed layout validates the immutable limits before the next motion tick.
/// Publication remains on the Vsync listener, before build and paint.
pub(super) struct FocalRun {
    /// Detached with the run; `None` after. A cell because the run detaches
    /// through a shared reference.
    pub(super) listener: std::cell::Cell<Option<flui_foundation::Subscription>>,
    pub(super) viewport: Rect<f64>,
    pub(super) boundary: Rect<f64>,
    pub(super) core: Weak<ViewerCore>,
    pub(super) post_frame: PostFrameHandle,
    pub(super) animation: AnimationController,
    pub(super) live: Rc<Cell<bool>>,
    pub(super) sequence: Option<Rc<ScrollSequence>>,
    pub(super) motion: Motion,
    /// The velocity a boundary stopped a friction run with.
    pub(super) blocked: Rc<Cell<Option<Offset<f64>>>>,
}

impl std::fmt::Debug for FocalRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocalRun")
            .field("live", &self.live.get())
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

impl FocalRun {
    /// Detach the run once. Returns whether it was still live.
    pub(super) fn detach(&self) -> bool {
        if !self.live.replace(false) {
            return false;
        }
        drop(self.listener.take());
        let _ = self.animation.stop();
        true
    }

    /// End the run once: detach it and end its sequence for `reason`. A
    /// snap-back that does not hand the scene to new input leaves it inside
    /// its bounds.
    pub(super) fn finish(&self, reason: ScrollEndReason) {
        if !self.detach() {
            return;
        }
        if self.motion != Motion::Friction
            && reason != ScrollEndReason::Preempted
            && let Some(core) = self.core.upgrade()
        {
            core.overlay.set(Overlay::default());
            core.travel.set(Travel::default());
            core.publish(core.base());
        }
        if let Some(sequence) = &self.sequence {
            let mut failure = FirstPanic::new();
            sequence.end(reason, &mut failure);
            failure.resume();
        }
    }

    pub(super) fn schedule(self: Rc<Self>) {
        let handle = self.post_frame.clone();
        let pending = Rc::downgrade(&self);
        if handle
            .schedule(move |_| {
                if let Some(run) = pending.upgrade() {
                    run.tick();
                }
            })
            .is_err()
        {
            self.finish(ScrollEndReason::Cancelled);
        }
    }

    pub(super) fn tick(self: Rc<Self>) {
        if !self.live.get() {
            return;
        }
        let core = self.core.upgrade();
        let geometry = core.as_ref().and_then(|core| core.geometry());
        if geometry != Some((self.viewport, self.boundary)) {
            self.finish(ScrollEndReason::Cancelled);
            return;
        }
        if let Some(velocity) = self.blocked.take()
            && let Some(core) = core
        {
            // The boundary stopped the coast: the same sequence overshoots.
            self.detach();
            core.fling.snap_back(
                &core,
                Motion::Overshoot(velocity),
                self.sequence.clone(),
                Some(self.post_frame.clone()),
            );
            return;
        }
        if self.animation.is_animating() {
            self.schedule();
        } else {
            self.finish(ScrollEndReason::Released);
        }
    }
}
