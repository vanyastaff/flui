//! The owned-target/surface protocol, factored out of `renderer.rs` so it
//! can be exercised without a GPU (issue #1043).
//!
//! `SurfaceLease<S>` is generic over the surface type `S` precisely so the
//! ownership and probe-before-build protocol can run under plain `#[test]`
//! (and later, Miri) with a fake `S` — the real `wgpu::Surface<'static>` is
//! only ever plugged in by `renderer.rs`.

use std::sync::Arc;

use crate::error::{EngineError, EngineResult};
use crate::window_target::WindowTarget;

/// Ties a surface to the [`WindowTarget`] it was built from, for as long as
/// the surface needs to exist.
///
/// # A lease may hold no surface at all
///
/// `surface` is an `Option` because a backend can report that the native
/// handle behind it is about to die, and a presentation that keeps a surface
/// built from a dead handle is the use-after-free class this lease's field
/// order exists to prevent. [`SurfaceLease::release`] takes the surface out
/// and drops it there and then; [`SurfaceLease::replace_surface`] puts a
/// newly built one back. The [`Arc`] of the target is retained across the
/// released span, which is what makes a re-acquire need no new ownership:
/// [`WindowTarget`]'s `HasWindowHandle`/`HasDisplayHandle` methods both take
/// `&self`, so the retained `Arc` answers with whatever handle is current
/// when it is asked again.
///
/// # Field order is load-bearing
///
/// `surface` is declared **before** `target`, so it is dropped **before**
/// `target` (fields drop in declaration order). wgpu's own [`wgpu::Surface`]
/// makes the identical choice for the identical reason: it holds a boxed
/// clone of its window-handle source in a field named `_handle_source`,
/// declared **last** in that struct (dropped last there too, since a normal
/// struct's last field is also its last-declared one) — both orderings agree
/// that the handle source must outlive the surface built from it. Swapping
/// this order would drop the window target while the surface built from it
/// may still be mid-teardown. An `Option` field drops what it holds at the
/// same point in the sequence, so a released lease keeps this property: the
/// surface taken out by [`SurfaceLease::release`] is already gone by the time
/// the target is dropped.
pub(crate) struct SurfaceLease<S> {
    surface: Option<S>,
    target: Arc<dyn WindowTarget>,
}

/// Proof that a lease's surface has been released: the only source of the
/// target a replacement may be built from, and the only key that
/// [`SurfaceLease::replace_surface`] accepts.
#[must_use = "build the replacement from `target()` and commit it with `replace_surface`, or bind the token to acknowledge a bare release"]
pub(crate) struct Released {
    target: Arc<dyn WindowTarget>,
}

impl Released {
    /// The owner the replacement surface is built from.
    pub(crate) fn target(&self) -> &Arc<dyn WindowTarget> {
        &self.target
    }
}

impl<S> SurfaceLease<S> {
    /// Re-probe the retained owner without rebuilding `S`.
    ///
    /// The rebuild protocol is probe → [`release`](Self::release) → build the
    /// replacement from the [`Released`] token's target → commit it with
    /// [`replace_surface`](Self::replace_surface). Release comes BEFORE the
    /// build because a window allows one surface at a time: creating a second
    /// one over a still-held surface is a process abort on Android
    /// (`VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`), the platform that emits the
    /// recreate signal in the first place. The token is what makes the order
    /// a type fact rather than a convention — `replace_surface` cannot be
    /// called without it, and its target is the only one to build from. A
    /// build that fails after the release leaves the lease released, which is
    /// the documented post-state the next recovery re-asks.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::SurfaceTargetUnavailable`] when the owner
    /// reports the native target is gone or suspended.
    pub(crate) fn probe(&self) -> EngineResult<()> {
        probe_target(&self.target)
    }

    /// Commit a surface built from `released`'s target, closing the released
    /// span that [`release`](Self::release) opened.
    pub(crate) fn replace_surface(&mut self, released: Released, surface: S) {
        debug_assert!(
            Arc::ptr_eq(&released.target, &self.target),
            "BUG: a Released token commits into the lease that issued it"
        );
        self.surface = Some(surface);
    }

    /// Drop the held surface and hold none.
    ///
    /// Call this at the last moment the native handle behind the surface is
    /// still valid — dropping a configured [`wgpu::Surface`] releases its
    /// swapchain, so it must not outlive the handle it was built from.
    /// Idempotent: a lease that already holds no surface stays that way. The
    /// target is deliberately **not** released; the returned [`Released`]
    /// token carries it so a replacement is built from the same owner, and is
    /// the only way to [`replace_surface`](Self::replace_surface). A caller
    /// that releases without replacing binds the token to say so.
    pub(crate) fn release(&mut self) -> Released {
        self.surface = None;
        Released {
            target: Arc::clone(&self.target),
        }
    }

    /// The window target this lease keeps alive.
    ///
    /// Production builds replacements from a [`Released`] token's target,
    /// never from here; this accessor is for the lease's own tests.
    #[cfg(test)]
    pub(crate) fn target(&self) -> &Arc<dyn WindowTarget> {
        &self.target
    }

    /// The current surface, or `None` while [`SurfaceLease::release`] holds.
    pub(crate) fn surface(&self) -> Option<&S> {
        self.surface.as_ref()
    }

    /// Whether a surface is held right now.
    ///
    /// Distinct from "the target is available": a released lease whose target
    /// answers [`raw_window_handle::HandleError::Unavailable`] is exactly the
    /// state a suspend leaves behind.
    pub(crate) fn has_surface(&self) -> bool {
        self.surface.is_some()
    }

    /// Wrap an already-probed target and its freshly built surface into a
    /// lease, without probing.
    ///
    /// The caller (`Renderer::new`) is required to have already called
    /// [`probe_target`] before doing the async work that produced `surface`
    /// — re-probing here would only duplicate that check.
    pub(crate) fn from_parts(target: Arc<dyn WindowTarget>, surface: S) -> Self {
        Self {
            surface: Some(surface),
            target,
        }
    }
}

/// Probe the owner for a live handle, mapping a reported
/// [`raw_window_handle::HandleError`] to a typed engine error.
///
/// This duplicates the call wgpu's own `Instance::create_surface` makes
/// internally — it exists ONLY because wgpu's `CreateSurfaceError` hides the
/// `HandleError` it wraps. Verified against
/// `wgpu-30.0.1/src/api/surface.rs`'s `CreateSurfaceError::source()`: its
/// `CreateSurfaceErrorKind::RawHandle` arm returns `None` directly when
/// wgpu's own `std` feature is off (this workspace's resolved feature set);
/// with that feature on it forwards to `HandleError::source()`, which is
/// `None` too (raw-window-handle's `impl std::error::Error for HandleError
/// {}` has no override) — either way only the formatted `Display` text
/// survives. Without probing here first, "the window owner says the native
/// target is gone/suspended" and "the GPU driver refused for some unrelated
/// reason" would collapse into one undifferentiated
/// [`EngineError::SurfaceCreation`], and callers like `flui-app`'s
/// device-recovery loop could not tell a transient, wait-and-retry
/// condition (a suspended Android surface) from one that needs the whole
/// renderer torn down. Do not delete this without restoring that
/// distinction some other way.
///
/// `pub(crate)` (not just used internally) because
/// [`Renderer::new`](crate::Renderer::new) must probe strictly before
/// it starts any GPU work — before a lease exists to call
/// [`SurfaceLease::probe`] on.
pub(crate) fn probe_target(target: &Arc<dyn WindowTarget>) -> EngineResult<()> {
    target
        .window_handle()
        .map_err(EngineError::surface_target_unavailable)?;
    target
        .display_handle()
        .map_err(EngineError::surface_target_unavailable)?;
    Ok(())
}

impl<S> Drop for SurfaceLease<S> {
    fn drop(&mut self) {
        // Both live-smoke harnesses assert this line: `just live-smoke`
        // (X11) and `just live-smoke-wayland` (Wayland) both check
        // `tools/live-smoke/src/self_close.rs`'s
        // `assert_surface_released_before_window_close`, which requires
        // this line to precede the winit backend's "Quitting event loop"
        // line, on BOTH close routes (compositor/`WM_DELETE` and
        // `FLUI_SELF_CLOSE_ROUTE=programmatic`). It is the only executed
        // witness that the surface — and this lease's `Arc` clone of the
        // target — was actually released rather than orphaned by the
        // frame-callback cycle described in
        // `crate::raster_owner::RasterOwner`'s module docs.
        //
        // `debug`, not `trace`: a lease is released far less often than a
        // surface is acquired (once per window life, not once per frame),
        // so it does not need `surface_acquired`'s per-frame trace budget —
        // both live-smoke harnesses filter `flui.gpu` broadly enough to
        // capture it either way.
        tracing::debug!(
            target: "flui.gpu",
            event = "surface_released",
            "surface and its window target lease is being dropped"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_ulong;
    use std::sync::Mutex;

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };

    use super::*;
    use crate::fake_window_target::{FakeTarget, xlib_window_id};

    #[test]
    fn dropping_the_lease_drops_the_only_strong_target_ref() {
        let target: Arc<dyn WindowTarget> = Arc::new(FakeTarget::new(1));
        let weak = Arc::downgrade(&target);

        let lease = SurfaceLease::from_parts(target, 0u8);
        assert!(
            weak.upgrade().is_some(),
            "the lease is the sole strong holder and is still alive"
        );

        drop(lease);
        assert!(
            weak.upgrade().is_none(),
            "no strong ref survives the lease that owned the only one"
        );
    }

    /// Where the recording fixtures below write the order in which they are
    /// dropped.
    type DropLog = Arc<Mutex<Vec<&'static str>>>;

    /// A surface that appends `"surface"` to the shared log when dropped.
    struct RecordingSurface(DropLog);

    impl Drop for RecordingSurface {
        fn drop(&mut self) {
            self.0
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
                .push("surface");
        }
    }

    /// A target that appends `"target"` to the shared log when dropped, and
    /// otherwise answers like [`FakeTarget`].
    struct RecordingTarget {
        inner: FakeTarget,
        log: DropLog,
    }

    impl Drop for RecordingTarget {
        fn drop(&mut self) {
            self.log
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
                .push("target");
        }
    }

    impl HasWindowHandle for RecordingTarget {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            self.inner.window_handle()
        }
    }

    impl HasDisplayHandle for RecordingTarget {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            self.inner.display_handle()
        }
    }

    /// A lease over both recording fixtures, plus the log they write to. The
    /// lease is returned rather than dropped here so each test can arrange the
    /// release it is about before the final drop.
    fn recording_lease() -> (SurfaceLease<RecordingSurface>, DropLog) {
        let log: DropLog = Arc::new(Mutex::new(Vec::new()));
        let target: Arc<dyn WindowTarget> = Arc::new(RecordingTarget {
            inner: FakeTarget::new(1),
            log: Arc::clone(&log),
        });
        (
            SurfaceLease::from_parts(target, RecordingSurface(Arc::clone(&log))),
            log,
        )
    }

    /// The log's contents, cloned out of the lock before it is returned: an
    /// assertion that panicked while holding the guard would poison the mutex
    /// and turn a clean test failure into a destructor panic during unwinding.
    fn drop_log(log: &DropLog) -> Vec<&'static str> {
        log.lock()
            .expect("BUG: test-only mutex is never poisoned")
            .clone()
    }

    #[test]
    fn drop_order_is_surface_before_target() {
        let (lease, log) = recording_lease();

        drop(lease);

        assert_eq!(
            drop_log(&log),
            vec!["surface", "target"],
            "the surface must be released before the target it was built from"
        );
    }

    #[test]
    fn probe_rejects_a_still_unavailable_target_and_replace_surface_commits_only_after_success() {
        let concrete = Arc::new(FakeTarget::new(1));
        let target: Arc<dyn WindowTarget> = concrete.clone();

        let initial = xlib_window_id(&target).expect("fake target starts available");
        let mut lease = SurfaceLease::from_parts(Arc::clone(&target), initial);
        assert_eq!(held_surface(&lease), 1);
        assert_eq!(
            concrete.call_log(),
            vec!["window_handle"],
            "seeding the lease queried window_handle exactly once"
        );

        concrete.set_available(false);
        let failed = lease.probe();
        assert!(matches!(
            failed,
            Err(EngineError::SurfaceTargetUnavailable { .. })
        ));
        assert_eq!(
            held_surface(&lease),
            1,
            "a failed probe must not disturb the still-live surface"
        );
        assert_eq!(
            concrete.call_log(),
            vec!["window_handle", "window_handle"],
            "the failed probe queried window_handle and stopped there — Unavailable \
             short-circuits before display_handle is ever called"
        );

        concrete.set_available(true);
        concrete.set_window_id(2);
        lease
            .probe()
            .expect("target is available again with a fresh handle");
        assert_eq!(
            concrete.call_log(),
            vec![
                "window_handle",
                "window_handle",
                "window_handle",
                "display_handle",
            ],
            "a successful probe queries both window_handle and display_handle, in that order"
        );
        assert_eq!(
            held_surface(&lease),
            1,
            "probe() alone must not have touched the surface yet — only a release does"
        );

        let released = lease.release();
        let fresh = xlib_window_id(released.target()).expect("target is available");
        lease.replace_surface(released, fresh);
        assert_eq!(
            held_surface(&lease),
            2,
            "the rebuilt surface must be built from the fresh handle value"
        );
        assert_eq!(
            concrete.call_log(),
            vec![
                "window_handle",
                "window_handle",
                "window_handle",
                "display_handle",
                "window_handle",
            ],
            "the final rebuild is a separate window_handle query, made only after probe() \
             committed to the target being live"
        );
    }

    /// The surface the lease holds, for tests that are asserting a value
    /// rather than the released-lease behavior.
    fn held_surface(lease: &SurfaceLease<c_ulong>) -> c_ulong {
        *lease
            .surface()
            .expect("BUG: this assertion is only meaningful while a surface is held")
    }

    #[test]
    fn release_drops_the_surface_and_keeps_the_target() {
        let (mut lease, log) = recording_lease();

        let _released = lease.release();

        let after_release = drop_log(&log);
        assert!(
            !lease.has_surface(),
            "release leaves the lease holding none"
        );
        assert!(
            lease.surface().is_none(),
            "surface() answers None once released"
        );
        assert_eq!(
            after_release,
            vec!["surface"],
            "release drops the held surface there and then, not at lease drop"
        );
        assert!(
            lease.target().window_handle().is_ok(),
            "the retained target survives the release, so a re-acquire needs no new ownership"
        );
    }

    #[test]
    fn releasing_twice_releases_once_and_a_replacement_is_held_again() {
        let target: Arc<dyn WindowTarget> = Arc::new(FakeTarget::new(1));
        let mut lease = SurfaceLease::from_parts(target, 1u64);

        let _first = lease.release();
        let second = lease.release();
        assert!(!lease.has_surface(), "a second release is the same state");

        lease.replace_surface(second, 7);
        assert!(lease.has_surface(), "replace_surface holds the new surface");
        assert_eq!(*lease.surface().expect("just replaced"), 7);
    }

    /// Release-then-replace is the order `Renderer::recreate_surface` and
    /// `Renderer::recover` both use to sidestep the one-surface-per-window
    /// rule, and it must not depend on a *dropped-and-rebuilt* lease: the
    /// whole point is that the target is retained across the released span,
    /// so the replacement is built from the same owner the release happened
    /// against.
    ///
    /// Pins the lease's half of that protocol: the old surface is gone
    /// before the replacement exists, the target stays live and answers
    /// across the span, and the lease ends holding exactly the replacement
    /// over the *same* owner.
    ///
    /// The renderer's call order — `release` before `create_surface` — is
    /// not asserted here because it no longer can be violated: the
    /// [`Released`] token is the only source of the target a replacement is
    /// built from and the only key `replace_surface` accepts, so a build-first
    /// renderer does not compile.
    #[test]
    fn release_then_replace_holds_only_the_replacement_over_a_live_target() {
        let (mut lease, log) = recording_lease();
        let target: Arc<dyn WindowTarget> = Arc::clone(lease.target());

        let released = lease.release();
        assert_eq!(
            drop_log(&log),
            vec!["surface"],
            "the old surface is dropped by the release, before any replacement exists"
        );
        assert!(
            released.target().window_handle().is_ok(),
            "the retained target still answers while no surface is held"
        );

        lease.replace_surface(released, RecordingSurface(Arc::clone(&log)));
        assert!(lease.has_surface(), "the lease holds the replacement");
        assert!(
            Arc::ptr_eq(lease.target(), &target),
            "the replacement was built against the same retained owner"
        );
    }

    #[test]
    fn dropping_a_released_lease_releases_only_the_target() {
        let (mut lease, log) = recording_lease();

        let released = lease.release();
        assert_eq!(
            drop_log(&log),
            vec!["surface"],
            "release drops the surface while the lease — and so the target — is still alive"
        );

        // Cleared so the assertion below is exclusive to what this lease drops
        // from here on: with the surface already gone there is no order left to
        // observe between two entries, only which single entry a released
        // lease's drop produces.
        log.lock()
            .expect("BUG: test-only mutex is never poisoned")
            .clear();

        // The token holds its own `Arc` of the target (it is what a
        // replacement is built from), so it must go before the lease's drop
        // can be the last owner.
        drop(released);
        drop(lease);

        assert_eq!(
            drop_log(&log),
            vec!["target"],
            "a released lease's drop releases the target and nothing else"
        );
    }
}
