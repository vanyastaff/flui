//! [`Vsync`] — a shared, restart-aware registry that drives
//! [`AnimationController`]s off a single virtual timeline.
//!
//! A deterministic frame driver (e.g. `flui_testing::HeadlessBinding`) owns one
//! `Vsync` and calls [`tick_all`](Vsync::tick_all) once per frame with the
//! current virtual instant. Controllers reach the same registry ambiently — in
//! the widget layer a `VsyncScope` inherited-view hands a clone down a subtree,
//! and an implicitly-animated widget registers its controller in `init_state`.
//! This is the FLUI-native, non-singleton analogue of Flutter's
//! `SchedulerBinding` owning every `Ticker`.
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
use std::sync::Arc;

use parking_lot::Mutex;

use crate::AnimationController;

/// Opaque handle identifying one controller registered with a [`Vsync`].
///
/// Returned by [`Vsync::register`]; pass it to [`Vsync::unregister`] when the
/// owner (typically an implicitly-animated widget's state in `dispose`) is torn
/// down, so the registry does not pin the controller alive past its widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VsyncRegistration(u64);

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
    id: VsyncRegistration,
    child: Vsync,
}

#[derive(Default)]
struct VsyncInner {
    /// Keyed by the registration id, which is also the registration
    /// *order*: ids are `next_id` post-increments and are never reused, so
    /// ascending key order is ascending registration order. `tick_all`'s
    /// cursor walk relies on that order to replace a per-frame id snapshot.
    controllers: BTreeMap<u64, RegisteredController>,
    /// Nested registries — Flutter's `TickerMode` mutes a *subtree*'s tickers
    /// (`ticker_provider.dart:397`); FLUI's widgets take their `Vsync` from the
    /// ambient `VsyncScope`, so a subtree's registry is a child of the one
    /// above it and muting is structural: a muted registry ticks neither its
    /// own controllers nor its children's. That is Flutter's
    /// `_updateEffectiveMode` AND (`ticker_provider.dart:246-252`) — a nested
    /// enabled `TickerMode` cannot re-enable a muted ancestor, because the
    /// ancestor never forwards the tick.
    children: Vec<RegisteredChild>,
    next_id: u64,
    muted: bool,
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
    pub fn register(&self, controller: AnimationController) -> VsyncRegistration {
        let mut inner = self.inner.lock();
        let id = inner.next_id;
        inner.next_id += 1;
        let last_gen = controller.run_generation();
        inner.controllers.insert(
            id,
            RegisteredController {
                controller,
                run_start_secs: None,
                last_gen,
            },
        );
        VsyncRegistration(id)
    }

    /// Remove the controller previously registered under `id`. Idempotent: an
    /// unknown or already-removed id is a no-op.
    pub fn unregister(&self, id: VsyncRegistration) {
        self.inner.lock().controllers.remove(&id.0);
    }

    /// Nest `child` under this registry: [`tick_all`](Self::tick_all) forwards
    /// to it — unless this registry is [`muted`](Self::set_muted).
    ///
    /// A cycle would hang the tick walk; nesting a registry under itself (or
    /// under one of its own descendants) is a caller bug, so it is refused and
    /// logged rather than linked.
    pub fn attach_child(&self, child: &Vsync) -> Option<VsyncRegistration> {
        if child.contains(self) {
            tracing::error!(
                "BUG: a Vsync registry cannot be nested inside itself or its own \
                 descendant; the child is not attached and its controllers will not tick"
            );
            return None;
        }
        let mut inner = self.inner.lock();
        let id = VsyncRegistration(inner.next_id);
        inner.next_id += 1;
        inner.children.push(RegisteredChild {
            id,
            child: child.clone(),
        });
        Some(id)
    }

    /// Detach the child registry previously attached under `id`. Idempotent.
    pub fn detach_child(&self, id: VsyncRegistration) {
        self.inner.lock().children.retain(|c| c.id != id);
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
    /// registry stop advancing (`ticker.dart:124-128`'s `muted` semantics,
    /// lifted to the registry a `TickerMode` owns).
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.inner.lock().muted
    }

    /// Mute or unmute this registry: while muted it delivers no ticks, to its
    /// own controllers or to a nested registry's.
    ///
    /// **The clock keeps running.** Run anchors are absolute, so an unmuted
    /// controller lands where the wall clock says it should be — it does not
    /// resume from where it stopped. That is Flutter's `Ticker.muted`
    /// convention: "a ticker's clock can still run, but the callback will not
    /// be called" (`ticker.dart:102-104`).
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
    /// Used by a production frame driver (e.g. `flui-app`'s `UiRealm`) to decide whether
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
    /// entries of *this* frame from ticking — Flutter honors a mid-frame
    /// `Ticker.muted = true` the same way: the `muted` setter's
    /// `unscheduleTick` call (`scheduler/ticker.dart`) hands the cancellation
    /// to `SchedulerBinding.cancelFrameCallbackWithId`, and it is
    /// `handleBeginFrame`'s callback loop (`scheduler/binding.dart`), which
    /// skips any id already in `_removedIds`, that actually honors it within
    /// the same frame. Nested `children` registries are still sampled **once
    /// at entry** and ticked before the cursor walk starts, exactly as
    /// before: a child attached via [`attach_child`](Self::attach_child) from
    /// a listener mid-walk is first ticked on the *next* call — a ticker
    /// started mid-frame schedules for the next frame in Flutter too.
    ///
    /// Cost: **O(log N)** per register/unregister/lookup — each
    /// `range_mut(cursor..fence).next()` is its own fresh seek, since the
    /// lock (and so the map borrow) is dropped between steps; one such seek
    /// per resident controller per pump, so **O(N log N)** per pump.
    /// [`has_running`](Self::has_running) stays O(N) — see its doc for why.
    pub fn tick_all(&self, now_secs: f64) {
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
        // where the wall clock says (`ticker.dart:102-104`).
        if muted {
            return;
        }

        for child in children {
            child.tick_all(now_secs);
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
                            now_secs - run_start,
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
                RegistryWalkStep::Running(controller, elapsed) => controller.tick_at(elapsed),
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

    use flui_foundation::Listenable;
    use flui_scheduler::UpdateScheduler;

    use super::*;
    use crate::{Animation, AnimationStatus};

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), &UpdateScheduler::new())
    }

    /// A muted registry delivers no ticks — neither to its own controllers nor
    /// to a nested registry's — while the **clock keeps running**: Flutter's
    /// `Ticker.muted` is "a ticker's clock can still run, but the callback will
    /// not be called" (`ticker.dart:102-104`), so an unmuted animation lands
    /// where the wall clock says it should be, not where it stopped.
    ///
    /// (FLUI's own `Ticker::mute` freezes elapsed time instead. That type is a
    /// different layer and has no consumer here; this registry follows the
    /// `Ticker.muted` *convention*, which is what `TickerMode` mutes.)
    ///
    /// Red-check: drop the `if muted { return; }` guard in `tick_all` — the
    /// muted controller advances and the freeze assertion fails.
    #[test]
    fn a_muted_registry_delivers_no_ticks_while_its_clock_runs_on() {
        let parent = Vsync::new();
        let child = Vsync::new();
        parent.attach_child(&child).expect("nested");

        let outer = controller(1000);
        let inner = controller(1000);
        parent.register(outer.clone());
        child.register(inner.clone());
        let _ = outer.forward();
        let _ = inner.forward();

        parent.tick_all(0.0);
        parent.tick_all(0.5);
        assert!(
            (outer.value() - 0.5).abs() < 1e-3,
            "the parent's controller ran"
        );
        assert!(
            (inner.value() - 0.5).abs() < 1e-3,
            "and the tick reached the nested registry"
        );

        // Mute the child only: the parent keeps running, the child freezes.
        child.set_muted(true);
        parent.tick_all(0.8);
        assert!(
            (outer.value() - 0.8).abs() < 1e-3,
            "the parent is unaffected"
        );
        assert!(
            (inner.value() - 0.5).abs() < 1e-3,
            "a muted registry does not advance"
        );

        // Unmute: the clock kept running, so the next tick lands at the wall
        // clock's position — Flutter's documented `Ticker.muted` convention.
        child.set_muted(false);
        parent.tick_all(0.9);
        assert!(
            (inner.value() - 0.9).abs() < 1e-3,
            "an unmuted registry catches up with the clock that kept running \
             (`ticker.dart:102-104`), it does not resume from where it stopped"
        );

        outer.dispose();
        inner.dispose();
    }

    /// Muting is **structural**, so nesting composes as Flutter's
    /// `_updateEffectiveMode` AND (`ticker_provider.dart:246-252`): an inner
    /// registry that is itself unmuted still never advances while an ancestor
    /// is muted — the ancestor simply never forwards the tick. There is no
    /// flag to compose, and no way to get the composition wrong.
    #[test]
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
    #[test]
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

    /// The frame-loop gate asks `has_running`. A `TickerMode` — and therefore
    /// every `Hero` child — puts its subtree's controllers in a **nested**
    /// registry, so a gate that only looked at the top level would report "no
    /// animation" and stall them after a single frame.
    ///
    /// Red-check: drop the recursion in `has_running` — the nested controller is
    /// invisible and the first assertion fails.
    #[test]
    fn has_running_sees_controllers_in_nested_registries() {
        let parent = Vsync::new();
        let child = Vsync::new();
        parent.attach_child(&child).expect("nested");

        let animation = controller(1000);
        child.register(animation.clone());
        assert!(!parent.has_running(), "nothing is running yet");

        let _ = animation.forward();
        assert!(
            parent.has_running(),
            "a running controller in a nested registry keeps the frame loop alive"
        );

        // A muted registry cannot advance anything, so it cannot keep the loop
        // alive either.
        child.set_muted(true);
        assert!(
            !parent.has_running(),
            "a muted registry's controllers do not hold the frame loop open"
        );

        animation.dispose();
    }

    #[test]
    fn register_and_unregister_track_the_count() {
        let vsync = Vsync::new();
        assert!(vsync.is_empty());

        let first = vsync.register(controller(100));
        let second = vsync.register(controller(100));
        assert_eq!(vsync.len(), 2);

        vsync.unregister(first);
        assert_eq!(vsync.len(), 1);
        // Idempotent: removing an already-removed id is a no-op.
        vsync.unregister(first);
        assert_eq!(vsync.len(), 1);

        vsync.unregister(second);
        assert!(vsync.is_empty());
    }

    #[test]
    fn tick_all_drives_a_running_controller_from_its_run_start() {
        let vsync = Vsync::new();
        let controller = controller(100);
        vsync.register(controller.clone());

        // Idle controllers are not advanced.
        vsync.tick_all(0.05);
        assert_eq!(controller.status(), AnimationStatus::Dismissed);

        // A forward run is anchored on the first tick that observes it, so the
        // detection tick holds the start value and later ticks climb.
        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(0.20); // anchor here → elapsed 0 this tick
        assert!(
            controller.value() < 1e-4,
            "the detection tick holds the run start, got {}",
            controller.value(),
        );
        vsync.tick_all(0.25); // +50 ms into a 100 ms run
        assert!(
            (controller.value() - 0.5).abs() < 1e-3,
            "halfway through the run the value is ~0.5, got {}",
            controller.value(),
        );

        controller.dispose();
    }

    #[test]
    fn unregistered_controller_is_no_longer_ticked() {
        let vsync = Vsync::new();
        let controller = controller(100);
        let registration = vsync.register(controller.clone());
        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0); // anchor
        vsync.tick_all(0.05); // +50 ms → ~0.5

        let held = controller.value();
        assert!(held > 0.1, "advanced before unregister, got {held}");

        vsync.unregister(registration);
        // Further ticks must not move a controller the registry no longer holds.
        vsync.tick_all(0.10);
        assert!(
            (controller.value() - held).abs() < 1e-6,
            "an unregistered controller is frozen by the registry, moved {} → {}",
            held,
            controller.value(),
        );

        controller.dispose();
    }

    /// A controller `dispose()`d mid-run, WITHOUT being unregistered, must
    /// neither hold the frame loop open nor keep advancing — the frame-loop
    /// leak issue #1171 closes. `dispose()` deliberately leaves `status`
    /// untouched (Flutter parity — a proxy replaying `status()` must see the
    /// status the run had), so `status().is_running()` alone would still
    /// read `true` here; `has_running`/`tick_all` must instead read
    /// [`AnimationController::walk_probe`]'s `live_running`, which folds in
    /// `disposed`.
    ///
    /// Red-check: read `c.controller.status().is_running()` in
    /// `has_running`/`tick_all` instead of `walk_probe().live_running` — this
    /// test fails because the disposed controller still reports running.
    #[test]
    fn a_registry_skips_a_controller_disposed_mid_run_without_unregistering_it() {
        let vsync = Vsync::new();
        let controller = controller(100);
        vsync.register(controller.clone());
        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0); // anchor
        vsync.tick_all(0.05); // +50 ms → ~0.5

        let status_before_dispose = controller.status();
        assert_eq!(
            status_before_dispose,
            AnimationStatus::Forward,
            "sanity: mid-run"
        );

        controller.dispose();
        assert_eq!(
            controller.status(),
            status_before_dispose,
            "parity pin: dispose() must not change status"
        );
        assert!(
            !vsync.has_running(),
            "a disposed controller must not hold the frame loop open, even \
             though status() still reads Forward"
        );

        let held = controller.value();
        vsync.tick_all(0.10);
        assert_eq!(
            controller.value(),
            held,
            "tick_all must skip a disposed controller entirely"
        );
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
    #[test]
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
                vsync_for_listener.unregister(registration);
            }
        }));

        controller.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0);
        vsync.tick_all(0.2); // past the 100 ms duration → Completed → unregisters

        assert!(slot.lock().is_none(), "the listener ran and unregistered");
        assert_eq!(vsync.len(), 0, "and the registry dropped the controller");

        controller.dispose();
    }

    /// The converse: registering from inside a listener is also legal, and the new
    /// controller simply waits for the next frame.
    #[test]
    fn a_listener_may_register_from_inside_tick_all() {
        let vsync = Vsync::new();
        let driver = AnimationController::new(Duration::from_millis(100), &UpdateScheduler::new());
        let _driver_reg = vsync.register(driver.clone());

        let late = AnimationController::new(Duration::from_millis(100), &UpdateScheduler::new());
        let vsync_for_listener = vsync.clone();
        let late_for_listener = late.clone();
        let registered = Arc::new(Mutex::new(false));
        let registered_for_listener = Arc::clone(&registered);
        driver.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed && !*registered_for_listener.lock() {
                *registered_for_listener.lock() = true;
                vsync_for_listener.register(late_for_listener.clone());
            }
        }));

        driver.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0);
        vsync.tick_all(0.2);

        assert!(*registered.lock());
        assert_eq!(vsync.len(), 2, "the late controller joined the registry");

        driver.dispose();
        late.dispose();
    }

    /// An **earlier** listener starting an **already-registered, later**
    /// controller must anchor AND TICK it in the SAME `tick_all` call — the
    /// same-frame scroll→fling handoff the walk's ordering exists to
    /// preserve. Status/value alone can't tell "B was anchored this call"
    /// from "B was ticked this call": B starts at its own run's `t = 0`
    /// either way, so both read `Forward` / value `0.0`. This test also
    /// counts B's VALUE-listener notifications — `tick_at` fires one
    /// unconditionally on every non-completing tick; anchoring alone does
    /// not — which is what actually tells the two apart.
    ///
    /// Red-check: a walk that decides "is this controller due" from a
    /// snapshot of `is_running()` taken at call ENTRY (before any listener
    /// runs), while still doing the anchor bookkeeping live at each
    /// controller's own turn, anchors B this call but never calls `tick_at`
    /// on it — B's status/value assertions below still pass (untouched-
    /// since-mount reads the same as anchored-to-zero), but the value-
    /// notification-count assertion does not: B's count stays flat instead
    /// of growing.
    #[test]
    fn an_earlier_listener_can_start_a_later_registered_controller_in_the_same_frame() {
        let vsync = Vsync::new();
        let a = controller(100);
        let b = controller(100);
        vsync.register(a.clone());
        vsync.register(b.clone());

        let b_notify_count = Arc::new(Mutex::new(0u32));
        let b_notify_count_for_listener = Arc::clone(&b_notify_count);
        b.add_listener(Arc::new(move || {
            *b_notify_count_for_listener.lock() += 1;
        }));

        let b_for_listener = b.clone();
        let count_at_forward: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let count_at_forward_for_listener = Arc::clone(&count_at_forward);
        let count_source_for_listener = Arc::clone(&b_notify_count);
        a.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                let _ = b_for_listener.forward();
                // `forward()` itself fires no value notification (the value
                // hasn't moved yet); snapshot right after it so the
                // assertion below measures growth from THIS call's tick,
                // not from `forward()` starting the run.
                *count_at_forward_for_listener.lock() = Some(*count_source_for_listener.lock());
            }
        }));

        a.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0); // anchor A
        assert_eq!(
            b.status(),
            AnimationStatus::Dismissed,
            "B is idle before this call"
        );

        vsync.tick_all(0.2); // past A's 100ms duration -> Completed -> starts B

        assert_eq!(a.status(), AnimationStatus::Completed);
        assert_eq!(
            b.status(),
            AnimationStatus::Forward,
            "B is anchored and ticked in the SAME call A completed it"
        );
        assert!(
            b.value() < 1e-4,
            "B's first observed tick is its own run's t = 0 (anchored this call), got {}",
            b.value(),
        );
        let snapshot = count_at_forward
            .lock()
            .expect("A's listener ran and captured the snapshot");
        assert!(
            *b_notify_count.lock() > snapshot,
            "B's value listener must fire from a REAL `tick_at` call during \
             THIS SAME `tick_all` — anchoring B without ticking it would \
             leave the count flat at {snapshot}, got {}",
            *b_notify_count.lock(),
        );

        // A following tick advances B from that anchor, proving it is a real
        // anchor and not a fluke of `now_secs - run_start == 0` on this call.
        vsync.tick_all(0.25);
        assert!(
            (b.value() - 0.5).abs() < 1e-3,
            "B advances from ITS OWN run start on the next tick, got {}",
            b.value(),
        );

        a.dispose();
        b.dispose();
    }

    /// A controller registered from a listener during `tick_all` is not
    /// ticked until the NEXT call — its value is untouched by the call that
    /// registered it, and only starts advancing afterward.
    ///
    /// Red-check: a walk that includes ids registered mid-walk in THIS
    /// call's range would still anchor `late` fresh right here (elapsed 0,
    /// same as the correct behavior), so the assertion right after this
    /// call does not discriminate. What actually reddens is the assertion
    /// after the NEXT call, `tick_all(0.25)` (`late.value() < 1e-4` below
    /// it): under that mutation `late` is already anchored from THIS call,
    /// so `tick_all(0.25)` reads a real elapsed (~0.05, value ~0.5) instead
    /// of landing on `late`'s own anchor tick.
    #[test]
    fn a_controller_registered_during_tick_all_waits_for_the_next_frame() {
        let vsync = Vsync::new();
        let driver = controller(100);
        vsync.register(driver.clone());

        let late = controller(100);
        let vsync_for_listener = vsync.clone();
        let late_for_listener = late.clone();
        driver.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                vsync_for_listener.register(late_for_listener.clone());
                let _ = late_for_listener.forward();
            }
        }));

        driver.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0);
        vsync.tick_all(0.2); // completes driver -> registers + starts `late`

        assert_eq!(
            late.status(),
            AnimationStatus::Forward,
            "late started running"
        );
        assert!(
            late.value() < 1e-4,
            "but this call did not tick it — its value is untouched, got {}",
            late.value(),
        );

        vsync.tick_all(0.25); // the NEXT call reaches it — but this is ITS OWN
        // anchor tick (first observation since registering), so it still
        // holds at its run start, same as any freshly registered controller.
        assert!(
            late.value() < 1e-4,
            "the next call anchors `late` — elapsed 0 relative to that anchor \
             — so it still holds at its run start, got {}",
            late.value(),
        );

        vsync.tick_all(0.35); // a later call advances it from that real anchor
        assert!(
            late.value() > 0.0,
            "a later call advances the late registration from its anchor, got {}",
            late.value(),
        );

        driver.dispose();
        late.dispose();
    }

    /// An **earlier** listener unregistering a **later**, running controller
    /// must stop it before its own turn: it does not move in this same
    /// `tick_all` call.
    ///
    /// Red-check: resolving each id through a snapshot taken before any
    /// listener ran (rather than a live lookup at that id's own turn) would
    /// tick B once more even though A already unregistered it.
    #[test]
    fn an_earlier_listener_unregistering_a_later_controller_skips_it_this_frame() {
        let vsync = Vsync::new();
        let a = controller(100);
        let b = controller(100);
        vsync.register(a.clone());
        let b_registration = vsync.register(b.clone());

        let vsync_for_listener = vsync.clone();
        a.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                vsync_for_listener.unregister(b_registration);
            }
        }));

        a.forward().expect("fresh controller forwards");
        b.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0); // anchors both
        vsync.tick_all(0.2); // completes A -> unregisters B before B's turn

        assert_eq!(a.status(), AnimationStatus::Completed);
        assert!(
            b.value() < 1e-4,
            "B never advanced past its anchor tick, got {}",
            b.value(),
        );

        a.dispose();
        b.dispose();
    }

    /// Unregistering a later, running controller and immediately
    /// re-registering the SAME controller from an earlier listener must not
    /// retarget this call's walk onto the new registration — new ids are
    /// always at or past this call's fence — and the re-registration's
    /// anchor is fresh, not inherited from the old one: the first tick that
    /// observes it re-anchors `t = 0` there, so the controller's value
    /// visibly holds at ITS OWN run start rather than continuing from
    /// wherever the old registration left it.
    ///
    /// Red-check: reusing ids, or letting a re-registration inherit the
    /// removed entry's `run_start_secs`, would either tick it this call or
    /// skip the re-anchor and keep advancing from the stale elapsed time.
    #[test]
    fn unregister_and_reregister_from_a_listener_cannot_retarget_the_walk() {
        let vsync = Vsync::new();
        let a = AnimationController::new(Duration::from_millis(300), &UpdateScheduler::new());
        let b = AnimationController::new(Duration::from_millis(1000), &UpdateScheduler::new());
        vsync.register(a.clone());
        let b_registration = vsync.register(b.clone());

        let vsync_for_listener = vsync.clone();
        let b_for_listener = b.clone();
        let retargeted = Arc::new(Mutex::new(false));
        let retargeted_for_listener = Arc::clone(&retargeted);
        a.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed && !*retargeted_for_listener.lock() {
                *retargeted_for_listener.lock() = true;
                vsync_for_listener.unregister(b_registration);
                vsync_for_listener.register(b_for_listener.clone());
            }
        }));

        a.forward().expect("fresh controller forwards");
        b.forward().expect("fresh controller forwards");

        vsync.tick_all(0.0); // anchors both at t = 0
        vsync.tick_all(0.2); // B progresses normally; A not yet completed
        let progressed = b.value();
        assert!(progressed > 0.1, "B made real progress, got {progressed}");

        vsync.tick_all(0.5); // A completes -> unregisters + re-registers B
        assert!(
            (b.value() - progressed).abs() < 1e-6,
            "B does not move in the same call that re-registered it, held at {}, got {}",
            progressed,
            b.value(),
        );

        vsync.tick_all(0.6); // the re-registration's first observed tick
        assert!(
            b.value() < 1e-3,
            "the re-registration's anchor is fresh, so this tick lands at B's \
             OWN run start rather than continuing from where the old \
             registration left it (~{}), got {}",
            progressed,
            b.value(),
        );

        vsync.tick_all(0.7); // a real anchor: the following tick advances from it
        assert!(
            b.value() > 0.0,
            "the fresh anchor is real — a later tick advances from it, got {}",
            b.value(),
        );

        a.dispose();
        b.dispose();
    }

    /// A listener that mutes the registry **mid-walk** stops the rest of
    /// THIS frame's entries from ticking — Flutter honors a mid-frame
    /// `Ticker.muted = true` the same way. The registry is unmuted when this
    /// call starts, so this is a genuinely mid-walk observation, distinct
    /// from the entry-only mute [`a_muted_registry_delivers_no_ticks_while_its_clock_runs_on`]
    /// covers.
    ///
    /// Red-check: reading `muted` only once, at entry, lets the walk keep
    /// ticking every remaining entry after a listener mutes it.
    #[test]
    fn a_listener_muting_the_registry_mid_walk_stops_this_frames_remaining_entries() {
        let vsync = Vsync::new();
        let a = controller(100);
        let b = controller(100);
        vsync.register(a.clone());
        vsync.register(b.clone());

        let vsync_for_listener = vsync.clone();
        a.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                vsync_for_listener.set_muted(true);
            }
        }));

        a.forward().expect("fresh controller forwards");
        b.forward().expect("fresh controller forwards");
        vsync.tick_all(0.0); // anchors both, registry unmuted at this point

        vsync.tick_all(0.2); // completes A -> mutes the registry before B's turn
        assert_eq!(a.status(), AnimationStatus::Completed);
        assert!(
            b.value() < 1e-4,
            "B does not advance in the SAME call that muted the registry, got {}",
            b.value(),
        );

        // The registry stays muted going into the next call, so it advances
        // nothing there either — the existing muted-registry contract,
        // checked here for completeness.
        vsync.tick_all(0.5);
        assert!(
            b.value() < 1e-4,
            "still muted on the next call, B still does not advance, got {}",
            b.value(),
        );

        vsync.set_muted(false);
        a.dispose();
        b.dispose();
    }

    /// A child registry attached from a listener **mid-walk** is first
    /// ticked on the NEXT call, not this one — children are sampled once,
    /// before the cursor walk starts.
    ///
    /// Red-check: ticking newly attached children within the SAME call
    /// would still anchor C fresh right here (elapsed 0, same as the
    /// correct behavior), so the assertion right after this call does not
    /// discriminate. What actually reddens is the assertion after the NEXT
    /// call, `parent.tick_all(0.25)` (`c.value() < 1e-4` below it): under
    /// that mutation C is already anchored from THIS call, so
    /// `tick_all(0.25)` reads a real elapsed (~0.05, value ~0.5) instead of
    /// landing on C's own anchor tick.
    #[test]
    fn a_child_attached_from_a_listener_is_first_ticked_on_the_next_call() {
        let parent = Vsync::new();
        let a = controller(100);
        parent.register(a.clone());

        let child = Vsync::new();
        let c = controller(100);
        child.register(c.clone());
        c.forward().expect("fresh controller forwards");

        let parent_for_listener = parent.clone();
        let child_for_listener = child.clone();
        a.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                parent_for_listener.attach_child(&child_for_listener);
            }
        }));

        a.forward().expect("fresh controller forwards");
        parent.tick_all(0.0);
        parent.tick_all(0.2); // completes A -> attaches `child` mid-walk

        assert!(
            c.value() < 1e-4,
            "C does not move in the call that attached its registry, got {}",
            c.value(),
        );

        parent.tick_all(0.25); // the next call reaches it — but this is the
        // child registry's OWN first observed tick, so C still holds at its
        // run start (same anchor-tick contract as any freshly ticked run).
        assert!(
            c.value() < 1e-4,
            "the next call reaches C's registry, but that is C's own anchor \
             tick — elapsed 0 relative to it — so it still holds at run \
             start, got {}",
            c.value(),
        );

        parent.tick_all(0.35); // a later call advances C from that real anchor
        assert!(
            c.value() > 0.0,
            "a later call advances C from the anchor the previous one set, got {}",
            c.value(),
        );

        a.dispose();
        c.dispose();
    }
}
