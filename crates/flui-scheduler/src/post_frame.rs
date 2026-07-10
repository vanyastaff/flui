//! [`PostFrameHandle`] — an owned capability to run work *after* the frame's
//! pipeline has committed layout and paint.
//!
//! # Why a handle and not `Scheduler::instance()`
//!
//! There is no single scheduler. `HeadlessBinding` owns a **binding-local**
//! `Scheduler`; production drives the `Scheduler::instance()` singleton. A
//! callback parked on the wrong one is never drained, and — worse — a headless
//! test of that code would pass while production silently did nothing. ADR-0021
//! §7c pins the distinction with
//! `pump_frame_drives_the_binding_local_scheduler_not_the_singleton`.
//!
//! So the capability is *handed to* the code that needs it, exactly as
//! [`RebuildHandle`] and [`AsyncDriver`](crate::AsyncDriver) are: the binding
//! publishes one naming *its* scheduler, and a view acquires it from its
//! `BuildContext` during a lifecycle hook.
//!
//! [`RebuildHandle`]: https://docs.rs/flui-view
//!
//! # Ordering guarantee
//!
//! [`Scheduler::drive_frame`](crate::Scheduler::drive_frame) runs
//! `begin → persistent → pipeline → post-frame → idle`, so a callback scheduled
//! here observes the geometry **this** frame committed. That is the contract
//! `HeroController` needs to measure a route it forced offstage
//! (`heroes.dart:964-968`).

use crate::{FrameTiming, PostFrameCallback, Scheduler};

/// Schedules work to run at the end of a frame, after layout and paint commit.
///
/// Owned, `'static` and cheap to clone: it holds the `Scheduler` (itself
/// `Arc`-backed), so every clone names the same callback queue.
///
/// A callback runs **exactly once**, on the next frame that completes. One
/// scheduled *from inside* a post-frame callback defers to the frame after that —
/// Flutter's `_postFrameCallbacks` is drained into a local buffer before any of
/// them is invoked (`scheduler/binding.dart:1350-1351`).
///
/// # Inert, never wrong
///
/// A handle outliving its binding keeps that binding's scheduler alive and its
/// callbacks continue to be queued; they simply never run, because nothing drives
/// that scheduler's frames any more. Scheduling through a stale handle is a no-op,
/// not a panic and not a callback on the *wrong* scheduler.
#[derive(Clone)]
pub struct PostFrameHandle {
    scheduler: Scheduler,
}

impl PostFrameHandle {
    /// A handle that schedules onto `scheduler`.
    ///
    /// Bindings call this. Application code receives the handle instead of
    /// constructing one, so it cannot accidentally target the wrong scheduler.
    #[must_use]
    pub fn new(scheduler: &Scheduler) -> Self {
        Self {
            scheduler: scheduler.clone(),
        }
    }

    /// Run `callback` at the end of the next frame that completes, after the
    /// pipeline has committed layout and paint.
    ///
    /// Not run if no further frame completes (e.g. the binding was dropped).
    pub fn schedule(&self, callback: impl FnOnce(&FrameTiming) + Send + 'static) {
        let boxed: PostFrameCallback = Box::new(callback);
        self.scheduler.add_post_frame_callback(boxed);
    }

    /// Whether this handle and `other` schedule onto the same scheduler.
    ///
    /// The check a test needs to prove a seam did not silently fall back to
    /// `Scheduler::instance()`.
    #[must_use]
    pub fn targets_same_scheduler(&self, other: &Scheduler) -> bool {
        self.scheduler.is_same_instance(other)
    }
}

impl std::fmt::Debug for PostFrameHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostFrameHandle").finish_non_exhaustive()
    }
}
