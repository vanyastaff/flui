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
//! ```rust,ignore
//! use flui_engine::wgpu::Renderer;
//!
//! // Create renderer (automatically selects backend)
//! let renderer = Renderer::new(window).await?;
//!
//! // Render frame
//! renderer.render(display_list)?;
//! ```

use std::sync::Arc;

use wgpu;

use crate::error::{EngineError, EngineResult};

/// GPU backend capabilities
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// Backend being used (Metal, DX12, Vulkan, WebGPU, etc.)
    pub backend: wgpu::Backend,

    /// GPU adapter name
    pub adapter_name: String,

    /// GPU vendor (NVIDIA, AMD, Intel, Apple, etc.)
    pub vendor: String,

    /// Maximum texture dimension (e.g., 16384)
    pub max_texture_size: u32,

    /// Supports HDR rendering
    pub supports_hdr: bool,

    /// Supports compute shaders
    pub supports_compute: bool,

    /// Supports immediates / push constants (not available on all mobile GPUs).
    /// Mapped from `wgpu::Features::IMMEDIATES` (renamed from PUSH_CONSTANTS in wgpu 28).
    pub supports_push_constants: bool,

    /// Supports BC texture compression (DX)
    pub supports_bc_compression: bool,

    /// Supports ASTC texture compression (mobile)
    pub supports_astc_compression: bool,

    /// Supports ETC2 texture compression (mobile)
    pub supports_etc2_compression: bool,

    /// Supports GPU timestamp queries required for the `gpu-profiler` feature.
    ///
    /// `true` only when BOTH `TIMESTAMP_QUERY` AND `TIMESTAMP_QUERY_INSIDE_ENCODERS`
    /// are present. The encoder-level scopes used by `GpuFrameProfiler` require
    /// `INSIDE_ENCODERS`; without it wgpu-profiler records 0.0 ms silently.
    ///
    /// Typically present on DX12, Vulkan, and Metal. Absent on GLES/WebGL2 and on
    /// some older/mobile drivers that support the base feature but not the encoder
    /// variant.
    pub supports_timestamp_queries: bool,
}

impl GpuCapabilities {
    /// Detect GPU capabilities from adapter
    pub fn detect(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let features = adapter.features();
        let limits = adapter.limits();

        Self {
            backend: info.backend,
            adapter_name: info.name.clone(),
            vendor: Self::vendor_name(info.vendor),
            max_texture_size: limits.max_texture_dimension_2d,
            supports_hdr: Self::check_hdr_support(info.backend),
            supports_compute: true, // Compute shaders are supported by default in wgpu
            supports_push_constants: features.contains(wgpu::Features::IMMEDIATES),
            supports_bc_compression: features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            supports_astc_compression: features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
            supports_etc2_compression: features.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2),
            // Encoder-level profiler scopes (used by the gpu-profiler feature) require
            // TIMESTAMP_QUERY_INSIDE_ENCODERS in addition to the base TIMESTAMP_QUERY.
            // Without INSIDE_ENCODERS, wgpu-profiler records 0.0 ms for every scope —
            // it passes tests while measuring nothing. Only set this flag when both
            // features are present so the profiler is never `Some` on an incapable adapter.
            supports_timestamp_queries: features.contains(wgpu::Features::TIMESTAMP_QUERY)
                && features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
        }
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

    fn check_hdr_support(backend: wgpu::Backend) -> bool {
        match backend {
            // macOS EDR (Extended Dynamic Range) on XDR displays,
            // Windows Auto HDR (Windows 11 24H2+)
            wgpu::Backend::Metal | wgpu::Backend::Dx12 => true,
            _ => false,
        }
    }
}

/// GPU context available during layer tree rendering.
///
/// Carries the surface-capability flags the layer walk needs. (Device, queue,
/// and surface format used to live here for mid-frame backdrop blur; that path
/// now sources them from the offscreen renderer inside
/// `Backend::apply_backdrop_blur`, so they were removed as dead fields.)
struct RenderContext {
    /// Whether the surface supports COPY_SRC (for backdrop filter on the
    /// common direct-render path).
    supports_copy_src: bool,
    /// Whether this frame renders into a pooled intermediate texture instead
    /// of directly into the swapchain surface.  When `true`, the intermediate
    /// already carries COPY_SRC (all pool textures have it), so backdrop-filter
    /// and advanced-blend dst-reads both work regardless of
    /// `supports_copy_src`.
    intermediate_active: bool,
}

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
    painter: super::painter::WgpuPainter,
    offscreen: super::offscreen::OffscreenRenderer,
    supports_copy_src: bool,
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: Option<super::profiler::GpuFrameProfiler>,
}

/// Cross-platform GPU renderer
///
/// `Send` (see the `unsafe impl Send` below, justified there) but
/// deliberately never `Sync`: the raster owner
/// (`crate::raster_owner::RasterOwner`) has sole mutable access to the
/// `Renderer`/`Surface`/`Device`/`Queue` it wraps, and a shared `&Renderer`
/// across threads would defeat that single-mutator contract. No field
/// grants `Sync` today, so this already holds without a marker type; the
/// doctest below pins the contract so a future field addition that
/// accidentally makes every field `Sync` fails loudly at compile time
/// instead of silently reopening `Arc<Renderer>` shared-mutation:
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<flui_engine::wgpu::Renderer>();
/// ```
pub struct Renderer {
    // `instance` and `adapter` are kept alive for the lifetime of the renderer
    // because `wgpu::Surface<'static>` and `wgpu::Device` depend on them. They
    // are not read post-init in production code; the `#[allow(dead_code)]`
    // markers document that the keep-alive shape is intentional.
    #[allow(dead_code)]
    instance: wgpu::Instance,
    #[allow(dead_code)]
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    capabilities: GpuCapabilities,
    painter: Option<super::painter::WgpuPainter>,
    offscreen: Option<super::offscreen::OffscreenRenderer>,
    /// Whether the surface supports COPY_SRC (for mid-frame texture copies)
    supports_copy_src: bool,
    /// Set by the device-lost callback; checked at frame start to trigger
    /// device recreation. `Arc<AtomicBool>` because the callback is `'static`.
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// Tracks dirty regions for incremental rendering (skip frames with no damage)
    damage_tracker: flui_layer::damage::DamageTracker,
    /// Raw window handle stored for self-contained device recovery.
    ///
    /// SAFETY: The stored handles are reused by `recover()` to rebuild the wgpu
    /// surface after a GPU device loss. They remain valid for the same reason the
    /// existing `wgpu::Surface<'static>` is sound — the window (owned by
    /// flui-app's `App`) outlives the `Renderer`. `None` for offscreen renderers.
    raw_window_handle: Option<raw_window_handle::RawWindowHandle>,
    /// Raw display handle stored alongside `raw_window_handle` for recovery.
    /// `None` for offscreen renderers.
    raw_display_handle: Option<raw_window_handle::RawDisplayHandle>,
    /// GPU timestamp profiler. `None` when the `gpu-profiler` feature is off
    /// or the adapter does not expose `wgpu::Features::TIMESTAMP_QUERY`.
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: Option<super::profiler::GpuFrameProfiler>,

    /// Test-only flag that forces the intermediate-texture present path ON,
    /// even when the surface supports COPY_SRC.  Allows C2/C3 tests to
    /// exercise and verify the intermediate path on COPY_SRC-capable hardware.
    ///
    /// Controlled by [`Renderer::force_intermediate_for_testing`].
    #[cfg(test)]
    force_intermediate: bool,

    /// When `true`, the NEXT call to `render_scene` will promote the
    /// damage to a full repaint before any scissor logic runs.
    ///
    /// Set when the current frame detected a partial-damage scissor AND a
    /// `DrawItem::AdvancedShape` (or SSAA-path with an advanced blend) whose
    /// `device_bounds` straddle the damage edge.  Such items call
    /// `flush_advanced_layer` with `LoadOp::Load` on the full `device_bounds`
    /// with no scissor — if the foreground is restricted by the scissor,
    /// the out-of-damage slice blends `transparent_fg` over the stale
    /// prior-frame backdrop, writing stale pixels.
    ///
    /// Self-healing: the next frame is forced full, repainting the shape
    /// over its true `device_bounds` without a scissor restriction.  The
    /// transient is unobservable today because callers use full repaint
    /// exclusively (see the `damage_rect()` call-site comment); a this-frame
    /// re-record or a precomputed `Scene` bit would be the upgrade path once
    /// partial damage becomes hot.
    force_full_repaint_next_frame: bool,
}

// SAFETY: `Renderer` stores `Option<RawWindowHandle>` and
// `Option<RawDisplayHandle>`, which contain `NonNull<c_void>` on some
// platforms (e.g. `UiKitWindowHandle`) and are therefore `!Send` by default.
// The handles are:
//   1. Extracted once at construction from the live window (or `None` for
//      offscreen renderers).
//   2. Read only inside `recover(&mut self)`, which requires exclusive access —
//      no two threads can call `recover` concurrently on the same `Renderer`.
//   3. Never sent to another thread while they are being dereferenced; the
//      dereferencing happens only inside the `unsafe` block in
//      `build_windowed_gpu_stack`, which runs in the same logical context as the
//      caller holding `&mut self`.
// This is the same Send posture that wgpu itself uses for its internal raw-handle
// wrappers: the window pointer is accessed exclusively and never aliased across
// threads. The owning window (flui-app's `App`) is alive for the lifetime of the
// `Renderer`, satisfying the validity requirement.
#[allow(unsafe_code)]
unsafe impl Send for Renderer {}

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
    /// ```rust,ignore
    /// use flui_engine::wgpu::Renderer;
    /// use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
    ///
    /// let renderer = Renderer::new(&window).await?;
    /// println!("Using backend: {:?}", renderer.capabilities().backend);
    /// ```
    pub async fn new<W>(window: &W) -> EngineResult<Self>
    where
        W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle,
    {
        // Extract raw handles before calling the GPU stack builder.
        // `window_handle()` and `display_handle()` are safe trait methods; no
        // unsafe is required here. The stored raw handles are later reused by
        // `recover()` to rebuild the surface; they remain valid for the same
        // reason the `wgpu::Surface<'static>` is sound — the window (owned by
        // flui-app's `App`) outlives the `Renderer`.
        //
        // wgpu 29.x multi-monitor fix: explicitly extract raw handles to work
        // around DisplayHandle lifetime issues on multi-display systems.
        let window_handle = window
            .window_handle()
            .map_err(EngineError::surface_creation)?;
        let display_handle = window
            .display_handle()
            .map_err(EngineError::surface_creation)?;
        let (raw_window_handle, raw_display_handle) =
            (window_handle.as_raw(), Some(display_handle.as_raw()));

        let (w, h) = (800u32, 600u32); // Will be updated on first resize
        let stack =
            Self::build_windowed_gpu_stack(raw_window_handle, raw_display_handle, w, h).await?;

        Ok(Self {
            instance: stack.instance,
            adapter: stack.adapter,
            device: stack.device,
            queue: stack.queue,
            surface: Some(stack.surface),
            config: Some(stack.config),
            capabilities: stack.capabilities,
            painter: Some(stack.painter),
            offscreen: Some(stack.offscreen),
            supports_copy_src: stack.supports_copy_src,
            device_lost: stack.device_lost,
            damage_tracker: flui_layer::damage::DamageTracker::new(),
            raw_window_handle: Some(raw_window_handle),
            raw_display_handle,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: stack.gpu_profiler,
            #[cfg(test)]
            force_intermediate: false,
            force_full_repaint_next_frame: false,
        })
    }

    /// Build the full windowed GPU stack from raw handles.
    ///
    /// Factored out of `new` so `recover` can call it without re-extracting
    /// the window handles. Called once at construction and again on device loss.
    ///
    /// # Errors
    ///
    /// Adapter or device creation can fail (e.g. driver still resetting after
    /// a TDR). Returns the underlying [`EngineError`]; the caller may retry on
    /// the next frame.
    async fn build_windowed_gpu_stack(
        raw_window_handle: raw_window_handle::RawWindowHandle,
        raw_display_handle: Option<raw_window_handle::RawDisplayHandle>,
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

        // Create surface from stored raw handles.
        //
        // SAFETY: The raw handles were captured from the live window at
        // construction time (see `Renderer::new`) and remain valid while the
        // window is alive. Both `SurfaceTargetUnsafe::RawHandle` and
        // `Instance::create_surface_unsafe` require the handles to stay valid
        // for the lifetime of the resulting `Surface<'static>`; that invariant
        // is upheld because flui-app's `App` owns the window for its lifetime.
        #[allow(unsafe_code)]
        let surface = unsafe {
            let surface_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            };
            instance
                .create_surface_unsafe(surface_target)
                .map_err(EngineError::surface_creation)?
        };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(EngineError::adapter_request)?;

        let capabilities = GpuCapabilities::detect(&adapter);
        tracing::info!(
            "Selected GPU: {} ({}), Backend: {:?}",
            capabilities.adapter_name,
            capabilities.vendor,
            capabilities.backend
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI GPU Device"),
                required_features: Self::required_features(&capabilities),
                required_limits: Self::required_limits(&capabilities),
                // Desktop UI: trade VRAM for faster per-frame GPU allocations.
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(EngineError::device_creation)?;

        let device_lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Self::install_device_diagnostics(&device, Arc::clone(&device_lost));

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = Self::select_surface_format(&surface_caps, &capabilities);

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
            width,
            height,
            present_mode: Self::select_present_mode(&surface_caps),
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            // 1 (not 2): during a live resize the displayed frame must track the
            // window size as tightly as possible. A latency of 2 lets the present
            // queue hold frames rendered for an older size, which the compositor
            // then stretches to the current window → visible resize jitter.
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let painter = super::painter::WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            surface_format,
            (config.width, config.height),
        );

        let offscreen = super::offscreen::OffscreenRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            surface_format,
        );

        // Create the GPU profiler if the feature is enabled AND the adapter
        // exposes TIMESTAMP_QUERY. A creation failure is non-fatal — profiling
        // is strictly additive and must never abort initialization.
        #[cfg(feature = "gpu-profiler")]
        let gpu_profiler = if capabilities.supports_timestamp_queries {
            match super::profiler::GpuFrameProfiler::new(&device) {
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

    /// Create an offscreen renderer (no window surface)
    ///
    /// Useful for headless rendering, tests, and compute-only tasks.
    pub async fn new_offscreen() -> EngineResult<Self> {
        let backends = Self::select_backend();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(EngineError::adapter_request)?;

        let capabilities = GpuCapabilities::detect(&adapter);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI Offscreen Device"),
                required_features: Self::required_features(&capabilities),
                required_limits: Self::required_limits(&capabilities),
                // Desktop UI: trade VRAM for faster per-frame GPU allocations.
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(EngineError::device_creation)?;

        let device_lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Self::install_device_diagnostics(&device, Arc::clone(&device_lost));

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface: None,
            config: None,
            capabilities,
            painter: None,
            offscreen: None,
            supports_copy_src: false,
            device_lost,
            damage_tracker: flui_layer::damage::DamageTracker::new(),
            raw_window_handle: None,
            raw_display_handle: None,
            // Offscreen renderers have no surface present, so profiling results
            // cannot be harvested with process_finished_frame. Disabled here.
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: None,
            #[cfg(test)]
            force_intermediate: false,
            force_full_repaint_next_frame: false,
        })
    }

    /// Returns `true` if the GPU device has been lost.
    ///
    /// After a TDR, driver crash, or GPU hardware failure the device-lost
    /// callback fires and sets this flag. The caller (runner frame loop)
    /// should call [`recover()`](Self::recover) to rebuild the GPU context.
    #[must_use]
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Whether the intermediate-texture present path is active for this frame.
    ///
    /// `true` when the swapchain surface lacks `COPY_SRC` (real adapter
    /// limitation) OR when the test flag `force_intermediate_for_testing` is
    /// set.  In both cases the frame is rendered into a pooled intermediate
    /// texture and blitted onto the swapchain at the end of the frame.
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
    #[allow(
        dead_code,
        reason = "called from live DX12 GPU tests run by the user, not from automated unit tests"
    )]
    pub(crate) fn force_intermediate_for_testing(&mut self) {
        self.force_intermediate = true;
    }

    /// Rebuild the GPU device and surface after a device-lost event.
    ///
    /// On the **windowed** path (`raw_window_handle` is `Some`) this rebuilds
    /// the entire GPU stack (instance → adapter → device → surface → painter →
    /// offscreen) and swaps the new pieces into `self`. The recovered surface
    /// is configured at the **current** surface size captured from `self.config`
    /// (falling back to 800×600), so the window keeps its correct dimensions
    /// without a separate resize call.
    ///
    /// On the **offscreen** path (`raw_window_handle` is `None`) only the
    /// device/queue are replaced; surface, painter, and offscreen are left as
    /// `None`.
    ///
    /// On success the device-lost flag is cleared (the fresh device starts
    /// healthy). On failure the underlying [`EngineError`] is returned — the
    /// driver may still be resetting; the runner should retry on the next frame.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::AdapterRequest`] or [`EngineError::DeviceCreation`]
    /// when the driver is still resetting or the adapter is no longer available.
    /// Returns [`EngineError::SurfaceCreation`] if the surface cannot be
    /// recreated from the stored raw handles (very unlikely while the window is
    /// alive).
    #[tracing::instrument(level = "warn", skip(self))]
    pub async fn recover(&mut self) -> EngineResult<()> {
        if let Some(raw_window) = self.raw_window_handle {
            // Capture current dimensions before rebuild so the recovered
            // surface matches the live window size instead of defaulting to
            // 800×600.
            let (width, height) = self
                .config
                .as_ref()
                .map_or((800u32, 600u32), |c| (c.width, c.height));

            let stack =
                Self::build_windowed_gpu_stack(raw_window, self.raw_display_handle, width, height)
                    .await?;

            self.instance = stack.instance;
            self.adapter = stack.adapter;
            self.device = stack.device;
            self.queue = stack.queue;
            self.surface = Some(stack.surface);
            self.config = Some(stack.config);
            self.capabilities = stack.capabilities;
            self.painter = Some(stack.painter);
            self.offscreen = Some(stack.offscreen);
            self.supports_copy_src = stack.supports_copy_src;
            // Replace with a fresh flag — the new device starts healthy.
            self.device_lost = stack.device_lost;
            // Reset profiler with the fresh device — timestamp queries from the
            // lost device are invalid and must not be carried over.
            #[cfg(feature = "gpu-profiler")]
            {
                self.gpu_profiler = stack.gpu_profiler;
            }
            // Force a full repaint so the first recovered frame is complete.
            self.damage_tracker.mark_full_repaint();
        } else {
            // Offscreen path: rebuild device/queue only.
            let backends = Self::select_backend();
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .map_err(EngineError::adapter_request)?;
            let capabilities = GpuCapabilities::detect(&adapter);
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("FLUI Offscreen Device"),
                    required_features: Self::required_features(&capabilities),
                    required_limits: Self::required_limits(&capabilities),
                    memory_hints: wgpu::MemoryHints::Performance,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    trace: wgpu::Trace::Off,
                })
                .await
                .map_err(EngineError::device_creation)?;

            let fresh_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            Self::install_device_diagnostics(&device, Arc::clone(&fresh_flag));

            self.instance = instance;
            self.adapter = adapter;
            self.device = Arc::new(device);
            self.queue = Arc::new(queue);
            self.capabilities = capabilities;
            self.device_lost = fresh_flag;
        }

        tracing::info!(
            width = self.config.as_ref().map_or(0, |c| c.width),
            height = self.config.as_ref().map_or(0, |c| c.height),
            "GPU device recovered successfully"
        );

        Ok(())
    }

    /// Select appropriate backend for the current platform
    fn select_backend() -> wgpu::Backends {
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
            device_lost_flag.store(true, std::sync::atomic::Ordering::Release);
        });
    }

    /// Required GPU features based on capabilities and adapter support.
    ///
    /// Only requests optional features when the adapter actually exposes them,
    /// so device creation never regresses on GPUs that lack them.
    fn required_features(capabilities: &GpuCapabilities) -> wgpu::Features {
        let mut features = wgpu::Features::empty();

        // Always enable texture adapter-specific formats.
        features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

        // Immediates (formerly push constants): only request if adapter supports them.
        // Some mobile GPUs (especially older Android devices) don't support this.
        if capabilities.supports_push_constants {
            features |= wgpu::Features::IMMEDIATES;
        }

        // Timestamp queries for GPU profiling: only request when the adapter exposes
        // BOTH features AND the gpu-profiler cargo feature is enabled. Device creation
        // must never fail because of an optional profiling feature the adapter lacks.
        // `supports_timestamp_queries` is already true only when both are present
        // (see `GpuCapabilities::detect`), so requesting both here is safe.
        #[cfg(feature = "gpu-profiler")]
        if capabilities.supports_timestamp_queries {
            features |=
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        }

        features
    }

    /// Required GPU limits based on capabilities and adapter support
    fn required_limits(capabilities: &GpuCapabilities) -> wgpu::Limits {
        let mut limits = wgpu::Limits {
            max_texture_dimension_2d: capabilities.max_texture_size.min(16384),
            ..wgpu::Limits::default()
        };

        // Immediate data size — only set if adapter supports immediates
        if capabilities.supports_push_constants {
            limits.max_immediate_size = 128;
        }

        limits
    }

    /// Select surface format based on capabilities
    fn select_surface_format(
        surface_caps: &wgpu::SurfaceCapabilities,
        capabilities: &GpuCapabilities,
    ) -> wgpu::TextureFormat {
        // Prefer plain UNorm onscreen formats over the *Srgb variants — this
        // matches Flutter/Impeller, whose default onscreen format is plain
        // UNorm on every backend (Metal kBGRA8UNorm, Vulkan eR8G8B8A8Unorm,
        // GLES kR8G8B8A8UNormInt), *not* the sRGB variants.
        //
        // `Color::to_f32_array()` (flui-types) returns the sRGB-encoded byte
        // value `/255` with no linearization, and the shaders emit that value
        // verbatim. Writing that to a UNorm target stores the sRGB byte 1:1
        // (no OETF on store), so authored `Color::rgb(128,128,128)` -> shader
        // 0.502 -> stored byte 0x80 — exactly what the user authored, and
        // blending happens in gamma space, which is Flutter's behavior.
        //
        // An sRGB target would instead treat the shader's already-sRGB output
        // as *linear* and apply the linear->sRGB OETF on store, brightening
        // mid-tones (0x80 -> ~0xBC) and forcing linear-space blends/gradient
        // interpolation that diverge from Flutter's gamma-space lerp. Primaries
        // (0 / 255) are OETF fixed points, so the divergence hides on solid
        // black/white but corrupts every mid-tone.
        let preferred_formats = if capabilities.supports_hdr {
            vec![
                wgpu::TextureFormat::Rgba16Float, // HDR
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
            ]
        } else {
            vec![
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ]
        };

        for format in preferred_formats {
            if surface_caps.formats.contains(&format) {
                tracing::debug!("Selected surface format: {:?}", format);
                return format;
            }
        }

        // Fallback: some drivers report zero formats (e.g. headless CI).
        // Default to a universally supported UNorm format (Impeller parity,
        // see above) rather than panicking.
        if let Some(fmt) = surface_caps.formats.first().copied() {
            fmt
        } else {
            tracing::error!("surface reported zero formats; defaulting to Bgra8Unorm");
            wgpu::TextureFormat::Bgra8Unorm
        }
    }

    /// Select present mode based on capabilities.
    ///
    /// Fifo (vsync-blocked present) is the default. `render_scene`'s blocking
    /// `get_current_texture()`/`present()` pair against Fifo is the
    /// steady-state pacing mechanism for the whole frame loop: every
    /// PRESENTED frame blocks at display cadence, which is what lets
    /// `flui-app`'s runner drop its fixed frame-budget sleep in favor of a
    /// real vsync block (see the frame-pacing ADR). Mailbox (triple
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
    /// which wgpu 29 does not expose.
    fn select_present_mode(surface_caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
        debug_assert!(
            surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Fifo),
            "BUG: wgpu guarantees Fifo is always a supported present mode"
        );
        wgpu::PresentMode::Fifo
    }

    /// Resize the surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if let (Some(config), Some(surface)) = (&mut self.config, &self.surface)
            && width > 0
            && height > 0
        {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);

            if let Some(painter) = &mut self.painter {
                painter.resize(width, height);
            }

            self.damage_tracker.mark_full_repaint();

            tracing::debug!("Surface resized to {}x{}", width, height);
        }
    }

    /// Get GPU capabilities
    pub fn capabilities(&self) -> &GpuCapabilities {
        &self.capabilities
    }

    /// Get reference to wgpu device
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Get reference to wgpu queue
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Get reference to wgpu surface (if available)
    pub fn surface(&self) -> Option<&wgpu::Surface<'_>> {
        self.surface.as_ref()
    }

    /// Get current surface configuration (if available)
    pub fn surface_config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.config.as_ref()
    }

    /// Mark a screen region as dirty (needs repaint).
    pub fn mark_dirty(&mut self, rect: flui_types::geometry::Rect<flui_types::geometry::Pixels>) {
        self.damage_tracker.mark_dirty(rect);
    }

    /// Mark the entire screen as needing repaint.
    pub fn mark_full_repaint(&mut self) {
        self.damage_tracker.mark_full_repaint();
    }

    /// Check if the renderer has pending damage.
    pub fn has_damage(&self) -> bool {
        self.damage_tracker.has_damage()
    }

    /// The latest completed GPU frame profile, or `None` when the `gpu-profiler`
    /// feature is off, the adapter lacks `TIMESTAMP_QUERY`, or fewer than
    /// `PENDING_FRAME_BUFFER_DEPTH` frames have been rendered.
    ///
    /// Implements [`Diagnosticable`](flui_foundation::Diagnosticable): call
    /// `profile.to_diagnostics_node()` to get a human-readable property tree.
    #[must_use]
    pub fn latest_gpu_frame_profile(&self) -> Option<&super::profiler::GpuFrameProfile> {
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

    /// Get current surface size as `(width, height)`.
    ///
    /// Returns `(0, 0)` if no surface is configured (e.g., offscreen renderer).
    pub fn size(&self) -> (u32, u32) {
        self.config.as_ref().map_or((0, 0), |c| (c.width, c.height))
    }

    /// Reconfigure the surface after loss or outdated error.
    ///
    /// This is called automatically by `render_scene()` when
    /// `CurrentSurfaceTexture::Outdated` or `CurrentSurfaceTexture::Lost`
    /// is encountered, but can also be called manually if needed.
    pub fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        if let (Some(config), Some(surface)) = (&self.config, &self.surface) {
            surface.configure(&self.device, config);
            self.damage_tracker.mark_full_repaint();
            tracing::info!("Surface reconfigured ({}x{})", config.width, config.height);
            Ok(())
        } else {
            Err(EngineError::NotInitialized)
        }
    }

    /// Render a `flui_layer::Scene` to the surface.
    ///
    /// Traverses the scene's LayerTree depth-first, dispatching each layer's
    /// DisplayList commands through the GPU backend (WgpuPainter).
    ///
    /// For scenes containing `BackdropFilterLayer`, the render flow supports
    /// mid-frame flush: painter batches are submitted early so the surface
    /// texture can be copied, blurred, and composited before continuing.
    /// Renders `scene` and returns whether it actually reached `present()`.
    ///
    /// `Ok(false)` covers every path that skips presentation without error —
    /// no damage, or the surface reporting `Occluded` — and carries no vsync
    /// signal: Fifo's blocking present never engaged, so the caller got no
    /// pacing out of this call. `Ok(true)` means `present()` ran, which
    /// (under the default Fifo present mode) blocked until the next vsync —
    /// the steady-state pacing the frame loop relies on.
    pub fn render_scene(&mut self, scene: &flui_layer::Scene) -> Result<bool, EngineError> {
        // Fine-grained damage tracking is the caller's responsibility: the
        // application layer calls `mark_dirty()` / `mark_full_repaint()` after
        // input events or state changes. When flui-view is wired up, widgets
        // will call `mark_dirty(bounds)` on state change; until then, callers
        // use `mark_full_repaint()` to force a frame.

        // If the previous frame detected a straddling advanced shape under partial
        // damage, promote this frame to a full repaint so the shape is redrawn
        // without scissor restriction, self-healing any stale out-of-damage pixels.
        // Consumed here (set to false) so it does not propagate beyond one frame.
        if self.force_full_repaint_next_frame {
            self.force_full_repaint_next_frame = false;
            self.damage_tracker.mark_full_repaint();
            tracing::trace!(
                "force_full_repaint_next_frame: promoting to full repaint \
                 (advanced shape straddled partial damage last frame)"
            );
        }

        // Check if we need to render at all
        if !self.damage_tracker.has_damage() && !self.damage_tracker.needs_full_repaint() {
            // Nothing changed — skip this frame entirely; no present, no vsync block.
            tracing::trace!("Skipping frame: no damage");
            return Ok(false);
        }

        // Acquire the swapchain texture; returns None when the frame should be
        // skipped (Occluded), or Err for unrecoverable surface states.
        let Some(output) = self.acquire_surface_texture()? else {
            return Ok(false);
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Observability: warn when the swapchain texture diverges from the
        // configured surface size (resize transient → stretched frame).
        self.warn_on_size_mismatch(&output.texture);

        // Determine whether this frame should go through the intermediate-texture
        // path (COPY_SRC-less adapters, or forced in tests).
        //
        // When intermediate-active:
        //   - ALL frame passes (clear, backdrop-flush, final render) target
        //     `render_view`/`render_texture`, which point at the intermediate.
        //   - Only the final blit encoder writes to the real swapchain `view`.
        //   - The intermediate already has COPY_SRC|COPY_DST (all pool textures
        //     carry those usages), so backdrop-filter and advanced-blend dst-reads
        //     both work correctly.
        //
        // When NOT intermediate-active (common path on COPY_SRC-capable adapters):
        //   - `render_view`/`render_texture` point directly at the swapchain.
        //   - No intermediate texture is allocated; no blit is issued.
        //   - Behaviour is byte-identical to the pre-PR-6 code.
        let intermediate_active = self.uses_intermediate_texture();

        let surface_format = self
            .config
            .as_ref()
            .map_or(wgpu::TextureFormat::Bgra8Unorm, |c| c.format);

        // Acquire the intermediate texture when the path is active.  The pool
        // texture has RENDER_ATTACHMENT|TEXTURE_BINDING|COPY_SRC|COPY_DST, so
        // it satisfies every downstream usage without extra flags.
        let intermediate_texture_slot: Option<super::texture_pool::PooledTexture> =
            if intermediate_active {
                if let Some(offscreen) = self.offscreen.as_mut() {
                    let (surface_w, surface_h) = self
                        .config
                        .as_ref()
                        .map_or((800u32, 600u32), |c| (c.width, c.height));
                    Some(
                        offscreen
                            .texture_pool()
                            .acquire(surface_w, surface_h, surface_format),
                    )
                } else {
                    // No offscreen renderer — cannot allocate intermediate.
                    // Fall back gracefully (direct path, no advanced blend).
                    tracing::warn!(
                        "Intermediate present path requested but offscreen renderer \
                         unavailable; falling back to direct swapchain render"
                    );
                    None
                }
            } else {
                None
            };

        // Select per-frame render view/texture.  Every pass in this frame
        // (clear, backdrop-flush, final render) writes to these targets.
        // Only the blit encoder writes to the real swapchain `view`.
        let effective_intermediate_active =
            intermediate_active && intermediate_texture_slot.is_some();
        let (render_view, render_texture): (&wgpu::TextureView, &wgpu::Texture) =
            if let Some(ref slot) = intermediate_texture_slot {
                (slot.view(), slot.texture())
            } else {
                (&view, &output.texture)
            };

        // 1. Clear pass — submit immediately so the render target is ready for
        //    mid-frame copy operations (backdrop blur needs pixels on the target).
        self.run_clear_pass(render_view);

        // 2. Build render context for backdrop filter support.
        //    `surface_format` was already computed above when selecting the
        //    intermediate texture, so we reuse it here.
        let ctx = RenderContext {
            supports_copy_src: self.supports_copy_src,
            intermediate_active: effective_intermediate_active,
        };

        // 3. Render scene content via LayerTree traversal
        self.render_scene_content(scene, render_view, render_texture, &ctx);

        // If the intermediate path was active, blit the fully-rendered
        // intermediate onto the real swapchain surface now.  This is the only
        // encoder that writes to `&view` (the swapchain view); no other pass
        // above touches it when intermediate_active = true.
        //
        // The blit uses Replace/Copy blend (no blend equation) so the surface
        // is pixel-identical to a direct render.  The intermediate is released
        // back to the pool when `intermediate_texture_slot` drops at the end of
        // this function.
        if effective_intermediate_active
            && let (Some(offscreen), Some(slot)) =
                (self.offscreen.as_mut(), intermediate_texture_slot.as_ref())
        {
            offscreen.blit_to_surface(slot.texture(), &view, surface_format);
        }

        output.present();

        // Signal end of frame to the profiler and harvest the oldest completed
        // result (if the pipeline has warmed up). Both calls are no-ops when
        // `gpu_profiler` is `None`.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_mut() {
            profiler.end_frame();
            let timestamp_period = self.queue.get_timestamp_period();
            profiler.process_finished_frame(timestamp_period);
        }

        // Reset damage for next frame
        self.damage_tracker.reset();

        Ok(true)
    }

    /// Acquire the current swapchain texture, handling device-lost and all
    /// `CurrentSurfaceTexture` variants with a single retry on Outdated/Lost.
    ///
    /// Returns `Ok(None)` when the frame should be silently skipped (Occluded).
    fn acquire_surface_texture(&mut self) -> Result<Option<wgpu::SurfaceTexture>, EngineError> {
        // Check for device-lost flag (set by the device-lost callback) before
        // attempting to acquire a surface texture. If the device is gone, we
        // cannot proceed with the current device — return an error that the
        // caller can handle by recreating the renderer.
        if self.device_lost.load(std::sync::atomic::Ordering::Acquire) {
            tracing::warn!("Device lost detected; returning DeviceLost error");
            return Err(EngineError::DeviceLost);
        }

        // wgpu 28+: get_current_texture() returns CurrentSurfaceTexture enum
        // instead of Result<SurfaceTexture, SurfaceError>.
        let surface = self.surface.as_ref().ok_or(EngineError::SurfaceLost)?;
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Some(frame)),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Suboptimal is still renderable; schedule a reconfigure next frame.
                tracing::debug!("Surface suboptimal; will reconfigure on next resize");
                Ok(Some(frame))
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // Auto-reconfigure and retry once.
                self.reconfigure_surface()?;
                let surface = self.surface.as_ref().ok_or(EngineError::SurfaceLost)?;
                match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
                    _ => Err(EngineError::SurfaceLost),
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout => Err(EngineError::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => {
                // Window is minimized or fully occluded — skip this frame
                // entirely rather than producing garbage frames.
                tracing::trace!("Surface occluded; skipping frame");
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                // Validation error — the surface texture could not be produced
                // due to a validation failure (surface misconfig: incompatible
                // format/usage/present mode). This is NOT a recoverable
                // SurfaceLost: retrying `get_current_texture` without
                // reconfiguring loops forever. Log and surface a distinct
                // non-recoverable error so the caller drops the frame and
                // reconfigures on the next pass.
                tracing::error!("Surface texture validation error");
                Err(EngineError::SurfaceValidation)
            }
        }
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
        if let Some(config) = self.config.as_ref() {
            let attachment = (output_texture.width(), output_texture.height());
            let configured = (config.width, config.height);
            if attachment != configured {
                tracing::warn!(
                    ?attachment,
                    ?configured,
                    "render_scene: swapchain texture size != configured surface size \
                     (resize transient — frame will present stretched)"
                );
            }
        }
    }

    /// Submit the clear render pass, cleaning the render target before scene
    /// traversal so backdrop-blur mid-frame copies see a cleared surface.
    fn run_clear_pass(&mut self, render_view: &wgpu::TextureView) {
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
                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
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
        self.queue.submit(std::iter::once(clear_encoder.finish()));
    }

    /// Traverse the scene's layer tree and flush all painter batches to the GPU,
    /// including the damage-straddle self-heal check and final encoder submission.
    fn render_scene_content(
        &mut self,
        scene: &flui_layer::Scene,
        render_view: &wgpu::TextureView,
        render_texture: &wgpu::Texture,
        ctx: &RenderContext,
    ) {
        use super::backend::Backend;

        if !scene.has_content() {
            return;
        }
        let Some(mut painter) = self.painter.take() else {
            return;
        };

        let mut backend = if let Some(ref mut offscreen) = self.offscreen {
            Backend::with_offscreen(&mut painter, offscreen)
        } else {
            Backend::new(&mut painter)
        };
        // Bind the frame render target so the DisplayList-level
        // `render_backdrop_filter` path can flush + blur the same
        // target the layer-level path uses.
        // When intermediate-active, `render_view`/`render_texture` point
        // at the intermediate; otherwise they point at the swapchain.
        // Without this bind, that command path falls back to passthrough
        // — a visible regression vs Flutter.
        backend.bind_surface(render_view, render_texture);

        // Reset per-frame clip/transform/opacity/layer state so that
        // partial-damage scissors from frame N cannot leak into frame N+1.
        // This must happen BEFORE the damage clip_rect below.
        backend.painter_mut().reset_frame_state();

        // Apply damage rect as scissor optimization: when only part of the
        // screen changed, limit GPU work to the damaged region.
        // `damage_rect()` returns `None` for full repaint (no scissor needed),
        // `Some(rect)` for partial damage.
        //
        // We capture `partial_damage` separately: after `render_layer_recursive`
        // populates `draw_order`, we check whether any advanced shape (or SSAA
        // path with an advanced blend) straddles the damage edge.  If so, we
        // schedule a full repaint next frame to self-heal stale pixels outside
        // the damage rect that `flush_advanced_layer` may have written.
        let partial_damage = self
            .damage_tracker
            .damage_rect()
            .filter(|r| r.width().0 > 0.0 && r.height().0 > 0.0);
        if let Some(damage) = partial_damage {
            backend.painter_mut().clip_rect(damage);
            tracing::trace!(
                left = damage.left().0,
                top = damage.top().0,
                width = damage.width().0,
                height = damage.height().0,
                "Damage scissor applied"
            );
        }

        // Depth-first traversal of layer tree.
        // `render_texture`/`render_view` point at the intermediate when
        // intermediate-active, or directly at the swapchain otherwise.
        // Backdrop-filter and advanced-blend passes read from
        // `render_texture`, which always has COPY_SRC in this context.
        if let Some(root_id) = scene.root() {
            Self::render_layer_recursive(
                scene.layer_tree(),
                scene.link_registry(),
                root_id,
                &mut backend,
                ctx,
                render_texture,
                render_view,
            );
        }

        // Damage-straddle self-healing: if a partial scissor was applied AND
        // `draw_order` now contains an advanced shape whose `device_bounds`
        // straddle the damage edge, schedule a full repaint for the next frame.
        //
        // Why next-frame and not this-frame: `render_layer_recursive` has
        // already populated the draw commands with the scissored geometry; a
        // this-frame re-record would require replaying the entire scene graph.
        // Partial damage is currently unused (callers use `mark_full_repaint`),
        // so the transient stale pixel is unobservable.  A precomputed Scene
        // bit or a re-record is the future upgrade path if partial damage
        // becomes a hot path.
        if let Some(damage) = partial_damage
            && backend.painter().has_advanced_shape_straddling(damage)
        {
            self.force_full_repaint_next_frame = true;
            tracing::debug!(
                left = damage.left().0,
                top = damage.top().0,
                width = damage.width().0,
                height = damage.height().0,
                "Advanced shape straddles partial damage; \
                 scheduling full repaint next frame"
            );
        }

        // 5. Final flush — submit remaining painter batches.
        // Drop the Backend first: Drop calls flush_active_transform(), which
        // balances any deferred lazy-coalescing save left by `with_transform`.
        // Once `backend` is dropped the exclusive borrow on `painter` ends, so
        // `painter` is directly accessible for the render and maintenance calls.
        drop(backend);
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
        //   - Common path (supports_copy_src=true, intermediate_active=false):
        //     `render_texture` = `output.texture` which has COPY_SRC.
        //   - Intermediate path (intermediate_active=true):
        //     `render_texture` = intermediate which has COPY_SRC|COPY_DST.
        // Both cases satisfy the dst-read contract required by advanced blend
        // and backdrop-filter.  The `view_only` fallback in `flush_opacity_layer`
        // is only reached from benches/tests that construct a bare TextureView
        // without a backing texture — see the reshaped fallback comments there.
        let frame_target =
            super::render_target::RenderTarget::sampleable(render_view, render_texture);
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
        if let Err(e) = render_result {
            tracing::error!("Painter render failed: {}", e);
        }
        // Resolve before finishing the encoder.
        #[cfg(feature = "gpu-profiler")]
        if let Some(profiler) = self.gpu_profiler.as_mut() {
            profiler.resolve_queries(&mut final_encoder);
        }
        self.queue.submit(std::iter::once(final_encoder.finish()));

        // Frame boundary: run texture-cache maintenance ONCE, after the
        // final flush. `painter.render` runs per-pass (backdrop-filter
        // flushes call it mid-frame), so maintenance lives here — not inside
        // `render` — to avoid resetting use-counters between passes.
        painter.end_frame_maintenance();

        // Return painter to Renderer for reuse
        self.painter = Some(painter);
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
    /// is resolved against `link_registry` and the already-fully-built
    /// `tree` before its children render.
    ///
    /// # Occlusion culling
    ///
    /// No per-layer opaque culling is performed here. A back-to-front walk
    /// registers bottom layers first and would see later (on-top, visible)
    /// layers as "occluded" — exactly backwards. A sound front-to-back cull
    /// requires a separate pre-pass that is a future optimization opportunity.
    fn render_layer_recursive(
        tree: &flui_layer::LayerTree,
        link_registry: &flui_layer::LinkRegistry,
        layer_id: flui_foundation::LayerId,
        backend: &mut super::backend::Backend<'_>,
        ctx: &RenderContext,
        surface_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
    ) {
        use super::layer_render::LayerRender;

        let Some(node) = tree.get(layer_id) else {
            return;
        };

        let layer = node.layer();

        // Special handling for BackdropFilter — requires mid-frame flush + copy.
        //
        // The gate passes when EITHER:
        //   - The swapchain surface itself has COPY_SRC (common path), OR
        //   - The intermediate texture is active (COPY_SRC-less adapter path):
        //     `surface_texture` points at the intermediate which always has COPY_SRC.
        if let flui_layer::Layer::BackdropFilter(bf_layer) = layer
            && (ctx.supports_copy_src || ctx.intermediate_active)
        {
            Self::handle_backdrop_filter(
                bf_layer,
                node,
                tree,
                link_registry,
                backend,
                ctx,
                surface_texture,
                surface_view,
            );
            return;
        }

        // Special handling for ShaderMask — captures children to an offscreen
        // texture, applies the shader as a GPU mask, then composites the
        // masked result. Requires an `OffscreenRenderer`; falls through to
        // the inert clip/save-layer `LayerRender<ShaderMaskLayer>` impl
        // (unmasked passthrough) when one isn't available, mirroring
        // `BackdropFilter`'s own non-`Blur` degrade above.
        if let flui_layer::Layer::ShaderMask(sm_layer) = layer
            && backend.offscreen_mut().is_some()
        {
            Self::handle_shader_mask(sm_layer, node, tree, link_registry, backend, ctx);
            return;
        }

        // Special handling for Follower — resolve its render-time position
        // (leader pose, or the plain unlinked fallback) before descending
        // into children; hide the subtree entirely when unlinked with
        // `show_when_unlinked == false` (oracle `FollowerLayer.addToScene`,
        // `layer.dart:2857-2865`).
        if let flui_layer::Layer::Follower(follower_layer) = layer {
            if let Some(resolved) =
                flui_layer::resolve_follower_offset(tree, link_registry, layer_id, follower_layer)
            {
                use crate::traits::LayerStateStack;

                let has_offset = resolved != flui_types::geometry::Offset::ZERO;
                if has_offset {
                    backend.push_offset(resolved);
                }
                for &child_id in node.children() {
                    Self::render_layer_recursive(
                        tree,
                        link_registry,
                        child_id,
                        backend,
                        ctx,
                        surface_texture,
                        surface_view,
                    );
                }
                if has_offset {
                    backend.pop_transform();
                }
            }
            return;
        }
        // Fall through to normal LayerRender path (clip + filter fallback)

        // Normal path: render → children → cleanup
        layer.render(backend);

        // Borrow children as a slice of Copy values; re-borrow `tree` inside the
        // call is shared and does not conflict with this shared borrow of `node`.
        for &child_id in node.children() {
            Self::render_layer_recursive(
                tree,
                link_registry,
                child_id,
                backend,
                ctx,
                surface_texture,
                surface_view,
            );
        }

        layer.cleanup(backend);
    }

    /// Handle a `BackdropFilterLayer` via mid-frame flush and Dual Kawase blur.
    ///
    /// Flow:
    /// 1. Flush current painter batches to the surface
    /// 2. Copy the backdrop region from the surface to an offscreen texture
    /// 3. Apply Dual Kawase blur via `OffscreenRenderer::render_blur`
    /// 4. Queue blurred result for compositing back to the surface
    /// 5. Render children on top
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::too_many_arguments,
        reason = "backdrop-filter pipeline needs the surface texture/view and layer-tree context to do its job — splitting these into a helper struct adds indirection without clarity"
    )]
    fn handle_backdrop_filter(
        bf_layer: &flui_layer::BackdropFilterLayer,
        node: &flui_layer::tree::LayerNode,
        tree: &flui_layer::LayerTree,
        link_registry: &flui_layer::LinkRegistry,
        backend: &mut super::backend::Backend<'_>,
        ctx: &RenderContext,
        surface_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
    ) {
        use flui_types::painting::ImageFilter;

        let bounds = bf_layer.bounds();

        // Extract sigma from blur filter; other filter types fall back to
        // normal child rendering (no GPU blur support yet).
        let sigma = if let ImageFilter::Blur { sigma_x, sigma_y } = bf_layer.filter() {
            f32::midpoint(*sigma_x, *sigma_y)
        } else {
            tracing::warn!(
                "Backdrop filter type not supported for GPU blur, rendering children only"
            );
            for &child_id in node.children() {
                Self::render_layer_recursive(
                    tree,
                    link_registry,
                    child_id,
                    backend,
                    ctx,
                    surface_texture,
                    surface_view,
                );
            }
            return;
        };

        // Map the layer's local-space `bounds` to a device-space rect using the
        // accumulated layer-walk CTM (the `RenderView` root `scale(dpr)` plus
        // every intervening transform/offset layer, carried in the painter's
        // `current_transform`). This is the layer-tree equivalent of the
        // `transform` argument Path B (`Backend::render_backdrop_filter`)
        // receives. The shared `apply_backdrop_blur` then clamps + copies +
        // blurs + composites (the off-screen-clamp logic lives there once, so it
        // can't drift between the two backdrop paths). A `false` return (no
        // offscreen renderer, or fully off-screen) just means no blur — children
        // still render below.
        let device_rect = backend
            .painter()
            .current_transform_matrix()
            .transform_rect(&bounds);
        backend.apply_backdrop_blur(device_rect, sigma, surface_texture, surface_view);

        // Render children on top of the (maybe-)blurred backdrop. No push/pop
        // state to clean up in this path.
        for &child_id in node.children() {
            Self::render_layer_recursive(
                tree,
                link_registry,
                child_id,
                backend,
                ctx,
                surface_texture,
                surface_view,
            );
        }
    }

    /// Handle a `ShaderMaskLayer` subtree by capturing its children to a
    /// private offscreen texture, applying the layer's shader as a GPU mask
    /// against that capture, then compositing the masked result onto the
    /// main render target.
    ///
    /// Data flow is the OPPOSITE of [`handle_backdrop_filter`](Self::handle_backdrop_filter):
    /// that path blurs content ALREADY on the surface and renders children
    /// on top unmodified; this path renders children into a private
    /// offscreen texture FIRST, masks that capture, then composites the
    /// masked result. Mirrors the six-step "capture subtree → mask →
    /// composite" pipeline [`crate::traits::CommandRenderer::render_shader_mask`]
    /// already runs for the `DisplayList` path (`Canvas::draw_shader_mask`),
    /// adapted to recurse into a `LayerTree` subtree instead of dispatching a
    /// flat command list.
    ///
    /// # Coordinate frame — do not copy `render_shader_mask`'s DPR-only reset
    ///
    /// [`ShaderMaskLayer::bounds`](flui_layer::ShaderMaskLayer::bounds) is
    /// expressed in the same ambient-CTM-relative frame the layer walk has
    /// already accumulated by the time
    /// this node is reached — the same frame `handle_backdrop_filter` reads
    /// `bounds()` against. The offscreen texture is sized to `bounds`'
    /// device-space extent but its OWN local frame starts at `device_bounds`'
    /// origin, so the offscreen painter's transform must be seeded with
    /// `translate(-device_bounds.origin) * ambient_ctm` — NOT reset to
    /// DPR-scale-only the way `render_shader_mask`'s `DisplayList` path
    /// does. That reset is correct there only because its children are
    /// recorded into a FRESH, self-relative `Canvas` (`Canvas::new()`), never
    /// into the ambient-CTM-relative `LayerTree`. Reusing it here would
    /// render children at their absolute device-space position instead of
    /// shifted into the texture's own coordinate window — silently
    /// mis-positioning (or entirely clipping away) any `ShaderMask` whose
    /// `bounds()` origin isn't `(0, 0)`.
    ///
    /// # Scope
    ///
    /// Only reached when `backend.offscreen_mut().is_some()` (checked by the
    /// caller); the no-offscreen-renderer degrade is the existing inert
    /// clip/save-layer `LayerRender<ShaderMaskLayer>` impl in
    /// `layer_render.rs` (unmasked passthrough). The temporary `Backend`
    /// wrapping the offscreen painter is built via `Backend::new` (no
    /// `OffscreenRenderer`), so a `ShaderMask`/`BackdropFilter` nested inside
    /// this layer's own children gracefully degrades to unmasked/unblurred —
    /// the same precedented limitation `render_shader_mask`'s `DisplayList`
    /// path already has (`backend.rs`, "no OffscreenRenderer, rendering child
    /// without mask").
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "device-pixel dimensions are `.max(1.0)`-clamped before the cast and bounded by GPU texture-size limits — matches the identical cast pattern already reviewed in `Backend::render_shader_mask`"
    )]
    fn handle_shader_mask(
        sm_layer: &flui_layer::ShaderMaskLayer,
        node: &flui_layer::tree::LayerNode,
        tree: &flui_layer::LayerTree,
        link_registry: &flui_layer::LinkRegistry,
        backend: &mut super::backend::Backend<'_>,
        ctx: &RenderContext,
    ) {
        use crate::traits::LayerStateStack;
        use flui_types::geometry::{Pixels, Size};

        let bounds = sm_layer.bounds();
        let shader = sm_layer.shader();
        let blend_mode = sm_layer.blend_mode();

        // Live ambient CTM/DPR, read exactly as `handle_backdrop_filter` and
        // `Backend::render_shader_mask` do — before anything below could
        // mutate the real painter's transform state.
        let ambient_ctm = backend.painter().current_transform_matrix();
        let dpr_scale = backend.painter().current_max_scale().max(1.0);

        // Device-resolution offscreen dimensions: logical extent x DPR.
        let dev_width = (bounds.width().0 * dpr_scale).round().max(1.0) as u32;
        let dev_height = (bounds.height().0 * dpr_scale).round().max(1.0) as u32;

        // Composite rect in device space — the layer-tree equivalent of
        // `Backend::render_shader_mask`'s `device_bounds`.
        let device_bounds = ambient_ctm.transform_rect(&bounds);

        // Step 1-3: acquire GPU handles and a device-sized pooled child
        // texture from the offscreen renderer (caller already confirmed
        // `Some`).
        let (device, queue, format, child_tex) = {
            let offscreen = backend
                .offscreen_mut()
                .expect("gated by caller: offscreen_mut().is_some()");
            let device = Arc::clone(offscreen.device());
            let queue = Arc::clone(offscreen.queue());
            let format = offscreen.surface_format();
            let child_tex = offscreen
                .texture_pool()
                .acquire(dev_width, dev_height, format);
            (device, queue, format, child_tex)
        };

        // Step 4-8: render this layer's children into the offscreen texture
        // through a temporary Backend, seeded with the coordinate-frame-
        // correct transform (see doc comment above).
        {
            let offscreen_painter = backend.get_or_create_offscreen_painter(
                &device,
                &queue,
                format,
                (dev_width, dev_height),
            );
            offscreen_painter.reset_frame_state();

            let mut temp_backend = super::backend::Backend::new(offscreen_painter);

            let mut seed_transform = ambient_ctm;
            seed_transform.translate(-device_bounds.left().0, -device_bounds.top().0, 0.0);
            temp_backend.push_transform(&seed_transform);

            for &child_id in node.children() {
                Self::render_layer_recursive(
                    tree,
                    link_registry,
                    child_id,
                    &mut temp_backend,
                    ctx,
                    child_tex.texture(),
                    child_tex.view(),
                );
            }
            temp_backend.pop_transform();
            // temp_backend drops here -> Drop calls flush_active_transform(),
            // balancing the push_transform save before the re-borrow below.
        }

        // Step 9: flush the offscreen painter's batches into the pooled
        // child texture (clear pass + render), exactly as
        // `Backend::render_shader_mask` does for the `DisplayList` path.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShaderMask Layer Child Render"),
        });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ShaderMask Layer Child Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: child_tex.view(),
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let child_target =
            super::render_target::RenderTarget::sampleable(child_tex.view(), child_tex.texture());
        let offscreen_painter = backend.get_or_create_offscreen_painter(
            &device,
            &queue,
            format,
            (dev_width, dev_height),
        );
        if let Err(e) = offscreen_painter.render(child_target, &mut encoder) {
            tracing::error!("Failed to render ShaderMask layer child content: {}", e);
        }
        queue.submit(std::iter::once(encoder.finish()));

        // Step 10-11: apply the shader as a GPU mask against the captured
        // child content, then queue the masked result for compositing on
        // the main target at the device-space rect.
        let result_size = Size::new(Pixels(dev_width as f32), Pixels(dev_height as f32));
        let masked_texture = {
            let offscreen = backend
                .offscreen_mut()
                .expect("checked is_some at function entry");
            let result = offscreen.render_masked(
                bounds,
                result_size,
                shader,
                blend_mode,
                child_tex.texture(),
            );
            result.into_texture()
        };

        backend
            .painter_mut()
            .queue_offscreen_result(masked_texture, device_bounds);

        tracing::debug!(
            "ShaderMask layer GPU pipeline complete: bounds={:?}, device_bounds={:?}, \
             dpr_scale={}, child_size={}x{}",
            bounds,
            device_bounds,
            dpr_scale,
            dev_width,
            dev_height
        );
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    #[test]
    fn test_backend_selection() {
        let backend = Renderer::select_backend();

        #[cfg(target_os = "macos")]
        assert_eq!(backend, wgpu::Backends::METAL);

        #[cfg(target_os = "windows")]
        assert_eq!(backend, wgpu::Backends::DX12);

        #[cfg(target_os = "linux")]
        assert_eq!(backend, wgpu::Backends::VULKAN);

        #[cfg(target_os = "android")]
        assert_eq!(backend, wgpu::Backends::VULKAN);

        #[cfg(target_arch = "wasm32")]
        assert!(backend.contains(wgpu::Backends::BROWSER_WEBGPU));
    }

    #[test]
    fn test_vendor_names() {
        assert_eq!(GpuCapabilities::vendor_name(0x1002), "AMD");
        assert_eq!(GpuCapabilities::vendor_name(0x10DE), "NVIDIA");
        assert_eq!(GpuCapabilities::vendor_name(0x8086), "Intel");
        assert_eq!(GpuCapabilities::vendor_name(0x106B), "Apple");
    }

    #[test]
    fn test_offscreen_renderer() {
        // This test may fail in CI without GPU. Driven via `pollster` to match
        // the rest of this crate's async tests (no tokio-macros dependency).
        pollster::block_on(async {
            if let Ok(renderer) = Renderer::new_offscreen().await {
                assert!(renderer.surface.is_none());
                assert!(renderer.config.is_none());
                assert!(!renderer.capabilities.adapter_name.is_empty());
            }
        });
    }

    /// Verify that `recover()` on an offscreen renderer:
    ///   1. Starts with `is_device_lost() == false`.
    ///   2. Reports `true` after the flag is set manually.
    ///   3. Returns to `false` after `recover()` (fresh device = fresh flag).
    ///   4. The recovered device is functional (buffer creation + empty submit).
    ///
    /// # Limitation
    ///
    /// wgpu has no public API to force a real device loss programmatically, so
    /// we simulate the flag being set by the driver callback by storing directly
    /// into the `Arc<AtomicBool>`. The windowed surface-rebuild path (raw handle
    /// → surface → adapter → device) cannot be unit-tested without a real window
    /// and a real GPU loss event; it is covered by compilation + code review only.
    #[test]
    fn offscreen_recover_clears_device_lost_flag() {
        pollster::block_on(async {
            let Ok(mut renderer) = Renderer::new_offscreen().await else {
                // No GPU in this environment (common in CI); skip gracefully.
                return;
            };

            // Precondition: flag starts clear on a healthy device.
            assert!(
                !renderer.is_device_lost(),
                "device_lost must be false on a freshly created offscreen renderer"
            );

            // Simulate the device-lost callback firing (wgpu sets this flag when
            // a real loss occurs; we set it directly because wgpu exposes no API
            // to force a device loss in tests).
            renderer
                .device_lost
                .store(true, std::sync::atomic::Ordering::Release);
            assert!(
                renderer.is_device_lost(),
                "flag must read true after simulated device loss"
            );

            // Recover — this builds a fresh device with a fresh flag.
            match renderer.recover().await {
                Ok(()) => {}
                Err(_) => {
                    // recover() can fail when the adapter is unavailable (e.g.
                    // software rasterizer CI). That's expected; the important
                    // property is that the flag is reset on *success*.
                    return;
                }
            }

            // Post-condition: fresh device, fresh flag.
            assert!(
                !renderer.is_device_lost(),
                "is_device_lost() must be false after a successful recover()"
            );

            // Verify the recovered device is functional: create a tiny buffer and
            // submit an empty command encoder without panicking.
            let _buf = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("recovery-probe"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let encoder = renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("recovery-probe-encoder"),
                });
            renderer.queue.submit(std::iter::once(encoder.finish()));
        });
    }

    /// Acquire a real device/queue for the HiDPI backdrop regression below.
    /// Returns `None` when no GPU adapter is available (CI without a GPU).
    fn test_device_and_queue() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Backdrop HiDPI Test Device"),
            ..Default::default()
        }))
        .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    }

    /// BUG 1 (HiDPI backdrop "Path A"): the layer-tree backdrop path must map
    /// the layer's logical `bounds` through the accumulated CTM (which carries
    /// the `RenderView` `scale(dpr)`) before sampling/compositing. Under a
    /// `scale(2)` CTM a backdrop at logical (100,100,200,200) must sample and
    /// composite the device rect (200,200,400,400), not the logical rect.
    ///
    /// Drives the real `Renderer::handle_backdrop_filter` (not a reimpl) with a
    /// synthetic surface texture and asserts the queued offscreen composite rect
    /// is the device rect. Red before the fix (logical (100,100,200,200)).
    #[test]
    fn backdrop_filter_path_a_composites_at_device_rect_under_dpr() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_layer::{BackdropFilterLayer, Layer, LayerTree};
        use flui_types::{
            geometry::{Rect, px},
            painting::ImageFilter,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            // No GPU in this environment; skip gracefully (matches the other
            // GPU tests in this module).
            return;
        };

        // Surface format used for the synthetic surface + offscreen pool. A
        // UNorm format with COPY_SRC|COPY_DST|RENDER_ATTACHMENT|TEXTURE_BINDING
        // is what the real surface uses (see `Renderer::new`).
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let surface_w = 800u32;
        let surface_h = 800u32;

        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backdrop HiDPI Test Surface"),
            size: wgpu::Extent3d {
                width: surface_w,
                height: surface_h,
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // Simulate the `RenderView` DPR root transform: scale(2) on the CTM.
        backend.painter_mut().scale(2.0, 2.0);

        // Build a one-node layer tree: a leaf BackdropFilter with no children.
        let mut tree = LayerTree::new();
        let logical_bounds = Rect::from_xywh(px(100.0), px(100.0), px(200.0), px(200.0));
        let bf = BackdropFilterLayer::new(
            ImageFilter::blur(5.0),
            flui_types::painting::BlendMode::SrcOver,
            logical_bounds,
        );
        let id = tree.insert(Layer::BackdropFilter(bf));
        tree.set_root(Some(id));
        let node = tree.get(id).expect("inserted backdrop node");
        let Layer::BackdropFilter(bf_layer) = node.layer() else {
            unreachable!("inserted a BackdropFilter layer");
        };

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &flui_layer::LinkRegistry::new(),
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
        );

        // The blurred backdrop must be queued for compositing at the DEVICE
        // rect — logical bounds (x=100, y=100, w=200, h=200) under scale(2)
        // maps to (x=200, y=200, w=400, h=400), i.e. corners (200,200)→(600,600).
        // The bug would leave the logical rect (corners (100,100)→(300,300)).
        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "backdrop must queue exactly one offscreen composite"
        );
        let (composite_rect, _tw, _th) = results[0];
        assert!(
            (composite_rect.left().0 - 200.0).abs() < 0.5
                && (composite_rect.top().0 - 200.0).abs() < 0.5
                && (composite_rect.width().0 - 400.0).abs() < 0.5
                && (composite_rect.height().0 - 400.0).abs() < 0.5,
            "backdrop composite rect must be the device rect (x=200,y=200,w=400,h=400) \
             under DPR=2; got {composite_rect:?} (logical (x=100,y=100,w=200,h=200) means \
             the DPR transform was dropped)"
        );
    }

    /// Locks that `handle_backdrop_filter` honours CTM TRANSLATION, not just
    /// scale. Pure-scale(2) is sufficient to catch the "dropped DPR" bug but
    /// insufficient to catch "translation eaten by the CTM reader".
    ///
    /// CTM: scale(2) THEN translate(+10, +10) (post-multiply order).
    /// The accumulated matrix maps (x,y) → (2x+20, 2y+20).
    /// Logical bounds (100,100)→(300,300) device-map to (220,220)→(620,620):
    ///   left  = 2*100 + 20 = 220
    ///   top   = 2*100 + 20 = 220
    ///   right = 2*300 + 20 = 620  →  width  = 400
    ///   bottom= 2*300 + 20 = 620  →  height = 400
    ///
    /// A regression that drops the translation but keeps the scale would give
    /// (200,200,400,400) with the position wrong at (200,200) instead of (220,220).
    #[test]
    fn backdrop_filter_path_a_honors_translation_under_dpr() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_layer::{BackdropFilterLayer, Layer, LayerTree};
        use flui_types::{
            geometry::{Offset, Rect, px},
            painting::ImageFilter,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return;
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;
        let surface_w = 1000u32;
        let surface_h = 1000u32;

        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backdrop Translation Test Surface"),
            size: wgpu::Extent3d {
                width: surface_w,
                height: surface_h,
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // CTM: scale(2) then translate(+10,+10).
        // Maps (x,y) → (2x+20, 2y+20).
        backend.painter_mut().scale(2.0, 2.0);
        backend
            .painter_mut()
            .translate(Offset::new(px(10.0), px(10.0)));

        let mut tree = LayerTree::new();
        let logical_bounds = Rect::from_xywh(px(100.0), px(100.0), px(200.0), px(200.0));
        let bf = BackdropFilterLayer::new(
            ImageFilter::blur(5.0),
            flui_types::painting::BlendMode::SrcOver,
            logical_bounds,
        );
        let id = tree.insert(Layer::BackdropFilter(bf));
        tree.set_root(Some(id));
        let node = tree.get(id).expect("inserted backdrop node");
        let Layer::BackdropFilter(bf_layer) = node.layer() else {
            unreachable!("inserted a BackdropFilter layer");
        };

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &flui_layer::LinkRegistry::new(),
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "backdrop must queue exactly one offscreen composite"
        );
        let (composite_rect, _tw, _th) = results[0];

        // Expected device rect corners: (220,220)→(620,620); w=h=400.
        // A translation-drop regression gives (200,200) position (not 220).
        assert!(
            (composite_rect.left().0 - 220.0).abs() < 0.5,
            "composite rect left must be ~220.0 (2*100+20); got {:.2} \
             (translation was likely dropped from CTM)",
            composite_rect.left().0
        );
        assert!(
            (composite_rect.top().0 - 220.0).abs() < 0.5,
            "composite rect top must be ~220.0 (2*100+20); got {:.2}",
            composite_rect.top().0
        );
        assert!(
            (composite_rect.width().0 - 400.0).abs() < 0.5,
            "composite rect width must be ~400.0; got {:.2}",
            composite_rect.width().0
        );
        assert!(
            (composite_rect.height().0 - 400.0).abs() < 0.5,
            "composite rect height must be ~400.0; got {:.2}",
            composite_rect.height().0
        );
    }

    /// Locks P2 #2: `handle_backdrop_filter` must composite the blurred texture
    /// at the CLAMPED device rect, not the unclamped `device_rect`.
    ///
    /// When a backdrop layer extends beyond the window boundary, the copy source
    /// and blur texture are sized at the CLAMPED extent (the portion that fits
    /// on screen). Before this fix, `queue_offscreen_result` received the
    /// unclamped `device_rect` — the blurred texture (w×h) would be stretched
    /// or misaligned across the larger rect that extends off-screen.
    ///
    /// Scenario: surface = 400×400, CTM = identity (DPR=1 for simplicity —
    /// device coords == logical coords). Backdrop layer at logical
    /// (350, 350, 200, 200) i.e. corners (350,350)→(550,550).
    /// Clamped: x=350, y=350, right=400, bottom=400 → w=50, h=50.
    ///
    /// Red-before: composite rect is (350,350,200,200) — the unclamped device
    ///   rect width/height (200,200 from the layer bounds), not the clamped (50,50).
    /// Green-after: composite rect is (350,350,50,50) — matches copy origin/extent.
    #[test]
    fn backdrop_filter_path_a_composites_clamped_rect_when_partially_offscreen() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_layer::{BackdropFilterLayer, Layer, LayerTree};
        use flui_types::{
            geometry::{Rect, px},
            painting::ImageFilter,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return;
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;

        // Small surface so the layer extends off the right/bottom edge.
        let surface_w = 400u32;
        let surface_h = 400u32;

        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backdrop Clamp Test Surface"),
            size: wgpu::Extent3d {
                width: surface_w,
                height: surface_h,
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        // CTM is identity (scale=1, DPR=1): device coords == logical coords.
        // Backdrop at (350,350,200,200) — corners (350,350)→(550,550).
        // Clamped to 400×400 surface: x=350,y=350,right=400,bottom=400 →
        // w=50, h=50.
        let mut tree = LayerTree::new();
        let logical_bounds = Rect::from_xywh(px(350.0), px(350.0), px(200.0), px(200.0));
        let bf = BackdropFilterLayer::new(
            ImageFilter::blur(5.0),
            flui_types::painting::BlendMode::SrcOver,
            logical_bounds,
        );
        let id = tree.insert(Layer::BackdropFilter(bf));
        tree.set_root(Some(id));
        let node = tree.get(id).expect("inserted backdrop node");
        let Layer::BackdropFilter(bf_layer) = node.layer() else {
            unreachable!("inserted a BackdropFilter layer");
        };

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &flui_layer::LinkRegistry::new(),
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
        );

        let results = backend.painter().offscreen_results_for_test();
        assert_eq!(
            results.len(),
            1,
            "backdrop must queue exactly one offscreen composite"
        );
        let (composite_rect, tex_w, tex_h) = results[0];

        // Blur texture must be the CLAMPED size, not the full logical size.
        assert_eq!(
            (tex_w, tex_h),
            (50, 50),
            "blur texture must be clamped size 50×50; got {tex_w}×{tex_h} — \
             200×200 indicates the copy was sourced at the unclamped region"
        );

        // Composite rect origin must match the clamped origin (350,350) and
        // extent must be the clamped size (50,50), NOT the unclamped (200,200).
        // Before the fix, width/height would be 200 (device_rect was passed
        // to queue_offscreen_result instead of clamped_composite_rect).
        assert!(
            (composite_rect.left().0 - 350.0).abs() < 0.5,
            "composite rect left must be ~350.0 (clamped origin); got {:.2}",
            composite_rect.left().0
        );
        assert!(
            (composite_rect.top().0 - 350.0).abs() < 0.5,
            "composite rect top must be ~350.0 (clamped origin); got {:.2}",
            composite_rect.top().0
        );
        assert!(
            (composite_rect.width().0 - 50.0).abs() < 0.5,
            "composite rect width must be ~50.0 (clamped extent); got {:.2} — \
             200.0 indicates the unclamped device_rect was passed to \
             queue_offscreen_result (blurred texture would be stretched)",
            composite_rect.width().0
        );
        assert!(
            (composite_rect.height().0 - 50.0).abs() < 0.5,
            "composite rect height must be ~50.0 (clamped extent); got {:.2}",
            composite_rect.height().0
        );
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

        let mapped = staging_buffer.slice(..4).get_mapped_range();
        let bytes: [u8; 4] = mapped[..4].try_into().ok()?;
        Some(bytes)
    }

    /// C2: `blit_to_surface` uses Replace (no blend), not SrcOver.
    ///
    /// A semi-transparent intermediate (50 % red: `[128, 0, 0, 128]`) is
    /// blitted onto a surface texture.  `blit_to_surface` issues
    /// `LoadOp::Clear(BLACK)` before the draw, so the effective background the
    /// blend sees is black.
    ///
    /// - **Replace** (correct): the surface texel becomes `[128, 0, 0, 128]`
    ///   verbatim — the intermediate pixel is copied with no compositing.
    /// - **SrcOver** (wrong): premultiplied SrcOver of `[64, 0, 0, 128]` over
    ///   black `[0, 0, 0, 255]` gives `[64, 0, 0, 255]` — different alpha.
    ///
    /// This test distinguishes the two outcomes; the previous opaque-red test
    /// could not because SrcOver of an opaque src equals the src itself.
    #[test]
    fn intermediate_blit_transfers_pixels_correctly() {
        use super::super::offscreen::OffscreenRenderer;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 64u32;
        let height = 64u32;

        // Semi-transparent intermediate: 50% red in Rgba8Unorm straight form.
        // Replace → surface gets [128, 0, 0, 128].
        // SrcOver of premul [64, 0, 0, 128] over black → [64, 0, 0, 255]. Different alpha!
        let semi_transparent_red = wgpu::Color {
            r: 128.0 / 255.0,
            g: 0.0,
            b: 0.0,
            a: 128.0 / 255.0,
        };

        let intermediate = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C2 Intermediate Texture"),
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
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("C2 Semi-Transparent Clear Encoder"),
            });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("C2 Semi-Transparent Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &intermediate_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(semi_transparent_red),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // _pass drops here, releasing the borrow on enc.
            }
            queue.submit(std::iter::once(enc.finish()));
        }

        // Readback the intermediate pixel to get the exact stored value after
        // the clear pass (may differ from 128 due to driver rounding).
        let intermediate_pixel = readback_rgba_pixel(&device, &queue, &intermediate, 0, 0)
            .expect("intermediate readback must succeed");

        // Create the surface — the blit destination.
        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C2 Surface Texture"),
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        offscreen.blit_to_surface(&intermediate, &surface_view, format);

        let pixel = readback_rgba_pixel(&device, &queue, &surface_texture, 0, 0)
            .expect("surface readback must succeed");

        // Replace: surface pixel == intermediate pixel verbatim.
        // SrcOver would produce a different alpha (255 instead of the original).
        assert_eq!(
            pixel, intermediate_pixel,
            "blit_to_surface must copy the intermediate pixel verbatim (Replace, no blend); \
             intermediate={intermediate_pixel:?}, got={pixel:?}. \
             alpha=255 when intermediate alpha<255 indicates SrcOver compositing (wrong). \
             black=[0,0,0,255] indicates the blit draw did not execute."
        );
        // Additionally: the alpha must NOT be 255 (SrcOver over black collapses alpha).
        assert_ne!(
            pixel[3], 255u8,
            "Replace blit must preserve the semi-transparent alpha; got alpha={} (expected ~128). \
             alpha=255 indicates SrcOver compositing occurred instead of Replace.",
            pixel[3]
        );
    }

    // =========================================================================
    // C3 — common-path byte-identity after blit
    //
    // Proves that blitting a solid-color intermediate into a surface gives the
    // same pixel as clearing the surface directly to that same color.  If the
    // blit pipeline introduced any color-space re-encoding, blending, or
    // gamma shift, the pixels would differ.
    // =========================================================================

    /// C3: A solid-color intermediate blitted to a surface gives the same
    /// pixel as clearing the surface directly to that color.
    ///
    /// Failure here means the blit pipeline re-encodes or composites instead
    /// of copying (e.g., sRGB double-encoding, gamma shift, blend residual).
    #[test]
    fn intermediate_blit_is_pixel_identical_to_direct_render() {
        use super::super::offscreen::OffscreenRenderer;

        let Some((device, queue)) = test_device_and_queue() else {
            return;
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 64u32;
        let height = 64u32;
        // Arbitrary non-trivial colour: mid-green with partial alpha.
        let test_color = wgpu::Color {
            r: 0.0,
            g: 0.5,
            b: 0.25,
            a: 1.0,
        };

        // --- Direct path: clear a surface texture to `test_color` directly ---
        let direct_surface = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C3 Direct Surface"),
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
        {
            let direct_view = direct_surface.create_view(&wgpu::TextureViewDescriptor::default());
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("C3 Direct Clear Encoder"),
            });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("C3 Direct Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &direct_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(test_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // _pass drops here, releasing the borrow on enc.
            }
            queue.submit(std::iter::once(enc.finish()));
        }

        // --- Intermediate path: clear intermediate, then blit to surface ---
        let intermediate = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C3 Intermediate"),
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
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("C3 Intermediate Clear Encoder"),
            });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("C3 Intermediate Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &intermediate_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(test_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // _pass drops here, releasing the borrow on enc.
            }
            queue.submit(std::iter::once(enc.finish()));
        }

        let blit_surface = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C3 Blit Surface"),
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
        let blit_surface_view = blit_surface.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        offscreen.blit_to_surface(&intermediate, &blit_surface_view, format);

        // Read back and compare pixels at (0,0).
        let direct_pixel = readback_rgba_pixel(&device, &queue, &direct_surface, 0, 0)
            .expect("direct surface readback must succeed");
        let blit_pixel = readback_rgba_pixel(&device, &queue, &blit_surface, 0, 0)
            .expect("blit surface readback must succeed");

        assert_eq!(
            blit_pixel, direct_pixel,
            "intermediate-blit pixel must be byte-identical to a direct render; \
             direct={direct_pixel:?}, blit={blit_pixel:?}. \
             A difference indicates color-space re-encoding or blend in the blit pipeline."
        );
    }

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

    /// C2-full: an advanced Multiply saveLayer rendered through the intermediate
    /// path produces a pixel that matches the `Color::blend` Multiply oracle and
    /// differs from the SrcOver fallback.
    ///
    /// Failure modes:
    /// - Pixel matches SrcOver → Multiply is still falling back (routing broken).
    /// - Panic during render → `debug_assert!(false)` in replay not removed.
    /// - Pixel matches neither → advanced blend formula or blit pipeline broken.
    #[test]
    fn intermediate_path_advanced_blend_matches_oracle() {
        use flui_painting::Paint;
        use flui_types::{Color, Rect, geometry::Pixels, painting::BlendMode};

        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;

        const W: u32 = 64;
        const H: u32 = 64;

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;

        // ── 1. Create a sampleable intermediate (COPY_SRC | TEXTURE_BINDING) ──
        let intermediate = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C2-full Intermediate"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());

        // ── 2. Pre-clear the intermediate to the backdrop color (opaque blue) ──
        let backdrop_blue = wgpu::Color {
            r: 40.0 / 255.0,
            g: 60.0 / 255.0,
            b: 220.0 / 255.0,
            a: 1.0,
        };
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("C2-full Backdrop Clear"),
            });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("C2-full Backdrop Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &intermediate_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(backdrop_blue),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            queue.submit(std::iter::once(enc.finish()));
        }

        // ── 3. Render Multiply saveLayer over the blue intermediate ──
        // Source: opaque orange inside a Multiply saveLayer.
        let source_orange = Color::rgba(200, 120, 40, 255);
        let backdrop_color = Color::rgba(40, 60, 220, 255);
        let layer_bounds =
            Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(W as f32), Pixels(H as f32));

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (W, H),
        );

        let multiply_paint = Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Multiply);
        painter.save_layer(Some(layer_bounds), &multiply_paint);
        painter.rect(layer_bounds, &Paint::fill(source_orange));
        painter.restore_layer();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("C2-full Render Encoder"),
        });
        // RenderTarget::sampleable — the intermediate path; gives advanced blend
        // access to the backdrop for dst-reads.
        let render_target = RenderTarget::sampleable(&intermediate_view, &intermediate);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // ── 4. Blit intermediate → surface ──
        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("C2-full Surface"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        offscreen.blit_to_surface(&intermediate, &surface_view, format);

        // ── 5. Readback center pixel and assert ≈ Multiply oracle, ≠ SrcOver ──
        // The blit uses LoadOp::Clear(BLACK) before drawing, so the surface
        // receives exactly what was in the intermediate center texel.
        let center_pixel = readback_rgba_pixel(&device, &queue, &surface_texture, W / 2, H / 2)
            .expect("C2-full readback must succeed on a COPY_SRC-capable test texture");

        // CPU oracle: what Multiply should produce.
        let blend_result = source_orange.blend(backdrop_color, BlendMode::Multiply);
        let [br, bg, bb, ba] = blend_result.to_f32_array();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to [0,1]*255 range; truncation is correct and safe"
        )]
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let multiply_oracle = [to_u8(br * ba), to_u8(bg * ba), to_u8(bb * ba), to_u8(ba)];

        // SrcOver of opaque orange over blue: opaque orange wins (src dominates).
        let srcover_result = source_orange.blend(backdrop_color, BlendMode::SrcOver);
        let [sr, sg, sb, sa] = srcover_result.to_f32_array();
        let srcover_oracle = [to_u8(sr * sa), to_u8(sg * sa), to_u8(sb * sa), to_u8(sa)];

        // Tolerance ±4: absorbs premul→u8→unpremul quantization at GPU texture boundary.
        let tolerance = 4i16;
        let within = |a: u8, b: u8| (i16::from(a) - i16::from(b)).abs() <= tolerance;

        let matches_multiply = center_pixel
            .iter()
            .zip(multiply_oracle.iter())
            .all(|(&a, &b)| within(a, b));

        let matches_srcover = center_pixel
            .iter()
            .zip(srcover_oracle.iter())
            .all(|(&a, &b)| within(a, b));

        assert!(
            matches_multiply,
            "C2-full: intermediate-path Multiply saveLayer must match the CPU oracle. \
             center_pixel={center_pixel:?}, multiply_oracle={multiply_oracle:?}, \
             srcover_oracle={srcover_oracle:?}. \
             Matches SrcOver={matches_srcover} — if true, Multiply is still falling back."
        );
        assert!(
            !matches_srcover,
            "C2-full: center pixel must NOT match SrcOver; Multiply must produce a \
             distinctly darker result. center_pixel={center_pixel:?}, \
             srcover_oracle={srcover_oracle:?}, multiply_oracle={multiply_oracle:?}."
        );
    }

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

    /// OCR-1: an `ImageFilterLayer` wrapping a sub-region `CanvasLayer` that sits
    /// on top of a full-screen opaque background must not be culled.
    ///
    /// Tree:
    /// ```text
    /// root (OpacityLayer α=1.0, SrcOver — transparent container, bounds()=None)
    ///   child[0]: CanvasLayer (800×600 full screen, draws a red rect) — is_opaque=true
    ///   child[1]: ImageFilterLayer(Blur σ=2.0) — bounds()=None, not cullable itself
    ///               child: CanvasLayer (400×600 right half, draws a blue rect)
    /// ```
    ///
    /// After walking, `filter_op_count_for_test()` must be 1.
    ///
    /// Before the fix: the inner CanvasLayer at (400,0,400,600) was contained
    /// within the full-screen opaque rect (0,0,800,600) → `is_occluded` fired
    /// → layer skipped → no offscreen content → `DrawItem::Filter` not emitted
    /// → count == 0 → RED.
    ///
    /// After the fix: occlusion cull removed → inner CanvasLayer renders its
    /// rect → offscreen segment non-empty → `DrawItem::Filter` emitted → count
    /// == 1 → GREEN.
    #[test]
    fn on_top_layer_not_culled_by_opaque_background() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use flui_layer::{CanvasLayer, ImageFilterLayer, Layer, LayerTree, OpacityLayer};
        use flui_painting::{Canvas, Paint};
        use flui_types::{
            Color,
            geometry::{Rect, px},
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return;
        };

        let format = wgpu::TextureFormat::Bgra8Unorm;
        let surface_w = 800u32;
        let surface_h = 600u32;

        let surface_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("OCR-1 Surface"),
            size: wgpu::Extent3d {
                width: surface_w,
                height: surface_h,
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
        let surface_view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };

        // Build the layer tree.
        let mut tree = LayerTree::new();

        // Root: OpacityLayer(α=1.0, SrcOver) — transparent container, bounds()=None.
        // render() is a no-op for SrcOver+opaque; children are still walked.
        let root_id = tree.insert(Layer::Opacity(OpacityLayer::new(1.0)));
        tree.set_root(Some(root_id));

        // child[0]: full-screen CanvasLayer — is_opaque()=true, bounds()=800×600.
        // Draws a red rect so its segment is non-empty; after render+cleanup it would
        // be registered as opaque by the (now-removed) bug.
        let mut bg_canvas = Canvas::new();
        bg_canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(800.0), px(600.0)),
            &Paint::fill(Color::rgba(255, 0, 0, 255)),
        );
        let bg_layer_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(bg_canvas))));
        tree.add_child(root_id, bg_layer_id);

        // child[1]: ImageFilterLayer(Blur σ=2.0) — bounds()=None (no occlusion check).
        let filter_id = tree.insert(Layer::ImageFilter(ImageFilterLayer::blur(2.0)));
        tree.add_child(root_id, filter_id);

        // child[1]'s child: CanvasLayer covering the right half (400×600).
        // bounds() = (400,0,400,600) ⊆ background (0,0,800,600).
        // BUG: this layer was culled by the back-to-front occlusion cull.
        let mut fg_canvas = Canvas::new();
        fg_canvas.draw_rect(
            Rect::from_xywh(px(400.0), px(0.0), px(400.0), px(600.0)),
            &Paint::fill(Color::rgba(0, 0, 255, 255)),
        );
        let fg_layer_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(fg_canvas))));
        tree.add_child(filter_id, fg_layer_id);

        // Walk the layer tree.
        Renderer::render_layer_recursive(
            &tree,
            &flui_layer::LinkRegistry::new(),
            root_id,
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
        );

        // After the fix the foreground CanvasLayer renders inside the filter's
        // offscreen accumulator.  When the ImageFilterLayer closes, restore_layer
        // finds non-empty offscreen content and emits exactly one DrawItem::Filter.
        //
        // Before the fix, the foreground CanvasLayer was culled by `is_occluded`
        // (its bounds (400,0,400,600) ⊆ full-screen opaque rect (0,0,800,600)),
        // leaving the filter layer empty → DrawItem::Filter was not emitted → 0.
        let filter_op_count = backend.painter().filter_op_count_for_test();
        assert_eq!(
            filter_op_count, 1,
            "OCR-1: the on-top ImageFilterLayer must produce exactly one DrawItem::Filter \
             after rendering; got {filter_op_count}. Zero means the foreground CanvasLayer \
             was culled by the unsound back-to-front occlusion check (or a regression \
             reintroduced equivalent culling)."
        );
    }

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

    /// Clears `view` to `color`. Mirrors `Renderer::run_clear_pass`'s body;
    /// duplicated here because these tests build `WgpuPainter`/`Backend`
    /// manually (like OCR-1 above) rather than through a full `Renderer`, so
    /// `run_clear_pass` (an inherent `&mut Renderer` method) isn't reachable.
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
    #[test]
    fn follower_gpu_renders_at_resolved_position_across_repaint_boundaries() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{
            CanvasLayer, FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, LinkRegistry,
            OffsetLayer,
        };
        use flui_painting::{Canvas, Paint};
        use flui_types::{Color, Offset, Size, geometry::Rect, geometry::px};

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 200u32;
        let height = 200u32;

        let link = LayerLink::new();
        let mut tree = LayerTree::new();

        let root_id = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root_id));

        // Leader lives under `branch_a`, offset (60,0) from root.
        let branch_a = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(60.0),
            px(0.0),
        ))));
        tree.add_child(root_id, branch_a);
        let leader_id = tree.insert(Layer::Leader(LeaderLayer::with_offset(
            link,
            Size::new(px(20.0), px(20.0)),
            Offset::new(px(5.0), px(5.0)),
        )));
        tree.add_child(branch_a, leader_id);

        // Follower lives under a DIFFERENT boundary, `branch_b`, offset
        // (0,90) from root.
        let branch_b = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(0.0),
            px(90.0),
        ))));
        tree.add_child(root_id, branch_b);
        let follower_id = tree.insert(Layer::Follower(
            FollowerLayer::new(link).with_size(Size::new(px(10.0), px(10.0))),
        ));
        tree.add_child(branch_b, follower_id);

        // The follower's child: a 10×10 opaque red rect at its own local origin.
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
            &Paint::fill(Color::rgba(255, 0, 0, 255)),
        );
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(follower_id, child_id);

        let mut registry = LinkRegistry::new();
        registry.register_leader(
            link,
            leader_id,
            Offset::new(px(5.0), px(5.0)),
            Size::new(px(20.0), px(20.0)),
        );
        registry.register_follower(follower_id, link);

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
        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::render_layer_recursive(
            &tree,
            &registry,
            root_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
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

    /// (b) An unlinked Follower (no `Layer::Leader` registered under its
    /// `link`) with `show_when_unlinked = true` renders its subtree at its
    /// own `target_offset` — the plain paint-origin-relative fallback, NOT
    /// routed through `calculate_offset`.
    #[test]
    fn follower_gpu_unlinked_show_when_unlinked_true_renders_at_target_offset() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{
            CanvasLayer, FollowerLayer, Layer, LayerLink, LayerTree, LinkRegistry, OffsetLayer,
        };
        use flui_painting::{Canvas, Paint};
        use flui_types::{Color, Offset, Size, geometry::Rect, geometry::px};

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 100u32;
        let height = 100u32;

        let link = LayerLink::new();
        let mut tree = LayerTree::new();

        let root_id = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root_id));

        // `target_offset` is large relative to the child's 10×10 rect so the
        // shifted and un-shifted rects don't overlap — a naive "no
        // resolution at all" implementation (rendering the child at its
        // natural, un-shifted local position) must fail the assertions
        // below, not accidentally satisfy them via overlap.
        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(true)
            .with_target_offset(Offset::new(px(30.0), px(30.0)))
            .with_size(Size::new(px(10.0), px(10.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.add_child(root_id, follower_id);

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
            &Paint::fill(Color::rgba(255, 0, 0, 255)),
        );
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(follower_id, child_id);

        // No leader registered under `link` at all — genuinely unlinked.
        let registry = LinkRegistry::new();

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Follower Unlinked-Shown GPU Test Texture"),
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
        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::render_layer_recursive(
            &tree,
            &registry,
            root_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Follower Unlinked-Shown GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // Unlinked fallback position: root (0,0) + target_offset (30,30).
        let shifted_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 35, 35)
            .expect("readback must succeed");
        assert!(
            shifted_pixel[0] > 200 && shifted_pixel[1] < 50,
            "an unlinked follower with show_when_unlinked=true must render \
             at its own target_offset; expected opaque red at (35,35), got {shifted_pixel:?}"
        );

        // The child's own NATURAL (un-shifted) local position must stay
        // untouched — proves the fallback actually applied `target_offset`
        // rather than rendering the child at its plain local position.
        let natural_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 5, 5)
            .expect("readback must succeed");
        assert!(
            natural_pixel[1] > 200,
            "the unlinked follower's un-shifted local position must stay \
             untouched white background; expected no draw at (5,5), got {natural_pixel:?}"
        );
    }

    /// (c) An unlinked Follower with `show_when_unlinked = false` renders
    /// NOTHING at all — the subtree is not painted anywhere (oracle
    /// `FollowerLayer.addToScene`'s early return, `layer.dart:2857-2865`).
    #[test]
    fn follower_gpu_unlinked_show_when_unlinked_false_hides_subtree() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{
            CanvasLayer, FollowerLayer, Layer, LayerLink, LayerTree, LinkRegistry, OffsetLayer,
        };
        use flui_painting::{Canvas, Paint};
        use flui_types::{Color, Offset, Size, geometry::Rect, geometry::px};

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 100u32;
        let height = 100u32;

        let link = LayerLink::new();
        let mut tree = LayerTree::new();

        let root_id = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root_id));

        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(false)
            .with_target_offset(Offset::new(px(5.0), px(5.0)))
            .with_size(Size::new(px(10.0), px(10.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.add_child(root_id, follower_id);

        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
            &Paint::fill(Color::rgba(255, 0, 0, 255)),
        );
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(follower_id, child_id);

        // No leader registered under `link` at all — genuinely unlinked.
        let registry = LinkRegistry::new();

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Follower Unlinked-Hidden GPU Test Texture"),
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
        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::render_layer_recursive(
            &tree,
            &registry,
            root_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Follower Unlinked-Hidden GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // The spot the content WOULD have rendered at if shown (target_offset
        // (5,5)) must stay untouched white background — the subtree must not
        // be painted anywhere.
        let pixel = readback_rgba_pixel(&device, &queue, &render_texture, 7, 7)
            .expect("readback must succeed");
        assert!(
            pixel[1] > 200,
            "an unlinked follower with show_when_unlinked=false must render \
             nothing at all; expected untouched white background, got {pixel:?}"
        );
    }

    // =========================================================================
    // `Layer::ShaderMask` engine visual-rendering fix — GPU-level, end-to-end
    // pixel-readback proof (design research plan
    // `2026-07-01-shader-mask-engine-render-plan.md`).
    //
    // `Layer::ShaderMask` previously fell through to the generic
    // `LayerRender` dispatch (`layer_render.rs`), which pushes an inert
    // `save_layer`/`push_clip_rect` pair that never reads the layer's
    // `shader()`/`blend_mode()` — masking silently never applied. These
    // tests exercise the REAL `render_layer_recursive` special case (not a
    // hand-rolled stand-in) against a real GPU texture, reading back actual
    // rendered pixels, the same style as the Follower Tier-2 tests above.
    // =========================================================================

    /// A `Layer::ShaderMask` mounted at the tree root, wrapping an opaque
    /// solid-colored child, must have its shader mask actually applied — not
    /// fall through to the pre-fix inert clip (which would show the child
    /// fully unmasked).
    ///
    /// Child: opaque red rect filling the mask bounds. Shader: solid mask at
    /// ~50% alpha (rgb is irrelevant to `solid.wgsl` — only alpha
    /// modulates). Composited (`PREMULTIPLIED_ALPHA_BLENDING`) over a white
    /// background, the masked pixel must land at a distinctly blended
    /// value — neither pure opaque red (unmasked passthrough) nor pure
    /// white (content missing).
    ///
    /// `bounds` is anchored at (0, 0) here deliberately — this sanity test
    /// does NOT exercise the coordinate-frame trap (see
    /// `shader_mask_nested_under_offset_ancestor_lands_at_correct_position`
    /// below for that); it only proves the GPU mask pipeline is reached at
    /// all.
    #[test]
    fn shader_mask_layer_root_gpu_pixel_readback_reflects_mask() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{CanvasLayer, Layer, LayerTree, LinkRegistry, ShaderMaskLayer};
        use flui_painting::{Canvas, Paint, Shader};
        use flui_types::{
            Color,
            geometry::{Rect, px},
            painting::BlendMode,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 64u32;
        let height = 64u32;

        let mut tree = LayerTree::new();
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
        let shader = Shader::solid(Color::rgba(10, 20, 30, 128));
        let mask_id = tree.insert(Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            BlendMode::SrcOver,
            bounds,
        )));
        tree.set_root(Some(mask_id));

        let mut canvas = Canvas::new();
        canvas.draw_rect(bounds, &Paint::fill(Color::rgba(255, 0, 0, 255)));
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(mask_id, child_id);

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
        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::render_layer_recursive(
            &tree,
            &LinkRegistry::new(),
            mask_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
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
        assert!(
            pixel[0] > 200 && (60..=200).contains(&pixel[1]) && (60..=200).contains(&pixel[2]),
            "ShaderMask must apply its shader as a mask, not fall through to the \
             inert clip (pure opaque red, ~[255,0,0]) and not drop the content \
             entirely (background white, ~[255,255,255]); expected a distinctly \
             blended pixel from the ~50% mask, got {pixel:?}"
        );
    }

    /// THE TRAP regression test: a `ShaderMask`
    /// whose `bounds()` origin is NOT `(0, 0)` and which sits under a
    /// non-zero-offset `Layer::Offset` ancestor must still render its masked
    /// content at the CORRECT on-screen position — not shifted/clipped by a
    /// naive "reset offscreen painter to DPR-scale-only" seed (the approach
    /// `Backend::render_shader_mask`'s `DisplayList` path uses, which is
    /// correct there only because its children are recorded into a FRESH,
    /// self-relative `Canvas`, never the ambient-CTM-relative `LayerTree`).
    ///
    /// Geometry (dpr = 1, ancestor offset = (60, 40), mask bounds =
    /// (30, 20, 60, 60) so `bounds()`'s origin is nonzero):
    ///   - `device_bounds` (always correct — the composite target is
    ///     unaffected by the seed bug) = (90, 60, 60, 60), corners
    ///     (90,60)→(150,120).
    ///   - A naive DPR-only-reset seed is IDENTITY here (dpr=1 skips the
    ///     scale branch entirely), so it paints the child at its RAW local
    ///     coordinates (30..90, 20..80) directly into the 60×60 offscreen
    ///     texture (valid pixel range 0..60), leaving only the sub-rectangle
    ///     (30..60, 20..60) actually filled — device (120..150, 80..120) —
    ///     while the rest of `device_bounds` (e.g. (100,70)) stays
    ///     empty/background.
    ///   - The correct seed (`translate(-device_bounds.origin) *
    ///     ambient_ctm`) fills the ENTIRE `device_bounds` rectangle.
    #[test]
    fn shader_mask_nested_under_offset_ancestor_lands_at_correct_position() {
        use super::super::backend::Backend;
        use super::super::offscreen::OffscreenRenderer;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{
            CanvasLayer, Layer, LayerTree, LinkRegistry, OffsetLayer, ShaderMaskLayer,
        };
        use flui_painting::{Canvas, Paint, Shader};
        use flui_types::{
            Color, Offset,
            geometry::{Rect, px},
            painting::BlendMode,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 200u32;
        let height = 200u32;

        let mut tree = LayerTree::new();

        let offset_id = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(60.0),
            px(40.0),
        ))));
        tree.set_root(Some(offset_id));

        let bounds = Rect::from_xywh(px(30.0), px(20.0), px(60.0), px(60.0));
        let shader = Shader::solid(Color::rgba(10, 20, 30, 128));
        let mask_id = tree.insert(Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            BlendMode::SrcOver,
            bounds,
        )));
        tree.add_child(offset_id, mask_id);

        let mut canvas = Canvas::new();
        canvas.draw_rect(bounds, &Paint::fill(Color::rgba(255, 0, 0, 255)));
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(mask_id, child_id);

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ShaderMask Nested-Offset GPU Test Texture"),
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
        let mut offscreen = OffscreenRenderer::new(Arc::clone(&device), Arc::clone(&queue), format);
        let mut backend = Backend::with_offscreen(&mut painter, &mut offscreen);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        Renderer::render_layer_recursive(
            &tree,
            &LinkRegistry::new(),
            offset_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShaderMask Nested-Offset GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        let is_masked =
            |p: [u8; 4]| p[0] > 200 && (60..=200).contains(&p[1]) && (60..=200).contains(&p[2]);
        let is_white = |p: [u8; 4]| p[0] > 240 && p[1] > 240 && p[2] > 240;

        // Sanity: deep in the bottom-right of device_bounds, filled under
        // BOTH the naive and the correct seed — proves content reaches the
        // screen at all (rules out a "nothing renders" false pass below).
        let sanity_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 140, 110)
            .expect("sanity-position readback must succeed");
        assert!(
            is_masked(sanity_pixel),
            "masked content must render somewhere inside device_bounds \
             (90,60)-(150,120); got {sanity_pixel:?} at (140,110)"
        );

        // THE TRAP: near the top-left of device_bounds. The correct seed
        // fills this; a naive DPR-only-reset seed leaves it empty
        // (background), per the worked geometry in the doc comment above.
        let trap_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 100, 70)
            .expect("trap-position readback must succeed");
        assert!(
            is_masked(trap_pixel),
            "ShaderMask nested under a non-zero-offset ancestor must fill its \
             ENTIRE device_bounds rect (90,60)-(150,120), not just the corner a \
             naive DPR-scale-only offscreen seed happens to hit; got {trap_pixel:?} \
             at (100,70) — background-colored means the offscreen painter's seed \
             transform dropped the `bounds()` origin subtraction, the exact \
             coordinate-frame trap this test guards."
        );

        // Outside device_bounds entirely: must stay untouched white background.
        let outside_pixel = readback_rgba_pixel(&device, &queue, &render_texture, 10, 10)
            .expect("outside-position readback must succeed");
        assert!(
            is_white(outside_pixel),
            "content must not leak outside the mask's device_bounds; \
             got {outside_pixel:?} at (10,10)"
        );
    }

    /// When no `OffscreenRenderer` is available, `Layer::ShaderMask` must
    /// fall through to the pre-fix inert clip/save-layer path
    /// (`LayerRender<ShaderMaskLayer>` in `layer_render.rs`) without
    /// panicking — mirroring `BackdropFilter`'s own non-`Blur`-filter
    /// degrade. The inert path does not mask; child content renders
    /// unmodified (still clipped to bounds).
    #[test]
    fn shader_mask_without_offscreen_renderer_falls_through_to_inert_clip() {
        use super::super::backend::Backend;
        use super::super::painter::WgpuPainter;
        use super::super::render_target::RenderTarget;
        use flui_layer::{CanvasLayer, Layer, LayerTree, LinkRegistry, ShaderMaskLayer};
        use flui_painting::{Canvas, Paint, Shader};
        use flui_types::{
            Color,
            geometry::{Rect, px},
            painting::BlendMode,
        };

        let Some((device, queue)) = test_device_and_queue() else {
            return; // No GPU — skip gracefully.
        };

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let width = 64u32;
        let height = 64u32;

        let mut tree = LayerTree::new();
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(64.0), px(64.0));
        let shader = Shader::solid(Color::rgba(10, 20, 30, 128));
        let mask_id = tree.insert(Layer::ShaderMask(ShaderMaskLayer::new(
            shader,
            BlendMode::SrcOver,
            bounds,
        )));
        tree.set_root(Some(mask_id));

        let mut canvas = Canvas::new();
        canvas.draw_rect(bounds, &Paint::fill(Color::rgba(255, 0, 0, 255)));
        let child_id = tree.insert(Layer::Canvas(Box::new(CanvasLayer::from_canvas(canvas))));
        tree.add_child(mask_id, child_id);

        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ShaderMask No-Offscreen GPU Test Texture"),
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
        // No `OffscreenRenderer` bound — `Backend::new`, not `with_offscreen`.
        let mut backend = Backend::new(&mut painter);

        let ctx = RenderContext {
            supports_copy_src: true,
            intermediate_active: false,
        };
        // Must not panic.
        Renderer::render_layer_recursive(
            &tree,
            &LinkRegistry::new(),
            mask_id,
            &mut backend,
            &ctx,
            &render_texture,
            &render_view,
        );
        drop(backend);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ShaderMask No-Offscreen GPU Test Render Encoder"),
        });
        let render_target = RenderTarget::sampleable(&render_view, &render_texture);
        painter
            .render(render_target, &mut encoder)
            .expect("painter.render must succeed on a GPU-enabled host");
        queue.submit(std::iter::once(encoder.finish()));

        // Inert clip degrade: content renders UNMASKED (full opaque red),
        // proving the fallback still paints children rather than silently
        // dropping the whole subtree.
        let pixel = readback_rgba_pixel(&device, &queue, &render_texture, 32, 32)
            .expect("center readback must succeed");
        assert!(
            pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50,
            "without an OffscreenRenderer, ShaderMask must fall through to the \
             inert clip/save-layer path and still render its child UNMASKED \
             (opaque red); got {pixel:?}"
        );
    }
}
