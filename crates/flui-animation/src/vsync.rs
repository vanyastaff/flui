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

use flui_foundation::panic::RecoveryScope;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use std::cell::RefCell;

use crate::AnimationController;
use crate::animation::{Retirement, Terminal};
use crate::{AnimationTime, FrameTick};
use std::time::Duration;

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

/// A controller could not be admitted to a virtual frame registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VsyncRegistrationError {
    /// The registry permanently consumed its available registration identities.
    #[error("Vsync registration capacity exhausted")]
    Exhausted,
}

/// Seconds from `start` to `now`, both readings of a nanosecond clock, taken on
/// that clock's integer grid.
///
/// Subtracting the two `f64` readings directly leaves the difference one ulp
/// off the value a `Duration` of the same length converts to (`0.12 - 0.02` is
/// `0.09999999999999999`), so a 100 ms run anchored at 20 ms would stop one ulp
/// short of its end and never complete. Measuring elapsed time in integer
/// nanoseconds avoids the trap.
/// One registered controller plus the registry's per-run anchor.
///
/// `run_start_secs` is the virtual instant treated as the current run's
/// `t = 0`; `last_gen` is the controller's `run_generation` observed when that
/// anchor was set. `run_start_secs` is `None` until the first tick anchors it,
/// so registration needs no clock reading.
///
/// Keyed by the registration id (the `u64` inside [`VsyncRegistration`]) in
/// [`VsyncInner::controllers`] rather than carrying its own id — the map key
/// *is* the identity.
struct RegisteredController {
    controller: AnimationController,
    anchor: RunAnchor,
    last_gen: u64,
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
    parents: Vec<VsyncRegistration>,
    request_frame: Option<Rc<dyn Fn()>>,
    next_id: u64,
    muted: bool,
    last_time: crate::AnimationTime,
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
        let outgoing = std::mem::replace(&mut self.inner.borrow_mut().request_frame, request_frame);
        let mut retirement = Retirement::new();
        if self.has_running() {
            Self::request_frame_from(&self.inner, &mut retirement.scope());
        }
        retirement.retire(outgoing);
        retirement.finish();
    }

    fn request_frame_from(owner: &Rc<RefCell<VsyncInner>>, retirement: &mut RecoveryScope<'_>) {
        let (request, parents) = {
            let mut inner = owner.borrow_mut();
            if inner.muted {
                return;
            }
            inner.parents.retain(VsyncRegistration::owner_is_alive);
            (inner.request_frame.clone(), inner.parents.clone())
        };
        if let Some(request) = request {
            retirement.run(|| request());
            retirement.retire(request);
        }
        for parent in parents {
            parent.request_frame(retirement);
        }
    }

    /// Register `controller` so each [`tick_all`](Self::tick_all) advances it on
    /// the virtual timeline.
    ///
    /// The controller is `Clone` (`Rc`-backed); register a clone and keep your
    /// own handle to drive it (`forward`, `reverse`, …). The current run is
    /// anchored lazily on the first tick (or whenever a fresh run bumps
    /// `run_generation`), so this needs no clock reading and the common
    /// register-then-`forward` order anchors `t = 0` cleanly on the first frame
    /// the new run is observed.
    ///
    /// # Panics
    ///
    /// Panics permanently after all available registration identities have been
    /// consumed. Use [`try_register`](Self::try_register) for typed refusal.
    #[cfg(test)]
    pub(crate) fn register(&self, controller: AnimationController) -> VsyncRegistration {
        let controller = Terminal::new(controller);
        match self.try_register(controller.get()) {
            Ok(registration) => registration,
            Err(VsyncRegistrationError::Exhausted) => {
                panic!("Vsync registration capacity exhausted");
            }
        }
    }

    /// Register a borrowed controller, reporting permanent identity exhaustion.
    ///
    /// A refusal leaves the controller and every admitted registration intact.
    ///
    /// # Panics
    /// If requesting an already running controller's first sample fails, the
    /// provisional seat is removed before the wake failure propagates.
    #[cfg(test)]
    pub(crate) fn try_register(
        &self,
        controller: &AnimationController,
    ) -> Result<VsyncRegistration, VsyncRegistrationError> {
        let registration = self.try_register_with_anchor(controller, RunAnchor::Fresh)?;
        let mut retirement = Retirement::new();
        if controller.walk_probe().live_running {
            registration.request_frame(&mut retirement.scope());
        }
        if retirement.has_failure() {
            retirement.run(|| self.unregister(&registration));
        }
        retirement.finish();
        Ok(registration)
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
        let id = inner
            .reserve_slot()
            .ok_or(VsyncRegistrationError::Exhausted)?;
        inner.controllers.insert(
            id,
            RegisteredController {
                controller: controller.clone(),
                anchor,
                last_gen,
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
    /// A cycle would hang the tick walk; nesting a registry under itself (or
    /// under one of its own descendants) is a caller bug, so it is refused and
    /// logged rather than linked.
    /// A registry whose identities are exhausted also refuses attachment.
    pub fn attach_child(&self, child: &Vsync) -> Option<VsyncRegistration> {
        if child.contains(self) {
            tracing::error!(
                "BUG: a Vsync registry cannot be nested inside itself or its own \
                 descendant; the child is not attached and its controllers will not tick"
            );
            return None;
        }
        let mut inner = self.inner.borrow_mut();
        let slot = inner.reserve_slot()?;
        inner.children.push(RegisteredChild {
            slot,
            child: child.clone(),
        });
        let registration = VsyncRegistration {
            owner: Rc::downgrade(&self.inner),
            slot,
        };
        drop(inner);
        child.inner.borrow_mut().parents.push(registration.clone());
        let mut retirement = Retirement::new();
        if child.has_running() {
            registration.request_frame(&mut retirement.scope());
        }
        if retirement.has_failure() {
            retirement.run(|| self.detach_child(&registration));
        }
        retirement.finish();
        Some(registration)
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
            removed
                .child
                .inner
                .borrow_mut()
                .parents
                .retain(|parent| parent != id);
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

    /// Whether `other` is this registry or one of its (transitive) children.
    fn contains(&self, other: &Vsync) -> bool {
        if Rc::ptr_eq(&self.inner, &other.inner) {
            return true;
        }
        let children: Vec<Vsync> = self
            .inner
            .borrow_mut()
            .children
            .iter()
            .map(|registered| registered.child.clone())
            .collect();
        children.iter().any(|child| child.contains(other))
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
    /// controller lands where the wall clock says it should be — it does not
    /// resume from where it stopped: a muted clock still runs, only the
    /// callback is withheld.
    pub fn set_muted(&self, muted: bool) {
        let changed = {
            let mut inner = self.inner.borrow_mut();
            let changed = inner.muted != muted;
            inner.muted = muted;
            changed
        };
        if changed && !muted && self.has_running() {
            let mut retirement = Retirement::new();
            Self::request_frame_from(&self.inner, &mut retirement.scope());
            retirement.finish();
        }
    }

    /// The number of controllers registered **with this registry**, not
    /// counting nested ones (see [`attach_child`](Self::attach_child)).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.borrow_mut().controllers.len()
    }

    /// Whether no controllers are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.borrow_mut().controllers.is_empty()
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
    /// bare `status().is_running()`: a controller `dispose()`d mid-run keeps
    /// whatever running status it had (`dispose()` deliberately leaves
    /// `status` untouched), so `live_running` is what tells this apart from
    /// a genuinely live run — otherwise a disposed-but-not-yet-unregistered
    /// controller would hold the frame loop open forever.
    #[must_use]
    pub fn has_running(&self) -> bool {
        let (mine, children) = {
            let inner = self.inner.borrow_mut();
            // A muted registry advances nothing — not its own controllers, not a
            // nested registry's — so nothing under it can hold the frame loop
            // open.
            if inner.muted {
                return false;
            }
            (
                inner
                    .controllers
                    .values()
                    .any(|c| c.controller.walk_probe().live_running),
                inner
                    .children
                    .iter()
                    .map(|registered| registered.child.clone())
                    .collect::<Vec<_>>(),
            )
        };
        // Nested registries hold real controllers: a `TickerMode` (and so every
        // `Hero` child) puts its subtree's animations in one, and a frame-loop
        // gate that only looked at this level would stall them after one frame.
        mine || children.iter().any(Vsync::has_running)
    }

    /// Advance live controller registrations using a presentation's typed tick.
    ///
    /// A fresh run anchors on its first observed frame. Registration migration
    /// preserves its last sampled elapsed time. Older ticks hold at this
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
        retirement.run_with(|retirement| self.tick_all_with_retirement(tick.now(), retirement));
        retirement.finish();
    }

    fn tick_all_with_retirement(
        &self,
        now: crate::AnimationTime,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let (fence, children, muted) = {
            let mut inner = self.inner.borrow_mut();
            inner.last_time = inner.last_time.max(now);
            (
                inner.next_id,
                inner
                    .children
                    .iter()
                    .map(|registered| registered.child.clone())
                    .collect::<Vec<_>>(),
                inner.muted,
            )
        };

        // A muted registry delivers no tick — not to its own controllers, not
        // to a nested registry's. The anchors are absolute and left alone, so
        // the clock keeps running underneath: an unmute lands the animation
        // where the wall clock says.
        if muted {
            return;
        }

        for child in children {
            let child = Terminal::new(child);
            let now = self.inner.borrow().last_time;
            retirement.run_with(|retirement| child.tick_all_with_retirement(now, retirement));
            retirement.retire(child);
        }

        let mut cursor = 0u64;
        loop {
            let step = {
                let mut inner = self.inner.borrow_mut();
                let now = inner.last_time;
                // Re-read per iteration, not only captured at entry above: a
                // listener that mutes the registry mid-walk must stop the
                // rest of this frame's entries from ticking.
                if inner.muted {
                    RegistryWalkStep::Finished
                } else if let Some((&id, registered)) =
                    inner.controllers.range_mut(cursor..fence).next()
                {
                    cursor = id + 1;
                    // One lock instead of two: `walk_probe` reads
                    // `run_generation` AND `live_running` (which folds in
                    // `disposed` — see its own doc) under a single
                    // controller lock.
                    let probe = registered.controller.walk_probe();
                    if probe.generation != registered.last_gen {
                        registered.last_gen = probe.generation;
                        registered.anchor = RunAnchor::Fresh;
                    }
                    if probe.live_running {
                        // `run_start_secs` is `Some` here — set in the branch
                        // above on this same call if it was `None`.
                        let elapsed = registered.anchor.elapsed(now);
                        RegistryWalkStep::Running(registered.controller.clone(), elapsed)
                    } else {
                        RegistryWalkStep::NotRunning
                    }
                } else {
                    RegistryWalkStep::Finished
                }
            };

            match step {
                RegistryWalkStep::Finished => break,
                RegistryWalkStep::NotRunning => {}
                RegistryWalkStep::Running(controller, elapsed) => {
                    let controller = Terminal::new(controller);
                    retirement.run_with(|retirement| {
                        controller.tick_at_with_retirement(elapsed, retirement);
                    });
                    retirement.retire(controller);
                }
            }
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
    /// The controller is running; tick it with the given elapsed seconds
    /// once the registry lock guarding this step is released.
    Running(AnimationController, Duration),
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
    use crate::{Animation, AnimationStatus};

    fn controller(ms: u64) -> AnimationController {
        AnimationController::builder(Duration::from_millis(ms)).build()
    }

    /// Muting is **structural**, so nesting composes as a logical AND: an inner
    /// registry that is itself unmuted still never advances while an ancestor
    /// is muted — the ancestor simply never forwards the tick. There is no
    /// flag to compose, and no way to get the composition wrong.
    fn a_muted_ancestor_starves_an_unmuted_descendant() {
        let outer = Vsync::new();
        let middle = Vsync::new();
        let inner = Vsync::new();
        outer.attach_child(&middle).expect("nested");
        middle.attach_child(&inner).expect("nested");

        let animation = controller(1000);
        inner.register(animation.clone());
        let _ = animation.forward();

        middle.set_muted(true);
        assert!(!inner.is_muted(), "the innermost registry is enabled");

        outer.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)));
        outer.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)));
        assert_eq!(
            animation.value(),
            0.0,
            "a muted ancestor starves the enabled descendant"
        );

        // Unmute: the first tick through anchors this run's `t = 0` (a
        // controller that has never been ticked has no anchor yet), the next
        // one advances it.
        middle.set_muted(false);
        outer.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)));
        outer.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.4)));
        assert!(
            animation.value() > 0.0,
            "the tick flows through the unmuted ancestor"
        );

        animation.dispose();
    }

    /// A cycle would hang the tick walk, so nesting a registry inside itself
    /// (or inside one of its own descendants) is refused, not linked.
    fn a_cyclic_nesting_is_refused() {
        let outer = Vsync::new();
        let inner = Vsync::new();
        outer.attach_child(&inner).expect("nested");

        assert!(
            inner.attach_child(&outer).is_none(),
            "nesting an ancestor under its own descendant is refused"
        );
        assert!(
            outer.attach_child(&outer).is_none(),
            "and so is nesting a registry under itself"
        );

        // The tick walk still terminates.
        outer.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)));
    }

    /// A status listener that unregisters its own controller — what a route does
    /// when its exit transition reaches `dismissed` and it disposes itself — must
    /// not deadlock.
    ///
    /// Regression: `tick_all` used to hold the registry lock across `tick_at`, so
    /// the listener's `unregister` re-entered a non-reentrant `parking_lot::Mutex`
    /// and hung. Found by the first end-to-end `PopupRoute` pop and recorded in
    /// ADR-0020 ("A real deadlock in `flui-animation`, found by the first end-to-end
    /// pop").
    fn a_listener_may_unregister_from_inside_tick_all() {
        let vsync = Vsync::new();
        let controller = AnimationController::builder(Duration::from_millis(100)).build();
        let registration = vsync.register(controller.clone());

        let slot: Rc<RefCell<Option<VsyncRegistration>>> =
            Rc::new(RefCell::new(Some(registration)));
        let vsync_for_listener = vsync.clone();
        let slot_for_listener = Rc::clone(&slot);
        controller.add_status_listener(Rc::new(move |status| {
            if status == AnimationStatus::Completed
                && let Some(registration) = slot_for_listener.borrow_mut().take()
            {
                vsync_for_listener.unregister(&registration);
            }
        }));

        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)));
        vsync.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.2))); // past the 100 ms duration → Completed → unregisters

        assert!(slot.borrow().is_none(), "the listener ran and unregistered");
        assert_eq!(vsync.len(), 0, "and the registry dropped the controller");

        controller.dispose();
    }

    fn registration_exhaustion_preserves_admitted_work() {
        for (remaining, child_last) in [(1, false), (1, true), (2, false), (2, true)] {
            let registry = Vsync::new();
            // Only the counter boundary requires private setup. Every admission,
            // refusal, removal and tick below uses the production public API.
            registry.inner.borrow_mut().next_id = u64::MAX - remaining;
            let preceding = AnimationController::builder(Duration::from_secs(1)).build();
            let preceding_child = Vsync::new();
            let preceding_id = if remaining == 2 {
                let id = if child_last {
                    registry.register(preceding.clone())
                } else {
                    preceding_child.register(preceding.clone());
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
                child.register(animation.clone());
                registry
                    .attach_child(&child)
                    .expect("last child identity admitted")
            } else {
                registry
                    .try_register(&animation)
                    .expect("last controller identity admitted")
            };
            animation.forward().expect("last admitted run starts");
            let refused = AnimationController::builder(Duration::from_secs(1)).build();
            for handle in [&registry, &registry.clone()] {
                assert_eq!(
                    handle.try_register(&refused),
                    Err(VsyncRegistrationError::Exhausted)
                );
                assert!(handle.attach_child(&Vsync::new()).is_none());
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
                registry.try_register(&refused),
                Err(VsyncRegistrationError::Exhausted)
            );
            assert!(
                registry.attach_child(&child).is_none(),
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
            let rejected = AnimationController::builder(Duration::from_secs(1)).build();
            let probe = RejectedCapture(drops.clone());
            rejected.add_status_listener(Rc::new(move |_| {
                let _capture = &probe;
            }));
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registry.register(rejected)
            }))
            .expect_err("owned wrapper preserves intentional exhaustion panic");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("Vsync registration capacity exhausted")
            );
            flui_foundation::panic::retain_opaque_payload(failure);
            assert_eq!(
                drops.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "rejected opaque owner retained during exhaustion unwind"
            );
            assert_eq!(
                registry.try_register(&refused),
                Err(VsyncRegistrationError::Exhausted)
            );
            let fresh = Vsync::new();
            let fresh_id = fresh
                .try_register(&refused)
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
        for repeat in [false, true] {
            let old = Vsync::new();
            let exhausted = Vsync::new();
            exhausted.inner.borrow_mut().next_id = u64::MAX;
            let mut owner =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
            let observer = owner.controller().clone();
            let run = if repeat {
                observer.repeat(false)
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

    #[test]
    fn vsync_nesting_and_reentrancy() {
        crate::test_cases::run_cases(&[
            (
                "exhausted registry leaves an owner unbound",
                rebind_refused_by_an_exhausted_registry_settles_unbound,
            ),
            (
                "registration exhaustion preserves admitted work",
                registration_exhaustion_preserves_admitted_work,
            ),
            (
                "a muted ancestor starves an unmuted descendant",
                a_muted_ancestor_starves_an_unmuted_descendant,
            ),
            ("a cyclic nesting is refused", a_cyclic_nesting_is_refused),
            (
                "a listener may unregister from inside tick all",
                a_listener_may_unregister_from_inside_tick_all,
            ),
        ]);
    }
}
