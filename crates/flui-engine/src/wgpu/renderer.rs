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

    /// Required GPU features based on capabilities and adapter support
    fn required_features(capabilities: &GpuCapabilities) -> wgpu::Features {
        let mut features = wgpu::Features::empty();

        // Always enable texture adapter-specific formats
        features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

        // Immediates (formerly push constants): only request if adapter supports them.
        // Some mobile GPUs (especially older Android devices) don't support this.
        if capabilities.supports_push_constants {
            features |= wgpu::Features::IMMEDIATES;
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
        // Prefer sRGB formats for correct color rendering
        let preferred_formats = if capabilities.supports_hdr {
            vec![
                wgpu::TextureFormat::Rgba16Float, // HDR
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ]
        } else {
            vec![
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
            ]
        };

        for format in preferred_formats {
            if surface_caps.formats.contains(&format) {
                tracing::debug!("Selected surface format: {:?}", format);
                return format;
            }
        }

        // Fallback: some drivers report zero formats (e.g. headless CI).
        // Default to a universally supported sRGB format rather than panicking.
        if let Some(fmt) = surface_caps.formats.first().copied() {
            fmt
        } else {
            tracing::error!("surface reported zero formats; defaulting to Bgra8UnormSrgb");
            wgpu::TextureFormat::Bgra8UnormSrgb
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
            {
                let _pass = clear_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("FLUI Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
            self.queue.submit(std::iter::once(clear_encoder.finish()));
        }

        // 2. Build render context for backdrop filter support
        let surface_format = self
            .config
            .as_ref()
            .map_or(wgpu::TextureFormat::Bgra8UnormSrgb, |c| c.format);

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
            if let Err(e) = painter.render(&view, &mut final_encoder) {
                tracing::error!("Painter render failed: {}", e);
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

        // 1. Flush current painter batches to the surface so pixels are available
        let mut flush_encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Backdrop Flush Encoder"),
                });
        if let Err(e) = backend
            .painter_mut()
            .render(surface_view, &mut flush_encoder)
        {
            tracing::error!("Backdrop flush failed: {}", e);
        }

        // 2. Copy region from surface to offscreen texture for blur input
        let x = bounds.left().0.max(0.0) as u32;
        let y = bounds.top().0.max(0.0) as u32;
        let w = bounds.width().0.max(1.0) as u32;
        let h = bounds.height().0.max(1.0) as u32;

        if let Some(offscreen_arc) = backend.offscreen().cloned() {
            let blur_input = {
                let offscreen = offscreen_arc.lock();
                offscreen.texture_pool().acquire(w, h, ctx.surface_format)
            };

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

            // 3. Apply Dual Kawase blur
            let blurred = {
                let mut offscreen = offscreen_arc.lock();
                offscreen.render_blur(&blur_input, sigma)
            };

            // 4. Queue blurred result for compositing
            backend
                .painter_mut()
                .queue_offscreen_result(blurred, bounds);
        } else {
            // No offscreen renderer available — just submit the flush
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
}
