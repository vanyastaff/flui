//! [`Vsync`] — a shared, restart-aware registry that drives
//! [`AnimationController`]s off a single virtual timeline.
//!
//! A deterministic frame driver (e.g. `flui_testing::HeadlessBinding`) owns one
//! `Vsync` and calls [`tick_all`](Vsync::tick_all) once per frame with the
//! current virtual instant. Controllers reach the same registry ambiently — in
//! the widget layer a `VsyncScope` inherited-view hands a clone down a subtree,
//! and an implicitly-animated widget registers its controller in `init_state`.
//! It is not a process-wide singleton.
//!
//! Both runtime and headless presentations use this registry. A presentation
//! owns its [`MotionClock`](crate::MotionClock) and supplies a typed
//! [`FrameTick`] to [`Vsync::tick_all`]. Controllers have no
//! scheduler ticker or wall-clock sampling path.
//!
//! ## Restart-awareness
//!
//! A controller re-zeros its run epoch on every fresh
//! `forward`/`reverse`/`animate_to`/… (it bumps
//! [`AnimationController::run_generation`]). `Vsync` watches that counter and
//! re-anchors each controller's per-run `t = 0` whenever it advances — so a
//! controller run twice (forward to completion, then reverse) is ticked from the
//! second run's own start instead of snapping to its target on the first frame.

use flui_foundation::panic::{FailureClass, RecoveryScope};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use std::cell::{Cell, RefCell};

use crate::AnimationController;
use crate::animation::{Retirement, Terminal};
use crate::controller::RunStart;
use crate::{AnimationTime, FrameTick};
use std::time::Duration;

mod retirement;
pub use retirement::VsyncRetirement;

/// Opaque identity for one admission to a [`Vsync`] registry.
///
/// [`Vsync::attach_child`] returns a token for [`Vsync::detach_child`]. Controller
/// admissions belong to [`crate::DrivenController`] and expose no removal token.
#[derive(Debug, Clone)]
pub struct VsyncRegistration {
    owner: Weak<RefCell<VsyncInner>>,
    slot: u64,
}

impl PartialEq for VsyncRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && Weak::ptr_eq(&self.owner, &other.owner)
    }
}

impl Eq for VsyncRegistration {}

impl VsyncRegistration {
    pub(crate) fn is_registered(&self) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.borrow().controllers.contains_key(&self.slot))
    }

    pub(crate) fn requires_sample(&self, probe: &crate::controller::WalkProbe) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        probe.needs_sample(owner.borrow().last_tick)
    }

    pub(crate) fn owner_is_alive(&self) -> bool {
        self.owner.strong_count() != 0
    }

    pub(crate) fn request_frame(&self, retirement: &mut RecoveryScope<'_>) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let live = {
            let inner = owner.borrow();
            inner.controllers.contains_key(&self.slot)
                || inner.children.iter().any(|child| child.slot == self.slot)
        };
        if live {
            Vsync::request_frame_from(&owner, retirement);
        }
        retirement.retire(owner);
    }
}

impl Hash for VsyncRegistration {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.owner.as_ptr().hash(state);
        self.slot.hash(state);
    }
}

/// A controller or child could not be admitted to a virtual frame registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VsyncRegistrationError {
    /// The registry permanently consumed its available registration identities.
    #[error("Vsync registration capacity exhausted")]
    Exhausted,
    /// The owning presentation permanently closed this registry.
    #[error("Vsync registry is closed")]
    Closed,
    /// The child already belongs to a live parent, including this parent.
    #[error("Vsync child already has a parent")]
    AlreadyAttached,
    /// Attaching the child would make an ancestor its own descendant.
    #[error("Vsync attachment would create a cycle")]
    WouldCycle,
}

/// One registered controller plus the registry's per-run anchor.
///
/// Registration needs no clock reading: the first sample establishes an anchor.
/// `last_gen` identifies the run whose anchor and last dispatched tick are stored.
/// Elapsed arithmetic stays on the clock's integer nanosecond grid, so a run
/// reaches its exact deadline without subtracting rounded floating-point times.
///
/// Keyed by the registration id (the `u64` inside [`VsyncRegistration`]) in
/// [`VsyncInner::controllers`] rather than carrying its own id — the map key
/// *is* the identity.
struct RegisteredController {
    controller: AnimationController,
    anchor: RunAnchor,
    last_gen: u64,
    last_tick: Option<RegistryTick>,
}

impl RegisteredController {
    /// What frame `frame` at `tick` does with this entry: settle it under
    /// the motion policy or an exhausted clock, sample it, or leave it.
    /// Updates the entry's anchor for a new run and records the sampled tick.
    fn step(&mut self, tick: FrameTick, frame: u64) -> RegistryWalkStep {
        // One borrow instead of two: `walk_probe` reads `run_generation` AND
        // `live_running` (which folds in `disposed` — see its own doc) under a
        // single controller borrow.
        let probe = self.controller.walk_probe();
        let now = tick.time(probe.behavior);
        let exhausted = now.as_duration() == Duration::MAX;
        if probe.generation != self.last_gen {
            self.last_gen = probe.generation;
            self.anchor = match (probe.start, self.last_tick) {
                (
                    RunStart::Continue {
                        generation,
                        elapsed,
                    },
                    Some(last),
                ) if frame != u64::MAX
                    && last.frame >= frame.saturating_sub(1)
                    && generation == last.generation =>
                {
                    RunAnchor::Established {
                        at: last.now,
                        // A source failure may leave the published seam
                        // earlier than the dispatched tick.
                        elapsed: last.elapsed.saturating_sub(elapsed),
                    }
                }
                _ => RunAnchor::Fresh,
            };
        }
        let resumed = probe.can_resume(tick) && self.controller.resume_motion_run(probe.generation);
        if resumed {
            self.anchor = RunAnchor::Fresh;
            self.last_tick = None;
        }
        if probe.has_run
            && !probe.parked
            && (exhausted
                || (probe.behavior == crate::AnimationBehavior::Normal
                    && tick.policy() == crate::MotionPolicy::Reduce))
        {
            let reason = if exhausted {
                crate::controller::SettleReason::ExhaustedClock
            } else {
                crate::controller::SettleReason::ReducedMotion
            };
            RegistryWalkStep::Settling(self.controller.clone(), probe.generation, reason)
        } else if probe.live_running || resumed {
            let elapsed = self.anchor.elapsed(now);
            self.last_tick = Some(RegistryTick {
                frame,
                now,
                generation: probe.generation,
                elapsed,
            });
            RegistryWalkStep::Running(self.controller.clone(), elapsed)
        } else {
            RegistryWalkStep::NotRunning
        }
    }
}

#[derive(Clone, Copy)]
struct RegistryTick {
    frame: u64,
    now: AnimationTime,
    generation: u64,
    elapsed: Duration,
}

enum RunAnchor {
    Fresh,
    Resume(Duration),
    Established {
        at: AnimationTime,
        elapsed: Duration,
    },
}

impl RunAnchor {
    fn elapsed(&mut self, now: AnimationTime) -> Duration {
        match *self {
            Self::Fresh => {
                *self = Self::Established {
                    at: now,
                    elapsed: Duration::ZERO,
                };
                Duration::ZERO
            }
            Self::Resume(elapsed) => {
                *self = Self::Established { at: now, elapsed };
                elapsed
            }
            Self::Established { at, elapsed } => {
                elapsed.saturating_add(now.saturating_duration_since(at))
            }
        }
    }
}

/// A nested registry: a `TickerMode`'s subtree registry, ticked through its
/// parent unless the parent is muted.
struct RegisteredChild {
    slot: u64,
    child: Vsync,
}

#[derive(Default)]
struct VsyncInner {
    frame: u64,
    /// Keyed by the registration id, which is also the registration
    /// *order*: ids are `next_id` post-increments and are never reused, so
    /// ascending key order is ascending registration order. `tick_all`'s
    /// cursor walk relies on that order to replace a per-frame id snapshot.
    controllers: BTreeMap<u64, RegisteredController>,
    /// Nested registries — muting applies to a whole *subtree*'s controllers.
    /// Widgets take their `Vsync` from the ambient `VsyncScope`, so a
    /// subtree's registry is a child of the one above it and muting is
    /// structural: a muted registry ticks neither its own controllers nor its
    /// children's. A nested unmuted registry cannot re-enable a muted
    /// ancestor, because the ancestor never forwards the tick.
    children: Vec<RegisteredChild>,
    /// Reusable allocation for an entry snapshot. Stored empty: only the active
    /// walk owns child handles, so a detached child is never retained by a cache.
    child_snapshot: Vec<(u64, Terminal<Vsync>)>,
    parent: Option<VsyncRegistration>,
    request_frame: Option<Rc<dyn Fn()>>,
    next_id: u64,
    closed: bool,
    muted: bool,
    last_tick: Option<FrameTick>,
    /// Backend identity of the ultimate live parent at the last frame. Weak custody
    /// neither retains the old presentation nor permits address reuse to alias it.
    frame_authority: Option<Weak<RefCell<VsyncInner>>>,
    /// Admission and gate mutations invalidate cached ancestry for active walks.
    /// Saturation disables caching instead of reusing an earlier revision.
    walk_revision: u64,
}

/// Borrowed admission chain for one recursive frame delivery. A callback can
/// withdraw or mute any ancestor while a descendant is being sampled.
/// Root revisions cache a successful chain only until its next mutation.
/// `saturated_walk_revisions_revalidate_ancestor_admission` pins the uncached
/// fallback when no fresh revision can be issued.
struct FrameWalk<'a> {
    registry: &'a Vsync,
    parent: Option<(&'a Self, u64)>,
    authority: &'a Weak<RefCell<VsyncInner>>,
    validated_revision: Cell<Option<u64>>,
}

impl FrameWalk<'_> {
    #[inline]
    fn has_live_parent(&self) -> bool {
        let Some((parent, slot)) = self.parent else {
            return true;
        };
        let revision = self
            .authority
            .upgrade()
            .map(|root| root.borrow().walk_revision);
        if revision.is_some_and(|revision| {
            revision != u64::MAX && self.validated_revision.get() == Some(revision)
        }) {
            return true;
        }
        let attached = {
            let inner = self.registry.inner.borrow();
            inner.parent.as_ref().is_some_and(|seat| {
                seat.slot == slot
                    && std::ptr::eq(seat.owner.as_ptr(), Rc::as_ptr(&parent.registry.inner))
            })
        };
        let live = attached && parent.is_live();
        if live {
            self.validated_revision.set(revision);
        }
        live
    }

    #[inline]
    fn is_live(&self) -> bool {
        let enabled = {
            let inner = self.registry.inner.borrow();
            !inner.closed && !inner.muted
        };
        enabled && self.has_live_parent()
    }
}

impl VsyncInner {
    fn reserve_slot(&mut self) -> Option<u64> {
        let next = self.next_id.checked_add(1)?;
        Some(std::mem::replace(&mut self.next_id, next))
    }
}

/// A shared, restart-aware controller registry driven once per frame.
///
/// Cloning a `Vsync` clones an `Rc`-backed handle: every clone observes the
/// same registry, so the handle a `VsyncScope` hands to a subtree and the one a
/// binding ticks are the same registry.
#[derive(Clone, Default)]
pub struct Vsync {
    inner: Rc<RefCell<VsyncInner>>,
}

impl Vsync {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind this registry's frame demand to its presentation's frame driver.
    ///
    /// Existing running controllers request a sample from the new driver.
    /// Invocation and outgoing capture retirement occur outside registry borrows.
    ///
    /// # Panics
    /// Propagates the first callback or capture-retirement failure after recovery.
    pub fn set_frame_requester(&self, request_frame: Option<Rc<dyn Fn()>>) {
        let outgoing = {
            let mut inner = self.inner.borrow_mut();
            if inner.closed {
                request_frame
            } else {
                std::mem::replace(&mut inner.request_frame, request_frame)
            }
        };
        let mut retirement = Retirement::new();
        if self.has_frame_demand(None) {
            Self::request_frame_from(&self.inner, &mut retirement.scope());
        }
        retirement.retire(outgoing);
        retirement.finish();
    }

    fn request_frame_from(owner: &Rc<RefCell<VsyncInner>>, retirement: &mut RecoveryScope<'_>) {
        let (request, parent) = {
            let mut inner = owner.borrow_mut();
            if inner.muted {
                return;
            }
            if inner
                .parent
                .as_ref()
                .is_some_and(|parent| !parent.owner_is_alive())
            {
                inner.parent = None;
            }
            (inner.request_frame.clone(), inner.parent.clone())
        };
        if let Some(request) = request {
            retirement.run_classified(FailureClass::Delivery, |_| request());
            retirement.retire_classified(FailureClass::Delivery, request);
        }
        if let Some(parent) = parent {
            parent.request_frame(retirement);
        }
    }

    pub(crate) fn try_register_resuming(
        &self,
        controller: &AnimationController,
        elapsed: Duration,
    ) -> Result<VsyncRegistration, VsyncRegistrationError> {
        self.try_register_with_anchor(controller, RunAnchor::Resume(elapsed))
    }

    fn try_register_with_anchor(
        &self,
        controller: &AnimationController,
        anchor: RunAnchor,
    ) -> Result<VsyncRegistration, VsyncRegistrationError> {
        let last_gen = controller.run_generation();
        let mut inner = self.inner.borrow_mut();
        if inner.closed {
            return Err(VsyncRegistrationError::Closed);
        }
        let id = inner
            .reserve_slot()
            .ok_or(VsyncRegistrationError::Exhausted)?;
        inner.controllers.insert(
            id,
            RegisteredController {
                controller: controller.clone(),
                anchor,
                last_gen,
                last_tick: None,
            },
        );
        let registration = VsyncRegistration {
            owner: Rc::downgrade(&self.inner),
            slot: id,
        };
        drop(inner);
        controller.add_frame_route(registration.clone());
        Ok(registration)
    }

    /// Remove the controller previously registered under `id`. Idempotent: an
    /// unknown or already-removed id is a no-op.
    pub(crate) fn unregister(&self, id: &VsyncRegistration) {
        if !Weak::ptr_eq(&id.owner, &Rc::downgrade(&self.inner)) {
            return;
        }
        let removed = {
            let mut inner = self.inner.borrow_mut();
            inner.controllers.remove(&id.slot)
        };
        // The last controller owner can retire user captures that reenter this
        // registry. Its registration is absent and the guard is released first.
        if let Some(removed) = removed {
            removed.controller.remove_frame_route(id);
            drop(removed);
        }
    }

    /// Nest `child` under this registry: [`tick_all`](Self::tick_all) forwards
    /// to it — unless this registry is [`muted`](Self::set_muted).
    ///
    /// A registry has at most one live parent. A returned admission refusal
    /// changes neither edge nor registration identity; detach the existing
    /// seat before moving a child. A wake panic after admission withdraws the
    /// new edge under recovery, rather than undoing every callout effect.
    /// A released parent does not prevent a surviving child from attaching again.
    ///
    /// # Errors
    /// Returns [`VsyncRegistrationError::Closed`] if either registry is closed,
    /// [`VsyncRegistrationError::WouldCycle`] for self or ancestor attachment,
    /// [`VsyncRegistrationError::AlreadyAttached`] for a live existing parent,
    /// or [`VsyncRegistrationError::Exhausted`] for permanent identity exhaustion.
    pub fn attach_child(&self, child: &Vsync) -> Result<VsyncRegistration, VsyncRegistrationError> {
        if self.inner.borrow().closed || child.inner.borrow().closed {
            return Err(VsyncRegistrationError::Closed);
        }
        if self.has_ancestor(child) {
            return Err(VsyncRegistrationError::WouldCycle);
        }
        if child
            .inner
            .borrow()
            .parent
            .as_ref()
            .is_some_and(VsyncRegistration::owner_is_alive)
        {
            return Err(VsyncRegistrationError::AlreadyAttached);
        }
        let mut inner = self.inner.borrow_mut();
        let slot = inner
            .reserve_slot()
            .ok_or(VsyncRegistrationError::Exhausted)?;
        inner.children.push(RegisteredChild {
            slot,
            child: child.clone(),
        });
        let registration = VsyncRegistration {
            owner: Rc::downgrade(&self.inner),
            slot,
        };
        drop(inner);
        child.invalidate_walks();
        child.inner.borrow_mut().parent = Some(registration.clone());
        self.invalidate_walks();
        let mut retirement = Retirement::new();
        let tick = self.inner.borrow().last_tick;
        if child.has_frame_demand(tick) {
            registration.request_frame(&mut retirement.scope());
        }
        if retirement.has_failure() {
            retirement.run(|| self.detach_child(&registration));
        }
        retirement.finish();
        Ok(registration)
    }

    /// Detach the child registry previously attached under `id`. Idempotent.
    pub fn detach_child(&self, id: &VsyncRegistration) {
        if !Weak::ptr_eq(&id.owner, &Rc::downgrade(&self.inner)) {
            return;
        }
        let removed = {
            let mut inner = self.inner.borrow_mut();
            inner
                .children
                .iter()
                .position(|child| child.slot == id.slot)
                .map(|index| inner.children.remove(index))
        };
        // Removing one child preserves the remaining registration order. Its
        // last controller captures must retire after releasing the parent guard.
        if let Some(removed) = removed {
            self.invalidate_walks();
            let mut child = removed.child.inner.borrow_mut();
            if child.parent.as_ref() == Some(id) {
                child.parent = None;
            }
            drop(child);
            drop(removed);
        }
    }

    /// Whether both handles name the **same** registry (`Rc` identity) — how a
    /// consumer tells "the ambient registry changed" from "same registry, fresh
    /// clone".
    #[must_use]
    pub fn is_same(&self, other: &Vsync) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Upward traversal needs no descendant snapshot or frame-path cycle stack:
    /// the owner-local tree admits every edge only after this check.
    fn has_ancestor(&self, other: &Vsync) -> bool {
        let mut ancestor = Some(self.clone());
        while let Some(current) = ancestor {
            if current.is_same(other) {
                return true;
            }
            ancestor = current
                .inner
                .borrow()
                .parent
                .as_ref()
                .and_then(|parent| parent.owner.upgrade())
                .map(|inner| Vsync { inner });
            debug_assert!(
                ancestor
                    .as_ref()
                    .is_none_or(|parent| !parent.is_same(&current)),
                "BUG: every admitted registry edge is acyclic"
            );
        }
        false
    }

    /// Whether this registry is muted — its controllers and every nested
    /// registry stop advancing.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.inner.borrow_mut().muted
    }

    /// Mute or unmute this registry: while muted it delivers no ticks, to its
    /// own controllers or to a nested registry's.
    ///
    /// **The clock keeps running.** Run anchors are absolute, so an unmuted
    /// controller lands where its presentation clock says it should be — it does not
    /// resume from where it stopped: a muted clock still runs, only the
    /// callback is withheld.
    pub fn set_muted(&self, muted: bool) {
        let changed = {
            let mut inner = self.inner.borrow_mut();
            let changed = inner.muted != muted;
            inner.muted = muted;
            changed
        };
        if changed {
            self.invalidate_walks();
        }
        if changed && !muted && self.has_frame_demand(None) {
            let mut retirement = Retirement::new();
            Self::request_frame_from(&self.inner, &mut retirement.scope());
            retirement.finish();
        }
    }

    /// Include one-shot settlement and resumption in wake demand, without
    /// making parked or paused work request continuous frames. Ancestor policy
    /// also reaches children that have not observed it while muted.
    fn has_frame_demand(&self, inherited: Option<FrameTick>) -> bool {
        let inner = self.inner.borrow();
        if inner.muted {
            return false;
        }
        let tick = inherited.map_or(inner.last_tick, |tick| {
            Some(tick.hold_after(inner.last_tick.unwrap_or(tick)))
        });
        // These probes read framework-owned scalar state only. No callback or
        // outgoing ownership is invoked or retired while this borrow is held.
        inner
            .controllers
            .values()
            .any(|registered| registered.controller.walk_probe().needs_sample(tick))
            || inner
                .children
                .iter()
                .any(|registered| registered.child.has_frame_demand(tick))
    }

    /// The number of controllers registered **with this registry**, not
    /// counting nested ones (see [`attach_child`](Self::attach_child)).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.borrow().controllers.len()
    }

    /// Whether no controllers are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().controllers.is_empty()
    }

    /// Whether at least one registered controller is currently running.
    ///
    /// Used by a production frame driver (e.g. `flui-app`'s `UiRuntime`) to decide whether
    /// to request the next frame: call this after [`tick_all`](Self::tick_all)
    /// and, if `true`, schedule a wake so the frame loop keeps going. Once the
    /// last running controller completes, `has_running()` returns `false` and
    /// the driver does NOT re-request, so the window quiesces cleanly — no
    /// infinite redraw after all animations settle.
    ///
    /// **O(N)**: the indexed registry (see [`tick_all`](Self::tick_all)'s doc)
    /// speeds up *resolving one id*, not "is anything running", which still
    /// reads every entry's status. No active-set index: that would need
    /// start/stop notifications from the controller to track membership,
    /// which nothing here provides.
    ///
    /// Reads the controller's crate-private `walk_probe().live_running`, not
    /// bare `status().is_directional()`: a controller `dispose()`d mid-run keeps
    /// whatever running status it had (`dispose()` deliberately leaves
    /// `status` untouched), so `live_running` is what tells this apart from
    /// a genuinely live run — otherwise a disposed-but-not-yet-unregistered
    /// controller would hold the frame loop open forever.
    #[must_use]
    pub fn has_running(&self) -> bool {
        let inner = self.inner.borrow();
        // A muted registry advances nothing — not its own controllers, not a
        // nested registry's — so nothing under it can hold the frame loop open.
        if inner.muted {
            return false;
        }
        // Nested registries hold real controllers: a `TickerMode` (and so every
        // `Hero` child) puts its subtree's animations in one, and a frame-loop
        // gate that only looked at this level would stall them after one frame.
        // Recursive probes invoke no user code and retire no owned values.
        inner
            .controllers
            .values()
            .any(|registered| registered.controller.walk_probe().live_running)
            || inner
                .children
                .iter()
                .any(|registered| registered.child.has_running())
    }

    /// Advance live controller registrations using a presentation's typed tick.
    ///
    /// A fresh run anchors on its first observed frame. Registration migration
    /// preserves its last sampled elapsed time. Moving a child subtree to another
    /// root also re-anchors on its first delivered frame, without replacing runs.
    /// Reattaching under the same root retains the existing absolute anchors.
    /// Older ticks from the same root hold at this
    /// registry's last accepted time; stopped and disposed kernels are skipped.
    ///
    /// State borrows end before sampling user curves or delivering callbacks.
    /// A listener may release or dispose its [owning controller](crate::DrivenController)
    /// during the walk. A later registration withdrawn by that listener is
    /// skipped, while other live controllers remain deliverable.
    ///
    /// Admissions made during this call first tick on the next call. Restarting
    /// an already visited controller also waits for the next call; starting a
    /// resident controller that has not yet been visited can sample this frame.
    /// Registration identities are never reused, so withdrawing and re-admitting
    /// a kernel cannot give the replacement an earlier turn in the current walk.
    ///
    /// Child registries admitted at entry tick before this registry's controllers.
    /// Muting is rechecked between controller samples, so muting during delivery
    /// stops the remaining samples. Ancestor mute gates apply to nested registries.
    ///
    /// A controller or child-registry failure leaves healthy frame peers
    /// deliverable. The walk resumes its first failure after those peers tick,
    /// preserving that failure through callback and capture retirement.
    pub fn tick_all(&self, tick: &FrameTick) {
        let mut retirement = Retirement::new();
        self.tick_all_with_recovery(tick, &mut retirement.scope());
        retirement.finish();
    }

    /// Run the registry under its presentation's existing failure custody.
    ///
    /// The runtime supplies one owner for independent sampling stages, then
    /// chooses publication and report disposition from the captured provenance.
    /// Standalone [`Self::tick_all`] resumes the same first payload after this
    /// walk. Neither path loses live peers or admitted delivery on failure.
    #[doc(hidden)]
    pub fn tick_all_with_recovery(&self, tick: &FrameTick, retirement: &mut RecoveryScope<'_>) {
        retirement.run_with(|retirement| {
            let authority = self.frame_authority();
            self.tick_all_with_retirement(*tick, &authority, None, retirement);
        });
    }

    /// A directly ticked subtree retains the clock authority of its live parent
    /// chain. Single-parent acyclic admission bounds this upward traversal.
    fn frame_authority(&self) -> Weak<RefCell<VsyncInner>> {
        let mut registry = Rc::clone(&self.inner);
        loop {
            let parent = {
                let inner = registry.borrow();
                inner.parent.as_ref().and_then(|seat| seat.owner.upgrade())
            };
            let Some(parent) = parent else {
                return Rc::downgrade(&registry);
            };
            registry = parent;
        }
    }

    /// No user code runs between a hierarchy mutation and its invalidation.
    /// Walks revalidate their own ancestry, so unrelated healthy peers continue.
    fn invalidate_walks(&self) {
        let root = self
            .frame_authority()
            .upgrade()
            .expect("BUG: synchronous authority lookup retains a live root");
        let mut inner = root.borrow_mut();
        inner.walk_revision = inner.walk_revision.saturating_add(1);
    }

    fn tick_all_with_retirement<'a>(
        &'a self,
        tick: FrameTick,
        authority: &Weak<RefCell<VsyncInner>>,
        parent: Option<(&'a FrameWalk<'a>, u64)>,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let walk = FrameWalk {
            registry: self,
            parent,
            authority,
            validated_revision: Cell::new(None),
        };
        if !walk.has_live_parent() {
            return;
        }
        let (fence, mut children) = {
            let mut inner = self.inner.borrow_mut();
            if inner
                .frame_authority
                .as_ref()
                .is_some_and(|previous| !Weak::ptr_eq(previous, authority))
            {
                inner.last_tick = None;
                for registered in inner.controllers.values_mut() {
                    // Published elapsed may precede a failed source's dispatched
                    // tick. Resume the actual sample, as an individual rebind does.
                    registered.anchor = RunAnchor::Resume(registered.controller.last_elapsed());
                    registered.last_gen = registered.controller.run_generation();
                    registered.last_tick = None;
                }
            }
            if inner
                .frame_authority
                .as_ref()
                .is_none_or(|previous| !Weak::ptr_eq(previous, authority))
            {
                inner.frame_authority = Some(authority.clone());
            }
            inner.last_tick = Some(tick.hold_after(inner.last_tick.unwrap_or(tick)));
            inner.frame = inner.frame.saturating_add(1);
            let mut children = std::mem::take(&mut inner.child_snapshot);
            children.extend(
                inner
                    .children
                    .iter()
                    .map(|registered| (registered.slot, Terminal::new(registered.child.clone()))),
            );
            (inner.next_id, children)
        };

        // Capture this walk's admissions before a tracing subscriber can reenter
        // registration. Only the public entry owns a span; descendants share it.
        let profile = parent
            .is_none()
            .then(crate::observability::ProfileSpan::tick);

        // A muted registry delivers no tick — not to its own controllers, not
        // to a nested registry's. The anchors are absolute and left alone, so
        // the clock keeps running underneath: an unmute lands the animation
        // where its presentation clock says. A new root first establishes its
        // own anchors instead of comparing unrelated absolute clock readings.
        for (slot, child) in children.drain(..) {
            if walk.is_live() {
                let tick = self
                    .inner
                    .borrow()
                    .last_tick
                    .expect("BUG: registry frame stores its tick before walking children");
                retirement.run_with(|retirement| {
                    child.tick_all_with_retirement(
                        tick,
                        authority,
                        Some((&walk, slot)),
                        retirement,
                    );
                });
            }
            // Retire visited and withdrawn entries independently, before the
            // next peer. A failed destructor joins this walk's first failure.
            retirement.retire(child);
        }
        {
            let mut inner = self.inner.borrow_mut();
            // A reentrant walk may have returned another empty allocation.
            // Preserve the larger one; neither buffer owns user captures here.
            if children.capacity() > inner.child_snapshot.capacity() {
                std::mem::swap(&mut children, &mut inner.child_snapshot);
            }
        }

        let mut cursor = 0u64;
        loop {
            if !walk.has_live_parent() {
                break;
            }
            let step = {
                let mut inner = self.inner.borrow_mut();
                let tick = inner
                    .last_tick
                    .expect("BUG: registry frame stores its tick before walking controllers");
                let frame = inner.frame;
                // Re-read per iteration, not only captured at entry above: a
                // listener that mutes the registry mid-walk must stop the
                // rest of this frame's entries from ticking.
                if inner.muted {
                    RegistryWalkStep::Finished
                } else if let Some((&id, registered)) =
                    inner.controllers.range_mut(cursor..fence).next()
                {
                    cursor = id + 1;
                    registered.step(tick, frame)
                } else {
                    RegistryWalkStep::Finished
                }
            };

            match step {
                RegistryWalkStep::Finished => break,
                RegistryWalkStep::NotRunning => {}
                RegistryWalkStep::Settling(controller, generation, reason) => {
                    let controller = Terminal::new(controller);
                    retirement.run_with(|retirement| {
                        controller.settle_run(generation, reason, retirement);
                    });
                    retirement.retire(controller);
                }
                RegistryWalkStep::Running(controller, elapsed) => {
                    let controller = Terminal::new(controller);
                    retirement.run_with(|retirement| {
                        controller.tick_at_with_retirement(elapsed, retirement);
                    });
                    retirement.retire(controller);
                }
            }
        }
        if let Some(profile) = profile {
            profile.close(retirement);
        }
    }
}

/// One step of [`Vsync::tick_all`]'s cursor walk over `controllers`.
enum RegistryWalkStep {
    /// The walk's range is exhausted, or the registry was muted mid-walk:
    /// either way the walk ends here.
    Finished,
    /// The controller at this position is not running; skip it and continue.
    NotRunning,
    /// The controller is running; tick it with the given elapsed duration
    /// once the registry borrow guarding this step is released.
    Running(AnimationController, Duration),
    Settling(AnimationController, u64, crate::controller::SettleReason),
}

impl std::fmt::Debug for Vsync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vsync")
            .field("registered", &self.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod retirement_tests;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::Animation;

    fn saturated_walk_revisions_revalidate_ancestor_admission() {
        for revision in [u64::MAX - 1, u64::MAX] {
            for mute in [false, true] {
                let root = Vsync::new();
                let middle = Vsync::new();
                let leaf = Vsync::new();
                let seat = root.attach_child(&middle).expect("middle admission");
                middle.attach_child(&leaf).expect("leaf admission");
                let trigger =
                    AnimationController::builder(Duration::from_millis(50)).build_on(Some(&leaf));
                let resident =
                    AnimationController::builder(Duration::from_millis(200)).build_on(Some(&leaf));
                trigger.controller().forward().expect("trigger starts");
                let run = resident.controller().forward().expect("resident starts");
                // Only a private seed can reach the saturation boundary.
                root.inner.borrow_mut().walk_revision = revision;
                let mut clock = crate::MotionClock::new();
                root.tick_all(&clock.frame(Duration::ZERO));
                let changing_root = root.clone();
                let _subscription = trigger.controller().subscribe_status(Rc::new(move |_| {
                    if mute {
                        changing_root.set_muted(true);
                    } else {
                        changing_root.detach_child(&seat);
                    }
                }));
                root.tick_all(&clock.frame(Duration::from_millis(100)));
                assert_eq!(
                    resident.controller().value(),
                    0.0,
                    "saturation cannot preserve stale admission: revision={revision}, mute={mute}"
                );
                assert!(run.is_pending());
                if mute {
                    root.set_muted(false);
                } else {
                    root.attach_child(&middle).expect("fresh admission");
                }
                root.tick_all(&clock.frame(Duration::from_millis(200)));
                assert!(
                    run.is_complete(),
                    "a recovered admission remains deliverable"
                );
            }
        }
    }

    fn registration_exhaustion_preserves_admitted_work() {
        for (remaining, child_last) in [(1, false), (1, true), (2, false), (2, true)] {
            let registry = Vsync::new();
            // Seed the private counter boundary, then use the production admission
            // seam and public owning-handle refusal and retirement.
            registry.inner.borrow_mut().next_id = u64::MAX - remaining;
            let preceding = AnimationController::builder(Duration::from_secs(1)).build();
            let preceding_child = Vsync::new();
            let preceding_id = if remaining == 2 {
                let id = if child_last {
                    registry
                        .try_register_resuming(&preceding, Duration::ZERO)
                        .expect("private token admission")
                } else {
                    preceding_child
                        .try_register_resuming(&preceding, Duration::ZERO)
                        .expect("private token admission");
                    registry
                        .attach_child(&preceding_child)
                        .expect("penultimate child identity admitted")
                };
                preceding
                    .forward()
                    .expect("penultimate admitted run starts");
                Some(id)
            } else {
                None
            };
            let child = Vsync::new();
            let animation = AnimationController::builder(Duration::from_secs(1)).build();
            let last = if child_last {
                child
                    .try_register_resuming(&animation, Duration::ZERO)
                    .expect("private token admission");
                registry
                    .attach_child(&child)
                    .expect("last child identity admitted")
            } else {
                registry
                    .try_register_resuming(&animation, Duration::ZERO)
                    .expect("last controller identity admitted")
            };
            animation.forward().expect("last admitted run starts");
            let refused = AnimationController::builder(Duration::from_secs(1)).build();
            for handle in [&registry, &registry.clone()] {
                assert_eq!(
                    handle.try_register_resuming(&refused, Duration::ZERO),
                    Err(VsyncRegistrationError::Exhausted)
                );
                assert_eq!(
                    handle.attach_child(&Vsync::new()),
                    Err(VsyncRegistrationError::Exhausted)
                );
            }
            registry.tick_all(
                &crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
            );
            registry.tick_all(
                &crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
            );
            assert_eq!(
                animation.value(),
                0.5,
                "final slot remains in the exclusive fence"
            );
            if preceding_id.is_some() {
                assert_eq!(
                    preceding.value(),
                    0.5,
                    "mixed preceding admission remains deliverable"
                );
            }
            registry.tick_all(
                &crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
            );
            assert_eq!(
                animation.value(),
                1.0,
                "last admitted run makes progress after refusal"
            );
            assert!(!registry.has_running());
            if let Some(preceding_id) = preceding_id {
                assert_eq!(preceding.value(), 1.0, "both mixed admissions complete");
                registry.unregister(&preceding_id);
                registry.detach_child(&preceding_id);
            }
            registry.unregister(&last);
            registry.detach_child(&last);
            assert_eq!(
                registry.try_register_resuming(&refused, Duration::ZERO),
                Err(VsyncRegistrationError::Exhausted)
            );
            assert!(
                registry.attach_child(&child) == Err(VsyncRegistrationError::Exhausted),
                "removal cannot reset exhaustion"
            );

            struct RejectedCapture(Rc<std::sync::atomic::AtomicUsize>);
            impl Drop for RejectedCapture {
                fn drop(&mut self) {
                    self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    panic!("rejected controller capture");
                }
            }
            let drops = Rc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut rejected = AnimationController::builder(Duration::from_secs(1)).build_on(None);
            let probe = RejectedCapture(drops.clone());
            rejected
                .controller()
                .subscribe_status(Rc::new(move |_| {
                    let _capture = &probe;
                }))
                .detach();
            assert_eq!(
                rejected.rebind(Some(&registry)),
                Err(VsyncRegistrationError::Exhausted)
            );
            assert!(!rejected.is_bound());
            assert_eq!(
                drops.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "refusal preserves the owning handle's captures"
            );
            let failure =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rejected.dispose()))
                    .expect_err("explicit owner retirement fails");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("rejected controller capture")
            );
            flui_foundation::panic::retain_opaque_payload(failure);
            assert_eq!(drops.load(std::sync::atomic::Ordering::SeqCst), 1);
            rejected.dispose();
            assert_eq!(
                registry.try_register_resuming(&refused, Duration::ZERO),
                Err(VsyncRegistrationError::Exhausted)
            );
            let fresh = Vsync::new();
            let fresh_id = fresh
                .try_register_resuming(&refused, Duration::ZERO)
                .expect("independent registry still admits");
            refused.forward().expect("fresh run");
            fresh.tick_all(
                &crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
            );
            fresh.tick_all(
                &crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
            );
            assert_eq!(
                refused.value(),
                1.0,
                "fresh registry advances after contained failure"
            );
            fresh.unregister(&fresh_id);
        }
    }

    fn rebind_refused_by_an_exhausted_registry_settles_unbound() {
        for fails in [false, true] {
            let old = Vsync::new();
            let exhausted = Vsync::new();
            let healthy = Vsync::new();
            exhausted.inner.borrow_mut().next_id = u64::MAX;
            let motion = crate::MotionSpec::Curve {
                duration: Duration::from_secs(1),
                curve: crate::ArcCurve::new(crate::curves::LINEAR),
            };
            let mut refused = crate::AnimatedValue::new(0.0, motion.clone(), Some(&old)).unwrap();
            let mut accepted = crate::AnimatedValue::new(0.0, motion, Some(&old)).unwrap();
            let refused_run = refused.animate_to(1.0).unwrap();
            let accepted_run = accepted.animate_to(2.0).unwrap();
            refused_run.when_complete_or_cancel({
                let old = old.clone();
                let healthy = healthy.clone();
                move |result| {
                    assert!(result.is_ok());
                    assert!(old.is_empty());
                    assert_eq!(
                        healthy.len(),
                        1,
                        "accepted tail binds before refusal delivery"
                    );
                    assert!(!fails, "refused migration delivery failure");
                }
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::VsyncUpdate::run(|update| {
                    update.rebind(&mut refused, Some(&exhausted));
                    update.rebind(&mut accepted, Some(&healthy));
                })
            }));
            if fails {
                assert_eq!(
                    flui_foundation::panic::payload_text(result.unwrap_err().as_ref()),
                    Some("refused migration delivery failure")
                );
            } else {
                assert_eq!(result.unwrap(), Err(VsyncRegistrationError::Exhausted));
            }
            assert!(old.is_empty() && exhausted.is_empty());
            assert_eq!(refused.value(), 1.0);
            assert!(refused_run.is_complete() && accepted_run.is_pending());
            let mut clock = crate::MotionClock::new();
            healthy.tick_all(&clock.frame(Duration::ZERO));
            healthy.tick_all(&clock.frame(Duration::from_secs(1)));
            assert_eq!(accepted.value(), 2.0);
            assert!(accepted_run.is_complete());
            drop((refused, accepted));
            assert!(healthy.is_empty());
        }
        for repeat in [false, true] {
            let old = Vsync::new();
            let exhausted = Vsync::new();
            exhausted.inner.borrow_mut().next_id = u64::MAX;
            let mut owner =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
            let observer = owner.controller().clone();
            let run = if repeat {
                observer.repeat(crate::RepeatMode::Restart)
            } else {
                observer.forward()
            }
            .expect("admitted run");
            let mut clock = crate::MotionClock::new();
            old.tick_all(&clock.frame(Duration::ZERO));
            old.tick_all(&clock.frame(Duration::from_millis(400)));
            assert_eq!(observer.value(), 0.4);
            assert_eq!(
                owner.rebind(Some(&exhausted)),
                Err(VsyncRegistrationError::Exhausted)
            );
            assert!(old.is_empty(), "refusal withdraws the preceding seat");
            assert!(exhausted.is_empty());
            assert!(!owner.is_bound());
            assert_eq!(
                owner.rebind(Some(&exhausted)),
                Err(VsyncRegistrationError::Exhausted)
            );
            if repeat {
                assert_eq!(observer.value(), 0.0);
                assert!(run.is_pending(), "an infinite repeat parks on refusal");
                owner
                    .rebind(Some(&old))
                    .expect("independent registry still admits");
                old.tick_all(&clock.frame(Duration::from_secs(1)));
                old.tick_all(&clock.frame(Duration::from_millis(1200)));
                assert_eq!(observer.value(), 0.2);
                drop(owner);
                assert!(run.is_canceled());
            } else {
                assert_eq!(observer.value(), 1.0);
                assert!(run.is_complete(), "a finite run lands without a clock");
                assert!(!observer.is_animating());
            }
        }
    }

    fn a_terminal_frame_counter_still_delivers_fresh_runs() {
        use crate::{Animation, ArcCurve, MotionClock, MotionSpec, curves};
        let registry = Vsync::new();
        registry.inner.borrow_mut().frame = u64::MAX - 2;
        let mut clock = MotionClock::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller();
        let _old = controller.forward().expect("initial run");
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
        for millis in [200, 400] {
            let seam = controller.value();
            let _next = controller
                .retarget(
                    0.8,
                    &MotionSpec::Curve {
                        duration: Duration::from_secs(1),
                        curve: ArcCurve::new(curves::LINEAR),
                    },
                )
                .expect("terminal counter retains run admission");
            registry.tick_all(&clock.frame(Duration::from_millis(millis)));
            assert_eq!(
                controller.value(),
                seam,
                "exhausted frame stamps cannot qualify a continuation"
            );
            registry.tick_all(&clock.frame(Duration::from_millis(millis + 100)));
            assert!(
                controller.value() > seam,
                "subsequent frames still advance the admitted run"
            );
        }
    }

    #[test]
    fn vsync_nesting_and_reentrancy() {
        crate::test_cases::run_cases(&[
            (
                "saturated walk revisions revalidate ancestor admission",
                saturated_walk_revisions_revalidate_ancestor_admission,
            ),
            (
                "a terminal frame counter still delivers fresh runs",
                a_terminal_frame_counter_still_delivers_fresh_runs,
            ),
            (
                "exhausted registry leaves an owner unbound",
                rebind_refused_by_an_exhausted_registry_settles_unbound,
            ),
            (
                "registration exhaustion preserves admitted work",
                registration_exhaustion_preserves_admitted_work,
            ),
        ]);
    }
}
