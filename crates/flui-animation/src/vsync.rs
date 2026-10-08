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
//! ## Why the binding drives controllers here, not via each controller's own
//! scheduler-ticker
//!
//! [`AnimationController`] also carries an auto-scheduling `Ticker` that
//! advances it off wall-clock `Instant::now()` — correct for a real display, but
//! non-deterministic. `Vsync` bypasses that ticker entirely: it calls
//! [`AnimationController::tick_at`] with *virtual* seconds, so a headless frame
//! driver can step animations frame-by-frame with no `thread::sleep`.
//!
//! ## Restart-awareness
//!
//! A controller re-zeros its run epoch on every fresh
//! `forward`/`reverse`/`animate_to`/… (it bumps
//! [`AnimationController::run_generation`]). `Vsync` watches that counter and
//! re-anchors each controller's per-run `t = 0` whenever it advances — so a
//! controller run twice (forward to completion, then reverse) is ticked from the
//! second run's own start instead of snapping to its target on the first frame.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::AnimationController;
use crate::animation::{Retirement, Terminal};

/// Opaque handle identifying one controller registered with a [`Vsync`].
///
/// Returned by [`Vsync::register`]; pass it to [`Vsync::unregister`] when the
/// owner (typically an implicitly-animated widget's state in `dispose`) is torn
/// down, so the registry does not pin the controller alive past its widget.
#[derive(Debug, Clone)]
pub struct VsyncRegistration {
    owner: Weak<Mutex<VsyncInner>>,
    slot: u64,
}

impl PartialEq for VsyncRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && Weak::ptr_eq(&self.owner, &other.owner)
    }
}

impl Eq for VsyncRegistration {}

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
fn elapsed_since(start: f64, now: f64) -> f64 {
    const NANOS_PER_SEC: f64 = 1e9;
    let nanos = (now * NANOS_PER_SEC).round() - (start * NANOS_PER_SEC).round();
    nanos / NANOS_PER_SEC
}

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
    run_start_secs: Option<f64>,
    last_gen: u64,
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
    next_id: u64,
    muted: bool,
}

impl VsyncInner {
    fn reserve_slot(&mut self) -> Option<u64> {
        let next = self.next_id.checked_add(1)?;
        Some(std::mem::replace(&mut self.next_id, next))
    }
}

/// A shared, restart-aware controller registry driven once per frame.
///
/// Cloning a `Vsync` clones an `Arc`-backed handle: every clone observes the
/// same registry, so the handle a `VsyncScope` hands to a subtree and the one a
/// binding ticks are the same registry.
#[derive(Clone, Default)]
pub struct Vsync {
    inner: Arc<Mutex<VsyncInner>>,
}

impl Vsync {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `controller` so each [`tick_all`](Self::tick_all) advances it on
    /// the virtual timeline.
    ///
    /// The controller is `Clone` (`Arc`-backed); register a clone and keep your
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
    pub fn register(&self, controller: AnimationController) -> VsyncRegistration {
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
    pub fn try_register(
        &self,
        controller: &AnimationController,
    ) -> Result<VsyncRegistration, VsyncRegistrationError> {
        let last_gen = controller.run_generation();
        let mut inner = self.inner.lock();
        let id = inner
            .reserve_slot()
            .ok_or(VsyncRegistrationError::Exhausted)?;
        inner.controllers.insert(
            id,
            RegisteredController {
                controller: controller.clone(),
                run_start_secs: None,
                last_gen,
            },
        );
        Ok(VsyncRegistration {
            owner: Arc::downgrade(&self.inner),
            slot: id,
        })
    }

    /// Remove the controller previously registered under `id`. Idempotent: an
    /// unknown or already-removed id is a no-op.
    pub fn unregister(&self, id: &VsyncRegistration) {
        if !Weak::ptr_eq(&id.owner, &Arc::downgrade(&self.inner)) {
            return;
        }
        let removed = {
            let mut inner = self.inner.lock();
            inner.controllers.remove(&id.slot)
        };
        // The last controller owner can retire user captures that reenter this
        // registry. Its registration is absent and the guard is released first.
        drop(removed);
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
        let mut inner = self.inner.lock();
        let slot = inner.reserve_slot()?;
        inner.children.push(RegisteredChild {
            slot,
            child: child.clone(),
        });
        Some(VsyncRegistration {
            owner: Arc::downgrade(&self.inner),
            slot,
        })
    }

    /// Detach the child registry previously attached under `id`. Idempotent.
    pub fn detach_child(&self, id: &VsyncRegistration) {
        if !Weak::ptr_eq(&id.owner, &Arc::downgrade(&self.inner)) {
            return;
        }
        let removed = {
            let mut inner = self.inner.lock();
            inner
                .children
                .iter()
                .position(|child| child.slot == id.slot)
                .map(|index| inner.children.remove(index))
        };
        // Removing one child preserves the remaining registration order. Its
        // last controller captures must retire after releasing the parent guard.
        drop(removed);
    }

    /// Whether both handles name the **same** registry (`Arc` identity) — how a
    /// consumer tells "the ambient registry changed" from "same registry, fresh
    /// clone".
    #[must_use]
    pub fn is_same(&self, other: &Vsync) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether `other` is this registry or one of its (transitive) children.
    fn contains(&self, other: &Vsync) -> bool {
        if Arc::ptr_eq(&self.inner, &other.inner) {
            return true;
        }
        let children: Vec<Vsync> = self
            .inner
            .lock()
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
        self.inner.lock().muted
    }

    /// Mute or unmute this registry: while muted it delivers no ticks, to its
    /// own controllers or to a nested registry's.
    ///
    /// **The clock keeps running.** Run anchors are absolute, so an unmuted
    /// controller lands where the wall clock says it should be — it does not
    /// resume from where it stopped: a muted clock still runs, only the
    /// callback is withheld.
    pub fn set_muted(&self, muted: bool) {
        self.inner.lock().muted = muted;
    }

    /// The number of controllers registered **with this registry**, not
    /// counting nested ones (see [`attach_child`](Self::attach_child)).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().controllers.len()
    }

    /// Whether no controllers are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().controllers.is_empty()
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
            let inner = self.inner.lock();
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

    /// Advance every registered, running controller to virtual instant
    /// `now_secs` (elapsed seconds on the driver's virtual clock).
    ///
    /// For each controller: if its `run_generation` advanced since the last
    /// observation (a fresh run was just established) or it has no anchor yet,
    /// re-anchor `t = 0` to `now_secs`; then, if the controller reports running,
    /// tick it with the raw seconds elapsed since that anchor. A non-running
    /// controller is skipped (its anchor is set on the frame it next starts) —
    /// folding in `disposed` (see the controller's crate-private
    /// `walk_probe`), so a disposed-but-not-unregistered controller is
    /// skipped too, not just one that settled normally.
    ///
    /// `now_secs` is expected to be a **non-decreasing** virtual clock across
    /// calls. There is no clamp here: [`tick_at`](AnimationController::tick_at)
    /// already clamps its own run-relative elapsed time at 0, so a backwards
    /// step re-samples the controller's pure time function — never below that
    /// run's `t = 0` — rather than "holding" the run; a `.max(0.0)` in this
    /// method would change no observed value.
    ///
    /// # The registry lock is **not** held while ticking
    ///
    /// `tick_at` fires the controller's status and value listeners, and a listener
    /// may legitimately [`unregister`](Self::unregister): a route whose exit
    /// transition reaches `dismissed` disposes itself from that very listener, and
    /// disposal unregisters its controller. Holding the lock across `tick_at` made
    /// that re-entrant — and `parking_lot::Mutex` is not reentrant, so it
    /// deadlocked rather than panicked.
    ///
    /// So the registry is walked one entry at a time: each controller is looked
    /// up, its bookkeeping updated, and the lock dropped *before* it is ticked.
    /// Walking one entry at a time — rather than snapshotting every due
    /// controller up front — preserves the property the original loop had: a
    /// controller that an **earlier** controller's listener starts during this
    /// same call (a `Scrollable` handing off to its fling controller) is
    /// anchored and ticked in this frame, not the next.
    ///
    /// # An indexed cursor walk, not a per-frame id snapshot
    ///
    /// `controllers` is a [`BTreeMap`] keyed by registration id, and ids are
    /// `next_id` post-increments that are never reused — so ascending key
    /// order *is* registration order. `tick_all` reads `fence = next_id` once
    /// at entry, then walks `controllers.range_mut(cursor..fence)` one entry
    /// at a time, advancing `cursor` past each id it visits. That range bound
    /// — not a captured id list — is what gives the walk the same reentrancy
    /// guarantees the old snapshot-then-`find` scan had:
    ///
    /// - A controller *registered* during this call gets an id ≥ `fence`
    ///   (`next_id` only grows), so the walk's upper bound excludes it —
    ///   ticked next frame.
    /// - A controller *unregistered* during this call (by an earlier
    ///   listener) is removed from the map outright, so the walk simply never
    ///   reaches its key — skipped, with no lookup-miss branch to write.
    /// - Unregistering a later controller and re-registering the same
    ///   controller from an earlier listener gives the new registration an id
    ///   that is also ≥ `fence`: not ticked this call, and its anchor starts
    ///   fresh (`run_start_secs: None`) rather than inheriting the old
    ///   registration's — no aliasing between the two ids.
    /// - A listener that restarts an **earlier**, already-visited controller
    ///   does not get it re-ticked this call: the cursor only moves forward,
    ///   never back. Same as the old snapshot's behavior.
    ///
    /// `muted` is **re-read under the per-iteration lock**, not only at
    /// entry: a listener that mutes the registry mid-walk stops the remaining
    /// entries of *this* frame from ticking. Nested `children` registries are
    /// still sampled **once at entry** and ticked before the cursor walk
    /// starts, exactly as before: a child attached via
    /// [`attach_child`](Self::attach_child) from a listener mid-walk is first
    /// ticked on the *next* call — a ticker started mid-frame schedules for
    /// the next frame.
    ///
    /// Cost: **O(log N)** per register/unregister/lookup — each
    /// `range_mut(cursor..fence).next()` is its own fresh seek, since the
    /// lock (and so the map borrow) is dropped between steps; one such seek
    /// per resident controller per pump, so **O(N log N)** per pump.
    /// [`has_running`](Self::has_running) stays O(N) — see its doc for why.
    ///
    /// Non-finite instants are ignored before any run anchor changes. A child
    /// registry or controller failure leaves the remaining admitted frame peers
    /// deliverable; the walk resumes its first failure after those peers tick.
    pub fn tick_all(&self, now_secs: f64) {
        let mut retirement = Retirement::new();
        retirement.run_with(|retirement| self.tick_all_with_retirement(now_secs, retirement));
        retirement.finish();
    }

    fn tick_all_with_retirement(&self, now_secs: f64, retirement: &mut Retirement) {
        if !now_secs.is_finite() {
            return;
        }
        let (fence, children, muted) = {
            let inner = self.inner.lock();
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
            retirement.run_with(|retirement| child.tick_all_with_retirement(now_secs, retirement));
            retirement.retire(child);
        }

        let mut cursor = 0u64;
        loop {
            let step = {
                let mut inner = self.inner.lock();
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
                    if probe.generation != registered.last_gen
                        || registered.run_start_secs.is_none()
                    {
                        registered.last_gen = probe.generation;
                        registered.run_start_secs = Some(now_secs);
                    }
                    if probe.live_running {
                        // `run_start_secs` is `Some` here — set in the branch
                        // above on this same call if it was `None`.
                        let run_start = registered.run_start_secs.unwrap_or(now_secs);
                        RegistryWalkStep::Running(
                            registered.controller.clone(),
                            elapsed_since(run_start, now_secs),
                        )
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
    Running(AnimationController, f64),
}

impl std::fmt::Debug for Vsync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vsync")
            .field("registered", &self.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_scheduler::UpdateScheduler;

    use super::*;
    use crate::{Animation, AnimationStatus};

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), &UpdateScheduler::new())
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

        outer.tick_all(0.0);
        outer.tick_all(0.5);
        assert_eq!(
            animation.value(),
            0.0,
            "a muted ancestor starves the enabled descendant"
        );

        // Unmute: the first tick through anchors this run's `t = 0` (a
        // controller that has never been ticked has no anchor yet), the next
        // one advances it.
        middle.set_muted(false);
        outer.tick_all(1.0);
        outer.tick_all(1.4);
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
        outer.tick_all(0.0);
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
        let controller =
            AnimationController::new(Duration::from_millis(100), &UpdateScheduler::new());
        let registration = vsync.register(controller.clone());

        let slot: Arc<Mutex<Option<VsyncRegistration>>> = Arc::new(Mutex::new(Some(registration)));
        let vsync_for_listener = vsync.clone();
        let slot_for_listener = Arc::clone(&slot);
        controller.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed
                && let Some(registration) = slot_for_listener.lock().take()
            {
                vsync_for_listener.unregister(&registration);
            }
        }));

        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0);
        vsync.tick_all(0.2); // past the 100 ms duration → Completed → unregisters

        assert!(slot.lock().is_none(), "the listener ran and unregistered");
        assert_eq!(vsync.len(), 0, "and the registry dropped the controller");

        controller.dispose();
    }

    fn registration_exhaustion_preserves_admitted_work() {
        for (remaining, child_last) in [(1, false), (1, true), (2, false), (2, true)] {
            let registry = Vsync::new();
            // Only the counter boundary requires private setup. Every admission,
            // refusal, removal and tick below uses the production public API.
            registry.inner.lock().next_id = u64::MAX - remaining;
            let preceding = AnimationController::without_ticker(Duration::from_secs(1));
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
            let animation = AnimationController::without_ticker(Duration::from_secs(1));
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
            let refused = AnimationController::without_ticker(Duration::from_secs(1));
            for handle in [&registry, &registry.clone()] {
                assert_eq!(
                    handle.try_register(&refused),
                    Err(VsyncRegistrationError::Exhausted)
                );
                assert!(handle.attach_child(&Vsync::new()).is_none());
            }
            registry.tick_all(0.0);
            registry.tick_all(0.5);
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
            registry.tick_all(1.0);
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

            struct RejectedCapture(Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for RejectedCapture {
                fn drop(&mut self) {
                    self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    panic!("rejected controller capture");
                }
            }
            let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let rejected = AnimationController::without_ticker(Duration::from_secs(1));
            let probe = RejectedCapture(drops.clone());
            rejected.add_status_listener(Arc::new(move |_| {
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
            fresh.tick_all(0.0);
            fresh.tick_all(1.0);
            assert_eq!(
                refused.value(),
                1.0,
                "fresh registry advances after contained failure"
            );
            fresh.unregister(&fresh_id);
        }
    }

    #[test]
    fn vsync_nesting_and_reentrancy() {
        crate::test_cases::run_cases(&[
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
