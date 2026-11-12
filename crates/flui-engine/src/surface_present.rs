//! A window surface's acquire-and-present protocol, shared by every owner of
//! a swapchain: the escalation from reconfigure to recreate, the
//! obligations it leaves between frames, and the classification of what a
//! frame became.

use crate::error::EngineError;
use std::sync::Arc;

use crate::error::EngineResult;
use crate::raster::PresentDisposition;
use crate::surface_color::OutputEncoding;

/// `DXGI_ERROR_FRAME_STATISTICS_DISJOINT`: the statistics broke off (a mode
/// change, a move to another output), so what came before does not compare.
#[cfg(windows)]
const DXGI_ERROR_FRAME_STATISTICS_DISJOINT: u32 = 0x887A_000B;

/// What the swap chain behind `surface` says about the frames it presented
/// before the one about to be: DXGI's frame statistics on DX12, and nothing
/// on the other backends or without a surface.
pub(crate) fn swap_chain_statistics(
    surface: Option<&wgpu::Surface<'static>>,
) -> flui_platform::SwapChainStatistics {
    #[cfg(windows)]
    if let Some(surface) = surface {
        return dxgi_frame_statistics(surface);
    }
    #[cfg(not(windows))]
    let _ = surface;
    flui_platform::SwapChainStatistics::None
}

/// The frame statistics of the DXGI swap chain behind `surface`, when wgpu
/// presents it through DX12: a shown present, a break in the statistics, or
/// nothing (another backend, or nothing shown yet: DXGI then answers an
/// error, not statistics).
///
/// The crate's one production `unsafe`: wgpu reaches its swap chain only
/// through the hal escape (`Surface::as_hal`), and DXGI's statistics are COM
/// calls on it.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "DXGI frame statistics are read from wgpu's swap chain, reachable only through the hal"
)]
#[expect(
    clippy::default_trait_access,
    reason = "`DXGI_FRAME_STATISTICS` is wgpu-hal's `windows` type, which this crate does not name"
)]
pub(crate) fn dxgi_frame_statistics(
    surface: &wgpu::Surface<'static>,
) -> flui_platform::SwapChainStatistics {
    use flui_platform::{DxgiFrameStatistics, SwapChainStatistics};

    // SAFETY: `as_hal` requires that the resource it hands out is not
    // destroyed unless its guard is the last reference to it and the GPU no
    // longer uses it, and that wgpu-hal's own rules hold. Nothing here
    // destroys anything: the guard is only read for a counted reference to
    // the swap chain (`swap_chain()` clones the COM pointer, an AddRef), and
    // is dropped before any call on that reference; the swap chain stays
    // wgpu's, alive while wgpu keeps it. The read runs on the thread that
    // presents through this surface, between its presents, so no wgpu call on
    // the surface overlaps it.
    let Some(hal) = (unsafe { surface.as_hal::<wgpu::hal::api::Dx12>() }) else {
        return SwapChainStatistics::None;
    };
    // Released at the end of this function: holding it past the present would
    // keep a swap chain wgpu replaced alive against its window.
    let Some(swap_chain) = hal.swap_chain() else {
        return SwapChainStatistics::None;
    };
    drop(hal);
    let mut statistics = Default::default();
    // SAFETY: a read of the swap chain's counters through a reference this
    // function holds, on the thread that presents through it; `statistics`
    // is an initialized `DXGI_FRAME_STATISTICS` the call fills and outlives.
    let read = unsafe { swap_chain.GetFrameStatistics(&raw mut statistics) };
    match read {
        Ok(()) => SwapChainStatistics::Dxgi(DxgiFrameStatistics::new(
            statistics.PresentCount,
            statistics.SyncRefreshCount,
            statistics.SyncQPCTime,
        )),
        #[expect(
            clippy::cast_sign_loss,
            reason = "an HRESULT's bits, compared with the documented value"
        )]
        Err(error) if error.code().0 as u32 == DXGI_ERROR_FRAME_STATISTICS_DISJOINT => {
            SwapChainStatistics::DxgiDisjoint
        }
        Err(_) => SwapChainStatistics::None,
    }
}

/// What a frame's presentation became, from the two facts `render_scene`
/// establishes before it paints anything.
///
/// Split out and named because the third cell of this table is the whole
/// reason [`PresentDisposition`] exists, and at the acquire site it is
/// invisible: "the surface handed us no texture" and "we had nothing to
/// draw" both look like `Ok(None)`/no-error, and a caller that merges them
/// ends its frame loop on a frame the screen never saw. A naked
/// `return Ok(PresentDisposition::NotShown)` would say the right thing to a
/// reader and be untestable — the arm needs a windowed surface with an
/// occluded drawable, which no test in this crate can construct — so the
/// decision lives here where a test can cover every cell.
pub(crate) fn classify_frame(had_damage: bool, acquired_surface: bool) -> PresentDisposition {
    match (had_damage, acquired_surface) {
        (false, _) => PresentDisposition::NoDamage,
        (true, true) => PresentDisposition::Presented,
        (true, false) => PresentDisposition::NotShown,
    }
}

/// Surface-acquisition outcomes normalized away from wgpu's concrete frame
/// type so the retry policy can be tested without constructing a GPU surface.
pub(crate) enum SurfaceAcquireOutcome<T> {
    Success(T),
    Suboptimal(T),
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
    /// A windowed renderer whose surface is deliberately released right now
    /// ([`crate::Renderer::release_surface`]), so there is nothing to present into
    /// and nothing wrong.
    ///
    /// This is a legitimate state, not a failure: a windowed renderer that
    /// owns no frame at this instant is exactly what a suspend looks like
    /// from here, and the next [`crate::Renderer::recreate_surface`] restores it.
    ///
    /// Deliberately **not** unified with [`SurfaceAcquireOutcome::Lost`],
    /// which is the surface itself reporting loss and needs the reconfigure
    /// that follows; a released surface has nothing to reconfigure.
    Released,
}

impl From<wgpu::CurrentSurfaceTexture> for SurfaceAcquireOutcome<wgpu::SurfaceTexture> {
    fn from(outcome: wgpu::CurrentSurfaceTexture) -> Self {
        match outcome {
            wgpu::CurrentSurfaceTexture::Success(frame) => Self::Success(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Self::Suboptimal(frame),
            wgpu::CurrentSurfaceTexture::Timeout => Self::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded => Self::Occluded,
            wgpu::CurrentSurfaceTexture::Outdated => Self::Outdated,
            wgpu::CurrentSurfaceTexture::Lost => Self::Lost,
            wgpu::CurrentSurfaceTexture::Validation => Self::Validation,
        }
    }
}

/// What the acquire path owes the surface between frames.
///
/// Two obligations outlive the acquisition that raised them, so they live on
/// the renderer rather than in a local of [`acquire_surface_texture_with`]:
///
/// - a frame acquired `Suboptimal` leaves the surface presentable but
///   mismatched (a rotation or scale change with the same extent), and only a
///   `Surface::configure` matches it again. The surface is configured right
///   after that frame is presented, not before the next acquire: the frame
///   may be the last one with damage, and a clean frame never reaches the
///   acquire, so a configure left for it could wait indefinitely. One
///   configure answers an episode: if the surface stays suboptimal after it,
///   frames present as they are until a `Success` ends the episode, so a
///   surface the configuration cannot satisfy never costs a configure per
///   frame.
/// - a recreation after `Lost` drops the old surface before building the new
///   one ([`crate::Renderer::recreate_surface`]), so a failed one leaves the renderer
///   holding no surface. Its owner never released it, so the acquire that
///   finds it released must not read that as a deliberate release and skip
///   frames forever: it recreates again first.
///
/// The owner's own [`crate::Renderer::release_surface`] and a successful
/// [`crate::Renderer::recreate_surface`] supersede both, and a resize or reconfigure
/// discharges the configure.
#[derive(Debug, Default)]
pub(crate) struct SurfaceRecovery {
    reconfigure_after_present: bool,
    suboptimal_answered: bool,
    pub(crate) recreate_owed: bool,
}

impl SurfaceRecovery {
    /// The surface was configured from the current configuration.
    pub(crate) fn configured(&mut self) {
        self.reconfigure_after_present = false;
    }

    /// The owner released or rebuilt the surface; nothing earlier is owed.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

pub(crate) trait SurfaceAcquireBackend {
    type Frame;

    fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError>;

    fn reconfigure(&mut self) -> Result<(), EngineError>;

    /// Build a new surface against the same device; see
    /// [`crate::Renderer::recreate_surface`].
    fn recreate(&mut self) -> Result<(), EngineError>;

    fn recovery(&mut self) -> &mut SurfaceRecovery;
}

/// Recreate a surface the acquire path found lost, recording the obligation
/// when the recreation fails so the next acquire retries it.
///
/// A transient failure (the window has no handle right now) reports
/// [`EngineError::SurfaceLost`], which the caller retries. A fatal one
/// ([`EngineError::SurfaceCreation`], a handle kind the owner never
/// supports) is returned as it is, so the caller stops retrying instead of
/// arming a retry loop. The obligation is kept either way: the surface may
/// already be dropped, and without it the next frame the caller does ask for
/// would read the renderer as released on purpose and skip in silence rather
/// than report the failure again.
fn recreate_lost_surface<B>(backend: &mut B) -> Result<(), EngineError>
where
    B: SurfaceAcquireBackend,
{
    match backend.recreate() {
        Ok(()) => {
            backend.recovery().reset();
            Ok(())
        }
        Err(error) => {
            backend.recovery().recreate_owed = true;
            if error.recoverability() == crate::error::Recoverability::Recoverable {
                tracing::warn!(
                    target: "flui.gpu",
                    event = "surface_recreate_failed",
                    %error,
                    "lost surface could not be recreated; retrying on the next frame"
                );
                Err(EngineError::SurfaceLost)
            } else {
                tracing::error!(
                    target: "flui.gpu",
                    event = "surface_recreate_failed",
                    %error,
                    "lost surface cannot be recreated"
                );
                Err(error)
            }
        }
    }
}

/// Acquire the next surface texture, escalating on failure: `Outdated`,
/// `Lost` and `Validation` reconfigure and retry once; a `Lost` that survives
/// the reconfigure recreates the surface, because `Lost` says the surface
/// itself is gone and no configuration of it can return a texture.
///
/// A recreation skips the frame (`Ok(None)`, which the caller retries) rather
/// than acquiring from the new surface: the frame was planned against the old
/// one, and the new one may differ in format or `COPY_SRC` support, which
/// decide where the frame renders.
pub(crate) fn acquire_surface_texture_with<B>(
    backend: &mut B,
) -> Result<Option<B::Frame>, EngineError>
where
    B: SurfaceAcquireBackend,
{
    if backend.recovery().recreate_owed {
        recreate_lost_surface(backend)?;
        return Ok(None);
    }
    let mut may_reconfigure = true;
    loop {
        match backend.acquire()? {
            SurfaceAcquireOutcome::Success(frame) => {
                backend.recovery().suboptimal_answered = false;
                return Ok(Some(frame));
            }
            SurfaceAcquireOutcome::Suboptimal(frame) => {
                let recovery = backend.recovery();
                if !recovery.suboptimal_answered {
                    tracing::debug!("Surface suboptimal; reconfiguring after this frame presents");
                    recovery.reconfigure_after_present = true;
                }
                return Ok(Some(frame));
            }
            SurfaceAcquireOutcome::Timeout => return Err(EngineError::Timeout),
            SurfaceAcquireOutcome::Occluded => {
                tracing::trace!("Surface occluded; skipping frame");
                return Ok(None);
            }
            SurfaceAcquireOutcome::Released => {
                // Not an error and not a retry: the windowed renderer holds
                // no surface right now because its owner released it, and
                // there is nothing to present into until the owner recreates
                // one. Reconfiguring would be meaningless (there is no
                // surface to configure) and a retry would spin.
                tracing::trace!("Surface released by its owner; skipping frame");
                return Ok(None);
            }
            SurfaceAcquireOutcome::Outdated | SurfaceAcquireOutcome::Lost if may_reconfigure => {
                backend.reconfigure()?;
                backend.recovery().configured();
                may_reconfigure = false;
            }
            SurfaceAcquireOutcome::Validation if may_reconfigure => {
                tracing::warn!("Surface texture validation error; reconfiguring and retrying once");
                backend.reconfigure()?;
                backend.recovery().configured();
                may_reconfigure = false;
            }
            SurfaceAcquireOutcome::Lost => {
                tracing::warn!(
                    target: "flui.gpu",
                    event = "surface_lost_after_reconfigure",
                    "surface still lost after a reconfigure; recreating it"
                );
                recreate_lost_surface(backend)?;
                return Ok(None);
            }
            SurfaceAcquireOutcome::Outdated => return Err(EngineError::SurfaceLost),
            SurfaceAcquireOutcome::Validation => {
                tracing::error!("Surface texture validation error after reconfigure");
                return Err(EngineError::SurfaceValidation);
            }
        }
    }
}

/// Configure a surface whose presented frame was acquired `Suboptimal`, and
/// mark its episode answered. Runs after the present, where the configure
/// cannot wait on a later frame that may never come (see [`SurfaceRecovery`]).
///
/// Returns whether it configured. A configure owes a full repaint that no
/// caller knows about (the frame reported itself presented), so the caller
/// repaints the same scene at once rather than leave the configured surface
/// waiting for unrelated damage.
pub(crate) fn reconfigure_after_suboptimal_present<B>(backend: &mut B) -> Result<bool, EngineError>
where
    B: SurfaceAcquireBackend,
{
    if !backend.recovery().reconfigure_after_present {
        return Ok(false);
    }
    backend.reconfigure()?;
    let recovery = backend.recovery();
    recovery.configured();
    recovery.suboptimal_answered = true;
    Ok(true)
}

/// The DX12 presentation system every window instance is made with: through
/// DirectComposition
/// (`CreateSwapChainForComposition` + an auto-created `IDCompositionVisual`)
/// instead of the default `CreateSwapChainForHwnd` redirection-bitmap path.
///
/// The HWND redirection bitmap + flip-model swapchain have no synchronization
/// between the swapchain flip and the window-rect change during a live resize,
/// so DWM stretches the in-flight back buffer to the new rect — the root cause
/// of resize "wobble" (confirmed: not fixable via present_mode / frame_latency /
/// DwmFlush / WM_SIZE timing — see winit#786, wgpu#2869, and the hardcoded
/// `DXGI_SCALING_STRETCH` in wgpu-hal dx12). Compositing through a DComp visual
/// lets DWM own the transform, which removes the stretch and gives smooth resize.
///
/// Opt out with `FLUI_DX12_NO_DCOMP=1` (RenderDoc cannot capture a composition
/// swapchain, so GPU-debugging the present path needs the plain HWND path).
pub(crate) fn dx12_options() -> wgpu::Dx12BackendOptions {
    if std::env::var_os("FLUI_DX12_NO_DCOMP").is_some() {
        tracing::debug!("DX12 DComp presentation disabled via FLUI_DX12_NO_DCOMP");
        wgpu::Dx12BackendOptions::default()
    } else {
        wgpu::Dx12BackendOptions {
            presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
            ..Default::default()
        }
    }
}

/// The windowed renderer's side of [`FrameProtocol::run`]: the swapchain
/// frame's clear, content and blit, over the renderer's fields borrowed apart
/// from its frame protocol.
///
/// [`FrameProtocol::run`]: crate::frame_protocol::FrameProtocol::run
pub(crate) struct SwapchainFrame<'a> {
    pub(crate) painter: &'a mut crate::painter::WgpuPainter,
    pub(crate) offscreen: &'a mut crate::offscreen::OffscreenRenderer,
    /// How the retained target reaches the swapchain.
    pub(crate) output: OutputEncoding,
    pub(crate) scene: &'a flui_layer::Scene,
    #[cfg(feature = "gpu-profiler")]
    pub(crate) gpu_profiler: &'a mut Option<crate::profiler::GpuFrameProfiler>,
}

impl crate::frame_protocol::FrameSteps for SwapchainFrame<'_> {
    /// Encode before content so ordered backdrop copies see a cleared target.
    fn clear(
        &mut self,
        render_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<()> {
        // Hoist the color attachments array before the #[cfg] split so both
        // the profiled and non-profiled paths share one definition. The descriptor
        // borrows `render_view`, so the array binding must live at the same scope level.
        let clear_color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: render_view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::frame_protocol::background_clear_value()),
                store: wgpu::StoreOp::Store,
            },
        })];
        let clear_pass_desc = wgpu::RenderPassDescriptor {
            label: Some("FLUI Clear Pass"),
            color_attachments: &clear_color_attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };
        // Profiler scope wraps the clear render pass. The scope borrows
        // the frame encoder exclusively; we access the encoder through
        // `scope.recorder` so the pass sees the same underlying encoder.
        // Scope drops at the end of the block; queries resolve at submission.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_ref() {
            let mut scope = profiler.scope("clear", encoder);
            {
                let _pass = scope.recorder().begin_render_pass(&clear_pass_desc);
            }
            // scope drops here → end_query fires
        } else {
            // Feature compiled but no capable adapter (gpu_profiler is None):
            // fall back to the unprofiled clear pass so the surface is still
            // cleared. Branching on the Option, not just the Cargo feature, is
            // what makes the documented "graceful no-op" actually graceful.
            let _pass = encoder.begin_render_pass(&clear_pass_desc);
        }
        #[cfg(not(feature = "gpu-profiler"))]
        {
            let _pass = encoder.begin_render_pass(&clear_pass_desc);
        }
        Ok(())
    }

    /// Traverses the scene's layer tree and flushes all painter batches to
    /// the GPU, reporting the damage-straddle check.
    fn content(
        &mut self,
        render_view: &wgpu::TextureView,
        render_texture: &wgpu::Texture,
        _intermediate_active: bool,
        partial_damage: Option<flui_foundation::geometry::Rect<f64>>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<bool> {
        let straddled =
            crate::Renderer::record_frame_content(self.painter, self.scene, partial_damage)?;
        let painter = &mut *self.painter;

        // Profiler scope wraps painter.render. The scope borrows
        // the frame encoder exclusively; we pass `scope.recorder` so
        // painter.render writes into the same underlying encoder.
        // Scope drops at the end of the block; queries resolve at submission.
        // Branch on the Option (runtime), not just the Cargo feature
        // (compile-time): when the feature is compiled but `gpu_profiler` is
        // None (incapable adapter), the render must STILL run — otherwise the
        // frame presents only the clear pass (blank content). This is the
        // documented graceful no-op.
        // The render target is always sampleable in this frame:
        //   - Direct path (supports_copy_src=true, intermediate_active=false):
        //     `render_texture` = `output.texture` which has COPY_SRC.
        //   - Retained path (intermediate_active=true): `render_texture` = the
        //     retained target, which has COPY_SRC|COPY_DST.
        // Both cases satisfy the dst-read contract required by advanced blend
        // and backdrop-filter.  The `view_only` fallback in `flush_opacity_layer`
        // is only reached from benches/tests that construct a bare TextureView
        // without a backing texture — see the reshaped fallback comments there.
        let frame_target =
            crate::render_target::RenderTarget::sampleable(render_view, render_texture);
        #[cfg(feature = "gpu-profiler")]
        let render_result = if let Some(profiler) = self.gpu_profiler.as_ref() {
            let mut scope = profiler.scope("final_render", encoder);
            painter.render(frame_target, scope.recorder())
            // scope drops here → end_query fires
        } else {
            painter.render(frame_target, encoder)
        };
        #[cfg(not(feature = "gpu-profiler"))]
        let render_result = painter.render(frame_target, encoder);
        render_result?;
        Ok(straddled)
    }

    fn blit(
        &mut self,
        retained: &wgpu::Texture,
        surface: &wgpu::Texture,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Arc<crate::device_domain::PreparedPermit>> {
        self.offscreen
            .encode_presentation(retained, surface, view, self.output, encoder)
    }

    fn submit(
        &mut self,
        encoder: wgpu::CommandEncoder,
        permits: Vec<Arc<crate::device_domain::PreparedPermit>>,
    ) -> EngineResult<()> {
        #[cfg(feature = "gpu-profiler")]
        let encoder = {
            let mut encoder = encoder;
            if let Some(profiler) = self.gpu_profiler.as_mut() {
                profiler.resolve_queries(&mut encoder);
            }
            encoder
        };
        self.painter
            .submit_frame_encoder(encoder, permits)
            .map(|_| ())
    }

    fn finish(&mut self) {
        self.painter.finish_frame();
    }
}

#[cfg(test)]
pub(crate) mod surface_acquisition_tests {
    use super::{
        SurfaceAcquireBackend, SurfaceAcquireOutcome, SurfaceRecovery,
        acquire_surface_texture_with, reconfigure_after_suboptimal_present,
    };
    use crate::error::EngineError;

    use SurfaceAcquireOutcome::{Lost, Outdated, Suboptimal, Success, Validation};

    /// A surface that answers from a script shared by every frame of a case
    /// and logs each call the acquire path makes on it.
    struct FakeSurface {
        outcomes: std::vec::IntoIter<SurfaceAcquireOutcome<u8>>,
        recreations: std::vec::IntoIter<Result<(), EngineError>>,
        calls: Vec<&'static str>,
        recovery: SurfaceRecovery,
    }

    impl SurfaceAcquireBackend for FakeSurface {
        type Frame = u8;

        fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError> {
            self.calls.push("acquire");
            Ok(self
                .outcomes
                .next()
                .expect("BUG: the case scripts every acquisition it makes"))
        }

        fn reconfigure(&mut self) -> Result<(), EngineError> {
            self.calls.push("reconfigure");
            Ok(())
        }

        fn recreate(&mut self) -> Result<(), EngineError> {
            self.calls.push("recreate");
            self.recreations.next().unwrap_or(Ok(()))
        }

        fn recovery(&mut self) -> &mut SurfaceRecovery {
            &mut self.recovery
        }
    }

    #[derive(Debug, PartialEq)]
    enum Acquired {
        Frame(u8),
        Skipped,
        SurfaceLost,
        SurfaceValidation,
        Fatal,
    }

    struct Case {
        name: &'static str,
        outcomes: Vec<SurfaceAcquireOutcome<u8>>,
        recreations: Vec<Result<(), EngineError>>,
        /// Per frame: what the acquisition returns and the calls it made.
        frames: Vec<(Acquired, &'static [&'static str])>,
    }

    fn cases() -> Vec<Case> {
        vec![
            Case {
                name: "outdated twice reconfigures once and never recreates",
                outcomes: vec![Outdated, Outdated],
                recreations: vec![],
                frames: vec![(
                    Acquired::SurfaceLost,
                    &["acquire", "reconfigure", "acquire"],
                )],
            },
            Case {
                name: "validation twice reconfigures once",
                outcomes: vec![Validation, Validation],
                recreations: vec![],
                frames: vec![(
                    Acquired::SurfaceValidation,
                    &["acquire", "reconfigure", "acquire"],
                )],
            },
            Case {
                name: "lost after a reconfigure recreates once and skips the frame",
                outcomes: vec![Lost, Lost, Success(1)],
                recreations: vec![],
                frames: vec![
                    (
                        Acquired::Skipped,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                    (Acquired::Frame(1), &["acquire"]),
                ],
            },
            Case {
                name: "outdated then lost recreates",
                outcomes: vec![Outdated, Lost, Success(1)],
                recreations: vec![],
                frames: vec![
                    (
                        Acquired::Skipped,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                    (Acquired::Frame(1), &["acquire"]),
                ],
            },
            Case {
                name: "a surface lost on every frame recreates once per frame",
                outcomes: vec![Lost, Lost, Lost, Lost],
                recreations: vec![],
                frames: vec![
                    (
                        Acquired::Skipped,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                    (
                        Acquired::Skipped,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                ],
            },
            Case {
                name: "a failed recreation is retried before the next acquire",
                outcomes: vec![Lost, Lost, Success(2)],
                recreations: vec![Err(EngineError::SurfaceTargetUnavailable {
                    source: raw_window_handle::HandleError::Unavailable,
                })],
                frames: vec![
                    (
                        Acquired::SurfaceLost,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                    (Acquired::Skipped, &["recreate"]),
                    (Acquired::Frame(2), &["acquire"]),
                ],
            },
            Case {
                name: "a fatal recreation failure is reported as it is",
                outcomes: vec![Lost, Lost],
                recreations: vec![
                    Err(EngineError::SurfaceTargetUnavailable {
                        source: raw_window_handle::HandleError::NotSupported,
                    }),
                    Err(EngineError::SurfaceTargetUnavailable {
                        source: raw_window_handle::HandleError::NotSupported,
                    }),
                ],
                frames: vec![
                    (
                        Acquired::Fatal,
                        &["acquire", "reconfigure", "acquire", "recreate"],
                    ),
                    (Acquired::Fatal, &["recreate"]),
                ],
            },
            Case {
                name: "one reconfigure answers a suboptimal episode",
                outcomes: vec![
                    Suboptimal(1),
                    Suboptimal(2),
                    Suboptimal(3),
                    Success(4),
                    Suboptimal(5),
                    Success(6),
                ],
                recreations: vec![],
                frames: vec![
                    (Acquired::Frame(1), &["acquire", "reconfigure", "repaint"]),
                    (Acquired::Frame(2), &["acquire"]),
                    (Acquired::Frame(3), &["acquire"]),
                    (Acquired::Frame(4), &["acquire"]),
                    (Acquired::Frame(5), &["acquire", "reconfigure", "repaint"]),
                    (Acquired::Frame(6), &["acquire"]),
                ],
            },
        ]
    }

    pub(crate) fn acquisition_escalates_from_reconfigure_to_recreate() {
        for case in cases() {
            let mut surface = FakeSurface {
                outcomes: case.outcomes.into_iter(),
                recreations: case.recreations.into_iter(),
                calls: Vec::new(),
                recovery: SurfaceRecovery::default(),
            };
            for (frame, (expected, calls)) in case.frames.into_iter().enumerate() {
                surface.calls.clear();
                let acquired = match acquire_surface_texture_with(&mut surface) {
                    Ok(Some(frame)) => {
                        // The frame presents; the renderer then settles a
                        // suboptimal acquisition, as `render_frame_inner` does.
                        if reconfigure_after_suboptimal_present(&mut surface)
                            .expect("the fake reconfigure succeeds")
                        {
                            surface.calls.push("repaint");
                        }
                        Acquired::Frame(frame)
                    }
                    Ok(None) => Acquired::Skipped,
                    Err(EngineError::SurfaceLost) => Acquired::SurfaceLost,
                    Err(EngineError::SurfaceValidation) => Acquired::SurfaceValidation,
                    Err(EngineError::SurfaceTargetUnavailable {
                        source: raw_window_handle::HandleError::NotSupported,
                    }) => Acquired::Fatal,
                    Err(other) => panic!("{}: frame {frame} failed with {other:?}", case.name),
                };
                assert_eq!(acquired, expected, "{}: frame {frame}", case.name);
                assert_eq!(surface.calls, calls, "{}: frame {frame}", case.name);
            }
        }
    }
}
