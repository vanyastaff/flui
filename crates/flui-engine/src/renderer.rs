//! Cross-platform GPU renderer with automatic backend selection
//!
//! This module provides a unified renderer that automatically selects the
//! appropriate GPU backend based on the target platform:
//!
//! - **macOS/iOS**: Metal 4
//! - **Windows**: DirectX 12 (Agility SDK)
//! - **Linux**: Vulkan 1.4 (Mesa 25.x)
//! - **Android**: Vulkan 1.3
//! - **Web**: WebGPU (with WebGL 2 fallback)
//!
//! # Architecture
//!
//! ```text
//! Renderer
//!   ├─ wgpu::Instance (backend selection)
//!   ├─ wgpu::Adapter (GPU selection)
//!   ├─ wgpu::Device (logical device)
//!   ├─ wgpu::Queue (command submission)
//!   └─ wgpu::Surface (window surface)
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! # async fn render(
//! #     window: impl flui_engine::WindowTarget,
//! #     scene: &flui_layer::Scene,
//! # ) -> Result<(), flui_engine::EngineError> {
//! use flui_engine::Renderer;
//!
//! // Create renderer (automatically selects backend). `window` is moved in
//! // — an owned, `'static` handle source (see `WindowTarget`), not a
//! // borrow — so the renderer can outlive the caller's stack frame.
//! let mut renderer = Renderer::new(window).await?;
//!
//! // Render frame. `true` means the frame reached the surface (a `false`
//! // is a no-damage/occluded skip, not a failure).
//! renderer.render_scene(scene)?;
//! # Ok(())
//! # }
//! ```

use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::error::{EngineError, EngineResult};
use crate::raster::PresentDisposition;
use crate::surface_lease::SurfaceLease;
use crate::window_target::WindowTarget;

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
fn classify_frame(had_damage: bool, acquired_surface: bool) -> PresentDisposition {
    match (had_damage, acquired_surface) {
        (false, _) => PresentDisposition::NoDamage,
        (true, true) => PresentDisposition::Presented,
        (true, false) => PresentDisposition::NotShown,
    }
}

/// Surface-acquisition outcomes normalized away from wgpu's concrete frame
/// type so the retry policy can be tested without constructing a GPU surface.
enum SurfaceAcquireOutcome<T> {
    Success(T),
    Suboptimal(T),
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
    /// A windowed renderer whose surface is deliberately released right now
    /// ([`Renderer::release_surface`]), so there is nothing to present into
    /// and nothing wrong.
    ///
    /// This is a legitimate state, not a failure: a windowed renderer that
    /// owns no frame at this instant is exactly what a suspend looks like
    /// from here, and the next [`Renderer::recreate_surface`] restores it.
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

trait SurfaceAcquireBackend {
    type Frame;

    fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError>;

    fn reconfigure(&mut self) -> Result<(), EngineError>;
}

fn acquire_surface_texture_with<B>(backend: &mut B) -> Result<Option<B::Frame>, EngineError>
where
    B: SurfaceAcquireBackend,
{
    let mut may_retry = true;
    loop {
        match backend.acquire()? {
            SurfaceAcquireOutcome::Success(frame) => return Ok(Some(frame)),
            SurfaceAcquireOutcome::Suboptimal(frame) => {
                tracing::debug!("Surface suboptimal; will reconfigure on next resize");
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
            SurfaceAcquireOutcome::Outdated | SurfaceAcquireOutcome::Lost if may_retry => {
                backend.reconfigure()?;
                may_retry = false;
            }
            SurfaceAcquireOutcome::Validation if may_retry => {
                tracing::warn!("Surface texture validation error; reconfiguring and retrying once");
                backend.reconfigure()?;
                may_retry = false;
            }
            SurfaceAcquireOutcome::Outdated | SurfaceAcquireOutcome::Lost => {
                return Err(EngineError::SurfaceLost);
            }
            SurfaceAcquireOutcome::Validation => {
                tracing::error!("Surface texture validation error after reconfigure");
                return Err(EngineError::SurfaceValidation);
            }
        }
    }
}

#[cfg(test)]
mod surface_acquisition_tests {
    use super::{SurfaceAcquireBackend, SurfaceAcquireOutcome, acquire_surface_texture_with};
    use crate::error::EngineError;

    struct FakeSurface {
        outcomes: std::vec::IntoIter<SurfaceAcquireOutcome<u8>>,
        reconfigure_count: usize,
    }

    impl FakeSurface {
        fn new(outcomes: Vec<SurfaceAcquireOutcome<u8>>) -> Self {
            Self {
                outcomes: outcomes.into_iter(),
                reconfigure_count: 0,
            }
        }
    }

    impl SurfaceAcquireBackend for FakeSurface {
        type Frame = u8;

        fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError> {
            self.outcomes.next().ok_or(EngineError::SurfaceLost)
        }

        fn reconfigure(&mut self) -> Result<(), EngineError> {
            self.reconfigure_count += 1;
            Ok(())
        }
    }

    pub(super) fn outdated_and_lost_still_share_the_single_retry_budget() {
        for initial in [SurfaceAcquireOutcome::Outdated, SurfaceAcquireOutcome::Lost] {
            let mut surface = FakeSurface::new(vec![initial, SurfaceAcquireOutcome::Lost]);

            let result = acquire_surface_texture_with(&mut surface);

            assert!(matches!(result, Err(EngineError::SurfaceLost)));
            assert_eq!(surface.reconfigure_count, 1);
        }
    }
}

#[cfg(test)]
mod new_probes_before_gpu_work_tests {
    use std::sync::Arc;

    use super::Renderer;
    use crate::error::EngineError;
    use crate::fake_window_target::FakeTarget;

    /// Pins the production entry point, GPU-free: `Renderer::new` must fail
    /// with `SurfaceTargetUnavailable` — and never touch `wgpu::Instance` —
    /// when the target's very first probe call reports the native handle is
    /// gone. `surface_lease::probe_target`'s own doc explains why this
    /// distinction exists; this is the production-path counterpart to
    /// `surface_lease.rs`'s `SurfaceLease`-level probe tests, which pin the
    /// same contract one layer down.
    ///
    /// The call log asserts it is genuinely the PROBE that answered, not
    /// `wgpu::Instance::create_surface` (which would also query
    /// `window_handle`/`display_handle`, just after already constructing an
    /// `Instance` and picking a backend) — `probe_target` calls
    /// `window_handle()` first and short-circuits via `?` on `Unavailable`
    /// without ever calling `display_handle()`, so a log of anything other
    /// than exactly `["window_handle"]` means the probe ran further than it
    /// should have, or something downstream of it ran at all.
    #[test]
    fn renderer_construction_fails_before_gpu_work_and_surface_acquisition_shares_one_retry() {
        super::surface_acquisition_tests::outdated_and_lost_still_share_the_single_retry_budget();
        renderer_new_fails_before_instance_creation_when_target_is_unavailable();
        device_request_uses_only_advertised_features();
    }

    fn device_request_uses_only_advertised_features() {
        for features in [
            wgpu::Features::empty(),
            wgpu::Features::DUAL_SOURCE_BLENDING,
            wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            wgpu::Features::IMMEDIATES,
            wgpu::Features::TIMESTAMP_QUERY,
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        ] {
            let capabilities = super::GpuCapabilities {
                backend: wgpu::Backend::BrowserWebGpu,
                adapter_name: String::new(),
                vendor: String::new(),
                features,
                limits: wgpu::Limits::default(),
            };
            let requested = Renderer::required_features(&capabilities);
            assert!(
                features.contains(requested),
                "device request {requested:?} exceeds adapter features {features:?}"
            );
        }
    }

    fn renderer_new_fails_before_instance_creation_when_target_is_unavailable() {
        let target = Arc::new(FakeTarget::unavailable());

        let result = pollster::block_on(Renderer::new(Arc::clone(&target)));

        // `Renderer` carries no `Debug` impl (wgpu handles don't), so match
        // explicitly instead of formatting the whole `Result`.
        match result {
            Err(EngineError::SurfaceTargetUnavailable { .. }) => {}
            Err(other) => {
                panic!("expected SurfaceTargetUnavailable, got a different error: {other:?}")
            }
            Ok(_) => panic!(
                "expected SurfaceTargetUnavailable, got Ok(Renderer) — the probe never fired"
            ),
        }
        assert_eq!(
            target.call_log(),
            vec!["window_handle"],
            "the probe must fail on the first query (window_handle) and never reach \
             display_handle, wgpu::Instance::new, or create_surface"
        );
    }
}

/// What the GPU adapter reports, and what this engine asked for.
///
/// Holds the adapter's own `Features`/`Limits` tables rather than one `bool`
/// per question: a query is a method (so a capability this engine starts
/// using needs no struct change and no `struct_field_names` suppression), and
/// the tables are what a caller needs when it wants to ask something this
/// type does not.
///
/// `#[non_exhaustive]`: wgpu adds features and limit buckets, and this struct
/// tracks them. Construction is [`Self::detect`] — an embedder reads the
/// fields, it does not build one.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// The backend in use (Metal, DX12, Vulkan, WebGPU, …).
    pub backend: wgpu::Backend,
    /// Adapter name, vendor, and device type as wgpu reports them.
    pub adapter_name: String,
    /// Human-readable vendor name resolved from the PCI vendor id.
    pub vendor: String,
    /// The features the adapter exposes.
    pub features: wgpu::Features,
    /// The limits the adapter reports.
    pub limits: wgpu::Limits,
}

impl GpuCapabilities {
    /// Detect GPU capabilities from an adapter.
    #[must_use]
    pub fn detect(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        Self {
            backend: info.backend,
            adapter_name: info.name,
            vendor: Self::vendor_name(info.vendor),
            features: adapter.features(),
            limits: adapter.limits(),
        }
    }

    /// Whether the adapter accepts immediate constants ("push constants").
    #[must_use]
    pub fn supports_push_constants(&self) -> bool {
        self.features.contains(wgpu::Features::IMMEDIATES)
    }

    /// Whether the adapter can run timestamp queries through an encoder.
    ///
    /// Both `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS` are
    /// required: the encoder-level scopes `GpuFrameProfiler` uses need the
    /// second, and without it wgpu-profiler records 0.0 ms for every scope —
    /// it passes tests while measuring nothing.
    #[must_use]
    pub fn supports_timestamp_queries(&self) -> bool {
        self.features.contains(wgpu::Features::TIMESTAMP_QUERY)
            && self
                .features
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
    }

    /// Whether the adapter exposes a second blend source (`@blend_src(1)`).
    ///
    /// Without it, the blend modes whose destination factor ignores source
    /// alpha (`Clear`, `Src`, `SrcIn`, `SrcOut`, `Modulate`, `DstIn`,
    /// `DstATop` — see `crate::pipeline_cache::destination_alpha_scale_for`)
    /// use independent source/coverage isolation and destination sampling for
    /// exact partial coverage. This flag identifies the optional fast path.
    pub fn supports_dual_source_blending(&self) -> bool {
        self.features.contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    }

    fn vendor_name(vendor_id: u32) -> String {
        match vendor_id {
            0x1002 => "AMD".to_string(),
            0x10DE => "NVIDIA".to_string(),
            0x8086 => "Intel".to_string(),
            0x106B => "Apple".to_string(),
            0x1414 => "Microsoft (WARP)".to_string(),
            0x5143 => "Qualcomm".to_string(),
            _ => format!("Unknown (0x{vendor_id:04X})"),
        }
    }
}

/// GPU context available during layer tree rendering.
///
/// Bundled GPU stack rebuilt by `new` (windowed path) and `recover`.
///
/// All fields are moved into `Renderer` after construction — this struct is
/// a local bundle, not a long-lived allocation.
struct WindowedGpuStack {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    capabilities: GpuCapabilities,
    painter: crate::painter::WgpuPainter,
    offscreen: crate::offscreen::OffscreenRenderer,
    supports_copy_src: bool,
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: Option<crate::profiler::GpuFrameProfiler>,
}

/// Cross-platform GPU renderer
///
/// `Send` by compiler derivation: every field is `Send`, including
/// `gpu_stack_origin`'s `Arc<dyn WindowTarget>` (via [`WindowTarget`]'s own
/// `Send + Sync + 'static` bound) and its `wgpu::Surface<'static>` (`Send +
/// Sync` per wgpu, on wasm32 via the crate's `fragile-send-sync-non-atomic-wasm`
/// feature). Issue #1043 deleted the crate's one hand-written `unsafe impl
/// Send` (it covered two raw platform handles the renderer no longer keeps —
/// see `SurfaceLease`/`WindowTarget` in the sibling modules) — there is no
/// manual `Send` assertion left anywhere in this file.
///
/// Deliberately never `Sync`: the raster owner
/// (`crate::raster_owner::RasterOwner`) has sole mutable access to the
/// `Renderer`/`Surface`/`Device`/`Queue` it wraps, and a shared `&Renderer`
/// across threads would defeat that single-mutator contract. Before #1043
/// this held only by accident, riding on `pre_present_hook`'s `Box<dyn
/// FnMut() + Send>` field (a `!Sync` type with no `!Sync` marker of its
/// own would have quietly gone `Sync` the day that field's type changed);
/// the `_single_mutator` marker field below now states the contract as a
/// field, not a side effect. The doctest below pins the contract so a
/// future field addition that accidentally makes every field `Sync` fails
/// loudly at compile time instead of silently reopening `Arc<Renderer>`
/// shared-mutation. The `assert_impl_all!`/`assert_not_impl_any!` pair after
/// this struct pins the `Send`/`!Sync` split itself.
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<flui_engine::Renderer>();
/// ```
pub struct Renderer {
    // `instance` and `adapter` are kept alive for the lifetime of the renderer
    // because `wgpu::Surface<'static>` and `wgpu::Device` depend on them. They
    // are not read post-init in production code; the `#[allow(dead_code)]`
    // markers document that the keep-alive shape is intentional.
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
    capabilities: GpuCapabilities,
    painter: crate::painter::WgpuPainter,
    offscreen: crate::offscreen::OffscreenRenderer,
    /// Whether the surface supports COPY_SRC (for mid-frame texture copies)
    supports_copy_src: bool,
    /// Set by the device-lost callback; checked at frame start to trigger
    /// device recreation. `Arc<AtomicBool>` because the callback is `'static`.
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// The damage owed, the retained last frame a partial frame repaints
    /// into, and the one-frame promotion to full — the protocol the headless
    /// retained capture runs too (see [`crate::frame_protocol`]).
    frame: crate::frame_protocol::FrameProtocol,
    /// Runs immediately before every `queue.present` — see
    /// [`crate::RasterBackend::set_pre_present_hook`].
    pre_present_hook: Option<crate::raster::PrePresentHook>,
    /// The owned window target and the surface built from it; the surface
    /// is absent while released (`release_surface`), which is the one state
    /// a renderer has besides "windowed". See [`SurfaceLease`].
    lease: SurfaceLease<wgpu::Surface<'static>>,
    /// States the "single mutator, never shared" rule as a field instead of
    /// a side effect of some other field's type. `Cell<()>` is `!Sync`;
    /// `PhantomData` of it carries that without occupying space or affecting
    /// `Send` (`Cell<()>` is `Send`) or drop-check (nothing to drop).
    _single_mutator: PhantomData<Cell<()>>,
    /// GPU timestamp profiler. `None` when the `gpu-profiler` feature is off
    /// or the adapter does not expose `wgpu::Features::TIMESTAMP_QUERY`.
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: Option<crate::profiler::GpuFrameProfiler>,

    /// Test-only flag that forces the intermediate-texture present path ON,
    /// even when the surface supports COPY_SRC.  Allows C2/C3 tests to
    /// exercise and verify the intermediate path on COPY_SRC-capable hardware.
    ///
    /// Controlled by [`Renderer::force_intermediate_for_testing`].
    #[cfg(test)]
    force_intermediate: bool,
}

// `Renderer: Send` is a compiler derivation: every field is `Send` —
// `gpu_stack_origin`'s `Arc<dyn WindowTarget>` and `wgpu::Surface<'static>`
// included, per their own bounds (see the struct doc above). There is no
// manual `Send` assertion anywhere in this crate any more (issue #1043
// deleted the private newtype that used to narrow two raw platform handles
// into one — the renderer no longer keeps raw handles at all). `Renderer:
// !Sync` is likewise no longer an accident of some other field's type:
// `_single_mutator: PhantomData<Cell<()>>` states it directly.
// Pinned below; the crate's `deny(unsafe_code)` also refuses a
// hand-reintroduced blanket `unsafe impl Send`/`Sync` for it.
static_assertions::assert_impl_all!(Renderer: Send);
static_assertions::assert_not_impl_any!(Renderer: Sync);

impl SurfaceAcquireBackend for Renderer {
    type Frame = wgpu::SurfaceTexture;

    fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError> {
        // Matched directly on the origin rather than through a
        // `is_windowed()`-style predicate: "windowed with no surface held" is
        // a legitimate state that must skip the present, and "owns no window"
        // is a program error that must stay loud, so the two cases need
        // different answers and the enum is where the difference lives.
        let Some(surface) = self.lease.surface() else {
            return Ok(SurfaceAcquireOutcome::Released);
        };
        // Under `Fifo` with a frame latency of 1 this is where the vsync
        // block lands (ADR-0045 decision 3), so its duration is the one
        // number that says whether the display is pacing this thread.
        // Measured on the native AppKit backend 2026-09-17: it is not the
        // steady-state pacer there (p50 62 µs of a 10 ms period — AppKit's
        // display-pass cadence is), but it IS where that backend's ~3.4 % of
        // late frames wait, at p50 8.3 ms, while their own CPU phases stay
        // near 80 µs. `acquire_us` is therefore the number this comment
        // promised it was.
        let acquire_started = crate::frame_timing::now();
        let acquired = surface.get_current_texture();
        let outcome = match &acquired {
            wgpu::CurrentSurfaceTexture::Success(_) => "success",
            wgpu::CurrentSurfaceTexture::Suboptimal(_) => "suboptimal",
            wgpu::CurrentSurfaceTexture::Timeout => "timeout",
            wgpu::CurrentSurfaceTexture::Occluded => "occluded",
            wgpu::CurrentSurfaceTexture::Outdated => "outdated",
            wgpu::CurrentSurfaceTexture::Lost => "lost",
            wgpu::CurrentSurfaceTexture::Validation => "validation",
        };
        tracing::trace!(
            target: "flui.gpu",
            event = "surface_acquired",
            acquire_us = acquire_started.elapsed().as_micros() as u64,
            outcome,
            "surface texture acquired"
        );
        Ok(SurfaceAcquireOutcome::from(acquired))
    }

    fn reconfigure(&mut self) -> Result<(), EngineError> {
        Self::reconfigure_surface(self);
        Ok(())
    }
}

impl Renderer {
    /// Create a new renderer with automatic backend selection
    ///
    /// # Platform Behavior
    ///
    /// - **macOS/iOS**: Uses Metal backend
    /// - **Windows**: Uses DirectX 12 backend
    /// - **Linux**: Uses Vulkan backend
    /// - **Android**: Uses Vulkan backend
    /// - **Web**: Uses WebGPU backend (falls back to WebGL 2)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn run(window: impl flui_engine::WindowTarget)
    /// #     -> Result<(), flui_engine::EngineError> {
    /// use flui_engine::Renderer;
    ///
    /// // `window` is moved in — an owned, `'static` handle source (see
    /// // `WindowTarget`), not a borrow.
    /// let renderer = Renderer::new(window).await?;
    /// println!("Using backend: {:?}", renderer.capabilities().backend);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # One GPU stack per renderer, today
    ///
    /// This builds a private `Instance → Adapter → Device → Queue` stack per
    /// call. ADR-0045 decision 2's alternative — one shared stack per owner
    /// thread, with the per-window surface created alongside its own
    /// `Instance` — is the target shape, and its windowed half needs the
    /// `ReplaceServices` re-pointing mechanism device recovery requires once
    /// several renderers share a device. That mechanism is not built, so
    /// this constructor is the one this crate ships; it is the advertised
    /// entry point, not a temporary beside an unwired shared value type.
    pub async fn new(target: impl WindowTarget) -> EngineResult<Self> {
        // An `Arc<dyn PlatformWindow>` (the common caller shape) becomes an
        // `Arc<Arc<dyn PlatformWindow>>` here — forced: `Arc<dyn
        // PlatformWindow>` cannot upcast to `Arc<dyn WindowTarget>` without
        // `PlatformWindow: WindowTarget`, which would invert the
        // flui-platform → flui-engine layer edge (`cargo xtask workspace`).
        // One extra pointer chase per surface creation; documented, not
        // fixed — see issue #1043.
        let (w, h) = (800u32, 600u32); // Will be updated on first resize
        let (target, stack) = Self::probe_then_build(Arc::new(target), |target| async move {
            let stack = Self::build_windowed_gpu_stack(&target, w, h).await?;
            Ok((target, stack))
        })
        .await?;
        let lease = SurfaceLease::from_parts(target, stack.surface);

        Ok(Self {
            instance: stack.instance,
            adapter: stack.adapter,
            device: stack.device,
            queue: stack.queue,
            config: stack.config,
            capabilities: stack.capabilities,
            painter: stack.painter,
            offscreen: stack.offscreen,
            supports_copy_src: stack.supports_copy_src,
            device_lost: stack.device_lost,
            frame: crate::frame_protocol::FrameProtocol::new(),
            pre_present_hook: None,
            lease,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: stack.gpu_profiler,
            #[cfg(test)]
            force_intermediate: false,
            _single_mutator: PhantomData,
        })
    }

    /// Probe the target, then build the GPU stack against it.
    ///
    /// The probe runs BEFORE any GPU work starts (before even
    /// `wgpu::Instance::new`, inside `build_windowed_gpu_stack`) — see
    /// `surface_lease::probe_target`'s doc for why this distinction from a
    /// generic `SurfaceCreation` failure matters to callers.
    ///
    /// `build` is a parameter rather than an inline call so the
    /// drop-on-cancel contract is testable without a GPU: a test can pass a
    /// builder that never resolves, then drop the future and observe that
    /// the only extra `Arc<dyn WindowTarget>` went with it. That is the one
    /// ownership fact a cancelled `Renderer::new` owes — an async fn dropped
    /// mid-`.await` must not strand a clone the caller cannot reach
    /// (issue #1149).
    async fn probe_then_build<S, F, Fut>(
        target: Arc<dyn WindowTarget>,
        build: F,
    ) -> EngineResult<(Arc<dyn WindowTarget>, S)>
    where
        F: FnOnce(Arc<dyn WindowTarget>) -> Fut,
        Fut: std::future::Future<Output = EngineResult<(Arc<dyn WindowTarget>, S)>>,
    {
        crate::surface_lease::probe_target(&target)?;
        build(target).await
    }

    /// Derive the surface-dependent half of a [`wgpu::SurfaceConfiguration`]
    /// from a freshly created surface.
    ///
    /// Returns the config — with `width`/`height` threaded in from the
    /// caller, since only the caller knows which size is authoritative — and
    /// whether the surface supports `COPY_SRC` (which both picks `usage` here
    /// and is mirrored on `Renderer` for mid-frame texture copies).
    ///
    /// There are exactly two callers and they must agree: `new`/
    /// `recover`'s [`Self::build_windowed_gpu_stack`], and
    /// [`Renderer::recreate_surface`], which rebuilds a surface against the
    /// same device. That second site is the reason this is a helper rather
    /// than a block inside the builder: a recreated surface can report
    /// different capabilities, and `Surface::configure` panics on a stale
    /// `format`/`color_space`/`present_mode`/`alpha_mode` (see its own
    /// `# Panics` list), so every one of those is re-derived here rather than
    /// carried over. Re-deriving is necessary but not sufficient for
    /// `format`, the one field of the four with consumers inside
    /// [`Renderer`]: the pipelines and the offscreen pool bake the format
    /// they are built with, so [`Renderer::recreate_surface`] rebuilds both
    /// when this derivation returns a different one — see that method's own
    /// doc for why leaving them would blank the window rather than fail
    /// loudly. Keeping the derivation in one function is also what
    /// keeps `desired_maximum_frame_latency`'s long comment below true, since
    /// a second construction site would be a second place for that literal to
    /// drift.
    fn derive_surface_config(
        surface: &wgpu::Surface<'_>,
        adapter: &wgpu::Adapter,
        width: u32,
        height: u32,
    ) -> EngineResult<(wgpu::SurfaceConfiguration, bool)> {
        let surface_caps = surface.get_capabilities(adapter);
        let (surface_format, color_space) = Self::select_surface_format(&surface_caps)?;

        let supports_copy_src = surface_caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if !supports_copy_src {
            tracing::warn!(
                "Surface does not support COPY_SRC; backdrop blur will use fallback path"
            );
        }

        let surface_usage = if supports_copy_src {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };

        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
            format: surface_format,
            color_space,
            width,
            height,
            present_mode: Self::select_present_mode(&surface_caps),
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            // 2 (wgpu's own default), raised from 1 on 2026-09-22 on a
            // measurement that settled a judgement call the other way.
            //
            // The literal sat at 1 for a live-resize argument: the tightest
            // pool means the displayed frame tracks the window edge as closely
            // as possible during a drag. The in-process half of that argument
            // was refuted first (`cargo xtask device macos-resize-jitter`: the acquired
            // texture never diverges from the configured size at either
            // setting, four runs — `render_scene` acquires and presents
            // inside one call and `resize` reconfigures before it, so no
            // drawable is alive across a `Surface::configure`), leaving only
            // an unmeasurable compositor-side lag of ~one display period after
            // a resize as the reason to stay at 1. ADR-0058's AppKit
            // subsection had already measured the cost of 1 on the tail —
            // 3 % of frames stalling a full period in `get_current_texture()`
            // — and judged it tolerable.
            //
            // It is not tolerable once the frame does real work. With two
            // drawables (`maximum_frame_latency + 1`) the acquire for frame
            // N+1 waits until frame N's drawable is released by scanout, so
            // a frame whose own work is longer than what is left of the
            // period after that release misses the next vsync EVERY time, not
            // on a 3 % tail: `examples/workload_probe.rs` (a Scaffold with a
            // 2,000-row ListView and a Material TextField, 900×700 logical
            // on a 100 Hz panel) presented at a rock-steady 20.0 ms p50 —
            // exactly two periods, 50 fps — through both its scrolling and
            // its typing phases, while `flui-platform`'s bare frame pump on
            // the same display ran 100 fps. At 2 the same probe ran 10.0 ms
            // p50 (p99 12.3 ms scrolling, 10.05 ms typing). Half the frame
            // rate of every non-trivial app is a measured cost; a possible
            // one-period edge lag during a live drag is a conjecture nothing
            // in-process can observe. The pool is widened on that basis, and
            // `cargo xtask device macos-workload` is the regression gate: its scroll and
            // type p99 budgets are stated in display periods. The
            // measurement is AppKit/Metal on one 100 Hz panel; the literal
            // is global to every wgpu backend, where 2 is wgpu's own default
            // and the other backends are unmeasured either way.
            //
            // Pinned independently of the clock-side produce-capacity threshold
            // (`flui_scheduler::FrameClock::set_max_in_flight`, issue #556): that
            // number is never threaded into this field, and this field is never
            // derived from it.
            // Re-coupling the two is a separate decision that needs its own
            // evidence, not something to slip in by widening this literal. Two
            // implementer notes worth having in one place: (a)
            // wgpu ignores `desired_maximum_frame_latency` entirely on the GL
            // backend (live here — `Backends::GL` is selectable via the `gles`
            // feature), so on GL the clock-side in-flight counter is the ONLY
            // in-flight bound that exists; (b) this field only takes effect at
            // `Surface::configure` — every reconfigure must resupply it. `resize`
            // and `reconfigure_surface` below both mutate and re-`configure` THIS
            // SAME `SurfaceConfiguration` value (so it never needs resupplying —
            // it was never removed), and `recover` and `recreate_surface`
            // rebuild through this exact function again rather than a second
            // constructor — the first the whole stack, the second the surface
            // plus whatever bakes its format — so the literal is written in
            // exactly one place in the source, not scattered across call sites
            // that could drift out of sync.
            desired_maximum_frame_latency: 2,
        };

        Ok((config, supports_copy_src))
    }

    /// Build the two objects that bake a surface `format`: the painter (whose
    /// `PipelineSet` and glyph atlas are constructed against it) and the
    /// offscreen pool (whose textures are sized from it).
    ///
    /// One construction site on purpose, for the same reason
    /// [`Self::derive_surface_config`] is one: the pair is built at
    /// [`Self::build_windowed_gpu_stack`] (`new` and `recover`) and rebuilt
    /// by [`Renderer::recreate_surface`] when the re-derived format differs
    /// from the one the painter holds, and a format consumer added to only
    /// one of those two sites would be the exact blank-window defect the
    /// rebuild exists to remove. A third consumer belongs in this function,
    /// where both callers pick it up together.
    fn build_format_consumers(
        domain: Arc<crate::device_domain::DeviceDomain>,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> (
        crate::painter::WgpuPainter,
        crate::offscreen::OffscreenRenderer,
    ) {
        let painter = crate::painter::WgpuPainter::with_domain(Arc::clone(&domain), format, size);
        let offscreen = crate::offscreen::OffscreenRenderer::with_domain(domain);
        (painter, offscreen)
    }

    /// Build the full windowed GPU stack from an owned [`WindowTarget`].
    ///
    /// Factored out of `new` so `recover` can rebuild the SAME stack shape
    /// against the retained target. Called once at construction and again
    /// on device loss; the caller is responsible for probing the target
    /// first (see `surface_lease::probe_target`) — this function does not
    /// probe on its own.
    ///
    /// # Errors
    ///
    /// Adapter or device creation can fail (e.g. driver still resetting after
    /// a TDR). Returns the underlying [`EngineError`]; the caller may retry on
    /// the next frame.
    async fn build_windowed_gpu_stack(
        target: &Arc<dyn WindowTarget>,
        width: u32,
        height: u32,
    ) -> EngineResult<WindowedGpuStack> {
        let backends = Self::select_backend();
        tracing::info!("Creating wgpu instance with backends: {:?}", backends);

        // DX12 presentation system: route through DirectComposition
        // (`CreateSwapChainForComposition` + an auto-created `IDCompositionVisual`)
        // instead of the default `CreateSwapChainForHwnd` redirection-bitmap path.
        //
        // The HWND redirection bitmap + flip-model swapchain have no synchronization
        // between the swapchain flip and the window-rect change during a live resize,
        // so DWM stretches the in-flight back buffer to the new rect — the root cause
        // of resize "wobble" (confirmed: not fixable via present_mode / frame_latency /
        // DwmFlush / WM_SIZE timing — see winit#786, wgpu#2869, and the hardcoded
        // `DXGI_SCALING_STRETCH` in wgpu-hal dx12). Compositing through a DComp visual
        // lets DWM own the transform, which removes the stretch and gives smooth resize.
        //
        // Opt out with `FLUI_DX12_NO_DCOMP=1` (RenderDoc cannot capture a composition
        // swapchain, so GPU-debugging the present path needs the plain HWND path).
        let dx12_options = if std::env::var_os("FLUI_DX12_NO_DCOMP").is_some() {
            tracing::debug!("DX12 DComp presentation disabled via FLUI_DX12_NO_DCOMP");
            wgpu::Dx12BackendOptions::default()
        } else {
            wgpu::Dx12BackendOptions {
                presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
                ..Default::default()
            }
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            backend_options: wgpu::BackendOptions {
                dx12: dx12_options,
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // Create the surface through wgpu's SAFE owned-target path (issue
        // #1043): `Arc<dyn WindowTarget>` satisfies wgpu's
        // `DisplayAndWindowHandle + 'static` bound through raw-window-handle
        // 0.6.2's `Arc<H: ?Sized>` blanket impls of `HasWindowHandle`/
        // `HasDisplayHandle` plus wgpu's own `impl<T: DisplayAndWindowHandle>
        // From<T> for SurfaceTarget`. wgpu queries `target.window_handle()`/
        // `display_handle()` itself here — this is the "wgpu 29 multi-monitor
        // fix" note's successor: that workaround extracted raw handles by
        // hand to route the display handle around a lifetime issue; the safe
        // path lets wgpu do that query on the retained `Arc` instead.
        //
        // No `unsafe` block: the old unsafe raw-handle surface-creation call
        // and the newtype that narrowed its two handle fields are both gone
        // from this crate, and its `deny(unsafe_code)` keeps them out.
        let surface: wgpu::Surface<'static> = instance
            .create_surface(Arc::clone(target))
            .map_err(EngineError::surface_creation)?;

        let adapter = instance
            .request_adapter(&crate::adapter::trusted_adapter_options(
                wgpu::PowerPreference::HighPerformance,
                Some(&surface),
            ))
            .await
            .map_err(EngineError::adapter_request)?;

        let capabilities = GpuCapabilities::detect(&adapter);
        tracing::info!(
            "Selected GPU: {} ({}), LayerDispatcher: {:?}",
            capabilities.adapter_name,
            capabilities.vendor,
            capabilities.backend
        );

        let (device, queue) =
            crate::adapter::request_flui_device(&adapter, &capabilities, "FLUI GPU Device").await?;

        let device_lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Self::install_device_diagnostics(&device, Arc::clone(&device_lost));

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let (config, supports_copy_src) =
            Self::derive_surface_config(&surface, &adapter, width, height)?;
        surface.configure(&device, &config);

        let (painter, offscreen) = Self::build_format_consumers(
            crate::device_domain::DeviceDomain::new(Arc::clone(&device), Arc::clone(&queue)),
            config.format,
            (config.width, config.height),
        );

        // Create the GPU profiler if the feature is enabled AND the adapter
        // exposes TIMESTAMP_QUERY. A creation failure is non-fatal — profiling
        // is strictly additive and must never abort initialization.
        #[cfg(feature = "gpu-profiler")]
        let gpu_profiler = if capabilities.supports_timestamp_queries() {
            match crate::profiler::GpuFrameProfiler::new(&device) {
                Ok(profiler) => {
                    tracing::info!("GPU profiler enabled (TIMESTAMP_QUERY available)");
                    Some(profiler)
                }
                Err(err) => {
                    tracing::warn!(
                        error = ?err,
                        "GPU profiler creation failed; profiling disabled for this session"
                    );
                    None
                }
            }
        } else {
            tracing::debug!(
                "TIMESTAMP_QUERY not available on this adapter; GPU profiling disabled"
            );
            None
        };

        Ok(WindowedGpuStack {
            instance,
            adapter,
            device,
            queue,
            surface,
            config,
            capabilities,
            painter,
            offscreen,
            supports_copy_src,
            device_lost,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler,
        })
    }

    /// Returns `true` if the GPU device has been lost.
    ///
    /// After a TDR, driver crash, or GPU hardware failure the device-lost
    /// callback fires and sets this flag. The caller (runner frame loop)
    /// should call [`recover()`](Self::recover) to rebuild the GPU context.
    #[must_use]
    pub fn is_device_lost(&self) -> bool {
        // `Relaxed`: see the store in `install_device_diagnostics` — the flag
        // carries no data, so there is nothing for an acquire to pair with.
        let lost = self.device_lost.load(std::sync::atomic::Ordering::Relaxed);
        if lost {
            self.painter.domain().mark_lost();
        }
        lost || self.painter.domain().is_lost()
    }

    /// Whether the intermediate-texture present path is active for this frame.
    ///
    /// `true` when the swapchain surface lacks `COPY_SRC` (real adapter
    /// limitation) OR when the test flag `force_intermediate_for_testing` is
    /// set.  In both cases every frame renders into the retained target and
    /// is blitted onto the swapchain at the end of the frame (a full frame as
    /// `FramePlan::RetainedFull`, never `Direct`).
    ///
    /// When `false` (the common path on COPY_SRC-capable adapters) the frame
    /// renders directly into the swapchain surface — no allocation, no blit.
    #[must_use]
    fn uses_intermediate_texture(&self) -> bool {
        if !self.supports_copy_src {
            return true;
        }
        #[cfg(test)]
        if self.force_intermediate {
            return true;
        }
        false
    }

    /// Force the intermediate-texture present path on for this renderer
    /// instance, regardless of the adapter's COPY_SRC support.
    ///
    /// Used by C2 (forced-intermediate GPU correctness) and C3 (byte-identity)
    /// tests to exercise the intermediate path on COPY_SRC-capable hardware.
    #[cfg(test)]
    #[expect(
        dead_code,
        reason = "called from live DX12 GPU tests run by the user, not from automated unit tests"
    )]
    pub(crate) fn force_intermediate_for_testing(&mut self) {
        self.force_intermediate = true;
    }

    /// Rebuild the GPU device and surface after a device-lost event.
    ///
    /// Re-probes the SAME retained [`WindowTarget`] the renderer was built
    /// from, then — only if that probe succeeds — rebuilds the entire GPU
    /// stack (instance → adapter → device → surface → painter → offscreen)
    /// and swaps the new pieces into `self`. The recovered surface is
    /// configured at the **current** surface size from `self.config`, so
    /// the window keeps its dimensions without a separate resize call.
    ///
    /// The held surface is released before the rebuild (the `Released`
    /// token is what the rebuild is built from), for the
    /// one-surface-per-window rule `recreate_surface`
    /// documents at length: the rebuild creates a fresh `VkSurfaceKHR`, and
    /// on Android a second one for the same live `ANativeWindow` is refused
    /// by an `expect` inside `wgpu-hal` rather than surfaced as an error. A
    /// failed rebuild therefore leaves the presentation released; the next
    /// attempt re-asks, and [`Renderer::recreate_surface`] restores it just
    /// as it does after an owner-requested release.
    ///
    /// On success the device-lost flag is cleared (the fresh device starts
    /// healthy). On failure the underlying [`EngineError`] is returned — the
    /// driver may still be resetting; the runner should retry on the next frame.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::SurfaceTargetUnavailable`] — before starting
    /// any GPU work — if the window owner reports the native target is gone
    /// or suspended (a destroyed window, a suspended Android surface). The
    /// caller (`flui-app`'s device-recovery loop) should treat this as
    /// transient and retry once the owner reports the target live again.
    ///
    /// Returns [`EngineError::AdapterRequest`] or [`EngineError::DeviceCreation`]
    /// when the driver is still resetting or the adapter is no longer available.
    /// Returns [`EngineError::SurfaceCreation`] if wgpu refuses to build a
    /// surface from the still-live target for some other reason.
    #[tracing::instrument(level = "warn", skip(self))]
    pub async fn recover(&mut self) -> EngineResult<()> {
        // Resolved before any `.await` so the borrow of `gpu_stack_origin`
        // never needs to live across one; `target` is an owned `Arc` clone,
        // not a borrow of `self`.
        // Released before any `.await` so the borrow of `gpu_stack_origin`
        // never lives across one; the token carries an owned `Arc` of the
        // target, not a borrow of `self`. Releasing first is what a window
        // that allows one surface at a time requires (see
        // `SurfaceLease::probe`); a rebuild that fails leaves the presentation
        // released, which the next recovery re-asks.
        self.lease.probe()?;
        let released = self.lease.release();

        // Capture current dimensions before rebuild so the recovered
        // surface matches the live window size instead of defaulting to
        // 800×600.
        let (width, height) = (self.config.width, self.config.height);

        let stack = Self::build_windowed_gpu_stack(released.target(), width, height).await?;

        self.instance = stack.instance;
        self.adapter = stack.adapter;
        self.device = stack.device;
        self.queue = stack.queue;
        self.config = stack.config;
        self.capabilities = stack.capabilities;
        self.painter = stack.painter;
        self.offscreen = stack.offscreen;
        self.supports_copy_src = stack.supports_copy_src;
        // Replace with a fresh flag — the new device starts healthy.
        self.device_lost = stack.device_lost;
        // Reset profiler with the fresh device — timestamp queries from the
        // lost device are invalid and must not be carried over.
        #[cfg(feature = "gpu-profiler")]
        {
            self.gpu_profiler = stack.gpu_profiler;
        }
        self.lease.replace_surface(released, stack.surface);
        // Force a full repaint so the first recovered frame is complete.
        self.frame.mark_full_repaint();
        // The retained target belongs to the lost device.
        self.frame.release_target();

        tracing::info!(
            width = self.config.width,
            height = self.config.height,
            "GPU device recovered successfully"
        );

        Ok(())
    }

    /// Select appropriate backend for the current platform
    ///
    /// One definition for every construction path in this module, so
    /// backend selection is written in exactly one place rather than
    /// re-derived per call site.
    pub(super) fn select_backend() -> wgpu::Backends {
        #[cfg(target_os = "macos")]
        {
            tracing::debug!("Platform: macOS, selecting Metal backend");
            wgpu::Backends::METAL
        }

        #[cfg(target_os = "ios")]
        {
            tracing::debug!("Platform: iOS, selecting Metal backend");
            wgpu::Backends::METAL
        }

        #[cfg(target_os = "windows")]
        {
            tracing::debug!("Platform: Windows, selecting DirectX 12 backend");
            wgpu::Backends::DX12
        }

        #[cfg(target_os = "linux")]
        {
            tracing::debug!("Platform: Linux, selecting Vulkan backend");
            wgpu::Backends::VULKAN
        }

        #[cfg(target_os = "android")]
        {
            tracing::debug!("Platform: Android, selecting Vulkan backend");
            wgpu::Backends::VULKAN
        }

        #[cfg(target_arch = "wasm32")]
        {
            tracing::debug!("Platform: Web, selecting WebGPU backend (with WebGL fallback)");
            wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "windows",
            target_os = "linux",
            target_os = "android",
            target_arch = "wasm32"
        )))]
        {
            tracing::warn!("Unknown platform, using all available backends");
            wgpu::Backends::all()
        }
    }

    /// Installs diagnostic callbacks on a freshly created device.
    ///
    /// wgpu's default behaviour translates an uncaptured error (validation
    /// bug, out-of-memory, internal GPU failure) into a thread panic, and a
    /// lost device surfaces only as repeated surface failures with no cause.
    /// Both callbacks route the fault through `tracing` so it is logged and
    /// diagnosable instead of aborting the process or spinning the render
    /// loop blind.
    ///
    /// Called exactly once per device this crate constructs — from
    /// `build_windowed_gpu_stack` (and so from `new` and `recover`). The
    /// "exactly one install per device" invariant ADR-0045 decision 2 names
    /// (a second install is last-writer-wins and orphans the first flag)
    /// holds because every construction path owns its device outright; the
    /// windowed shared-services path that would need an explicit guard
    /// arrives with its own `ReplaceServices` mechanism.
    fn install_device_diagnostics(
        device: &wgpu::Device,
        device_lost_flag: Arc<std::sync::atomic::AtomicBool>,
    ) {
        device.on_uncaptured_error(Arc::new(|error: wgpu::Error| {
            tracing::error!(
                %error,
                "wgpu uncaptured error (validation / out-of-memory / internal)",
            );
        }));
        device.set_device_lost_callback(move |reason, message| {
            tracing::error!(
                ?reason,
                %message,
                "wgpu device lost — the GPU context is gone; the renderer will \
                 attempt device recreation on the next frame",
            );
            // `Relaxed`, and paired with the `Relaxed` load in
            // `is_device_lost`: this flag carries no data. It answers one
            // self-contained question ("did the driver report a loss?"), and
            // every fact a reader acts on after seeing `true` comes from
            // wgpu's own validation, not from anything published through
            // this atomic. A release/acquire pair here would synchronize
            // nothing.
            device_lost_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }

    /// Required GPU features based on capabilities and adapter support.
    ///
    /// Only requests optional features when the adapter actually exposes them,
    /// so device creation never regresses on GPUs that lack them.
    ///
    /// One definition for every construction path, so all of them request
    /// the same feature set.
    pub(super) fn required_features(capabilities: &GpuCapabilities) -> wgpu::Features {
        let mut features = wgpu::Features::empty();

        // Native format extensions are optional, and unavailable on WebGPU.
        features |=
            capabilities.features & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

        // Immediates (formerly push constants): only request if adapter supports them.
        // Some mobile GPUs (especially older Android devices) don't support this.
        if capabilities.supports_push_constants() {
            features |= wgpu::Features::IMMEDIATES;
        }

        // Timestamp queries for GPU profiling: only request when the adapter exposes
        // BOTH features AND the gpu-profiler cargo feature is enabled. Device creation
        // must never fail because of an optional profiling feature the adapter lacks.
        // `supports_timestamp_queries` is already true only when both are present
        // (see `GpuCapabilities::detect`), so requesting both here is safe.
        #[cfg(feature = "gpu-profiler")]
        if capabilities.supports_timestamp_queries() {
            features |=
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        }

        // A second blend source, so an anti-aliased clip can feather a
        // destination-destructive blend instead of applying it at full strength
        // across the whole fringe. Requested only where the adapter exposes it;
        // `PipelineCache` falls back to the folded shader otherwise, and the
        // destination-sensitive coverage uses portable isolation there. See
        // `GpuCapabilities::supports_dual_source_blending`.
        if capabilities.supports_dual_source_blending() {
            features |= wgpu::Features::DUAL_SOURCE_BLENDING;
        }

        features
    }

    /// Required GPU limits based on capabilities and adapter support
    ///
    /// Same one-definition rationale as [`Self::required_features`]. Every
    /// field is clamped down to what the adapter actually advertises.
    /// `wgpu::Limits::default()` is the *desktop* baseline: it asks for
    /// `max_inter_stage_shader_variables: 16`, which the iOS simulator's Metal
    /// adapter caps at 15, so a plain `..default()` makes device creation fail
    /// with `LimitsExceeded` on that platform. Requesting a limit above the
    /// adapter's own can never succeed — the adapter's `Limits` are what
    /// `request_device` validates against — so the engine takes the adapter's
    /// value wherever the default exceeds it. The `max_texture_dimension_2d`
    /// clamp below has always worked this way; the rest of the struct simply
    /// had not met an adapter small enough to need it.
    pub(super) fn required_limits(
        capabilities: &GpuCapabilities,
        adapter_limits: &wgpu::Limits,
    ) -> wgpu::Limits {
        let mut limits = wgpu::Limits {
            max_texture_dimension_2d: capabilities.limits.max_texture_dimension_2d.min(16384),
            ..wgpu::Limits::default()
        };

        // Immediate data size — only set if adapter supports immediates
        if capabilities.supports_push_constants() {
            limits.max_immediate_size = 128;
        }

        // Never ask for more than the adapter offers. `min` per field rather
        // than a whole-struct `min` so a future field added to `Limits` is
        // covered by the default `..default()` and this line keeps the ones
        // that matter honest.
        limits.max_inter_stage_shader_variables = limits
            .max_inter_stage_shader_variables
            .min(adapter_limits.max_inter_stage_shader_variables);

        limits
    }

    /// Select a supported presentation pair for the shaders' encoded sRGB output.
    fn select_surface_format(
        surface_caps: &wgpu::SurfaceCapabilities,
    ) -> EngineResult<(wgpu::TextureFormat, wgpu::SurfaceColorSpace)> {
        // Color::to_f32_array and the shaders preserve encoded values. Plain
        // UNorm stores those bytes without another transfer function; explicit
        // Srgb tells the compositor how to interpret them. An FP16/Auto surface
        // can instead select ExtendedSrgbLinear and brighten the same values.
        // Keep the existing encoded-space blending contract until the entire
        // pipeline deliberately adopts linear/HDR color management.
        for format in [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ] {
            if surface_caps
                .color_spaces(format)
                .contains(wgpu::SurfaceColorSpaces::SRGB)
            {
                tracing::debug!(?format, color_space = ?wgpu::SurfaceColorSpace::Srgb, "Selected surface color configuration");
                return Ok((format, wgpu::SurfaceColorSpace::Srgb));
            }
        }
        Err(EngineError::UnsupportedSurfaceColorConfiguration {
            supported: surface_caps.format_capabilities.clone(),
        })
    }

    /// Select present mode based on capabilities.
    ///
    /// Fifo is the default. On the Vulkan/Wayland path `render_scene`'s
    /// blocking `get_current_texture()`/`present()` pair against Fifo is the
    /// steady-state pacing mechanism: every PRESENTED frame blocks at display
    /// cadence, which is what lets `flui-app`'s runner drop its fixed
    /// frame-budget sleep in favor of a real vsync block (see the frame-pacing
    /// ADR). That is a per-backend fact, not a property of Fifo: measured
    /// 2026-09-17 on the native AppKit backend, `queue.present()` returns in
    /// ~42 µs and the display cadence comes from AppKit's display-pass
    /// scheduling instead — same vsync-locked period, different mechanism
    /// (ADR-0058's per-backend facts; Windows' native backend is unmeasured).
    /// Mailbox (triple
    /// buffering, uncapped present, lower latency) is a documented future
    /// opt-in for latency-sensitive apps that accept trading pacing for
    /// responsiveness — it is not the default because pairing an uncapped
    /// present mode with a wake-driven redraw loop reproduces the exact
    /// busy-spin (~30 000 fps, observed) this pacing model exists to avoid.
    ///
    /// NB: Fifo does not cure live-resize wobble either — it blocks on
    /// vsync and ghosts a stale frame during the modal resize loop, same as
    /// Mailbox stretching the in-flight frame. The wobble is inherent
    /// flip-model DWM compositing; the only real fix is DXGI_SCALING_NONE,
    /// which wgpu 30 still does not expose.
    fn select_present_mode(surface_caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
        debug_assert!(
            surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Fifo),
            "BUG: wgpu guarantees Fifo is always a supported present mode"
        );
        wgpu::PresentMode::Fifo
    }

    /// Install the hook run immediately before every present — see
    /// [`crate::RasterBackend::set_pre_present_hook`].
    pub fn set_pre_present_hook(&mut self, hook: Option<crate::raster::PrePresentHook>) {
        self.pre_present_hook = hook;
    }

    /// Resize the surface
    ///
    /// While the surface is released the size still lands in the configuration
    /// (and on the painter), because a window resize can arrive during a
    /// released span and the size the recreate builds at must be the current
    /// one. Only the `Surface::configure` call is skipped: there is no surface
    /// to configure, and `Surface::configure` panics on a zero dimension, so a
    /// zero-size resize is refused outright either way.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        let config = &mut self.config;
        config.width = width;
        config.height = height;
        if let Some(surface) = self.lease.surface() {
            surface.configure(&self.device, config);
        }

        self.painter.resize(width, height);

        self.frame.surface_changed();

        tracing::debug!("Surface resized to {}x{}", width, height);
    }

    /// Get GPU capabilities
    #[must_use]
    pub fn capabilities(&self) -> &GpuCapabilities {
        &self.capabilities
    }

    /// Mark a screen region as dirty (needs repaint).
    pub fn mark_dirty(&mut self, rect: flui_foundation::geometry::Rect<f64>) {
        self.frame.mark_dirty(rect);
    }

    /// Mark the entire screen as needing repaint.
    pub fn mark_full_repaint(&mut self) {
        self.frame.mark_full_repaint();
    }

    /// Check if the renderer has pending damage.
    #[must_use]
    pub fn has_damage(&self) -> bool {
        self.frame.has_damage()
    }

    /// The latest completed GPU frame profile, or `None` when the `gpu-profiler`
    /// feature is off, the adapter lacks `TIMESTAMP_QUERY`, or fewer than
    /// `PENDING_FRAME_BUFFER_DEPTH` frames have been rendered.
    ///
    /// Implements [`Diagnosticable`](flui_foundation::Diagnosticable): call
    /// `profile.to_diagnostics_node()` to get a human-readable property tree.
    #[must_use]
    pub fn latest_gpu_frame_profile(&self) -> Option<&crate::profiler::GpuFrameProfile> {
        #[cfg(feature = "gpu-profiler")]
        {
            self.gpu_profiler
                .as_ref()
                .and_then(|p| p.latest_completed_frame())
        }
        #[cfg(not(feature = "gpu-profiler"))]
        {
            None
        }
    }

    /// The configured surface size as `(width, height)`.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Reconfigure the surface after a lost, outdated, or validation result.
    ///
    /// This is called automatically by `render_scene()` when
    /// `CurrentSurfaceTexture::Outdated`, `CurrentSurfaceTexture::Lost`, or
    /// `CurrentSurfaceTexture::Validation` is encountered, but can also be
    /// called manually if needed.
    ///
    /// While the surface is released this is a no-op: a released surface has
    /// nothing to reconfigure and its owner configures the fresh one when it
    /// recreates.
    pub fn reconfigure_surface(&mut self) {
        let Some(surface) = self.lease.surface() else {
            tracing::trace!("Surface released; reconfigure is a no-op until it is recreated");
            return;
        };
        surface.configure(&self.device, &self.config);
        self.frame.surface_changed();
        tracing::info!(
            "Surface reconfigured ({}x{})",
            self.config.width,
            self.config.height
        );
    }

    /// Drop the held surface, keeping the instance, adapter, device, queue and
    /// the window target.
    ///
    /// Call this at the last moment the native handle behind the surface is
    /// still valid. On Android that moment is inside the platform's
    /// `TerminateWindow` callback, which is delivered before `poll_events`
    /// returns and before `NativeWindow` is cleared — a surface that outlives
    /// its native handle is the use-after-free class the lease's field order
    /// exists to prevent, and there is no later event that still has a valid
    /// handle to drop against.
    ///
    /// # Cost on the calling thread
    ///
    /// Dropping a *configured* `wgpu::Surface` releases its swapchain, and on
    /// Vulkan that path calls `vkDeviceWaitIdle` before destroying anything —
    /// wgpu-hal's own comment says there is no portable way to wait only for
    /// presentation work. On the Android release path that wait therefore
    /// lands on the paused UI thread. It is accepted, because the alternative
    /// is dropping the surface after the handle is gone; what is not
    /// negotiable is that nothing else happens here: no probe, no submit, no
    /// wait on any lock beyond this renderer's own.
    ///
    /// Idempotent, and a no-op for a renderer that owns no window — there the
    /// requested post-state ("no surface is held") is already true, and this
    /// runs on a lifetime-critical path where a branch is preferable to an
    /// error the caller would have to ignore. The `SurfaceLease`'s `Arc` of
    /// the target is retained, so [`Renderer::recreate_surface`] needs no new
    /// ownership.
    pub fn release_surface(&mut self) {
        let lease = &mut self.lease;
        if !lease.has_surface() {
            return;
        }
        // A bare release: the owner asked for it and will ask for a recreate
        // later, which mints its own token.
        let _released = lease.release();
        // A released presentation is suspended; its frame memory goes too.
        self.frame.release_target();
        // Distinct from `SurfaceLease`'s own `surface_released` event, which
        // fires when the lease is dropped: this one says the owner asked for
        // the release, and a reader of a log needs to tell those apart.
        tracing::debug!(
            target: "flui.gpu",
            event = "surface_released_by_owner",
            "surface released at its owner's request; target and GPU stack retained"
        );
    }

    /// Build a fresh surface for a windowed renderer whose surface was
    /// released, keeping the instance, adapter, device and queue.
    ///
    /// This is the resume half of [`Renderer::release_surface`], and it is
    /// **stateless**: it always attempts to build a surface, it never consults
    /// whether one is already held. A caller cannot know whether the matching
    /// release arrived — a signal can be lost — and "skip if a surface is
    /// present" would then preserve a surface built from a handle that is
    /// already gone for the rest of the process's life. A held surface is
    /// dropped at the top of this call, before the replacement is created.
    ///
    /// "Attempts", not "builds", because the platform gets a say. The Vulkan
    /// specification allows only one `VkSurfaceKHR` per `ANativeWindow` at a
    /// time and refuses a second at `vkCreateAndroidSurfaceKHR` with
    /// `VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`. Dropping the held surface first is
    /// what keeps that rule out of this method's way: a `true` that finds a
    /// surface still bound to the same, still-connected window — a missed
    /// `false`, or two `true`s with no `false` between them (ADR-0063
    /// decision 6 books both, and marks whether the second ordering occurs at
    /// all as unverified) — releases it and then creates cleanly, where
    /// build-first would be refused. That refusal is not a recoverable error
    /// on this stack: `wgpu-hal` 30.0.1's `create_surface_android` `expect`s
    /// the create result ("AndroidSurface failed", `src/vulkan/instance.rs`),
    /// so under that version it aborts the process on the calling thread.
    /// Releasing first is therefore what makes the stateless re-ask above
    /// actually safe to attempt.
    ///
    /// What dropping first gives up is the old surface surviving a failed
    /// create. On the one platform that emits this signal the old surface's
    /// window is either the same one — so the create is refused, and the old
    /// surface is the only one that could be kept — or already dead, in which
    /// case it is useless. On a create failure the presentation is left
    /// released, which is the documented post-state; the next `true` re-asks.
    ///
    /// Only the surface is rebuilt. [`Renderer::recover`] stays the device-loss
    /// path and rebuilds the whole stack; a suspend never sets the device-lost
    /// flag, so routing a resume through it would discard a perfectly good
    /// device. The new surface is created from the lease's retained
    /// [`WindowTarget`], so it binds to whatever native handle is current now,
    /// which after a real activity recreation is a different one from the
    /// released surface's.
    ///
    /// Every capability-dependent configuration field (`usage`, `format`,
    /// `present_mode`, `alpha_mode`) is re-derived from the fresh surface,
    /// because a recreated surface on a new window can report different
    /// capabilities and `Surface::configure` panics on a stale one.
    /// `format` is the one of the four with consumers other than
    /// `self.config`: `PipelineSet`'s nine pipelines and the offscreen
    /// texture pool are both built from a format and bake it, so a fresh
    /// format rebuilds `painter` and `offscreen` here, in the body below.
    /// `width`/`height` are deliberately **not** re-derived: a resize may
    /// have arrived while the surface was released, and [`Renderer::resize`]
    /// keeps updating them for exactly this reason.
    ///
    /// A successful recreation marks a full repaint — a fresh surface has
    /// undefined contents while the damage tracker is incremental, so damage
    /// carried over from before the release would repaint one region and
    /// leave garbage around it.
    ///
    /// # Errors
    ///
    /// [`EngineError::NotInitialized`] when this renderer owns no window: only
    /// a windowed renderer has a lifecycle that can ask for a recreation, so
    /// reaching here from an offscreen or shared-services renderer is a
    /// program error and stays loud. Otherwise the probe's
    /// [`EngineError::SurfaceTargetUnavailable`] (the target has no handle
    /// right now) or [`EngineError::SurfaceCreation`] propagates, and the
    /// renderer is left released rather than holding a half-built surface.
    pub fn recreate_surface(&mut self) -> EngineResult<()> {
        let lease = &mut self.lease;
        let config = &self.config;
        let (width, height) = (config.width, config.height);

        // Probe first: "the target has no handle right now" is a recoverable,
        // typed condition from here, and asking wgpu to create a surface
        // against a dead handle would collapse it into a `SurfaceCreation`
        // failure (see `surface_lease::probe_target`'s doc).
        lease.probe()?;

        // Drop the held surface BEFORE creating the replacement.
        //
        // The two orders trade different losses, and the platform decides
        // which one is affordable. Build-first keeps a still-valid surface
        // when the create fails — but on Android the old surface's window is
        // either the same one (so the create is refused, see below) or
        // already dead (so the surface is useless). Drop-first costs only
        // that, and buys the one-surface-per-window rule:
        // `vkCreateAndroidSurfaceKHR` refuses a second surface for a live
        // `ANativeWindow` with `VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`, and
        // `wgpu-hal` 30.0.1's `create_surface_android` `expect`s that result
        // ("AndroidSurface failed", `src/vulkan/instance.rs`), which aborts
        // the process on the callback thread. Reaching that needs a `true`
        // over a surface still bound to the same live window — a missed
        // `false`, or two `true`s with no `false` between them — which is
        // outside the ordinary cycle but is exactly the class this method is
        // stateless for: it must be able to re-ask unconditionally, because a
        // missed signal is not detectable from here. Dropping first makes the
        // re-ask always safe to attempt.
        //
        // A drop of a *configured* surface runs `wgpu-core`'s `unconfigure`
        // into `vkDeviceWaitIdle` (see `release_surface`'s doc for what that
        // wait costs and why it is accepted); a create that then fails leaves
        // the lease released, which is the documented post-state. The caller
        // owns the log line for that failure — see `surface_lifecycle.rs`'s
        // `a_failed_recreation_carries_the_error_and_leaves_the_presentation_released`.
        let released = lease.release();

        let surface = self
            .instance
            .create_surface(Arc::clone(released.target()))
            .map_err(EngineError::surface_creation)?;
        let (fresh_config, supports_copy_src) =
            Self::derive_surface_config(&surface, &self.adapter, width, height)?;

        // Whether the fresh surface moved the format away from the one the
        // pipelines and the offscreen pool were built with. Read off the
        // painter rather than off the old `self.config`, because the painter
        // is the consumer that bakes it: the two agree today (nothing but the
        // two commit sites below ever writes `self.config.format`), and
        // binding the condition to the thing that actually has to change is
        // what keeps them agreeing.
        let pipelines_format = Some(self.painter.surface_format());

        // Build FIRST, commit LAST (the lease's two-step protocol): a
        // configure that fails must leave the lease released rather than
        // holding a surface this call never finished preparing.
        surface.configure(&self.device, &fresh_config);

        if pipelines_format != Some(fresh_config.format) {
            // `format` is baked into every pipeline (`PipelineSet::new` is
            // handed it once) and into the offscreen pool's textures, while
            // the per-frame target format is read from `self.config` — so
            // committing a fresh format without rebuilding these two would
            // leave every pipeline declaring a target format the render
            // attachment does not have. wgpu raises that as a validation
            // error per frame and this backend's `on_uncaptured_error` handler
            // only logs it (it does not set `device_lost`), so nothing here
            // would self-heal: the window stays blank with one `error!` per
            // frame, which is the defect class this whole path exists to
            // remove. `recover` rebuilds both unconditionally; only the
            // format actually moving obliges it here, so a resume that
            // re-derives the format it already had pays nothing for this.
            //
            // What it costs when the format did move: this is the one
            // mid-life path that constructs nine pipelines and a glyph atlas
            // synchronously, on the callback thread, under the caller's held
            // lane — the startup cost, paid again. It neither submits nor
            // waits on the GPU; shader compilation is the whole of it.
            let (painter, offscreen) = Self::build_format_consumers(
                Arc::clone(self.painter.domain()),
                fresh_config.format,
                (width, height),
            );
            self.painter = painter;
            self.offscreen = offscreen;
            tracing::info!(
                target: "flui.gpu",
                event = "surface_format_changed",
                previous = ?pipelines_format,
                current = ?fresh_config.format,
                "recreated surface selected a different format; pipelines and offscreen pool rebuilt"
            );
        }

        lease.replace_surface(released, surface);

        self.config = fresh_config;
        self.supports_copy_src = supports_copy_src;
        self.frame.surface_changed();

        tracing::debug!(
            target: "flui.gpu",
            event = "surface_recreated",
            width,
            height,
            "surface rebuilt against the current native handle; full repaint marked"
        );
        Ok(())
    }

    /// Renders `scene` in full, outside the damage protocol a raster owner
    /// drives, and reports what became of the frame.
    ///
    /// This is the entry point for a caller that holds no damage producer (a
    /// direct-mode app, an example, a hot-reload plugin's scene). The frame
    /// always repaints everything, and because it shows a scene the owner's
    /// producer never saw, the retained target stops describing the screen
    /// and the next frame renders in full too, even one the producer found
    /// unchanged. A frame a [`crate::RasterOwner`] retires goes through
    /// [`RasterBackend::render_scene`](crate::RasterBackend::render_scene)
    /// instead, which renders the damage the owner applied.
    ///
    /// [`PresentDisposition::Presented`] means `present()` ran. Whether that
    /// call paced the frame depends on the backend: under the default Fifo
    /// present mode it blocks until the next vsync on the Vulkan/Wayland
    /// path, while the native AppKit backend returns from it in ~42 µs and
    /// takes its cadence from AppKit's display-pass scheduling instead
    /// (ADR-0058's per-backend facts).
    /// [`PresentDisposition::NotShown`] skips presentation without error and
    /// so carries no vsync signal: this backend owed content it could not put
    /// on screen (the surface reporting `Occluded`, or released by its owner
    /// via [`Renderer::release_surface`]) and the caller should come back for
    /// it rather than counting the frame finished.
    pub fn render_scene(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<PresentDisposition, EngineError> {
        self.frame.begin_unmanaged();
        let result = self.render_frame(scene);
        self.frame.end_unmanaged();
        result
    }

    /// Renders `scene` with the damage applied since the last presented
    /// frame: the raster owner's path (`RasterBackend::render_scene`).
    ///
    /// Traverses the scene's LayerTree depth-first, dispatching each layer's
    /// DisplayList commands through the GPU backend (WgpuPainter). For scenes
    /// containing `BackdropFilterLayer`, painter batches are submitted early
    /// so the target can be copied, blurred, and composited before
    /// continuing. [`PresentDisposition::NoDamage`] means nothing was owed;
    /// otherwise the dispositions are [`Self::render_scene`]'s.
    pub(crate) fn render_frame(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<PresentDisposition, EngineError> {
        // Damage arrives from the raster owner, which applies each frame's
        // `flui_layer::DamageRegion` (the `LayerDiffer`'s comparison of
        // consecutive layer trees, ADR-0087 §3) to the tracker before calling
        // this. Widgets reporting their own bounds does not work, and ADR-0061
        // records why: the objects that always repaint cover the screen.
        self.frame
            .include_backdrop_dependencies(scene, (self.config.width, self.config.height));
        let plan = self.frame.plan(self.uses_intermediate_texture());
        if plan == crate::damage::FramePlan::Skip {
            // Nothing changed — skip this frame entirely; no present, no vsync block.
            tracing::trace!("Skipping frame: no damage");
            return Ok(classify_frame(false, false));
        }

        // Acquire the swapchain texture; returns None when the frame should be
        // skipped (Occluded, or the surface is released), or Err for
        // unrecoverable surface states. All the `None` causes are the same
        // answer to the caller — content was owed and there is nothing to
        // present it into — so they collapse here on purpose, at the one place
        // that knows the frame got past the damage check.
        let Some(output) = self.acquire_surface_texture()? else {
            // Past the damage check, so this frame owed content — see
            // `classify_frame`, which is where that distinction is pinned.
            return Ok(classify_frame(true, false));
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Observability: warn when the swapchain texture diverges from the
        // configured surface size (resize transient → stretched frame).
        self.warn_on_size_mismatch(&output.texture);

        let surface_format = self.config.format;

        // Where this frame renders is `FrameProtocol::run`'s: the swapchain
        // image for a direct frame; otherwise the retained target, whose
        // `COPY_SRC | COPY_DST` also serves backdrop-filter and advanced-blend
        // dst-reads on a surface without `COPY_SRC`, and which reaches the
        // swapchain through one blit of the whole target — the only encoder
        // that writes `&view` on that path. The blit uses Replace/Copy blend
        // (no blend equation), so the surface is pixel-identical to a direct
        // render. A failed content pass is the frame's failure: nothing below
        // presents it.
        let domain = Arc::clone(self.painter.domain());
        #[cfg(feature = "gpu-profiler")]
        let mut profile_frame =
            crate::profiler::ProfileFrame::begin(&mut self.gpu_profiler, &self.device);
        let mut steps = SwapchainFrame {
            device: &self.device,
            painter: &mut self.painter,
            offscreen: &mut self.offscreen,
            scene,
            format: surface_format,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: profile_frame.profiler(),
        };
        self.frame.run(
            plan,
            &domain,
            (self.config.width, self.config.height),
            surface_format,
            (&view, &output.texture),
            &mut steps,
        )?;
        #[cfg(feature = "gpu-profiler")]
        profile_frame.complete();

        // The platform's frame-pacing signal, armed strictly before the
        // present that follows (see `RasterBackend::set_pre_present_hook`).
        // The trace event is the live-smoke harness's ordering oracle: every
        // `present_submitted` must be preceded by one of these.
        if let Some(hook) = self.pre_present_hook.as_mut() {
            hook();
            tracing::trace!(
                target: "flui.gpu",
                event = "pre_present_notified",
                "platform notified before present"
            );
        }
        let present_started = crate::frame_timing::now();
        self.queue.present(output);
        let present_us = present_started.elapsed().as_micros() as u64;

        // Dedicated target so a harness can count REAL per-frame GPU work
        // from the log (`RUST_LOG` filter: `flui.gpu=trace`): this line is
        // reached only after the frame's encoders were submitted to the
        // queue and the swapchain texture presented — the live oracle for
        // hidden-surface gating ("an occluded window issues zero GPU
        // submissions"), which `tools/live-smoke`'s occlusion check counts.
        // The `event` field is the machine-oriented marker that check
        // matches on — keep it stable; the message text is for humans and
        // may be reworded freely.
        tracing::trace!(
            target: "flui.gpu",
            event = "present_submitted",
            present_us,
            "surface frame submitted and presented"
        );

        // Harvest the oldest completed result (if the pipeline has warmed up).
        // Profiling is already closed before the potentially panicking present hook.
        // This is a no-op when
        // `gpu_profiler` is `None`.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_mut() {
            let timestamp_period = self.queue.get_timestamp_period();
            profiler.process_finished_frame(timestamp_period);
        }

        // The damage this frame covered is paid.
        self.frame.presented();

        Ok(classify_frame(true, true))
    }

    /// Acquire the current swapchain texture, handling device-lost and all
    /// `CurrentSurfaceTexture` variants with one reconfigure-and-retry on
    /// Outdated, Lost, or Validation.
    ///
    /// Returns `Ok(None)` when the frame should be skipped rather than
    /// presented — the surface reports itself `Occluded`, or its owner has
    /// released it. Both mean the same thing to `render_scene` (content was
    /// owed and there is nowhere to put it), which classifies them together;
    /// the distinction is only ever useful in a trace, where each cause logs
    /// its own line.
    fn acquire_surface_texture(&mut self) -> Result<Option<wgpu::SurfaceTexture>, EngineError> {
        // Check for device-lost flag (set by the device-lost callback) before
        // attempting to acquire a surface texture. If the device is gone, we
        // cannot proceed with the current device — return an error that the
        // caller can handle by recreating the renderer.
        if self.is_device_lost() {
            tracing::warn!("Device lost detected; returning DeviceLost error");
            return Err(EngineError::DeviceLost);
        }

        // The generic driver gives this concrete wgpu path a CPU-only test seam
        // while preserving static dispatch and zero allocation in the frame path.
        acquire_surface_texture_with(self)
    }

    /// Warn when the acquired swapchain texture dimensions differ from the
    /// configured surface size, indicating a resize transient.
    fn warn_on_size_mismatch(&self, output_texture: &wgpu::Texture) {
        // Observability: the acquired swapchain texture must match the configured
        // surface size, which is the size the geometry was laid out / the viewport
        // uniform was written against. A divergence means a resize landed between
        // `surface.configure` and `get_current_texture` (or the OS handed back a
        // stale backbuffer) — the frame would then be presented stretched, which
        // reads as resize "jitter". Warn (don't spam): only when they actually differ.
        {
            let config = &self.config;
            let attachment = (output_texture.width(), output_texture.height());
            let configured = (config.width, config.height);
            if attachment != configured {
                // A dedicated target, not the module path: this fires at most a
                // handful of times even on a pathological resize storm, so it is
                // easy to miss in a log — but it is the one signal that says the
                // swapchain handed back a stale-size backbuffer. On its own
                // target it can be counted directly (see
                // `examples/resize_jitter_probe.rs`, which fails the run on a
                // non-zero count), while `RUST_LOG=flui.gpu` still selects it by
                // prefix and plain `warn` still selects it outright.
                tracing::warn!(
                    target: "flui.gpu.resize_transient",
                    ?attachment,
                    ?configured,
                    "render_scene: swapchain texture size != configured surface size \
                     (resize transient — frame will present stretched)"
                );
            }
        }
    }

    /// Records one frame's content into `painter`: binds the target, resets the
    /// per-frame state, opens a partial frame when `partial_damage` is set
    /// (`damage::begin_partial`: the scissor and the clear inside it), walks
    /// the scene, and reports whether an advanced shape straddles the damage
    /// edge — in which case the caller forces its next frame full.
    ///
    /// The windowed renderer and the crate's headless retained capture both
    /// record through this, so the partial-frame protocol a readback test
    /// pins is the one the swapchain path runs. Submitting the painter's
    /// batches stays with the caller.
    pub(crate) fn record_frame_content(
        painter: &mut crate::painter::WgpuPainter,
        scene: &flui_layer::Scene,
        partial_damage: Option<flui_foundation::geometry::Rect<f64>>,
    ) -> EngineResult<bool> {
        use crate::layer_dispatcher::LayerDispatcher;

        let mut backend = LayerDispatcher::new(painter);

        // Reset per-frame clip/transform/opacity/layer state so that
        // partial-damage scissors from frame N cannot leak into frame N+1.
        // This must happen BEFORE the damage clip_rect below.
        backend.painter_mut().begin_frame_in_scope();

        // A partial frame scissors every draw to its damage and repaints the
        // background there first (`damage::begin_partial`); `partial_damage`
        // is `None` for a full frame, which needs neither.
        if let Some(damage) = partial_damage {
            crate::damage::begin_partial(backend.painter_mut(), damage);
            tracing::trace!(
                left = damage.left(),
                top = damage.top(),
                width = damage.width(),
                height = damage.height(),
                "Damage scissor applied"
            );
        }

        // Depth-first traversal of layer tree. Backdrop-filter and
        // advanced-blend passes read from `render_texture`, which always has
        // COPY_SRC here: the swapchain image when the surface offers it, the
        // retained target (created with it) otherwise.
        Self::render_layer_recursive(scene.tree(), scene.root(), &mut backend)?;

        // Damage-straddle self-healing: if a partial scissor was applied AND
        // `draw_order` now contains an advanced shape whose `device_bounds`
        // straddle the damage edge, the caller repaints the next frame in full.
        //
        // Why next-frame and not this-frame: `render_layer_recursive` has
        // already populated the draw commands with the scissored geometry; a
        // this-frame re-record would require replaying the entire scene graph.
        // The frame rendered into the retained target, so outside the damage
        // it holds the correct previous frame and the straddling shape's own
        // out-of-damage slice is the only pixel it can disturb, for one frame.
        // A precomputed Scene bit or a re-record is the upgrade path if that
        // transient ever shows.
        let straddled = partial_damage
            .is_some_and(|damage| backend.painter().has_advanced_shape_straddling(damage));
        if straddled {
            tracing::debug!(
                "Advanced shape straddles partial damage; scheduling full repaint next frame"
            );
        }
        // The dispatcher drops here: its Drop calls flush_active_transform(),
        // which balances any deferred lazy-coalescing save left by
        // `with_transform`, before the caller renders the painter's batches.
        drop(backend);
        Ok(straddled)
    }

    /// Recursively render a layer and its children (depth-first, back-to-front /
    /// painter's algorithm).
    ///
    /// Each layer's `render()` pushes state (transforms, clips, opacity),
    /// children are rendered in order, then `cleanup()` pops the state.
    ///
    /// `BackdropFilterLayer` is handled specially at the Renderer level when
    /// the surface supports `COPY_SRC`, enabling mid-frame flush + blur.
    /// `FollowerLayer` is handled specially too — its render-time position
    /// is resolved against the already-fully-built `tree` (which indexes its
    /// leaders) before its children render.
    ///
    /// # Occlusion culling
    ///
    /// No per-layer opaque culling is performed here. A back-to-front walk
    /// registers bottom layers first and would see later (on-top, visible)
    /// layers as "occluded" — exactly backwards. A sound front-to-back cull
    /// requires a separate pre-pass that is a future optimization opportunity.
    /// Walk a layer subtree, rendering every node.
    ///
    /// The traversal itself is [`crate::layer_walk::walk_layer_tree`] — an
    /// explicit-stack walk, because one Rust stack frame per layer means a
    /// deep-but-valid chain aborts the process rather than panicking, and a
    /// deep composited chain is ordinary. This function supplies the visit
    /// steps in the shared recording visitor.
    fn render_layer_recursive(
        tree: &flui_layer::LayerTree,
        layer_id: flui_foundation::LayerId,
        backend: &mut crate::layer_dispatcher::LayerDispatcher<'_>,
    ) -> EngineResult<()> {
        crate::layer_walk::record_layer_tree(tree, layer_id, backend)
    }
}

/// The windowed renderer's side of [`FrameProtocol::run`]: the swapchain
/// frame's clear, content and blit, over the renderer's fields borrowed apart
/// from its frame protocol.
///
/// [`FrameProtocol::run`]: crate::frame_protocol::FrameProtocol::run
struct SwapchainFrame<'a> {
    device: &'a wgpu::Device,
    painter: &'a mut crate::painter::WgpuPainter,
    offscreen: &'a mut crate::offscreen::OffscreenRenderer,
    scene: &'a flui_layer::Scene,
    format: wgpu::TextureFormat,
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: &'a mut Option<crate::profiler::GpuFrameProfiler>,
}

impl crate::frame_protocol::FrameSteps for SwapchainFrame<'_> {
    /// Submits the clear pass at once, so mid-frame copies (backdrop blur
    /// reads the target) see a cleared target.
    fn clear(&mut self, render_view: &wgpu::TextureView) -> EngineResult<()> {
        let mut clear_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("FLUI Clear Encoder"),
                });
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
        // `clear_encoder` exclusively; we access the encoder through
        // `scope.recorder` so the pass sees the same underlying encoder.
        // Scope drops at end of block → end_query fires → resolve_queries
        // copies the result → encoder finishes and is submitted.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_ref() {
            let mut scope = profiler.scope("clear", &mut clear_encoder);
            {
                let _pass = scope.recorder().begin_render_pass(&clear_pass_desc);
            }
            // scope drops here → end_query fires
        } else {
            // Feature compiled but no capable adapter (gpu_profiler is None):
            // fall back to the unprofiled clear pass so the surface is still
            // cleared. Branching on the Option, not just the Cargo feature, is
            // what makes the documented "graceful no-op" actually graceful.
            let _pass = clear_encoder.begin_render_pass(&clear_pass_desc);
        }
        #[cfg(not(feature = "gpu-profiler"))]
        {
            let _pass = clear_encoder.begin_render_pass(&clear_pass_desc);
        }
        // Resolve query results into the GPU buffer before finishing the encoder.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_mut() {
            profiler.resolve_queries(&mut clear_encoder);
        }
        self.painter.submit_encoder(clear_encoder)?;
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
    ) -> EngineResult<bool> {
        let straddled = Renderer::record_frame_content(self.painter, self.scene, partial_damage);
        let straddled = match straddled {
            Ok(straddled) => straddled,
            Err(error) => {
                self.painter.finish_frame();
                return Err(error);
            }
        };
        let painter = &mut *self.painter;

        // Final flush — submit remaining painter batches.
        let mut final_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("FLUI Final Render Encoder"),
                });
        // Profiler scope wraps painter.render. The scope borrows
        // `final_encoder` exclusively; we pass `scope.recorder` so
        // painter.render writes into the same underlying encoder.
        // Scope drops at end of block → end_query fires → resolve_queries
        // copies the result → encoder finishes and is submitted.
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
            let mut scope = profiler.scope("final_render", &mut final_encoder);
            painter.render(frame_target, scope.recorder())
            // scope drops here → end_query fires
        } else {
            painter.render(frame_target, &mut final_encoder)
        };
        #[cfg(not(feature = "gpu-profiler"))]
        let render_result = painter.render(frame_target, &mut final_encoder);
        if let Err(error) = render_result {
            // The frame is not presented; the per-frame painter state is
            // still reset so the next frame starts clean, and the caller's
            // classifier (`Recoverability`, `RasterOwner::handle_render_failure`)
            // finally sees the error `render_scene` documents.
            drop(final_encoder);
            painter.finish_frame();
            return Err(error);
        }
        // Resolve before finishing the encoder.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_mut() {
            profiler.resolve_queries(&mut final_encoder);
        }
        let submitted = painter.submit_encoder(final_encoder);

        // Frame boundary: run texture-cache maintenance ONCE, after the
        // final flush. `painter.render` runs per-pass (backdrop-filter
        // flushes call it mid-frame), so maintenance lives here — not inside
        // `render` — to avoid resetting use-counters between passes.
        painter.finish_frame();
        submitted?;
        Ok(straddled)
    }

    fn blit(&mut self, retained: &wgpu::Texture, surface: &wgpu::TextureView) -> EngineResult<()> {
        self.offscreen
            .blit_to_surface(retained, surface, self.format)
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;

    fn surface_caps(
        pairs: &[(wgpu::TextureFormat, wgpu::SurfaceColorSpaces)],
    ) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: pairs.iter().map(|&(format, _)| format).collect(),
            format_capabilities: pairs
                .iter()
                .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities {
                    format,
                    color_spaces,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn sdr_surface_selection_rejects_incompatible_pairs() {
        use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};
        for pairs in [
            vec![],
            vec![(Format::Bgra8UnormSrgb, Spaces::SRGB)],
            vec![(Format::Rgba8UnormSrgb, Spaces::SRGB)],
            vec![(Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR)],
            vec![(Format::Bgra8Unorm, Spaces::EXTENDED_SRGB_LINEAR)],
        ] {
            let error = Renderer::select_surface_format(&surface_caps(&pairs))
                .expect_err("unsupported color contract");
            assert_eq!(
                error.recoverability(),
                crate::error::Recoverability::Unrecoverable
            );
            assert!(
                matches!(error, EngineError::UnsupportedSurfaceColorConfiguration { supported } if supported.len() == pairs.len())
            );
        }
        let caps = surface_caps(&[
            (Format::Bgra8Unorm, Spaces::EXTENDED_SRGB_LINEAR),
            (Format::Rgba8Unorm, Spaces::SRGB),
        ]);
        assert_eq!(
            Renderer::select_surface_format(&caps)
                .expect("RGBA alternative")
                .0,
            Format::Rgba8Unorm
        );
    }

    fn sdr_surface_selection_painter_readback_preserves_swatches_and_blending() {
        use crate::painter::WgpuPainter;
        use flui_foundation::geometry::Rect;
        use flui_painting::Paint;
        use flui_painting::styling::Color;
        use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};

        let (device, queue) = crate::test_support::test_device_and_queue("SDR transfer regression");
        for alternative in [Format::Bgra8Unorm, Format::Rgba8Unorm] {
            let caps = surface_caps(&[
                (Format::Rgba16Float, Spaces::EXTENDED_SRGB_LINEAR),
                (alternative, Spaces::SRGB),
            ]);
            let (format, space) = Renderer::select_surface_format(&caps).expect("SDR pair");
            assert_eq!(space, wgpu::SurfaceColorSpace::Srgb);
            let size = 64;
            let (target, view) = crate::test_support::create_target(
                &device,
                "SDR swatches",
                size,
                size,
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
            let mut painter = WgpuPainter::with_shared_device(
                Arc::clone(&device),
                Arc::clone(&queue),
                format,
                (size, size),
            );
            let colors = [
                Color::rgb(18, 18, 18),
                Color::rgb(24, 24, 24),
                Color::rgb(128, 128, 128),
                Color::rgb(229, 57, 53),
                Color::rgb(255, 0, 0),
                Color::rgba(128, 128, 128, 128),
            ];
            for (index, color) in colors.iter().enumerate() {
                painter.draw_rect(
                    Rect::from_xywh(f64::from(index as f32 * 10.0), 0.0, 10.0, 64.0),
                    &Paint::fill(*color),
                );
            }
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            painter
                .render_to_view(&view, &mut encoder)
                .expect("paint swatches");
            queue.submit([encoder.finish()]);
            let bytes = crate::test_support::readback_bytes(&device, &queue, &target, size, size);
            for (index, expected) in [
                [18, 18, 18],
                [24, 24, 24],
                [128, 128, 128],
                [229, 57, 53],
                [255, 0, 0],
                [64, 64, 64],
            ]
            .iter()
            .enumerate()
            {
                let offset = (32 * size as usize + index * 10 + 5) * 4;
                let raw = &bytes[offset..offset + 4];
                let rgb = if format == Format::Bgra8Unorm {
                    [raw[2], raw[1], raw[0]]
                } else {
                    [raw[0], raw[1], raw[2]]
                };
                for channel in 0..3 {
                    assert!(
                        (i32::from(rgb[channel]) - expected[channel]).abs() <= 1,
                        "{format:?} swatch {index}: {rgb:?}, expected {expected:?}"
                    );
                }
                assert_eq!(raw[3], 255, "opaque black underlay");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn quarantined_domain_reaches_the_backend_recovery_predicate() {
        struct ReleasedWindow;
        impl raw_window_handle::HasWindowHandle for ReleasedWindow {
            fn window_handle(
                &self,
            ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
            {
                Err(raw_window_handle::HandleError::Unavailable)
            }
        }
        impl raw_window_handle::HasDisplayHandle for ReleasedWindow {
            fn display_handle(
                &self,
            ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
            {
                Err(raw_window_handle::HandleError::Unavailable)
            }
        }
        // This tests the actual backend predicate in the released-surface state;
        // it needs no fabricated native handles and never creates a surface.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(
            &crate::adapter::trusted_adapter_options(wgpu::PowerPreference::LowPower, None),
        ))
        .expect("GPU adapter for recovery predicate");
        let capabilities = GpuCapabilities::detect(&adapter);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Quarantine recovery predicate"),
            ..wgpu::DeviceDescriptor::default()
        }))
        .expect("GPU device for recovery predicate");
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let domain =
            crate::device_domain::DeviceDomain::new(Arc::clone(&device), Arc::clone(&queue));
        let (painter, offscreen) =
            Renderer::build_format_consumers(Arc::clone(&domain), format, (16, 16));
        let callback_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut renderer = Renderer {
            instance,
            adapter,
            device,
            queue,
            config: wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Srgb,
                width: 16,
                height: 16,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
            capabilities,
            painter,
            offscreen,
            supports_copy_src: false,
            device_lost: Arc::clone(&callback_flag),
            frame: crate::frame_protocol::FrameProtocol::new(),
            pre_present_hook: None,
            lease: SurfaceLease::released_for_test(Arc::new(ReleasedWindow)),
            _single_mutator: PhantomData,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: None,
            force_intermediate: false,
        };
        assert!(!crate::RasterBackend::is_device_lost(&renderer));
        // Domain quarantine can happen without a wgpu device-lost callback.
        domain.mark_lost();
        assert!(!callback_flag.load(std::sync::atomic::Ordering::Relaxed));
        assert!(
            crate::RasterBackend::is_device_lost(&renderer),
            "native lane must enter recovery"
        );
        assert!(matches!(
            domain.reserve(crate::device_domain::PreparedCost::default()),
            Err(crate::device_domain::DomainError::Unavailable)
        ));
        // Recovery replaces the generation. This deliberately does not claim
        // Renderer::recover succeeded with an unavailable native window.
        let fresh = crate::device_domain::DeviceDomain::new(
            Arc::clone(&renderer.device),
            Arc::clone(&renderer.queue),
        );
        (renderer.painter, renderer.offscreen) =
            Renderer::build_format_consumers(Arc::clone(&fresh), format, (16, 16));
        assert!(!crate::RasterBackend::is_device_lost(&renderer));
        let encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        fresh
            .submit(
                fresh
                    .prepare(vec![encoder.finish()], vec![])
                    .expect("fresh prepared submission"),
            )
            .expect("fresh generation progresses after quarantine");
    }

    /// Acquire a real device/queue for the HiDPI backdrop regression below.
    /// Returns `None` when no GPU adapter is available (CI without a GPU).
    fn test_device_and_queue() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        crate::test_support::try_test_device_and_queue("Backdrop HiDPI Test Device")
    }

    // =========================================================================
    // C2 — forced-intermediate blit correctness
    //
    // Proves that `blit_to_surface` transfers pixels from the intermediate
    // texture to the swapchain surface without modification.  A known RGBA8
    // pattern is rendered into the intermediate; after the blit, the surface
    // is read back and asserted pixel-for-pixel identical.
    //
    // This is NOT a full `render_scene` test (that requires a live swapchain).
    // It exercises the `OffscreenRenderer::blit_to_surface` method — the same
    // code path the PR-6 frame loop calls — with a synthetic intermediate
    // texture as input, which is sufficient to prove the blit pipeline is
    // correct.  The full present-path integration (intermediate-active
    // `render_scene` + advanced blend readback) is exercised by the caller
    // with `force_intermediate_for_testing` on a live DX12 window.
    // =========================================================================

    /// Helper: GPU-copy the first four bytes of a texture into a host Vec.
    /// Returns `None` when no GPU is available in this environment.
    fn readback_rgba_pixel(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        x: u32,
        y: u32,
    ) -> Option<[u8; 4]> {
        // 256-byte-aligned staging buffer (wgpu requirement: bytes_per_row % 256 == 0)
        // For a single texel readback we only need 4 bytes of payload, but the
        // buffer must be at least 256 bytes to satisfy `MAP_READ` alignment.
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback Staging Buffer"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map synchronously: submit, poll-wait, then read.
        staging_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok()?;

        // The `?`s above return `None` for "no usable device", which is what
        // this helper's callers read. A map that fails AFTER the poll-wait
        // succeeded is not that — it is a broken invariant, and folding it into
        // the same `None` would report a real failure as an absent GPU.
        let mapped = staging_buffer
            .slice(..4)
            .get_mapped_range()
            .expect("staging buffer must be mapped: the poll above waited for the map to complete");
        let bytes: [u8; 4] = mapped[..4].try_into().ok()?;
        Some(bytes)
    }

    // =========================================================================
    // C3 — common-path byte-identity after blit
    //
    // Proves that blitting a solid-color intermediate into a surface gives the
    // same pixel as clearing the surface directly to that same color.  If the
    // blit pipeline introduced any color-space re-encoding, blending, or
    // gamma shift, the pixels would differ.
    // =========================================================================

    // =========================================================================
    // C2-full — intermediate path: advanced blend through intermediate → blit
    //
    // Proves that the full data path (painter with advanced Multiply saveLayer →
    // sampleable intermediate → blit → surface) produces the correct Multiply
    // pixel, NOT the SrcOver fallback pixel.
    //
    // `render_scene` requires a live swapchain surface and cannot run headlessly.
    // This test exercises the same data path manually:
    //   1. Render a Multiply saveLayer into a pooled sampleable intermediate
    //      via `painter.render(RenderTarget::sampleable(...))`.
    //   2. Blit the intermediate onto a synthetic surface.
    //   3. Readback the surface center and assert ≈ Multiply oracle AND ≠ SrcOver.
    //
    // `force_intermediate_for_testing` is the design anchor that marks the intent;
    // the headless test exercises the identical constituent operations.
    // =========================================================================

    // =========================================================================
    // OCR-1 — occlusion-cull regression
    //
    // Reproduces the bug where `render_layer_recursive` culled visible on-top
    // layers because the occlusion tracker was fed in back-to-front (painter's
    // algorithm) order, which is exactly backwards from the front-to-back order
    // that `OcclusionTracker::add_opaque` requires for sound culling.
    //
    // Concretely: a full-screen opaque `CanvasLayer` background (child[0]) was
    // registered as opaque AFTER rendering, then a sibling `ImageFilterLayer`
    // (child[1]) wrapping a sub-region `CanvasLayer` had that inner canvas
    // culled by `is_occluded` — even though it was drawn on top and fully visible.
    //
    // The fix removes the unsound occlusion pass from `render_layer_recursive`
    // entirely.  There is no sound way to do per-layer opaque culling in a
    // single back-to-front walk without knowing future (on-top) content first.
    //
    // This test is RED before the fix (filter_op_count == 0, child was culled)
    // and GREEN after (filter_op_count == 1, child rendered into the filter).
    // =========================================================================

    // =========================================================================
    // Tier-2 Follower render-time resolution — GPU-level, end-to-end
    // pixel-readback proof.
    //
    // Render-object-harness-level testing cannot check on-screen positioning
    // (explicitly out of harness scope) — these
    // tests exercise the REAL `render_layer_recursive` Follower special case
    // (not a hand-rolled stand-in) against a real GPU texture, reading back
    // actual rendered pixels.
    // =========================================================================

    /// Clears `view` to `color`. Mirrors `SwapchainFrame::clear`'s body;
    /// duplicated here because these tests build `WgpuPainter`/`LayerDispatcher`
    /// manually (like OCR-1 above) rather than through a full `Renderer`, so
    /// the swapchain frame's clear step isn't reachable.
    /// `painter.render` uses `LoadOp::Load` (see the C2-full test's own
    /// pre-clear), so every one of these tests must pre-clear its target.
    fn clear_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        color: wgpu::Color,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Follower Test Clear Encoder"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Follower Test Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// (a) A `Layer::Follower` linked to a `Layer::Leader` under a DIFFERENT
    /// `Layer::Offset` ancestor (the cross-repaint-boundary case that
    /// motivated the whole render-time-resolution design) must
    /// render its subtree at the LEADER's resolved position, not at its own
    /// natural (pre-resolution) tree position.
    fn follower_gpu_renders_at_resolved_position_across_repaint_boundaries() {
        use crate::layer_dispatcher::LayerDispatcher;
        use crate::painter::WgpuPainter;
        use crate::render_target::RenderTarget;
        use flui_foundation::geometry::{Offset, Rect, Size};
        use flui_layer::{
            CanvasLayer, FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, OffsetLayer,
        };
        use flui_painting::styling::Color;
        use flui_painting::{Canvas, Paint};

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 200u32;
        let height = 200u32;

        let link = LayerLink::new();
        let mut tree = LayerTree::new(Layer::Offset(OffsetLayer::zero()));

        let root_id = tree.root();

        // Leader lives under `branch_a`, offset (60,0) from root.
        let branch_a = tree.push_child(
            root_id,
            Layer::Offset(OffsetLayer::new(Offset::new(60.0, 0.0))),
        );
        let _leader_id = tree.push_child(
            branch_a,
            Layer::Leader(LeaderLayer::with_offset(
                link,
                Size::new(20.0, 20.0),
                Offset::new(5.0, 5.0),
            )),
        );

        // Follower lives under a DIFFERENT boundary, `branch_b`, offset
        // (0,90) from root.
        let branch_b = tree.push_child(
            root_id,
            Layer::Offset(OffsetLayer::new(Offset::new(0.0, 90.0))),
        );
        let follower_id = tree.push_child(
            branch_b,
            Layer::Follower(FollowerLayer::new(link).with_size(Size::new(10.0, 10.0))),
        );

        // The follower's child: a 10×10 opaque red rect at its own local origin.
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            &Paint::fill(Color::rgba(255, 0, 0, 255)),
        );
        let _child_id = tree.push_child(
            follower_id,
            Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))),
        );

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Follower GPU Test Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
        clear_texture(&device, &queue, &render_view, wgpu::Color::WHITE);

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (width, height),
        );
        let mut backend = LayerDispatcher::new(&mut painter);

        Renderer::render_layer_recursive(&tree, root_id, &mut backend).expect("layer tree records");
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Follower GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // Resolved leader-relative position: branch_a (60,0) + leader local
        // (5,5) = (65,5). Sample a couple pixels inset to dodge edge AA.
        let resolved_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 67, 7)
            .expect("resolved-position readback must succeed");
        assert!(
            resolved_pixel[0] > 200 && resolved_pixel[1] < 50,
            "follower content must render at the LEADER's resolved position \
             (65,5)-ish; expected opaque red, got {resolved_pixel:?}"
        );

        // The follower's own NATURAL (pre-resolution) tree position: branch_b
        // (0,90) + local (0,0). Must stay untouched white background,
        // proving the content actually MOVED to the leader's position rather
        // than painting at (or in addition to) its natural slot.
        let natural_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 2, 92)
            .expect("natural-position readback must succeed");
        assert!(
            natural_pixel[1] > 200,
            "follower must NOT render at its own natural (pre-resolution) \
             tree position; expected untouched white background, got {natural_pixel:?}"
        );
    }

    // =========================================================================
    // `Layer::ShaderMask` engine visual-rendering fix — GPU-level, end-to-end
    // pixel-readback proof.
    //
    // `Layer::ShaderMask` previously fell through to the generic
    // `LayerRender` dispatch (`layer_render.rs`), which pushes an inert
    // `save_layer`/`push_clip_rect` pair that never reads the layer's
    // `shader()`/`blend_mode()` — masking silently never applied. These
    // tests exercise the REAL `render_layer_recursive` special case (not a
    // hand-rolled stand-in) against a real GPU texture, reading back actual
    // rendered pixels, the same style as the Follower Tier-2 tests above.
    // =========================================================================

    fn shader_mask_layer_root_gpu_pixel_readback_reflects_mask() {
        use crate::layer_dispatcher::LayerDispatcher;
        use crate::painter::WgpuPainter;
        use crate::render_target::RenderTarget;
        use flui_foundation::geometry::Rect;
        use flui_layer::{CanvasLayer, Layer, LayerTree, ShaderMaskLayer};
        use flui_painting::{Canvas, Paint, Shader};
        use flui_painting::{paint::BlendMode, styling::Color};

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 64u32;
        let height = 64u32;

        let bounds = Rect::from_xywh(0.0, 0.0, 64.0, 64.0);
        let shader = Shader::solid(Color::rgba(10, 20, 30, 128));
        let mut tree = LayerTree::new(Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            BlendMode::SrcOver,
            bounds,
        )));
        let mask_id = tree.root();

        let mut canvas = Canvas::new();
        canvas.draw_rect(bounds, &Paint::fill(Color::rgba(255, 0, 0, 255)));
        let _child_id = tree.push_child(
            mask_id,
            Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))),
        );

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ShaderMask Root GPU Test Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
        clear_texture(&device, &queue, &render_view, wgpu::Color::WHITE);

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (width, height),
        );
        let mut backend = LayerDispatcher::new(&mut painter);

        Renderer::render_layer_recursive(&tree, mask_id, &mut backend).expect("layer tree records");
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShaderMask Root GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        let pixel = readback_rgba_pixel(&device, &queue, &render_texture, 32, 32)
            .expect("center readback must succeed");
        // SrcOver combines the coloured shader with the opaque red child;
        // the result is opaque before it reaches the white parent.
        let alpha = 128.0_f64 / 255.0;
        let expected = [
            (10.0 * alpha + 255.0 * (1.0 - alpha)).round() as u8,
            (20.0 * alpha).round() as u8,
            (30.0 * alpha).round() as u8,
            255,
        ];
        assert!(
            pixel.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2),
            "shader SrcOver child: {pixel:?}, expected {expected:?}"
        );
    }

    /// Surface selection and compositor readbacks, one row per feature: SDR pair
    /// selection (rejections and swatch/blending readback), backdrop filters under
    /// DPR, followers across repaint boundaries, and the shader-mask layer root.
    #[test]
    fn renderer_surface_selection_and_layer_compositing_read_back_as_specified() {
        shader_mask_external_texture_registrations_follow_parent();
        #[cfg(not(target_arch = "wasm32"))]
        quarantined_domain_reaches_the_backend_recovery_predicate();
        sdr_surface_selection_rejects_incompatible_pairs();
        sdr_surface_selection_painter_readback_preserves_swatches_and_blending();
        follower_gpu_renders_at_resolved_position_across_repaint_boundaries();
        shader_mask_layer_root_gpu_pixel_readback_reflects_mask();
        #[cfg(feature = "gpu-profiler")]
        crate::profiler::tests::failed_frames_do_not_pollute_the_next_profile();
    }

    fn shader_mask_external_texture_registrations_follow_parent() {
        use flui_foundation::geometry::Rect;
        use flui_layer::{Layer, LayerTree, PictureLayer, Scene, ShaderMaskLayer};
        use flui_painting::paint::{FilterQuality, TextureId};
        use flui_painting::styling::Color;
        use flui_painting::{Canvas, Paint, Shader};

        let Some(renderer) = crate::test_support::renderer_or_skip() else {
            return;
        };
        let mut capture = renderer
            .retained_capture((128, 128))
            .expect("capture target");
        let texture = TextureId::new(73);
        let scene = |extent: f64, source| {
            let mut background = Canvas::new();
            background.draw_rect(
                Rect::from_xywh(0.0, 0.0, 128.0, 128.0),
                &Paint::fill(Color::WHITE),
            );
            let mut tree = LayerTree::new(Layer::from(PictureLayer::new(background.finish())));
            let bounds = Rect::from_xywh(20.0, 20.0, extent, extent);
            let mask = tree.push_child(
                tree.root(),
                Layer::from(ShaderMaskLayer::new(
                    Shader::solid(Color::rgba(255, 255, 255, 128)),
                    flui_painting::BlendMode::DstIn,
                    bounds,
                )),
            );
            let mut canvas = Canvas::new();
            canvas.draw_texture(source, bounds, None, FilterQuality::None, 1.0);
            tree.push_child(mask, Layer::from(PictureLayer::new(canvas.finish())));
            Scene::new(tree)
        };
        // Equal extents reuse the child painter; a new extent rebuilds it.
        // Both must observe replacement on the parent's registry, not stale leases.
        for (extent, color, expected) in [
            (32.0, [255, 0, 0, 255], [255, 127, 127, 255]),
            (32.0, [0, 0, 255, 255], [127, 127, 255, 255]),
            (48.0, [0, 255, 0, 255], [127, 255, 127, 255]),
        ] {
            capture.set_solid_texture(texture, color);
            capture
                .render_unmanaged(&scene(extent, texture))
                .expect("masked external frame");
            let pixels = capture.read_rgba().expect("masked readback");
            let pixel = |x: usize, y: usize| {
                let index = (y * 128 + x) * 4;
                &pixels[index..index + 4]
            };
            for (&actual, expected) in pixel(28, 28).iter().zip(expected) {
                assert!(
                    (i32::from(actual) - expected).abs() <= 2,
                    "masked external pixel {:?}, expected {expected}",
                    pixel(28, 28)
                );
            }
            assert_eq!(pixel(8, 8), &[255, 255, 255, 255], "outside mask unchanged");
        }
        let error = capture
            .render_unmanaged(&scene(48.0, TextureId::new(74)))
            .expect_err("missing external ID fails");
        assert!(matches!(
            error,
            crate::EngineError::ExternalTexture(crate::ExternalTextureError::UnknownTexture {
                id: 74
            })
        ));
        capture
            .render_unmanaged(&scene(48.0, texture))
            .expect("mask painter recovers after failed lookup");
        let recovered = capture.read_rgba().expect("recovered masked readback");
        let offset = (28 * 128 + 28) * 4;
        for (&actual, expected) in recovered[offset..offset + 4]
            .iter()
            .zip([127, 255, 127, 255])
        {
            assert!(
                (i32::from(actual) - expected).abs() <= 2,
                "recovered frame must repaint the masked texture"
            );
        }
    }
}
