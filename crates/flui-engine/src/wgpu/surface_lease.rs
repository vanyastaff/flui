//! The owned-target/surface protocol, factored out of [`super::renderer`] so
//! it can be exercised without a GPU (issue #1043).
//!
//! `SurfaceLease<S>` is generic over the surface type `S` precisely so the
//! ownership and probe-before-build protocol can run under plain `#[test]`
//! (and later, Miri) with a fake `S` — the real `wgpu::Surface<'static>` is
//! only ever plugged in by `renderer.rs`.

use std::sync::Arc;

use super::window_target::WindowTarget;
use crate::error::{EngineError, EngineResult};

/// Ties a surface to the [`WindowTarget`] it was built from, for as long as
/// the surface needs to exist.
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
/// may still be mid-teardown.
pub(crate) struct SurfaceLease<S> {
    surface: S,
    target: Arc<dyn WindowTarget>,
}

impl<S> SurfaceLease<S> {
    /// Acquire a lease: probe the owner for a live handle, then build `S`
    /// from a clone of the same `Arc`.
    ///
    /// `renderer.rs` does not call this directly: building the full windowed
    /// GPU stack is `async` and produces far more than just `S` (instance,
    /// adapter, device, painter, …), which does not fit this method's
    /// synchronous `build` closure. Production instead calls
    /// [`probe_target`] standalone — strictly before starting that async
    /// work, which [`Renderer::new`](super::renderer::Renderer::new)'s own
    /// contract requires — then wraps the finished surface with
    /// [`SurfaceLease::from_parts`]. This method exists so the identical
    /// probe-then-build protocol is exercised end-to-end by a GPU-free unit
    /// test (issue #1043's acceptance criteria) and is ready to be the
    /// direct call site once a future slice gives the windowed path a
    /// surface-shaped builder (see `docs/adr/ADR-0045`'s `GpuServices`
    /// migration).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::SurfaceTargetUnavailable`] — without ever
    /// calling `build` — when the owner's `window_handle()` or
    /// `display_handle()` reports the native target is gone or suspended.
    /// Otherwise returns whatever `build` returns.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "exercised by this module's unit tests only; see the doc above for why \
                      `renderer.rs` cannot call this directly yet"
        )
    )]
    pub(crate) fn acquire(
        target: Arc<dyn WindowTarget>,
        build: impl FnOnce(Arc<dyn WindowTarget>) -> EngineResult<S>,
    ) -> EngineResult<Self> {
        probe_target(&target)?;
        let surface = build(Arc::clone(&target))?;
        Ok(Self { surface, target })
    }

    /// Re-probe the **same** retained owner and rebuild `S` from it.
    ///
    /// Not called by [`Renderer::recover`](super::renderer::Renderer::recover)
    /// for the same async-shape reason [`SurfaceLease::acquire`] documents;
    /// `recover` instead calls [`SurfaceLease::probe`] then
    /// [`SurfaceLease::replace_surface`] once the async rebuild finishes.
    /// Kept as the tested, generic form of that same two-step protocol.
    ///
    /// Never touches saved bytes; there are none to touch. `self.surface` is
    /// replaced only once `build` has succeeded, so a failed recovery leaves
    /// the previous (possibly still-usable) surface in place rather than
    /// tearing it down speculatively.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::SurfaceTargetUnavailable`] — leaving
    /// `self.surface` untouched — when the owner still reports the native
    /// target is gone or suspended. Otherwise returns whatever `build`
    /// returns, and `self.surface` is left untouched on that error too.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "exercised by this module's unit tests only; see the doc above for why \
                      `renderer.rs` cannot call this directly yet"
        )
    )]
    pub(crate) fn reacquire(
        &mut self,
        build: impl FnOnce(Arc<dyn WindowTarget>) -> EngineResult<S>,
    ) -> EngineResult<()> {
        self.probe()?;
        self.surface = build(Arc::clone(&self.target))?;
        Ok(())
    }

    /// Re-probe the retained owner without rebuilding `S`.
    ///
    /// Exposed so a caller that must build the replacement surface as part
    /// of a larger, non-`S`-shaped construction (`renderer.rs`'s
    /// `build_windowed_gpu_stack` returns the whole GPU stack, not just a
    /// surface, and is `async` where this crate's `build` closures are not)
    /// can still route through the identical probe used by
    /// [`SurfaceLease::acquire`]/[`SurfaceLease::reacquire`], then finish
    /// with [`SurfaceLease::replace_surface`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::SurfaceTargetUnavailable`] when the owner
    /// reports the native target is gone or suspended.
    pub(crate) fn probe(&self) -> EngineResult<()> {
        probe_target(&self.target)
    }

    /// Replace the surface after the caller has independently rebuilt it
    /// from [`SurfaceLease::target`] (see [`SurfaceLease::probe`]'s doc for
    /// why this exists as a separate step from `reacquire`).
    pub(crate) fn replace_surface(&mut self, surface: S) {
        self.surface = surface;
    }

    /// The window target this lease keeps alive.
    pub(crate) fn target(&self) -> &Arc<dyn WindowTarget> {
        &self.target
    }

    /// The current surface.
    pub(crate) fn surface(&self) -> &S {
        &self.surface
    }

    /// Wrap an already-probed target and its freshly built surface into a
    /// lease, without probing.
    ///
    /// The caller (`Renderer::new`) is required to have already called
    /// [`probe_target`] before doing the async work that produced `surface`
    /// — re-probing here would only duplicate that check.
    pub(crate) fn from_parts(target: Arc<dyn WindowTarget>, surface: S) -> Self {
        Self { surface, target }
    }
}

/// Probe the owner for a live handle, mapping a reported
/// [`raw_window_handle::HandleError`] to a typed engine error.
///
/// This duplicates the call wgpu's own `Instance::create_surface` makes
/// internally — it exists ONLY because wgpu's `CreateSurfaceError` hides the
/// `HandleError` it wraps (`CreateSurfaceErrorKind::RawHandle(_)`'s
/// `source()` returns `None`; only the formatted `Display` text survives —
/// verified against `wgpu-30.0.1/src/api/surface.rs`). Without probing here
/// first, "the window owner says the native target is gone/suspended" and
/// "the GPU driver refused for some unrelated reason" would collapse into
/// one undifferentiated [`EngineError::SurfaceCreation`], and callers like
/// `flui-app`'s device-recovery loop could not tell a transient,
/// wait-and-retry condition (a suspended Android surface) from one that
/// needs the whole renderer torn down. Do not delete this without restoring
/// that distinction some other way.
///
/// `pub(crate)` (not just used internally) because
/// [`Renderer::new`](super::renderer::Renderer::new) must probe strictly
/// before it starts any GPU work — before a lease exists to call
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
        // The Wayland live-smoke harness (`just live-smoke-wayland`) asserts
        // this line appears before the window-close line on both the
        // compositor-close and programmatic-close routes — it is the only
        // executed evidence that the surface (and this lease's `Arc` clone
        // of the target) was actually released rather than orphaned by the
        // pre-present-hook cycle described in
        // `crate::raster_owner::RasterOwner`'s module docs.
        tracing::debug!(
            target: "flui.gpu",
            event = "surface_released",
            "surface and its window target lease were dropped"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_ulong;
    use std::future::Future;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, WindowHandle, XlibDisplayHandle, XlibWindowHandle,
    };

    use super::*;

    /// A GPU-free [`WindowTarget`] double: hands out plain Xlib IDs (no
    /// pointer, so `WindowHandle::borrow_raw`/`DisplayHandle::borrow_raw`
    /// have no pointee whose validity to argue about), records every probe
    /// call, and can be switched to answer `Unavailable` mid-test.
    struct FakeTarget {
        window_id: Mutex<c_ulong>,
        available: Mutex<bool>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeTarget {
        fn new(window_id: c_ulong) -> Self {
            Self {
                window_id: Mutex::new(window_id),
                available: Mutex::new(true),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn set_available(&self, available: bool) {
            *self
                .available
                .lock()
                .expect("BUG: test-only mutex is never poisoned") = available;
        }

        fn set_window_id(&self, window_id: c_ulong) {
            *self
                .window_id
                .lock()
                .expect("BUG: test-only mutex is never poisoned") = window_id;
        }

        fn call_log(&self) -> Vec<&'static str> {
            self.calls
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
                .clone()
        }
    }

    impl HasWindowHandle for FakeTarget {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            self.calls
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
                .push("window_handle");
            if !*self
                .available
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
            {
                return Err(HandleError::Unavailable);
            }
            let window_id = *self
                .window_id
                .lock()
                .expect("BUG: test-only mutex is never poisoned");
            let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(window_id));
            // SAFETY: an Xlib window handle is a plain integer ID
            // (`c_ulong`), not a pointer — `borrow_raw`'s safety contract is
            // entirely about pointer fields staying valid, and there is no
            // pointee here for it to protect.
            #[expect(unsafe_code)] // test double, no FFI island: see SAFETY above
            let handle = unsafe { WindowHandle::borrow_raw(raw) };
            Ok(handle)
        }
    }

    impl HasDisplayHandle for FakeTarget {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            self.calls
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
                .push("display_handle");
            if !*self
                .available
                .lock()
                .expect("BUG: test-only mutex is never poisoned")
            {
                return Err(HandleError::Unavailable);
            }
            let raw = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
            // SAFETY: `display: None` carries no pointer at all, so there is
            // no pointee for `borrow_raw`'s safety contract to protect.
            #[expect(unsafe_code)] // test double, no FFI island: see SAFETY above
            let handle = unsafe { DisplayHandle::borrow_raw(raw) };
            Ok(handle)
        }
    }

    fn xlib_window_id(target: &Arc<dyn WindowTarget>) -> EngineResult<c_ulong> {
        let raw = target
            .window_handle()
            .map_err(EngineError::surface_target_unavailable)?
            .as_raw();
        match raw {
            RawWindowHandle::Xlib(handle) => Ok(handle.window),
            _ => unreachable!("BUG: FakeTarget only ever hands out Xlib handles"),
        }
    }

    #[test]
    fn dropping_the_lease_drops_the_only_strong_target_ref() {
        let target: Arc<dyn WindowTarget> = Arc::new(FakeTarget::new(1));
        let weak = Arc::downgrade(&target);

        let lease =
            SurfaceLease::acquire(target, |_| Ok(0u8)).expect("fake target starts available");
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

    #[test]
    fn drop_order_is_surface_before_target() {
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        struct RecordingSurface(Arc<Mutex<Vec<&'static str>>>);
        impl Drop for RecordingSurface {
            fn drop(&mut self) {
                self.0
                    .lock()
                    .expect("BUG: test-only mutex is never poisoned")
                    .push("surface");
            }
        }

        struct RecordingTarget {
            inner: FakeTarget,
            order: Arc<Mutex<Vec<&'static str>>>,
        }
        impl Drop for RecordingTarget {
            fn drop(&mut self) {
                self.order
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

        let target: Arc<dyn WindowTarget> = Arc::new(RecordingTarget {
            inner: FakeTarget::new(1),
            order: Arc::clone(&order),
        });
        let recorder = Arc::clone(&order);
        let lease = SurfaceLease::acquire(target, move |_| Ok(RecordingSurface(recorder)))
            .expect("fake target starts available");

        drop(lease);

        assert_eq!(
            *order
                .lock()
                .expect("BUG: test-only mutex is never poisoned"),
            vec!["surface", "target"],
            "the surface must be released before the target it was built from"
        );
    }

    #[test]
    fn polling_the_acquire_future_once_leaves_no_extra_strong_ref() {
        // `acquire` itself is synchronous; wrapping it in an `async` block
        // and driving it with a manual, no-op waker is how this pins "no
        // hidden leak of the `Arc::clone` handed to `build`" without a
        // runtime, and stays meaningful if a future slice makes `acquire`
        // genuinely `.await`-suspending.
        let target: Arc<dyn WindowTarget> = Arc::new(FakeTarget::new(1));
        let strong_before = Arc::strong_count(&target);

        let cloned = Arc::clone(&target);
        let mut future = Box::pin(async move { SurfaceLease::acquire(cloned, Ok) });

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let poll = future.as_mut().poll(&mut cx);
        assert!(matches!(poll, Poll::Ready(Ok(_))));

        drop(poll);
        drop(future);

        assert_eq!(
            Arc::strong_count(&target),
            strong_before,
            "no strong ref survives past the future and its produced lease"
        );
    }

    #[test]
    fn acquire_fails_before_build_when_target_reports_unavailable() {
        let fake = FakeTarget::new(1);
        fake.set_available(false);
        let target: Arc<dyn WindowTarget> = Arc::new(fake);
        let build_invoked = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&build_invoked);

        let result = SurfaceLease::acquire(target, move |_| {
            *flag.lock().expect("BUG: test-only mutex is never poisoned") = true;
            Ok(0u8)
        });

        assert!(matches!(
            result,
            Err(EngineError::SurfaceTargetUnavailable { .. })
        ));
        assert!(
            !*build_invoked
                .lock()
                .expect("BUG: test-only mutex is never poisoned"),
            "build must never run once the probe reports Unavailable"
        );
    }

    #[test]
    fn reacquire_leaves_the_surface_untouched_on_a_failed_probe_and_rebuilds_on_success() {
        let concrete = Arc::new(FakeTarget::new(1));
        let target: Arc<dyn WindowTarget> = concrete.clone();

        let mut lease = SurfaceLease::acquire(target, |t| xlib_window_id(&t))
            .expect("fake target starts available");
        assert_eq!(*lease.surface(), 1);

        concrete.set_available(false);
        let failed = lease.reacquire(|t| xlib_window_id(&t));
        assert!(matches!(
            failed,
            Err(EngineError::SurfaceTargetUnavailable { .. })
        ));
        assert_eq!(
            *lease.surface(),
            1,
            "a failed reacquire must not disturb the still-live surface"
        );

        concrete.set_available(true);
        concrete.set_window_id(2);
        lease
            .reacquire(|t| xlib_window_id(&t))
            .expect("target is available again with a fresh handle");
        assert_eq!(
            *lease.surface(),
            2,
            "the rebuilt surface must be built from the fresh handle value"
        );
        assert!(
            concrete.call_log().len() >= 4,
            "each attempt re-probes the owner"
        );
    }
}
