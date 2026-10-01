//! The inline raster lane — the app-side adopter of `flui_engine`'s raster
//! mailbox protocol (ADR-0045).
//!
//! [`RasterLane`] wraps a [`RasterOwner`] whose pump runs synchronously on
//! the owner (UI) thread — `RasterMode::Inline` in ADR-0045's terms, the
//! first-class mode wasm forces unconditionally and macOS is pinned to by an
//! upstream `wgpu-hal` constraint (issue #653). Every production frame the
//! desktop and Android runners submit now crosses the raster boundary as an
//! owned [`SceneSnapshot`] stamped with a [`FrameStamp`], is checked against
//! the lane's single [`flui_foundation::SurfaceGeneration`] counter, and is rendered by
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
//! Every [`flui_foundation::SurfaceGeneration`] mint routes through the mailbox's one
//! counter (ADR-0045 decision 4):
//!
//! - a platform resize mints eagerly via [`RasterHandle::resize`] and the
//!   returned value is stored in [`LaneStamp::surface_generation`] so the
//!   very next frame is stamped with it, before the pump has even applied
//!   the resize (generation-forward, no handshake);
//! - a mid-render surface loss mints inside the pump's render-failure path;
//!   the lane observes the rejection ([`PumpOutcome::SurfaceOutdated`]) and
//!   re-adopts [`flui_engine::SurfaceState::required_generation`] before the retry;
//! - device-loss recovery recreates the surface, and so does the platform's
//!   surface-availability signal (`PlatformWindow::on_surface_status_change`'s
//!   `false`/`true` pair, which Android emits from four arms: `false` on
//!   `MainEvent::Pause` and `MainEvent::TerminateWindow`, `true` on
//!   `MainEvent::Resume` and `MainEvent::InitWindow` — a `Resume` whose
//!   window survived the pause recreates and mints with no `InitWindow` at
//!   all), so
//!   [`RasterLane::note_surface_recreated`] mints through the same resize
//!   entry point at the platform's latest known size.
//!
//! No component keeps a private counter, and a frame stamped with a stale
//! or zero generation is rejected before `render_scene` is ever called.

// The mailbox-lane half of this module is not wired on wasm32: the web
// runner still drives the direct sink (see `DirectSink`'s doc), so the lane
// machinery is compiled out there rather than left as dead code.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Receiver;
use flui_engine::{EngineError, PresentDisposition, RasterBackend};
#[cfg(not(target_arch = "wasm32"))]
use flui_engine::{FrameDropReason, PumpOutcome, RasterAck, RasterHandle, RasterOwner};
#[cfg(not(target_arch = "wasm32"))]
use flui_foundation::{FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress};
use flui_layer::Scene;
#[cfg(not(target_arch = "wasm32"))]
use flui_layer::{DamageMode, LayerDiffer, SceneSnapshot};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;

/// Owner-affine stamp state shared between the lane and the platform's
/// resize hook.
///
/// Everything that touches it runs on the owner thread in practice, but
/// the platform's frame/resize callback registrations require `Send`
/// closures, so it is `Arc`-shared behind a private, uncontended
/// `parking_lot::Mutex` (never exposed) rather than `Rc`/`Cell`.
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
        // A zero-sized resize (a minimized window) mints nothing and is not
        // the layout authority either: the surface stays at its last real
        // configuration and so does the size the next frame is laid out at.
        let Some(generation) = self.handle.resize(width, height) else {
            return;
        };
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

/// The environment variable that switches damage production off for every
/// presentation the process opens: `FLUI_DAMAGE=off` sends every frame as
/// `Full`, retains nothing and never renders a partial frame (ADR-0087 §3).
/// It is the fallback for stale pixels in the field, read once per lane, and
/// stays until an `AppConfig` switch carries the same choice per app.
#[cfg(not(target_arch = "wasm32"))]
const DAMAGE_ENV: &str = "FLUI_DAMAGE";

/// The damage mode [`DAMAGE_ENV`]'s value selects: `off` (any case) turns
/// damage off; unset, empty or `on` keeps the default; anything else keeps
/// the default and says so, since a typo must not silently disable the
/// fallback someone reached for.
#[cfg(not(target_arch = "wasm32"))]
fn damage_mode_from(value: Option<&str>) -> DamageMode {
    match value.map(str::trim) {
        None | Some("") => DamageMode::default(),
        Some(value) if value.eq_ignore_ascii_case("off") => DamageMode::Off,
        Some(value) if value.eq_ignore_ascii_case("on") => DamageMode::default(),
        Some(value) => {
            tracing::warn!(value, "{DAMAGE_ENV} takes `on` or `off`; damage stays on");
            DamageMode::default()
        }
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
    /// Compares each submitted scene with the one submitted before it and
    /// stamps the frame's damage (ADR-0087 §3). Relative to the last
    /// SUBMITTED scene: a frame the owner rejects or a submit supersedes
    /// still hands its damage to the backend, which keeps it until a frame
    /// presents.
    damage: LayerDiffer,
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
        // A zero-sized initial window leaves the stamp at `ZERO`, which the
        // pump rejects until the first real resize mints a generation.
        let generation = handle
            .resize(width, height)
            .unwrap_or(flui_foundation::SurfaceGeneration::ZERO);
        let stamp = Arc::new(LaneStamp {
            inner: Mutex::new(LaneStampInner {
                surface_generation: generation,
                physical_size: (width, height),
            }),
        });
        let mut lane = Self {
            owner,
            handle,
            ack_rx,
            _shutdown_rx: shutdown_rx,
            address,
            epoch: FrameEpoch::ZERO,
            stamp,
            damage: LayerDiffer::default(),
        };
        lane.set_damage_mode(damage_mode_from(std::env::var(DAMAGE_ENV).ok().as_deref()));
        lane
    }

    /// Switches damage production on or off for this presentation.
    /// [`DamageMode::Off`] retains nothing and sends every frame as `Full`,
    /// at the per-frame cost of having no differ.
    pub(crate) fn set_damage_mode(&mut self, mode: DamageMode) {
        self.damage.set_mode(mode);
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

    /// Records that the surface was rebuilt under this lane and mints a fresh
    /// generation through the same mailbox counter a resize uses (ADR-0045
    /// decision 4 names surface recreation as a mint site), at the platform's
    /// latest known size — NOT the backend's readback, which predates any
    /// resize that arrived while the old surface was gone.
    ///
    /// Three production causes reach here, and the caller is the one that
    /// knows which: device-loss recovery (`Renderer::recover`) on every
    /// backend; the platform's surface-availability signal — `false` then
    /// `true` from `PlatformWindow::on_surface_status_change` — which the
    /// mobile backends emit (Android from four arms: `false` on
    /// `MainEvent::Pause` and `MainEvent::TerminateWindow`, `true` on
    /// `MainEvent::Resume` and `MainEvent::InitWindow`; iOS on scene
    /// disconnect/connect); and the deadline-paced retry of a rebuild that
    /// failed after such a signal (`runner::surface_lifecycle`). On Android
    /// the window pair is the activity-recreation path; the lifecycle pair
    /// means a `Resume` whose window survived the pause also lands here, with
    /// no `InitWindow` involved.
    pub(crate) fn note_surface_recreated(&mut self) {
        let (width, height) = self.stamp.physical_size();
        let Some(generation) = self.handle.resize(width, height) else {
            return;
        };
        self.stamp.set_surface_generation(generation);
        tracing::info!(
            width,
            height,
            surface_generation = ?generation,
            "raster lane: surface recreated, generation re-minted"
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
            // The windowed backend owns a private GPU stack per renderer,
            // so both sides of the resource-generation compare sit at
            // `ZERO` — the typed "no shared GPU services bound" state that
            // axis's own doc defines, not a bypass of the check.
            GpuResourceGeneration::ZERO,
        );
        let damage = self.damage.diff(&scene, self.stamp.physical_size());
        let snapshot = SceneSnapshot::new(stamp, damage, scene);
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
                // attempt; what the frame actually became rides the reliable
                // completion slot (see `RasterOwner::pump`'s own comment at
                // its `Ok(reported)` arm). The pacing decision must read
                // that, never infer it from the outcome name.
                let disposition = self
                    .handle
                    .surface_state()
                    .last_completion
                    .filter(|completion| completion.epoch == epoch)
                    .map(|completion| completion.disposition);
                match disposition {
                    Some(PresentDisposition::Presented) => SubmitVerdict::Presented,
                    Some(PresentDisposition::NotShown) => SubmitVerdict::NotShown,
                    // Two different facts, one verdict. `NoDamage` is the
                    // backend answering "this frame owed the screen
                    // nothing"; `None` is the slot having no answer at all
                    // for this epoch — the pump reported a successful
                    // render, so the frame did run, but it recorded no
                    // disposition. Neither is a reason to retry: only
                    // `NotShown` says content was produced and lost, and
                    // inventing a retry from a missing fact would repaint an
                    // app that has nothing to draw.
                    Some(PresentDisposition::NoDamage) | None => SubmitVerdict::NoPresent,
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
                // Unreachable until a shared-stack backend binds
                // GPU-resource generations (both sides sit at `ZERO`
                // today); classified
                // as stale-surface semantics — retry after rebinding —
                // rather than silently dropped, so the arm stays honest if
                // that wiring lands without this match being revisited.
                tracing::warn!("raster lane: frame rejected on the GPU-resource axis");
                SubmitVerdict::SurfaceStale
            }
            PumpOutcome::DeviceLost { .. } => SubmitVerdict::DeviceLost,
            PumpOutcome::Dropped { reason, .. } => {
                // Completion is the reliable retry authority. Telemetry acks
                // can be dropped, and a stale completion cannot retry this epoch.
                if self
                    .handle
                    .surface_state()
                    .last_completion
                    .is_some_and(|completion| {
                        completion.epoch == epoch && completion.retry_required
                    })
                {
                    return SubmitVerdict::Retry;
                }
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
/// generation checks.
///
/// Two [`FrameSink`]s exist, both production paths: [`RasterLane`], the
/// raster-mailbox path the desktop and Android runners drive (ADR-0045's
/// inline lane), and this one, still used by the web runner (whose renderer
/// arrives asynchronously and recovers across an `.await`, a shape the lane
/// does not yet accommodate) and by tests that pin the realm's frame
/// transaction against scripted backends.
#[cfg_attr(
    not(any(target_arch = "wasm32", test)),
    expect(
        dead_code,
        reason = "the web runner's production sink (wasm32) and the scripted-backend test \
                  seam; the native runners pump through the raster lane instead"
    )
)]
pub(crate) struct DirectSink<'a, R: RasterBackend> {
    renderer: &'a mut R,
}

impl<'a, R: RasterBackend> DirectSink<'a, R> {
    #[cfg_attr(
        not(any(target_arch = "wasm32", test)),
        expect(
            dead_code,
            reason = "see DirectSink's own expectation: no native production caller"
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
        // Every frame is full on this path: it holds no differ, so the web
        // runner repaints in full until it moves onto the lane.
        self.renderer.mark_full_repaint();
        match self.renderer.render_scene(&scene) {
            Ok(PresentDisposition::Presented) => SubmitVerdict::Presented,
            Ok(PresentDisposition::NoDamage) => SubmitVerdict::NoPresent,
            Ok(PresentDisposition::NotShown) => SubmitVerdict::NotShown,
            Err(EngineError::SurfaceLost) => {
                tracing::debug!("surface lost during render");
                SubmitVerdict::SurfaceStale
            }
            Err(EngineError::SurfaceValidation) => {
                tracing::error!("surface validation error - surface misconfig");
                SubmitVerdict::SurfaceStale
            }
            Err(
                EngineError::DeviceLost
                | EngineError::GpuUnavailable
                | EngineError::GpuProgress { .. },
            ) => SubmitVerdict::DeviceLost,
            Err(error) if error.recoverability() == flui_engine::Recoverability::Recoverable => {
                SubmitVerdict::Retry
            }
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
    use flui_foundation::{PresentationId, RealmId};
    use flui_layer::{CanvasLayer, Layer};

    use super::*;

    fn test_address() -> PresentationAddress {
        PresentationAddress {
            realm_id: RealmId::new(1),
            presentation_id: PresentationId::new(1),
        }
    }

    /// A minimal non-empty scene: one canvas layer under a root.
    fn scene_from_canvas() -> Scene {
        Scene::new(flui_layer::LayerTree::new(Layer::from(CanvasLayer::new())))
    }

    fn test_scene() -> Scene {
        scene_from_canvas()
    }

    /// A scripted backend for lane-behavior tests: every render outcome is
    /// queued up front, and the applied state (resize calls, render calls)
    /// is observable afterwards.
    struct ScriptedBackend {
        outcomes: std::collections::VecDeque<Result<PresentDisposition, EngineError>>,
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

        fn queue(mut self, outcome: Result<PresentDisposition, EngineError>) -> Self {
            self.outcomes.push_back(outcome);
            self
        }
    }

    impl RasterBackend for ScriptedBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<PresentDisposition, EngineError> {
            self.render_calls += 1;
            self.outcomes
                .pop_front()
                .unwrap_or(Ok(PresentDisposition::Presented))
        }
        fn resize(&mut self, width: u32, height: u32) {
            self.resizes.push((width, height));
        }
        fn is_device_lost(&self) -> bool {
            self.lost
        }
        fn mark_dirty(&mut self, _rect: flui_foundation::geometry::Rect<f64>) {}
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

    /// A backend that records the damage it is handed and, like the wgpu
    /// renderer, presents only when something is owed.
    #[derive(Default)]
    struct DamageRecordingBackend {
        dirty: Vec<flui_foundation::geometry::Rect<f64>>,
        full: u32,
        owed: bool,
        renders: u32,
    }

    impl RasterBackend for DamageRecordingBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<PresentDisposition, EngineError> {
            self.renders += 1;
            if std::mem::take(&mut self.owed) {
                Ok(PresentDisposition::Presented)
            } else {
                Ok(PresentDisposition::NoDamage)
            }
        }
        fn resize(&mut self, _width: u32, _height: u32) {
            self.owed = true;
        }
        fn is_device_lost(&self) -> bool {
            false
        }
        fn mark_dirty(&mut self, rect: flui_foundation::geometry::Rect<f64>) {
            self.dirty.push(rect);
            self.owed = true;
        }
        fn mark_full_repaint(&mut self) {
            self.full += 1;
            self.owed = true;
        }
        fn has_damage(&self) -> bool {
            self.owed
        }
        fn size(&self) -> (u32, u32) {
            (640, 480)
        }
        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    /// A stamped root with one stamped boundary at `at`, 10x10.
    fn stamped_scene(
        root: &flui_layer::ContentToken,
        child: &flui_layer::ContentToken,
        at: flui_foundation::geometry::Offset<f64>,
    ) -> Scene {
        use flui_layer::{LayerNode, OffsetLayer, PictureLayer};
        let mut tree = flui_layer::LayerTree::new(
            LayerNode::new(Layer::from(OffsetLayer::zero()))
                .with_boundary(flui_foundation::RenderId::new(1), root.clone()),
        );
        let root_id = tree.root();
        let boundary = tree.push_child(
            root_id,
            LayerNode::new(Layer::from(OffsetLayer::new(at)))
                .with_boundary(flui_foundation::RenderId::new(2), child.clone()),
        );
        let mut canvas = flui_painting::Canvas::new();
        canvas.draw_rect(
            flui_foundation::geometry::Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
        );
        tree.push_child(boundary, Layer::from(PictureLayer::new(canvas.finish())));
        Scene::new(tree)
    }

    fn a_changed_boundary_reaches_the_backend_as_a_dirty_rect() {
        let mut lane = RasterLane::new(DamageRecordingBackend::default(), test_address(), 640, 480);
        let root = flui_layer::ContentToken::mint();
        let at = flui_foundation::geometry::Offset::new(100.0, 50.0);
        assert_eq!(
            lane.submit_and_pump(stamped_scene(&root, &flui_layer::ContentToken::mint(), at)),
            SubmitVerdict::Presented
        );
        assert_eq!(
            lane.submit_and_pump(stamped_scene(&root, &flui_layer::ContentToken::mint(), at)),
            SubmitVerdict::Presented
        );
        lane.with_backend(|backend| {
            assert_eq!(backend.full, 1, "only the first frame is full");
            let expected = flui_layer::DamageRect::covering(
                flui_foundation::geometry::Rect::from_xywh(100.0, 50.0, 10.0, 10.0),
                (640, 480),
            )
            .expect("on the surface")
            .to_rect();
            assert_eq!(backend.dirty, vec![expected]);
        });
    }

    /// An unchanged scene owes the screen nothing, so the frame does not
    /// present and the loop may park.
    fn an_identical_scene_does_not_present() {
        let mut lane = RasterLane::new(DamageRecordingBackend::default(), test_address(), 640, 480);
        let (root, child) = (
            flui_layer::ContentToken::mint(),
            flui_layer::ContentToken::mint(),
        );
        let at = flui_foundation::geometry::Offset::new(100.0, 50.0);
        assert_eq!(
            lane.submit_and_pump(stamped_scene(&root, &child, at)),
            SubmitVerdict::Presented
        );
        assert_eq!(
            lane.submit_and_pump(stamped_scene(&root, &child, at)),
            SubmitVerdict::NoPresent
        );
    }

    /// `FLUI_DAMAGE` is the field fallback: `off` in any case turns damage
    /// off, and nothing else does, a typo included.
    fn the_damage_variable_selects_the_mode() {
        for off in ["off", "OFF", " Off "] {
            assert_eq!(damage_mode_from(Some(off)), DamageMode::Off, "{off:?}");
        }
        for on in [
            None,
            Some(""),
            Some("on"),
            Some("ON"),
            Some("of"),
            Some("0"),
        ] {
            assert_eq!(damage_mode_from(on), DamageMode::default(), "{on:?}");
        }
    }

    fn damage_off_sends_every_frame_full_and_retains_nothing() {
        let mut lane = RasterLane::new(DamageRecordingBackend::default(), test_address(), 640, 480);
        lane.set_damage_mode(DamageMode::Off);
        let (root, child) = (
            flui_layer::ContentToken::mint(),
            flui_layer::ContentToken::mint(),
        );
        let at = flui_foundation::geometry::Offset::new(100.0, 50.0);
        for _ in 0..3 {
            assert_eq!(
                lane.submit_and_pump(stamped_scene(&root, &child, at)),
                SubmitVerdict::Presented
            );
            assert_eq!(lane.damage.retained_boundaries(), 0);
        }
        lane.with_backend(|backend| {
            assert_eq!(backend.full, 3);
            assert!(backend.dirty.is_empty());
        });
    }

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

    fn transient_and_hard_failures_map_consistently_in_lane_and_direct_sink() {
        for (make_error, expected) in [
            (
                (|| EngineError::GpuBackpressure) as fn() -> EngineError,
                SubmitVerdict::Retry,
            ),
            (|| EngineError::MissingRetainedSource, SubmitVerdict::Retry),
            (|| EngineError::Timeout, SubmitVerdict::Retry),
            (|| EngineError::GpuUnavailable, SubmitVerdict::DeviceLost),
            (|| EngineError::DeviceDomainMismatch, SubmitVerdict::Failed),
            (
                || EngineError::PreparedResourceLimit {
                    resource: "test",
                    requested: 2,
                    limit: 1,
                },
                SubmitVerdict::Failed,
            ),
        ] {
            let backend = ScriptedBackend::presenting().queue(Err(make_error()));
            let mut lane = RasterLane::new(backend, test_address(), 100, 100);
            assert_eq!(lane.submit_and_pump(test_scene()), expected);
            assert_eq!(lane.submit_and_pump(test_scene()), SubmitVerdict::Presented);
            lane.with_backend(|backend| assert_eq!(backend.render_calls, 2));
            let mut backend = ScriptedBackend::presenting().queue(Err(make_error()));
            let mut direct = DirectSink::new(&mut backend);
            assert_eq!(direct.submit(test_scene()), expected);
            assert_eq!(direct.submit(test_scene()), SubmitVerdict::Presented);
        }
    }

    #[test]
    fn raster_lane_outcome_matrix() {
        crate::table_test::run_table(
            "raster_lane_outcome_matrix",
            &[
                (
                    "transient_and_hard_failures_map_consistently_in_lane_and_direct_sink",
                    transient_and_hard_failures_map_consistently_in_lane_and_direct_sink as fn(),
                ),
                (
                    "a_presented_frame_classifies_presented_and_renders_through_the_mailbox",
                    a_presented_frame_classifies_presented_and_renders_through_the_mailbox
                        as fn(),
                ),
                (
                    "a_device_loss_classifies_device_lost_and_recovery_reminting_unblocks",
                    a_device_loss_classifies_device_lost_and_recovery_reminting_unblocks as fn(),
                ),
            ],
        );
    }

    #[test]
    fn raster_lane_damage_matrix() {
        crate::table_test::run_table(
            "raster_lane_damage_matrix",
            &[
                (
                    "a_changed_boundary_reaches_the_backend_as_a_dirty_rect",
                    a_changed_boundary_reaches_the_backend_as_a_dirty_rect as fn(),
                ),
                (
                    "an_identical_scene_does_not_present",
                    an_identical_scene_does_not_present as fn(),
                ),
                (
                    "the_damage_variable_selects_the_mode",
                    the_damage_variable_selects_the_mode as fn(),
                ),
                (
                    "damage_off_sends_every_frame_full_and_retains_nothing",
                    damage_off_sends_every_frame_full_and_retains_nothing as fn(),
                ),
            ],
        );
    }
}
