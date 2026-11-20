//! The owned-target/surface protocol, kept apart from the window slot so it
//! can be exercised without a GPU (issue #1043).
//!
//! `SurfaceLease<S>` is generic over the surface type `S` precisely so the
//! ownership and drop-order protocol can run under plain `#[test]` (and
//! later, Miri) with a fake `S` — the real `wgpu::Surface<'static>` is only
//! ever plugged in by the `SurfaceFactory` and the window slot it feeds.

use std::sync::Arc;

use flui_platform::FramePacing;

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
/// and drops it there and then. A released lease never gets a surface back:
/// the window's owner makes a replacement surface, in a lease of its own,
/// with its `SurfaceFactory`, and the slot takes that lease in a restore.
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
    #[expect(
        dead_code,
        reason = "held, never read: it outlives the surface built from it (see the field order above)"
    )]
    target: Arc<dyn WindowTarget>,
    /// Who paces the target's surfaces: the target's word, read when the
    /// lease was made, and the same for every surface it holds.
    pacing: FramePacing,
}

impl<S> SurfaceLease<S> {
    /// Drop the held surface and hold none.
    ///
    /// Call this at the last moment the native handle behind the surface is
    /// still valid — dropping a configured [`wgpu::Surface`] releases its
    /// swapchain, so it must not outlive the handle it was built from.
    /// Idempotent: a lease that already holds no surface stays that way. The
    /// target is deliberately **not** released: it stays until the lease is
    /// dropped, after the surface.
    pub(crate) fn release(&mut self) {
        self.surface = None;
    }

    /// Who paces the surfaces the retained target makes, which picks their
    /// present mode: fixed when the lease was made, so a reconfiguration
    /// after the window retired its target presents as before.
    pub(crate) const fn pacing(&self) -> FramePacing {
        self.pacing
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
    /// The caller (`SurfaceFactory::create`, or the GPU probe for the first
    /// window) is required to have already called [`probe_target`] before
    /// it made `surface` — re-probing here would only duplicate that check.
    /// `pacing` is the target's
    /// [`SurfaceTarget::frame_pacing`](flui_platform::SurfaceTarget::frame_pacing).
    pub(crate) fn from_parts(
        target: Arc<dyn WindowTarget>,
        surface: S,
        pacing: FramePacing,
    ) -> Self {
        Self {
            surface: Some(surface),
            target,
            pacing,
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
/// [`EngineError::SurfaceCreation`], and an owner could not tell a
/// transient, wait-and-retry condition (a suspended Android surface) from
/// one that needs the window's presentation torn down. Do not delete this
/// without restoring that distinction some other way.
///
/// `pub(crate)` because the surface factory's `checked_target` probes a
/// target strictly before wgpu sees it, before a lease exists.
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
        // Harnesses assert this line on both close routes (the window system's
        // close and a programmatic one). It is the only executed
        // witness that the surface — and this lease's `Arc` clone of the
        // target — was actually released rather than orphaned.
        //
        // `debug`, not `trace`: a lease is released far less often than a
        // surface is acquired (once per window life, not once per frame),
        // so it does not need `surface_acquired`'s per-frame trace budget —
        // a harness filtering `flui.gpu` broadly captures it either way.
        tracing::debug!(
            target: "flui.gpu",
            event = "surface_released",
            "surface and its window target lease is being dropped"
        );
    }
}

#[cfg(test)]
mod tests {

    use std::sync::Mutex;

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };

    use super::*;
    use crate::fake_window_target::FakeTarget;

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
            SurfaceLease::from_parts(
                target,
                RecordingSurface(Arc::clone(&log)),
                FramePacing::Display,
            ),
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
}
