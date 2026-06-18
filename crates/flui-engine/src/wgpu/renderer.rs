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

use super::occlusion::OcclusionTracker;
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
/// Provides access to device, queue, and surface format for mid-frame
/// operations like backdrop blur (flush -> copy -> blur -> composite).
struct RenderContext {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    /// Whether the surface supports COPY_SRC (for backdrop filter)
    supports_copy_src: bool,
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
    offscreen: Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>,
    supports_copy_src: bool,
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "gpu-profiler")]
    gpu_profiler: Option<super::profiler::GpuFrameProfiler>,
}

/// Cross-platform GPU renderer
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
    offscreen: Option<Arc<parking_lot::Mutex<super::offscreen::OffscreenRenderer>>>,
    /// Whether the surface supports COPY_SRC (for mid-frame texture copies)
    supports_copy_src: bool,
    /// Set by the device-lost callback; checked at frame start to trigger
    /// device recreation. `Arc<AtomicBool>` because the callback is `'static`.
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// Tracks dirty regions for incremental rendering (skip frames with no damage)
    damage_tracker: flui_layer::damage::DamageTracker,
    /// Tracks opaque regions to skip fully-occluded layers during traversal
    occlusion: OcclusionTracker,
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
            .map_err(|e| EngineError::surface_creation(std::io::Error::other(e.to_string())))?;
        let display_handle = window
            .display_handle()
            .map_err(|e| EngineError::surface_creation(std::io::Error::other(e.to_string())))?;
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
            occlusion: OcclusionTracker::new(),
            raw_window_handle: Some(raw_window_handle),
            raw_display_handle,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: stack.gpu_profiler,
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

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
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
            desired_maximum_frame_latency: 2,
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
        let offscreen = Arc::new(parking_lot::Mutex::new(offscreen));

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
            occlusion: OcclusionTracker::new(),
            raw_window_handle: None,
            raw_display_handle: None,
            // Offscreen renderers have no surface present, so profiling results
            // cannot be harvested with process_finished_frame. Disabled here.
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: None,
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

    /// Select present mode based on capabilities
    fn select_present_mode(surface_caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
        // Prefer Mailbox (triple buffering, low latency) > Fifo (vsync)
        if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo // Always supported
        }
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
    pub fn render_scene(&mut self, scene: &flui_layer::Scene) -> Result<(), EngineError> {
        use super::backend::Backend;

        // Fine-grained damage tracking is the caller's responsibility: the
        // application layer calls `mark_dirty()` / `mark_full_repaint()` after
        // input events or state changes. When flui-view is wired up, widgets
        // will call `mark_dirty(bounds)` on state change; until then, callers
        // use `mark_full_repaint()` to force a frame.

        // Check if we need to render at all
        if !self.damage_tracker.has_damage() && !self.damage_tracker.needs_full_repaint() {
            // Nothing changed — skip this frame entirely
            tracing::trace!("Skipping frame: no damage");
            return Ok(());
        }

        let surface = self.surface.as_ref().ok_or(EngineError::SurfaceLost)?;

        // wgpu 28+: get_current_texture() returns CurrentSurfaceTexture enum
        // instead of Result<SurfaceTexture, SurfaceError>.

        // Check for device-lost flag (set by the device-lost callback) before
        // attempting to acquire a surface texture. If the device is gone, we
        // cannot proceed with the current device — return an error that the
        // caller can handle by recreating the renderer.
        if self.device_lost.load(std::sync::atomic::Ordering::Acquire) {
            tracing::warn!("Device lost detected; returning DeviceLost error");
            return Err(EngineError::DeviceLost);
        }

        let output = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Suboptimal is still renderable; schedule a reconfigure next frame.
                tracing::debug!("Surface suboptimal; will reconfigure on next resize");
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // Auto-reconfigure and retry once.
                self.reconfigure_surface()?;
                let surface = self.surface.as_ref().ok_or(EngineError::SurfaceLost)?;
                match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    _ => return Err(EngineError::SurfaceLost),
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout => return Err(EngineError::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => {
                // Window is minimized or fully occluded — skip this frame
                // entirely rather than producing garbage frames.
                tracing::trace!("Surface occluded; skipping frame");
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                // Validation error — the surface texture could not be produced
                // due to a validation failure. Log and return error.
                tracing::error!("Surface texture validation error");
                return Err(EngineError::SurfaceLost);
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 1. Clear pass — submit immediately so the surface is ready for
        //    mid-frame copy operations (backdrop blur needs pixels on the surface).
        {
            let mut clear_encoder =
                self.device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("FLUI Clear Encoder"),
                    });
            // Hoist the color attachments array before the #[cfg] split so both
            // the profiled and non-profiled paths share one definition. The descriptor
            // borrows `&view`, so the array binding must live at the same scope level.
            let clear_color_attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &view,
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

        // 2. Build render context for backdrop filter support
        let surface_format = self
            .config
            .as_ref()
            .map_or(wgpu::TextureFormat::Bgra8Unorm, |c| c.format);

        let ctx = RenderContext {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
            surface_format,
            supports_copy_src: self.supports_copy_src,
        };

        // 3. Reset occlusion tracker for this frame
        self.occlusion.reset();

        // 4. Render scene content via LayerTree traversal
        if scene.has_content()
            && let Some(painter) = self.painter.take()
        {
            let mut backend = if let Some(ref offscreen) = self.offscreen {
                Backend::with_offscreen(painter, Arc::clone(offscreen))
            } else {
                Backend::new(painter)
            };
            // Cycle 4 U-8: bind the frame surface so the
            // DisplayList-level `render_backdrop_filter` path (U-9)
            // can flush + blur the same surface the layer-level
            // path already uses. Without this bind, that command
            // path falls back to passthrough -- a visible regression
            // vs Flutter.
            backend.bind_surface(&view, &output.texture);

            // Reset per-frame clip/transform/opacity/layer state so that
            // partial-damage scissors from frame N cannot leak into frame N+1.
            // This must happen BEFORE the damage clip_rect below.
            backend.painter_mut().reset_frame_state();

            // Apply damage rect as scissor optimization: when only part of the
            // screen changed, limit GPU work to the damaged region.
            // `damage_rect()` returns `None` for full repaint (no scissor needed),
            // `Some(rect)` for partial damage.
            if let Some(damage) = self.damage_tracker.damage_rect()
                && damage.width().0 > 0.0
                && damage.height().0 > 0.0
            {
                backend.painter_mut().clip_rect(damage);
                tracing::trace!(
                    left = damage.left().0,
                    top = damage.top().0,
                    width = damage.width().0,
                    height = damage.height().0,
                    "Damage scissor applied"
                );
            }

            // Depth-first traversal of layer tree
            if let Some(root_id) = scene.root() {
                Self::render_layer_recursive(
                    scene.layer_tree(),
                    root_id,
                    &mut backend,
                    &ctx,
                    &output.texture,
                    &view,
                    &mut self.occlusion,
                );
            }

            tracing::trace!(
                opaque_regions = self.occlusion.opaque_count(),
                "Occlusion tracking complete for frame"
            );

            // 5. Final flush — submit remaining painter batches
            let mut painter = backend.into_painter();
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
            let frame_target =
                super::render_target::RenderTarget::sampleable(&view, &output.texture);
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

        Ok(())
    }

    /// Recursively render a layer and its children (depth-first).
    ///
    /// Each layer's `render()` pushes state (transforms, clips, opacity),
    /// children are rendered, then `cleanup()` pops the state.
    ///
    /// Layers that are fully occluded by previously-rendered opaque content
    /// are skipped entirely (including their children), reducing overdraw.
    ///
    /// `BackdropFilterLayer` is handled specially at the Renderer level when
    /// the surface supports `COPY_SRC`, enabling mid-frame flush + blur.
    fn render_layer_recursive(
        tree: &flui_layer::LayerTree,
        layer_id: flui_foundation::LayerId,
        backend: &mut super::backend::Backend<'_>,
        ctx: &RenderContext,
        surface_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        occlusion: &mut OcclusionTracker,
    ) {
        use super::layer_render::LayerRender;

        let Some(node) = tree.get(layer_id) else {
            return;
        };

        let layer = node.layer();

        // Occlusion culling: skip layers fully hidden behind opaque content.
        // Only check layers that report bounds — layers without bounds (Offset,
        // Transform, Opacity, etc.) are containers that affect their children
        // and cannot be culled independently.
        if let Some(bounds) = layer.bounds() {
            let x = bounds.left().0;
            let y = bounds.top().0;
            let w = bounds.width().0;
            let h = bounds.height().0;

            if w > 0.0 && h > 0.0 && occlusion.is_occluded(x, y, w, h) {
                tracing::trace!(?layer_id, x, y, w, h, "Skipping occluded layer");
                return; // Skip this layer and all its children
            }
        }

        // Special handling for BackdropFilter — requires mid-frame flush + copy
        if let flui_layer::Layer::BackdropFilter(bf_layer) = layer
            && ctx.supports_copy_src
        {
            Self::handle_backdrop_filter(
                bf_layer,
                node,
                tree,
                backend,
                ctx,
                surface_texture,
                surface_view,
                occlusion,
            );
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
                child_id,
                backend,
                ctx,
                surface_texture,
                surface_view,
                occlusion,
            );
        }

        layer.cleanup(backend);

        // Register opaque regions after rendering so that subsequent layers
        // (siblings rendered later in traversal order) can be culled.
        // Only leaf layers known to draw solid content are registered.
        if layer.is_opaque()
            && let Some(bounds) = layer.bounds()
        {
            let x = bounds.left().0;
            let y = bounds.top().0;
            let w = bounds.width().0;
            let h = bounds.height().0;

            if w > 0.0 && h > 0.0 {
                occlusion.add_opaque(x, y, w, h);
            }
        }
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
        reason = "backdrop-filter pipeline needs the surface texture/view, occlusion tracker, and layer-tree context to do its job — splitting these into a helper struct adds indirection without clarity"
    )]
    fn handle_backdrop_filter(
        bf_layer: &flui_layer::BackdropFilterLayer,
        node: &flui_layer::tree::LayerNode,
        tree: &flui_layer::LayerTree,
        backend: &mut super::backend::Backend<'_>,
        ctx: &RenderContext,
        surface_texture: &wgpu::Texture,
        surface_view: &wgpu::TextureView,
        occlusion: &mut OcclusionTracker,
    ) {
        use flui_types::geometry::{Pixels, Rect};
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
                    child_id,
                    backend,
                    ctx,
                    surface_texture,
                    surface_view,
                    occlusion,
                );
            }
            return;
        };

        // 2. Map the layer's local-space `bounds` to a device-space rect before
        //    sampling/compositing. `bf_layer.bounds()` is in logical pixels; the
        //    surface texture and the composite viewport are in physical pixels.
        //    The accumulated layer-walk CTM — which carries the `RenderView`
        //    root `scale(dpr)` plus every intervening transform/offset layer —
        //    lives in the painter's `current_transform` at this point (the walk
        //    pushes it via `push_transform`/`push_offset`). Reading it here is
        //    the layer-tree equivalent of the `transform` argument the
        //    display-list backdrop path ("Path B", `Backend::render_backdrop_filter`)
        //    receives. Without this mapping a backdrop at logical (100,100,200,200)
        //    sampled device region (100,100,200,200) on a 2x display — wrong
        //    source pixels, half size, half position.
        let transform = backend.painter().current_transform_matrix();
        let device_rect = transform.transform_rect(&bounds);

        // Clamp the device rect against the surface extent. Path A previously
        // only lower-clamped (`max(0.0)`/`max(1.0)`); a backdrop partially
        // off-screen (negative origin or extent beyond the frame) would feed
        // `copy_texture_to_texture` an out-of-range region and trip wgpu
        // validation at submit time, dropping the frame. Mirror Path B: clamp
        // both edges, derive extents from the clamped corners.
        let surface_extent = surface_texture.size();
        let surface_w = surface_extent.width;
        let surface_h = surface_extent.height;

        // Use `.round()` before truncation to match the shader-mask path and avoid
        // a 1-device-pixel undersize on sub-pixel boundaries (e.g. DPR=1.5 or
        // fractional-offset CTMs). Keep ≥ 0 via the prior `clamp`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let x = device_rect.left().0.clamp(0.0, surface_w as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y = device_rect.top().0.clamp(0.0, surface_h as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let right = device_rect.right().0.clamp(0.0, surface_w as f32).round() as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bottom = device_rect.bottom().0.clamp(0.0, surface_h as f32).round() as u32;
        let w = right.saturating_sub(x).max(1);
        let h = bottom.saturating_sub(y).max(1);

        // If the clamped region is empty the backdrop is entirely off-screen.
        // `copy_texture_to_texture` requires non-zero extents — fall through to
        // child rendering (no GPU blur) instead of tripping validation.
        if right <= x || bottom <= y {
            tracing::warn!(
                bounds_l = bounds.left().0,
                bounds_t = bounds.top().0,
                bounds_r = bounds.right().0,
                bounds_b = bounds.bottom().0,
                surface_w,
                surface_h,
                "Backdrop filter (Path A): clamped device region is empty (entirely off-screen); \
                 rendering children only"
            );
            for &child_id in node.children() {
                Self::render_layer_recursive(
                    tree,
                    child_id,
                    backend,
                    ctx,
                    surface_texture,
                    surface_view,
                    occlusion,
                );
            }
            return;
        }

        // 1. Flush current painter batches to the surface so pixels are available.
        // The encoder also carries copy_texture_to_texture (step 3) before submit,
        // keeping the flush → copy sequence in one submission (Fix #11 ordering).
        //
        // PROFILER-SKIP: this backdrop-flush encoder is intentionally excluded from
        // the frame profile. Its submit ordering is controlled by the backdrop-filter
        // logic (not the profiler), and adding a scope here would require threading
        // `&mut GpuFrameProfiler` through a static fn that has no access to Renderer
        // state. Backdrop GPU time is therefore absent from the per-frame profile;
        // the clear-pass and final-render scopes in `render_scene` cover the primary
        // frame timing. This is an explicit trade-off, not an oversight.
        let mut flush_encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Backdrop Flush Encoder"),
                });
        let flush_target =
            super::render_target::RenderTarget::sampleable(surface_view, surface_texture);
        if let Err(e) = backend
            .painter_mut()
            .render(flush_target, &mut flush_encoder)
        {
            tracing::error!("Backdrop flush failed: {}", e);
        }

        if let Some(offscreen_arc) = backend.offscreen().cloned() {
            let blur_input = {
                let offscreen = offscreen_arc.lock();
                offscreen.texture_pool().acquire(w, h, ctx.surface_format)
            };

            // 3. Copy the device-space region from the surface to the blur input.
            flush_encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: surface_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: blur_input.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            ctx.queue.submit(std::iter::once(flush_encoder.finish()));

            // 4. Apply Dual Kawase blur
            let blurred = {
                let mut offscreen = offscreen_arc.lock();
                offscreen.render_blur(&blur_input, sigma)
            };

            // 5. Queue the blurred result for compositing at the CLAMPED device
            //    rect — the copy source origin is (x,y) and extent (w,h), so the
            //    composite rect must match exactly. `device_rect` is unclamped
            //    (may extend outside the surface for a backdrop crossing the edge),
            //    meaning the smaller blurred texture would be stretched/misaligned
            //    across the larger unclamped rect. Build the composite rect from
            //    the same clamped u32 values used for the copy.
            let clamped_composite_rect = Rect::from_xywh(
                Pixels(x as f32),
                Pixels(y as f32),
                Pixels(w as f32),
                Pixels(h as f32),
            );
            backend
                .painter_mut()
                .queue_offscreen_result(blurred, clamped_composite_rect);
        } else {
            // No offscreen renderer available — just submit the flush.
            ctx.queue.submit(std::iter::once(flush_encoder.finish()));
            tracing::warn!("Backdrop blur skipped: no offscreen renderer available");
        }

        // 5. Render children on top of the blurred backdrop
        for &child_id in node.children() {
            Self::render_layer_recursive(
                tree,
                child_id,
                backend,
                ctx,
                surface_texture,
                surface_view,
                occlusion,
            );
        }
        // No cleanup needed — backdrop filter has no push/pop state in this path
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

        let offscreen = Arc::new(parking_lot::Mutex::new(OffscreenRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
        )));

        let painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(painter, Arc::clone(&offscreen));

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
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            surface_format: format,
            supports_copy_src: true,
        };
        let mut occlusion = OcclusionTracker::new();

        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
            &mut occlusion,
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

        let offscreen = Arc::new(parking_lot::Mutex::new(OffscreenRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
        )));
        let painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(painter, Arc::clone(&offscreen));

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
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            surface_format: format,
            supports_copy_src: true,
        };
        let mut occlusion = OcclusionTracker::new();

        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
            &mut occlusion,
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

        let offscreen = Arc::new(parking_lot::Mutex::new(OffscreenRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
        )));
        let painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            (surface_w, surface_h),
        );
        let mut backend = Backend::with_offscreen(painter, Arc::clone(&offscreen));

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
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            surface_format: format,
            supports_copy_src: true,
        };
        let mut occlusion = OcclusionTracker::new();

        Renderer::handle_backdrop_filter(
            bf_layer,
            node,
            &tree,
            &mut backend,
            &ctx,
            &surface_texture,
            &surface_view,
            &mut occlusion,
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
}
