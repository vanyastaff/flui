//! [`Vsync`] — a shared, restart-aware registry that drives
//! [`AnimationController`]s off a single virtual timeline.
//!
//! A deterministic frame driver (e.g. `flui_binding::HeadlessBinding`) owns one
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

use std::sync::Arc;

use parking_lot::Mutex;

use crate::{Animation, AnimationController};

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
struct RegisteredController {
    id: VsyncRegistration,
    controller: AnimationController,
    run_start_secs: Option<f64>,
    last_gen: u64,
}

#[derive(Default)]
struct VsyncInner {
    controllers: Vec<RegisteredController>,
    next_id: u64,
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
        let id = VsyncRegistration(inner.next_id);
        inner.next_id += 1;
        let last_gen = controller.run_generation();
        inner.controllers.push(RegisteredController {
            id,
            controller,
            run_start_secs: None,
            last_gen,
        });
        id
    }

    /// Remove the controller previously registered under `id`. Idempotent: an
    /// unknown or already-removed id is a no-op.
    pub fn unregister(&self, id: VsyncRegistration) {
        self.inner.lock().controllers.retain(|c| c.id != id);
    }

    /// The number of currently registered controllers.
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
    /// Used by a production frame driver (e.g. `AppBinding`) to decide whether
    /// to request the next frame: call this after [`tick_all`](Self::tick_all)
    /// and, if `true`, schedule a wake so the frame loop keeps going. Once the
    /// last running controller completes, `has_running()` returns `false` and
    /// the driver does NOT re-request, so the window quiesces cleanly — no
    /// infinite redraw after all animations settle.
    #[must_use]
    pub fn has_running(&self) -> bool {
        self.inner
            .lock()
            .controllers
            .iter()
            .any(|c| c.controller.status().is_running())
    }

    /// Advance every registered, running controller to virtual instant
    /// `now_secs` (elapsed seconds on the driver's virtual clock).
    ///
    /// For each controller: if its `run_generation` advanced since the last
    /// observation (a fresh run was just established) or it has no anchor yet,
    /// re-anchor `t = 0` to `now_secs`; then, if the controller reports running,
    /// tick it with the raw seconds elapsed since that anchor. A non-running
    /// controller is skipped (its anchor is set on the frame it next starts), so
    /// a disposed-but-not-unregistered controller is simply not ticked.
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
    /// So each controller is looked up, its bookkeeping updated, and the lock
    /// dropped *before* it is ticked. Ticking one controller at a time, rather than
    /// snapshotting them all up front, preserves the property the old loop had:
    /// a controller that an **earlier** controller's listener starts during this
    /// same call (a `Scrollable` handing off to its fling controller) is anchored
    /// and ticked in this frame, not the next.
    ///
    /// A controller *registered* during this call is not ticked until the next
    /// frame, and one *unregistered* during it is skipped from that point on.
    ///
    // ponytail: linear scan per controller. The registry holds a handful of
    // controllers; if it ever holds hundreds, key it by `VsyncRegistration`.
    pub fn tick_all(&self, now_secs: f64) {
        let registrations: Vec<VsyncRegistration> = self
            .inner
            .lock()
            .controllers
            .iter()
            .map(|registered| registered.id)
            .collect();

        for id in registrations {
            let due = {
                let mut inner = self.inner.lock();
                let Some(registered) = inner.controllers.iter_mut().find(|c| c.id == id) else {
                    continue; // A previous tick's listener unregistered it.
                };

                let generation = registered.controller.run_generation();
                if generation != registered.last_gen || registered.run_start_secs.is_none() {
                    registered.last_gen = generation;
                    registered.run_start_secs = Some(now_secs);
                }
                if registered.controller.status().is_running() {
                    // `run_start_secs` is `Some` here — set in the branch above on
                    // this same call if it was `None`.
                    let run_start = registered.run_start_secs.unwrap_or(now_secs);
                    Some((registered.controller.clone(), now_secs - run_start))
                } else {
                    None
                }
            };

            if let Some((controller, elapsed)) = due {
                controller.tick_at(elapsed);
            }
        }
    }
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

    use flui_scheduler::Scheduler;

    use super::*;
    use crate::AnimationStatus;

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()))
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

    /// A status listener that unregisters its own controller — what a route does
    /// when its exit transition reaches `dismissed` and it disposes itself — must
    /// not deadlock.
    ///
    /// Regression: `tick_all` used to hold the registry lock across `tick_at`, so
    /// the listener's `unregister` re-entered a non-reentrant `parking_lot::Mutex`
    /// and hung. Found by ADR-0020 U5.4's first end-to-end `PopupRoute` pop.
    #[test]
    fn a_listener_may_unregister_from_inside_tick_all() {
        let vsync = Vsync::new();
        let controller =
            AnimationController::new(Duration::from_millis(100), Arc::new(Scheduler::new()));
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
        let driver =
            AnimationController::new(Duration::from_millis(100), Arc::new(Scheduler::new()));
        let _driver_reg = vsync.register(driver.clone());

        let late = AnimationController::new(Duration::from_millis(100), Arc::new(Scheduler::new()));
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
}
