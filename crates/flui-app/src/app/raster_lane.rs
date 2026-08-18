//! The inline raster lane — the app-side adopter of `flui_engine`'s raster
//! mailbox protocol (ADR-0045).
//!
//! [`RasterLane`] wraps a [`RasterOwner`] whose pump runs synchronously on
//! the owner (UI) thread — `RasterMode::Inline` in ADR-0045's terms, the
//! first-class mode wasm forces unconditionally and macOS is pinned to by an
//! upstream `wgpu-hal` constraint (issue #653). Every production frame the
//! desktop and Android runners submit now crosses the raster boundary as an
//! owned [`SceneSnapshot`] stamped with a [`FrameStamp`], is checked against
//! the lane's single [`SurfaceGeneration`] counter, and is rendered by
//! [`RasterOwner::pump`] — the same protocol a dedicated raster thread will
//! drive via `run_until_shutdown`, so threading the lane later changes who
//! calls `pump`, not what a frame is.
//!
//! # What the lane owns, and what stays outside
//!
//! Per ADR-0045 decision 1, the lane owns the backend (`RasterOwner`
//! solely owns its `RasterBackend`); the platform remains the authority for
//! the surface size. [`LaneStamp::physical_size`] is exactly the
//! owner-affine surface-size cell that decision names: the platform resize
//! path writes it (through [`RasterResizeHook::apply`]) and layout reads it
//! (through [`FrameSink::surface_size`]) — the backend's own `size()` is no
//! longer a layout input on this path, so layout never reaches across the
//! raster boundary for a number the platform already knows.
//!
//! # Generation discipline
//!
//! Every [`SurfaceGeneration`] mint routes through the mailbox's one
//! counter (ADR-0045 decision 4):
//!
//! - a platform resize mints eagerly via [`RasterHandle::resize`] and the
//!   returned value is stored in [`LaneStamp::surface_generation`] so the
//!   very next frame is stamped with it, before the pump has even applied
//!   the resize (generation-forward, no handshake);
//! - a mid-render surface loss mints inside the pump's render-failure path;
//!   the lane observes the rejection ([`PumpOutcome::SurfaceOutdated`]) and
//!   re-adopts [`SurfaceState::required_generation`] before the retry;
//! - device-loss recovery recreates the surface, so
//!   [`RasterLane::note_surface_recreated`] mints through the same resize
//!   entry point at the platform's latest known size.
//!
//! No component keeps a private counter, and a frame stamped with a stale
//! or zero generation is rejected before `render_scene` is ever called.

// The mailbox-lane half of this module is not wired on wasm32: the web
// runner still drives the direct sink (see `FrameSink`'s doc), so the lane
// machinery is compiled out there rather than left as dead code.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Receiver;
#[cfg(not(target_arch = "wasm32"))]
use flui_engine::{FrameDropReason, PumpOutcome, RasterAck, RasterHandle, RasterOwner};
use flui_engine::{EngineError, RasterBackend};
#[cfg(not(target_arch = "wasm32"))]
use flui_foundation::{FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress};
use flui_layer::Scene;
#[cfg(not(target_arch = "wasm32"))]
use flui_layer::{DamageRegion, SceneSnapshot};
#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;

/// What one submitted frame did, as the realm's frame transaction needs to
/// classify it: the same five behavioral buckets
/// `UiRealm::render_frame_entered`'s arms already distinguish, produced
/// uniformly by both the direct backend path and the raster-lane path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmitVerdict {
    /// The frame rendered and reached `present()`.
    Presented,
    /// The frame rendered successfully but never presented (no damage, or
    /// an occluded surface) — no vsync block happened, so the caller's
    /// no-present fallback pacing applies.
    NoPresent,
    /// The surface this frame was produced against is gone, outdated, or
    /// misconfigured (surface lost, validation failure, or a stale
    /// [`flui_foundation::SurfaceGeneration`] stamp). A retry against the
    /// reconfigured/restamped surface can succeed, so the caller arms one
    /// and retains the frame's input epochs.
    SurfaceStale,
    /// The GPU device was lost. Recovery is the runner's job
    /// (`render_frame_with_device_recovery`); the caller arms a retry and
    /// retains the frame's input epochs.
    DeviceLost,
    /// The frame failed in a way no retry can fix this frame (a generic
    /// render error, or a refused submit). No retry is armed.
    Failed,
}

/// The seam `UiRealm`'s frame transaction submits through: the surface size
/// layout must use, and the submit itself.
///
/// Two implementations exist, both production paths:
///
/// - [`RasterLane`] — the raster-mailbox path the desktop and Android
///   runners drive (ADR-0045's inline lane);
/// - [`DirectSink`] — the pre-mailbox direct call into a
///   [`RasterBackend`], still used by the web runner (whose renderer
///   arrives asynchronously and recovers across an `.await`, a shape the
///   lane does not yet accommodate) and by tests that pin the realm's frame
///   transaction against scripted backends.
///
/// Both feed the same realm-side classification arms via [`SubmitVerdict`],
/// so the retry/telemetry semantics cannot drift between them.
pub(crate) trait FrameSink {
    /// Physical surface size in pixels, as layout's root constraints input.
    fn surface_size(&mut self) -> (u32, u32);

    /// Submit one composited scene for rasterization and classify what
    /// happened.
    fn submit(&mut self, scene: Scene) -> SubmitVerdict;
}

/// Owner-affine stamp state shared between the lane and the platform's
/// resize hook.
///
/// Everything that touches it runs on the owner thread in practice, but
/// the platform's frame/resize callback registrations require `Send`
/// closures, so it is `Arc`-shared behind a private, uncontended
/// `parking_lot::Mutex` (never exposed — SP-6) rather than `Rc`/`Cell`.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct LaneStamp {
    inner: Mutex<LaneStampInner>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy)]
struct LaneStampInner {
    /// The [`flui_foundation::SurfaceGeneration`] the next submitted frame
    /// is stamped with — the latest value minted through the mailbox's one
    /// counter that this side has observed.
    surface_generation: flui_foundation::SurfaceGeneration,
    /// The platform-authoritative physical surface size (ADR-0045
    /// decision 1's owner-affine surface-size cell): written by the
    /// platform resize path, read by layout. Deliberately NOT read back
    /// from the backend — the backend's applied size can lag a coalesced,
    /// not-yet-pumped resize, and layout must always use the size the
    /// platform most recently announced.
    physical_size: (u32, u32),
}

#[cfg(not(target_arch = "wasm32"))]
impl LaneStamp {
    fn surface_generation(&self) -> flui_foundation::SurfaceGeneration {
        self.inner.lock().surface_generation
    }

    fn set_surface_generation(&self, generation: flui_foundation::SurfaceGeneration) {
        self.inner.lock().surface_generation = generation;
    }

    fn physical_size(&self) -> (u32, u32) {
        self.inner.lock().physical_size
    }
}

/// The platform resize path's entry into the lane: mints a fresh
/// [`flui_foundation::SurfaceGeneration`] through the mailbox's one counter
/// and updates the shared stamp state, without touching the backend — the
/// pump applies the actual surface reconfiguration before the next render.
///
/// Cheap to clone into the surface-applier closure; holds no backend and no
/// lock beyond the mailbox's own internal one.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub(crate) struct RasterResizeHook {
    handle: RasterHandle,
    stamp: Arc<LaneStamp>,
}

#[cfg(not(target_arch = "wasm32"))]
impl RasterResizeHook {
    /// Coalesces a resize into the mailbox, adopts the minted generation
    /// for the next frame's stamp, and records the platform's new size as
    /// the layout authority.
    ///
    /// The mint and the stamp adoption are two separate critical sections
    /// (the mailbox's own lock, then this stamp's); that is safe for the
    /// same reason [`RasterHandle::resize`]'s own doc gives — a frame is
    /// always stamped with whatever this side most recently observed, and
    /// an interleaving that stamps an older mint is ordinary staleness the
    /// pump rejects, never a false accept.
    pub(crate) fn apply(&self, width: u32, height: u32) {
        let generation = self.handle.resize(width, height);
        let mut inner = self.stamp.inner.lock();
        inner.surface_generation = generation;
        inner.physical_size = (width, height);
        drop(inner);
        tracing::debug!(
            width,
            height,
            surface_generation = ?generation,
            "raster lane: resize requested, next frame stamps the minted generation"
        );
    }
}

/// The inline raster lane: a [`RasterOwner`] pumped synchronously on the
/// owner thread, plus the stamp state that keeps its frames fresh.
///
/// See the module docs for the full ownership and generation story.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct RasterLane<B: RasterBackend> {
    owner: RasterOwner<B>,
    handle: RasterHandle,
    ack_rx: Receiver<RasterAck>,
    /// Held so an eventual shutdown-complete signal has a live receiver;
    /// the inline lane never blocks on it (the owner is dropped on this
    /// same thread instead).
    _shutdown_rx: Receiver<()>,
    address: PresentationAddress,
    epoch: FrameEpoch,
    stamp: Arc<LaneStamp>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<B: RasterBackend> RasterLane<B> {
    /// Builds the lane around `backend`, bound to `address`, with the
    /// window's initial physical size.
    ///
    /// Mints the first real [`flui_foundation::SurfaceGeneration`] by
    /// routing the initial size through [`RasterHandle::resize`] — the same
    /// entry point every later platform resize uses — so the first frame is
    /// never stamped [`flui_foundation::SurfaceGeneration::ZERO`] (which
    /// the pump rejects outright, by design).
    pub(crate) fn new(backend: B, address: PresentationAddress, width: u32, height: u32) -> Self {
        let (owner, handle, ack_rx, shutdown_rx) = RasterOwner::new(backend, address);
        let generation = handle.resize(width, height);
        let stamp = Arc::new(LaneStamp {
            inner: Mutex::new(LaneStampInner {
                surface_generation: generation,
                physical_size: (width, height),
            }),
        });
        Self {
            owner,
            handle,
            ack_rx,
            _shutdown_rx: shutdown_rx,
            address,
            epoch: FrameEpoch::ZERO,
            stamp,
        }
    }

    /// The resize entry point for the platform's surface applier.
    pub(crate) fn resize_hook(&self) -> RasterResizeHook {
        RasterResizeHook {
            handle: self.handle.clone(),
            stamp: Arc::clone(&self.stamp),
        }
    }

    /// Scoped access to the wrapped backend, for backend-specific concerns
    /// the raster protocol does not cover (device-loss recovery, the
    /// hot-reload plugin's direct scene path, test-state reads).
    pub(crate) fn with_backend<R>(&mut self, f: impl FnOnce(&mut B) -> R) -> R {
        self.owner.with_backend(f)
    }

    /// Whether the backend currently reports its GPU device lost.
    pub(crate) fn is_device_lost(&mut self) -> bool {
        self.owner.with_backend(|backend| backend.is_device_lost())
    }

    /// Records that device-loss recovery rebuilt the surface: mints a fresh
    /// generation through the same mailbox counter a resize uses (ADR-0045
    /// decision 4 names recovery's surface recreation as a mint site), at
    /// the platform's latest known size — NOT the backend's readback, which
    /// predates any resize that arrived while the device was down.
    pub(crate) fn note_surface_recreated(&mut self) {
        let (width, height) = self.stamp.physical_size();
        let generation = self.handle.resize(width, height);
        self.stamp.set_surface_generation(generation);
        tracing::info!(
            width,
            height,
            surface_generation = ?generation,
            "raster lane: surface recreated by device recovery, generation re-minted"
        );
    }

    /// Drains the lossy telemetry ack channel into trace events.
    ///
    /// Inline, the producer and consumer share one thread, so the drain is
    /// bounded by the handful of acks a single pump can emit — no
    /// concurrent producer can extend it.
    fn drain_acks(&self) {
        for ack in self.ack_rx.try_iter() {
            tracing::trace!(?ack, "raster lane: ack");
        }
    }

    /// Stamps, submits, and synchronously pumps one frame, classifying the
    /// outcome for the realm's frame transaction.
    fn submit_and_pump(&mut self, scene: Scene) -> SubmitVerdict {
        self.epoch = self.epoch.next();
        let epoch = self.epoch;
        let stamp = FrameStamp::new(
            self.address,
            epoch,
            self.stamp.surface_generation(),
            // The windowed backend is not `GpuServices`-backed yet, so both
            // sides of the resource-generation compare sit at `ZERO` — the
            // typed "no shared GPU services bound" state that axis's own
            // doc defines, not a bypass of the check.
            GpuResourceGeneration::ZERO,
        );
        let snapshot = SceneSnapshot::new(stamp, DamageRegion::Full, scene);
        if let Err(error) = self.handle.submit(snapshot) {
            // Inline, the owner lives in this very struct, so
            // `OwnerGone`/`ShuttingDown` can only mean teardown is already
            // underway and `AddressMismatch` cannot happen (the stamp above
            // is built from this lane's own bound address).
            tracing::error!(%error, "raster lane: frame submit refused");
            self.drain_acks();
            return SubmitVerdict::Failed;
        }
        let outcome = self.owner.pump();
        self.drain_acks();
        match outcome {
            PumpOutcome::Presented { .. } => {
                // `PumpOutcome::Presented` classifies a successful render
                // attempt; whether the frame actually reached `present()`
                // rides the reliable completion slot (see
                // `RasterOwner::pump`'s own comment at its `Ok(did_present)`
                // arm). The pacing decision must read that bit, never infer
                // it from the outcome name.
                let presented = self
                    .handle
                    .surface_state()
                    .last_completion
                    .is_some_and(|completion| completion.epoch == epoch && completion.presented);
                if presented {
                    SubmitVerdict::Presented
                } else {
                    SubmitVerdict::NoPresent
                }
            }
            PumpOutcome::SurfaceOutdated { stale, current, .. } => {
                // Covers both a stale-stamp rejection and a mid-render
                // surface loss/validation failure (the pump mints a fresh
                // generation on that path). Either way the retry must stamp
                // the generation the owner now requires.
                let required = self.handle.surface_state().required_generation;
                self.stamp.set_surface_generation(required);
                tracing::debug!(
                    ?stale,
                    ?current,
                    adopted = ?required,
                    "raster lane: surface stale, restamped for retry"
                );
                SubmitVerdict::SurfaceStale
            }
            PumpOutcome::ResourceOutdated { .. } => {
                // Unreachable until the windowed backend binds `GpuServices`
                // generations (both sides sit at `ZERO` today); classified
                // as stale-surface semantics — retry after rebinding —
                // rather than silently dropped, so the arm stays honest if
                // that wiring lands without this match being revisited.
                tracing::warn!("raster lane: frame rejected on the GPU-resource axis");
                SubmitVerdict::SurfaceStale
            }
            PumpOutcome::DeviceLost { .. } => SubmitVerdict::DeviceLost,
            PumpOutcome::Dropped { reason, .. } => {
                debug_assert!(
                    !matches!(reason, FrameDropReason::Superseded),
                    "BUG: an inline submit-then-pump cannot be superseded — nothing else \
                     submits between the two calls on one thread"
                );
                SubmitVerdict::Failed
            }
            // `Idle`/`ShutdownComplete`: an accepted inline submit is
            // pumped by the very next call on this same thread, so
            // observing no frame means the mailbox protocol was violated.
            // The wildcard also absorbs any future `#[non_exhaustive]`
            // variant, which this lane must classify deliberately before
            // relying on it — `Failed` (no retry armed) is the conservative
            // default until then.
            _ => {
                tracing::error!(
                    ?outcome,
                    "raster lane: unclassified pump outcome right after an accepted submit"
                );
                SubmitVerdict::Failed
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<B: RasterBackend> FrameSink for RasterLane<B> {
    fn surface_size(&mut self) -> (u32, u32) {
        self.stamp.physical_size()
    }

    fn submit(&mut self, scene: Scene) -> SubmitVerdict {
        self.submit_and_pump(scene)
    }
}

/// The direct, pre-mailbox submit path: renders through a borrowed
/// [`RasterBackend`] on the calling thread with no stamping and no
/// generation checks. See [`FrameSink`]'s doc for who still uses it and
/// why.
pub(crate) struct DirectSink<'a, R: RasterBackend> {
    renderer: &'a mut R,
}

impl<'a, R: RasterBackend> DirectSink<'a, R> {
    #[cfg_attr(
        all(not(target_arch = "wasm32"), not(test)),
        expect(
            dead_code,
            reason = "constructed by UiRealm::render_frame_entered, whose native production \
                      callers moved to the raster lane -- see FrameSink's own doc for who \
                      still drives this path"
        )
    )]
    pub(crate) fn new(renderer: &'a mut R) -> Self {
        Self { renderer }
    }
}

impl<R: RasterBackend> FrameSink for DirectSink<'_, R> {
    fn surface_size(&mut self) -> (u32, u32) {
        self.renderer.size()
    }

    fn submit(&mut self, scene: Scene) -> SubmitVerdict {
        self.renderer.mark_full_repaint();
        match self.renderer.render_scene(&scene) {
            Ok(true) => SubmitVerdict::Presented,
            Ok(false) => SubmitVerdict::NoPresent,
            Err(EngineError::SurfaceLost) => {
                tracing::debug!("surface lost during render");
                SubmitVerdict::SurfaceStale
            }
            Err(EngineError::SurfaceValidation) => {
                tracing::error!("surface validation error - surface misconfig");
                SubmitVerdict::SurfaceStale
            }
            Err(EngineError::DeviceLost) => SubmitVerdict::DeviceLost,
            Err(error) => {
                tracing::error!(?error, "render error (non-recoverable this frame)");
                SubmitVerdict::Failed
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests {
    use flui_foundation::{PresentationId, RealmId, SurfaceGeneration};
    use flui_layer::{CanvasLayer, Layer};
    use flui_types::Size;

    use super::*;

    fn test_address() -> PresentationAddress {
        PresentationAddress {
            realm_id: RealmId::new(1),
            presentation_id: PresentationId::new(1),
        }
    }

    fn test_scene() -> Scene {
        Scene::from_layer(Size::ZERO, Layer::from(CanvasLayer::new()), 0)
    }

    /// A scripted backend for lane-behavior tests: every render outcome is
    /// queued up front, and the applied state (resize calls, render calls)
    /// is observable afterwards.
    struct ScriptedBackend {
        outcomes: std::collections::VecDeque<Result<bool, EngineError>>,
        render_calls: u32,
        resizes: Vec<(u32, u32)>,
        lost: bool,
    }

    impl ScriptedBackend {
        fn presenting() -> Self {
            Self {
                outcomes: std::collections::VecDeque::new(),
                render_calls: 0,
                resizes: Vec::new(),
                lost: false,
            }
        }

        fn queue(mut self, outcome: Result<bool, EngineError>) -> Self {
            self.outcomes.push_back(outcome);
            self
        }
    }

    impl RasterBackend for ScriptedBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<bool, EngineError> {
            self.render_calls += 1;
            self.outcomes.pop_front().unwrap_or(Ok(true))
        }
        fn resize(&mut self, width: u32, height: u32) {
            self.resizes.push((width, height));
        }
        fn is_device_lost(&self) -> bool {
            self.lost
        }
        fn mark_dirty(&mut self, _rect: flui_types::Rect<flui_types::geometry::Pixels>) {}
        fn mark_full_repaint(&mut self) {}
        fn has_damage(&self) -> bool {
            true
        }
        fn size(&self) -> (u32, u32) {
            (100, 100)
        }
        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    #[test]
    fn a_presented_frame_classifies_presented_and_renders_through_the_mailbox() {
        let mut lane = RasterLane::new(ScriptedBackend::presenting(), test_address(), 640, 480);
        let verdict = lane.submit_and_pump(test_scene());
        assert_eq!(verdict, SubmitVerdict::Presented);
        lane.with_backend(|backend| {
            assert_eq!(
                backend.render_calls, 1,
                "the scene reached the backend through the mailbox pump"
            );
            assert_eq!(
                backend.resizes,
                vec![(640, 480)],
                "the construction-time mint's resize was applied before the first render"
            );
        });
    }

    #[test]
    fn a_no_present_completion_classifies_no_present() {
        let backend = ScriptedBackend::presenting().queue(Ok(false));
        let mut lane = RasterLane::new(backend, test_address(), 640, 480);
        assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::NoPresent);
    }

    #[test]
    fn the_resize_hook_mints_forward_and_the_next_frame_is_accepted() {
        let mut lane = RasterLane::new(ScriptedBackend::presenting(), test_address(), 640, 480);
        let before = lane.stamp.surface_generation();
        let hook = lane.resize_hook();
        hook.apply(1024, 768);
        let after = lane.stamp.surface_generation();
        assert!(
            after > before,
            "a resize mints a strictly newer generation ({after} vs {before})"
        );
        assert_eq!(
            lane.surface_size(),
            (1024, 768),
            "layout reads the platform's announced size before any pump applies it"
        );
        // The frame stamped with the fresh mint is accepted by the pump that
        // applies the resize in the same pass — generation-forward, no
        // handshake.
        assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::Presented);
        lane.with_backend(|backend| {
            assert_eq!(
                backend.resizes.last(),
                Some(&(1024, 768)),
                "the pump applied the coalesced resize before rendering"
            );
        });
    }

    #[test]
    fn a_mid_render_surface_loss_restamps_and_the_retry_is_accepted() {
        let backend = ScriptedBackend::presenting().queue(Err(EngineError::SurfaceLost));
        let mut lane = RasterLane::new(backend, test_address(), 640, 480);
        let stamped_before = lane.stamp.surface_generation();

        assert_eq!(
            lane.submit_and_pump(test_scene()),
            SubmitVerdict::SurfaceStale
        );
        let restamped = lane.stamp.surface_generation();
        assert!(
            restamped > stamped_before,
            "the lane adopted the loss-minted generation for the retry"
        );

        // The retry (the next queued outcome defaults to Ok(true)) renders
        // against the restamped generation instead of being rejected.
        assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::Presented);
    }

    #[test]
    fn a_device_loss_classifies_device_lost_and_recovery_reminting_unblocks() {
        let backend = ScriptedBackend::presenting().queue(Err(EngineError::DeviceLost));
        let mut lane = RasterLane::new(backend, test_address(), 640, 480);

        assert_eq!(
            lane.submit_and_pump(test_scene()),
            SubmitVerdict::DeviceLost
        );
        assert!(
            lane.handle.surface_state().device_lost,
            "the reliable slot reports the loss"
        );

        // The runner's recovery path re-mints after rebuilding the surface.
        lane.note_surface_recreated();
        assert_eq!(
            lane.submit_and_pump(test_scene()),
            SubmitVerdict::Presented,
            "a recovered lane renders again"
        );
        lane.with_backend(|backend| {
            assert_eq!(
                backend.resizes.last(),
                Some(&(640, 480)),
                "recovery re-minted at the platform's latest size"
            );
        });
    }

    #[test]
    fn recovery_reminting_uses_the_platform_size_not_the_backend_readback() {
        // A resize arrives while the device is down; recovery must re-mint
        // at THAT size, never the backend's stale readback (100x100 here).
        let backend = ScriptedBackend::presenting().queue(Err(EngineError::DeviceLost));
        let mut lane = RasterLane::new(backend, test_address(), 640, 480);
        assert_eq!(
            lane.submit_and_pump(test_scene()),
            SubmitVerdict::DeviceLost
        );
        lane.resize_hook().apply(1920, 1080);
        lane.note_surface_recreated();
        assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::Presented);
        lane.with_backend(|backend| {
            assert_eq!(
                backend.resizes.last(),
                Some(&(1920, 1080)),
                "the re-mint carried the platform's latest announced size"
            );
        });
    }

    #[test]
    fn a_zero_generation_stamp_is_rejected_not_rendered() {
        // Force the stamp back to ZERO to prove the pump's rejection is
        // live on this path (the constructor exists precisely so production
        // never starts there).
        let mut lane = RasterLane::new(ScriptedBackend::presenting(), test_address(), 640, 480);
        lane.stamp.set_surface_generation(SurfaceGeneration::ZERO);
        assert_eq!(
            lane.submit_and_pump(test_scene()),
            SubmitVerdict::SurfaceStale
        );
        lane.with_backend(|backend| {
            assert_eq!(
                backend.render_calls, 0,
                "a ZERO-stamped frame must never reach render_scene"
            );
        });
    }

    #[test]
    fn a_generic_render_failure_classifies_failed() {
        let backend = ScriptedBackend::presenting().queue(Err(EngineError::Timeout));
        let mut lane = RasterLane::new(backend, test_address(), 640, 480);
        assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::Failed);
    }
}
