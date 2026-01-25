# Linux Platform Improvements Roadmap for FLUI

**Document Version:** 1.0  
**Last Updated:** January 25, 2026  
**Target Platforms:** Linux Desktop (Wayland, X11), Linux Kernel 6.12+ (LTS), Mesa 25.x  
**FLUI Crates:** `flui-platform` (linux module), `flui_engine` (Vulkan backend)

---

## Executive Summary

This document outlines modern Linux platform features and improvements relevant to FLUI framework implementation. Linux desktop has undergone a significant transformation in 2025-2026:

- **Wayland Maturity** - Major distributions (Ubuntu, Fedora, GNOME 49) dropped X11 sessions by default in 2025
- **NVIDIA Wayland Support** - Driver 590 series (2025) brings stable Wayland + NVIDIA compatibility
- **Mesa 25.x** - Vulkan 1.4 support across RADV (AMD), ANV (Intel), NVK (NVIDIA)
- **Linux Kernel 6.12 LTS** - Real-time PREEMPT_RT, sched_ext scheduler, supported until end of 2026+
- **PipeWire 1.6** - Unified audio/video/screen capture, ASHA hearing aid support, MIDI 2.0

**Key Architectural Shifts:**
1. **X11 → Wayland Migration** - X11 deprecated in GNOME 49 (2025), KDE Plasma (early 2027)
2. **Vulkan 1.4 Adoption** - All major Mesa drivers (RADV, ANV, NVK) support Vulkan 1.4
3. **COSMIC Desktop** - First major Rust-based desktop environment (December 11, 2025)
4. **XDG Portal Standardization** - File pickers, screen capture, notifications unified

---

## 1. Wayland Display Server (2025-2026 Maturity)

### 1.1 Wayland as Default ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Production-ready (2025), X11 deprecated  
**Impact:** Modern display protocol replaces X11

#### Overview

2025 marked Wayland's transition from "optional" to "default" on Linux desktop:

- **Ubuntu 24.04+** - Wayland by default
- **Fedora 40+** - Wayland by default, X11 session removed
- **GNOME 49** - X11 session support dropped by default (build-time option only)
- **KDE Plasma** - Formal X11 session end announced for early 2027

**Benefits over X11:**
- Better security (window isolation, no keylogging)
- Native fractional scaling (1.25x, 1.5x, 1.75x)
- Improved multi-monitor handling
- Variable Refresh Rate (VRR/FreeSync/G-Sync)
- Lower latency input handling
- Per-monitor different refresh rates
- Atomic display updates (no tearing)

#### Implementation for FLUI

**Wayland Client Library:**

```rust
// crates/flui-platform/src/platforms/linux/wayland/mod.rs

use wayland_client::{Connection, EventQueue, Proxy};
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

pub struct WaylandPlatform {
    connection: Connection,
    event_queue: EventQueue<AppState>,
    compositor: wl_compositor::WlCompositor,
    xdg_wm_base: xdg_wm_base::XdgWmBase,
}

impl WaylandPlatform {
    pub fn new() -> Result<Self> {
        let connection = Connection::connect_to_env()?;
        let display = connection.display();
        
        let event_queue = connection.new_event_queue();
        let qhandle = event_queue.handle();

        // Get registry and bind globals
        let registry = display.get_registry(&qhandle, ());
        
        // Bind compositor, xdg_wm_base, seat, etc.
        // Implementation details...
        
        Ok(Self {
            connection,
            event_queue,
            compositor,
            xdg_wm_base,
        })
    }

    pub fn create_window(&self, title: &str, width: u32, height: u32) -> WaylandWindow {
        let surface = self.compositor.create_surface();
        let xdg_surface = self.xdg_wm_base.get_xdg_surface(&surface);
        let xdg_toplevel = xdg_surface.get_toplevel();

        xdg_toplevel.set_title(title.to_string());
        xdg_toplevel.set_app_id("com.flui.app".to_string());

        surface.commit();

        WaylandWindow {
            surface,
            xdg_surface,
            xdg_toplevel,
            width,
            height,
        }
    }
}
```

**Fractional Scaling Support:**

```rust
// crates/flui-platform/src/platforms/linux/wayland/scaling.rs

use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;

pub struct FractionalScaling {
    manager: WpFractionalScaleManagerV1,
}

impl FractionalScaling {
    /// Get fractional scale factor (e.g., 1.25, 1.5, 1.75, 2.0)
    pub fn get_scale_factor(&self, surface: &wl_surface::WlSurface) -> f64 {
        let fractional_scale = self.manager.get_fractional_scale(surface);
        
        // Listen for preferred_scale event
        // Returns scale as fixed-point: scale / 120.0
        // Example: 150 = 1.25x scale (150 / 120 = 1.25)
        
        1.5  // Placeholder
    }
}
```

**Variable Refresh Rate (VRR):**

```rust
// crates/flui-platform/src/platforms/linux/wayland/vrr.rs

use wayland_protocols::wp::presentation_time::client::wp_presentation_feedback::WpPresentationFeedback;

pub struct VrrSupport {
    presentation: wp_presentation::WpPresentation,
}

impl VrrSupport {
    /// Enable VRR for adaptive refresh rate
    pub fn enable_vrr(&self, surface: &wl_surface::WlSurface) {
        let feedback = self.presentation.feedback(surface);
        
        // Listen for presented event with refresh rate
        // Adjust frame timing dynamically
    }

    /// Query monitor refresh rate capability
    pub fn get_refresh_rate_range(&self) -> (u32, u32) {
        // Query min/max refresh rates
        (48, 144)  // Example: 48Hz - 144Hz VRR range
    }
}
```

**Crate Assignment:** `flui-platform` (linux wayland module)

---

### 1.2 NVIDIA Wayland Compatibility ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Stable (NVIDIA Driver 590 series, 2025)  
**Impact:** Production-ready Wayland on NVIDIA GPUs

#### Overview

NVIDIA's 590 series driver (2025) fixed long-standing Wayland issues:

- **Explicit Sync Support** - Eliminates flickering and corruption
- **HDR Metadata Handling** - Proper HDR10 support
- **Multi-Monitor Fixes** - Stable multi-display configurations
- **Vulkan Extensions** - Enhanced gaming and professional app support

**Sway 1.11** (June 2025) and **wlroots 0.19.0** are the first versions with explicit sync support, critical for NVIDIA.

#### Implementation

**Explicit Sync Detection:**

```rust
// crates/flui-platform/src/platforms/linux/wayland/nvidia.rs

use wayland_protocols::linux_drm_syncobj::v1::client::wp_linux_drm_syncobj_manager_v1::WpLinuxDrmSyncobjManagerV1;

pub struct NvidiaExplicitSync {
    syncobj_manager: Option<WpLinuxDrmSyncobjManagerV1>,
}

impl NvidiaExplicitSync {
    /// Check if explicit sync is available (NVIDIA 590+)
    pub fn is_available(&self) -> bool {
        self.syncobj_manager.is_some()
    }

    /// Use explicit sync for buffer presentation
    pub fn sync_buffer_presentation(&self, buffer: &wl_buffer::WlBuffer) {
        if let Some(manager) = &self.syncobj_manager {
            // Create sync objects for acquire/release
            let acquire_point = manager.import_timeline(/* fd */);
            let release_point = manager.import_timeline(/* fd */);
            
            // Set sync points for GPU synchronization
            // Prevents tearing and corruption
        }
    }
}
```

**NVIDIA Driver Detection:**

```rust
// crates/flui-platform/src/platforms/linux/gpu_detect.rs

pub enum GpuVendor {
    Nvidia,
    AMD,
    Intel,
    Other,
}

pub fn detect_gpu_vendor() -> GpuVendor {
    // Read /sys/class/drm/card0/device/vendor
    let vendor_id = std::fs::read_to_string("/sys/class/drm/card0/device/vendor")
        .unwrap_or_default();
    
    match vendor_id.trim() {
        "0x10de" => GpuVendor::Nvidia,
        "0x1002" => GpuVendor::AMD,
        "0x8086" => GpuVendor::Intel,
        _ => GpuVendor::Other,
    }
}

pub fn requires_explicit_sync() -> bool {
    matches!(detect_gpu_vendor(), GpuVendor::Nvidia)
}
```

**Crate Assignment:** `flui-platform` (linux wayland nvidia support)

---

### 1.3 Wayland Protocol Extensions ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** KDE Plasma implemented many in 2025  
**Impact:** Advanced window management, PiP, pointer warp

#### KDE Plasma Protocol Support (2025)

KDE implemented support for numerous Wayland protocols in 2025:

- **xdg-toplevel-tag** - Window tagging for grouping
- **xdg_toplevel_icon** - Custom window icons
- **ext_idle_notifier** - Idle detection
- **color_representation** - Color space management
- **fifo** - FIFO scheduling for compositors
- **xx_pip** - Picture-in-Picture windows
- **pointer_warp** - Pointer warping (gaming)
- **single_pixel_buffer** - Efficient solid color surfaces

**Picture-in-Picture (PiP) Support:**

```rust
// crates/flui-platform/src/platforms/linux/wayland/pip.rs

use wayland_protocols::ext::picture_in_picture::v1::client::ext_pip_manager_v1::ExtPipManagerV1;

pub struct PictureInPicture {
    pip_manager: ExtPipManagerV1,
}

impl PictureInPicture {
    /// Create PiP window that stays on top
    pub fn create_pip_window(&self, surface: &wl_surface::WlSurface) {
        let pip_surface = self.pip_manager.get_pip_surface(surface);
        
        // PiP window automatically:
        // - Stays above other windows
        // - Floats above fullscreen apps
        // - Can be dragged/resized by user
        
        pip_surface.set_size(320, 180);  // Small overlay size
    }
}
```

**Pointer Warp (Gaming):**

```rust
// crates/flui-platform/src/platforms/linux/wayland/pointer.rs

use wayland_protocols::ext::pointer_constraints::v1::client::zwp_pointer_constraints_v1::ZwpPointerConstraintsV1;

pub struct PointerWarp {
    constraints: ZwpPointerConstraintsV1,
}

impl PointerWarp {
    /// Warp pointer for first-person camera control
    pub fn warp_pointer(&self, x: f64, y: f64) {
        // Request pointer warp (gaming use case)
        // Not all compositors support this (security concern)
    }

    /// Lock pointer to window (FPS games)
    pub fn lock_pointer(&self, surface: &wl_surface::WlSurface) {
        let locked_pointer = self.constraints.lock_pointer(
            surface,
            // pointer, region, lifetime
        );
        
        // Pointer confined to window, relative motion events
    }
}
```

**Crate Assignment:** `flui-platform` (linux wayland protocols)

---

## 2. Mesa Graphics Stack (Vulkan 1.4)

### 2.1 Mesa 25.x - Vulkan 1.4 Support ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Mesa 25.0 released February 2025  
**Impact:** Modern GPU API across AMD, Intel, NVIDIA

#### Overview

Mesa 25.0 (February 2025) brought **Vulkan 1.4** support to:

- **RADV** - AMD Radeon Vulkan driver
- **ANV** - Intel Vulkan driver
- **NVK** - NVIDIA open-source Vulkan driver (nouveau)
- **Turnip** - Qualcomm Adreno Vulkan driver
- **Asahi** - Apple M1/M2 GPU Vulkan driver (Asahi Linux)
- **Lavapipe** - Software Vulkan implementation

**Key Vulkan 1.4 Features:**
- Dynamic rendering enhancements
- Maintenance extensions (maintenance8, maintenance9)
- Shader float8/bfloat16 support
- Host image copy
- Pipeline binary (faster load times)

#### RADV (AMD) Improvements

**Mesa 25.0:**
- Initial GFX12 (RDNA4) support
- `VK_KHR_depth_clamp_zero_one`
- `VK_KHR_maintenance8`

**Mesa 25.2:**
- `VK_KHR_robustness2`
- `VK_EXT_zero_initialize_device_memory`
- `VK_KHR_maintenance9`
- `VK_KHR_shader_float8`
- `VK_KHR_shader_bfloat16`
- `VK_EXT_host_image_copy`
- `VK_EXT_scalar_block_layout`
- Vulkan video on GFX12 (RDNA4)

**Mesa 25.3:**
- `VK_KHR_pipeline_binary` (faster shader loading)

**Legacy AMD GPU Boost:**
Linux kernel 6.19+ and AMDGPU driver enable RADV Vulkan on GCN 1.0/1.1 GPUs, providing **30% performance improvement** over legacy Radeon driver.

#### ANV (Intel) Improvements

**Mesa 25.2:**
- `VK_KHR_shader_bfloat16`

**Mesa 25.3:**
- `VK_KHR_pipeline_binary`

#### Implementation

```rust
// crates/flui_engine/src/linux/vulkan.rs

use ash::vk;

pub struct LinuxVulkanRenderer {
    instance: ash::Instance,
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
}

impl LinuxVulkanRenderer {
    pub fn new() -> Result<Self> {
        let entry = ash::Entry::linked();
        
        // Request Vulkan 1.4
        let app_info = vk::ApplicationInfo::builder()
            .api_version(vk::API_VERSION_1_4);

        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::builder()
                    .application_info(&app_info)
                    .enabled_extension_names(&[
                        // VK_KHR_wayland_surface for Wayland
                        vk::KhrWaylandSurfaceFn::name().as_ptr(),
                        vk::KhrSurfaceFn::name().as_ptr(),
                    ]),
                None
            )?
        };

        // Detect GPU vendor and driver
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        let physical_device = Self::select_best_gpu(&instance, &physical_devices)?;

        let device_properties = unsafe {
            instance.get_physical_device_properties(physical_device)
        };

        tracing::info!(
            "Selected GPU: {}",
            unsafe { std::ffi::CStr::from_ptr(device_properties.device_name.as_ptr()) }
                .to_string_lossy()
        );

        // Create logical device with Vulkan 1.4 features
        let features14 = vk::PhysicalDeviceVulkan14Features::builder()
            .pipeline_binary(true);  // Faster shader loading

        let device = unsafe {
            instance.create_device(
                physical_device,
                &vk::DeviceCreateInfo::builder()
                    .push_next(&mut features14.build()),
                None
            )?
        };

        Ok(Self {
            instance,
            device,
            physical_device,
        })
    }

    fn select_best_gpu(
        instance: &ash::Instance,
        physical_devices: &[vk::PhysicalDevice]
    ) -> Result<vk::PhysicalDevice> {
        // Prefer discrete GPU over integrated
        for &device in physical_devices {
            let props = unsafe { instance.get_physical_device_properties(device) };
            if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                return Ok(device);
            }
        }

        // Fallback to integrated GPU
        physical_devices.first().copied()
            .ok_or_else(|| anyhow::anyhow!("No Vulkan GPU found"))
    }

    /// Check Mesa driver version
    pub fn get_mesa_version(&self) -> (u32, u32, u32) {
        let props = unsafe {
            self.instance.get_physical_device_properties(self.physical_device)
        };

        let driver_version = props.driver_version;

        // Mesa uses semantic versioning in driver_version
        let major = vk::api_version_major(driver_version);
        let minor = vk::api_version_minor(driver_version);
        let patch = vk::api_version_patch(driver_version);

        (major, minor, patch)
    }
}
```

**Crate Assignment:** `flui_engine` (linux vulkan backend)

---

### 2.2 NVK - NVIDIA Open-Source Vulkan Driver ⭐⭐⭐

**Priority:** MEDIUM (experimental, improving rapidly)  
**Status:** Mesa 25.0+ includes NVK with Vulkan 1.4  
**Impact:** Open-source alternative to NVIDIA proprietary driver

#### Overview

NVK is the **nouveau** project's open-source Vulkan driver for NVIDIA GPUs. Mesa 25.0+ includes Vulkan 1.4 support in NVK.

**Current Status (2025-2026):**
- Vulkan 1.4 support
- `VK_KHR_pipeline_binary` (Mesa 25.3+)
- Performance improving but still behind proprietary driver
- Best for Maxwell (GTX 900) and newer

**Use Cases:**
- Users who need open-source drivers (Wayland stability)
- Steam Deck-like devices
- Development/testing without proprietary blobs

**Detection:**

```rust
// crates/flui_engine/src/linux/driver_detect.rs

pub enum VulkanDriver {
    RADV,        // AMD Mesa driver
    ANV,         // Intel Mesa driver
    NVK,         // NVIDIA Mesa (nouveau) driver
    Proprietary, // NVIDIA proprietary driver
}

pub fn detect_vulkan_driver(device: &ash::Device, physical_device: vk::PhysicalDevice) -> VulkanDriver {
    let props = unsafe {
        device.instance().get_physical_device_properties(physical_device)
    };

    let driver_name = unsafe {
        std::ffi::CStr::from_ptr(props.device_name.as_ptr())
            .to_string_lossy()
    };

    if driver_name.contains("RADV") {
        VulkanDriver::RADV
    } else if driver_name.contains("ANV") {
        VulkanDriver::ANV
    } else if driver_name.contains("NVK") || driver_name.contains("nouveau") {
        VulkanDriver::NVK
    } else if props.vendor_id == 0x10de {
        VulkanDriver::Proprietary
    } else {
        VulkanDriver::Proprietary
    }
}
```

**Crate Assignment:** `flui_engine` (linux driver detection)

---

## 3. PipeWire Audio/Video/Screen Capture

### 3.1 PipeWire 1.6 - Unified Media Framework ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** PipeWire 1.6 released (2025)  
**Impact:** Replaces PulseAudio + JACK, adds screen capture

#### Overview

PipeWire has matured into the **unified audio, video, and screen capture solution** for Linux desktop.

**PipeWire 1.6 Features (2025):**
- **Bluetooth ASHA** - Audio Streaming for Hearing Aid support
- **MIDI 2.0** - Modern MIDI protocol
- **Explicit Sync** - Better Wayland integration
- **Dolby Surround** - Pro Logic II filter configs
- **Vulkan Video** - Vulkan-powered video converters
- **Multi-threaded Execution** - Better performance
- **FFmpeg Integration** - Hardware-accelerated conversions

**Key Benefits:**
- Low latency audio (< 5ms achievable)
- Professional audio routing (JACK compatibility)
- Screen capture for Wayland (via XDG portals)
- Bluetooth codec support (AAC-ELD, Opus, ASHA)
- Video format conversions (Vulkan-accelerated)

#### Audio Backend Implementation

```rust
// crates/flui-platform/src/platforms/linux/pipewire/audio.rs

use pipewire as pw;
use pw::properties::properties;

pub struct PipeWireAudio {
    mainloop: pw::MainLoop,
    context: pw::Context,
    core: pw::Core,
}

impl PipeWireAudio {
    pub fn new() -> Result<Self> {
        pw::init();

        let mainloop = pw::MainLoop::new()?;
        let context = pw::Context::new(&mainloop)?;
        let core = context.connect(None)?;

        Ok(Self {
            mainloop,
            context,
            core,
        })
    }

    /// Play audio buffer
    pub fn play_audio(&self, samples: &[f32], sample_rate: u32) -> Result<()> {
        let props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Game",
            *pw::keys::NODE_NAME => "flui-audio",
        };

        let stream = pw::stream::Stream::new(
            &self.core,
            "flui-playback",
            props,
        )?;

        // Configure audio format
        let mut audio_info = spa::param::audio::AudioInfoRaw::new();
        audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
        audio_info.set_rate(sample_rate);
        audio_info.set_channels(2);

        // Connect stream and write samples
        // Implementation details...

        Ok(())
    }

    /// Record audio from microphone
    pub fn record_audio(&self, callback: impl Fn(&[f32])) -> Result<()> {
        let props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::NODE_NAME => "flui-capture",
        };

        let stream = pw::stream::Stream::new(
            &self.core,
            "flui-capture",
            props,
        )?;

        // Process captured audio
        // Implementation details...

        Ok(())
    }
}
```

#### Screen Capture Implementation

```rust
// crates/flui-platform/src/platforms/linux/pipewire/screen_capture.rs

use pipewire as pw;

pub struct PipeWireScreenCapture {
    mainloop: pw::MainLoop,
    stream: pw::stream::Stream,
}

impl PipeWireScreenCapture {
    /// Capture screen via XDG Desktop Portal
    pub fn capture_screen(&self) -> Result<()> {
        // Use XDG Desktop Portal to request screen capture permission
        // Returns PipeWire stream node ID
        
        let props = properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        };

        let stream = pw::stream::Stream::new(
            &self.core,
            "screen-capture",
            props,
        )?;

        // Connect to portal-provided stream
        // Receive video frames
        
        Ok(())
    }

    /// Process captured frame
    pub fn on_frame(&self, frame: &[u8], width: u32, height: u32) {
        // Convert to texture, display in app
    }
}
```

**Crate Assignment:** `flui-platform` (linux pipewire module)

---

### 3.2 Vulkan Video in PipeWire ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Work in progress (2025)  
**Impact:** Hardware-accelerated video encoding/decoding

#### Overview

PipeWire is adding **Vulkan Video** support for hardware-accelerated video conversions and effects.

**Benefits:**
- GPU-accelerated format conversions (YUV ↔ RGB)
- Video effects (scaling, color correction) on GPU
- Lower CPU usage
- Better integration with Vulkan-based renderers (like FLUI)

**Implementation:**

```rust
// crates/flui_engine/src/linux/video_decode.rs

use ash::vk;

pub struct VulkanVideoDecoder {
    device: ash::Device,
    video_queue: vk::Queue,
}

impl VulkanVideoDecoder {
    /// Decode H.264/H.265 video on GPU
    pub fn decode_frame(&self, encoded_data: &[u8]) -> Result<vk::Image> {
        // Use Vulkan Video extensions
        // VK_KHR_video_queue
        // VK_KHR_video_decode_queue
        // VK_KHR_video_decode_h264 or h265

        // Submit decode commands to video queue
        // Return decoded image

        todo!("Vulkan Video implementation")
    }

    /// Convert YUV to RGB on GPU
    pub fn yuv_to_rgb(&self, yuv_image: vk::Image) -> Result<vk::Image> {
        // Compute shader or fragment shader conversion
        // Efficient GPU-side color space conversion

        todo!()
    }
}
```

**Crate Assignment:** `flui_engine` (linux video decode)

---

## 4. XDG Desktop Portal

### 4.1 File Picker Portal ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Stable, widely adopted  
**Impact:** Native file dialogs across all desktop environments

#### Overview

XDG Desktop Portal provides **unified file picker API** that works across GNOME, KDE, COSMIC, etc.

**Benefits:**
- Sandboxed apps (Flatpak, Snap) can access files safely
- Consistent UI across desktop environments
- Permission-based file access
- Recent files, bookmarks, cloud storage integration

**Implementation:**

```rust
// crates/flui-platform/src/platforms/linux/portal/file_picker.rs

use zbus::{Connection, Proxy};
use std::collections::HashMap;

pub struct FileChooserPortal {
    connection: Connection,
}

impl FileChooserPortal {
    pub fn new() -> Result<Self> {
        let connection = Connection::session()?;
        Ok(Self { connection })
    }

    /// Open file picker dialog
    pub async fn open_file(&self, title: &str, multiple: bool) -> Result<Vec<std::path::PathBuf>> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
        ).await?;

        let mut options = HashMap::new();
        options.insert("title", title);
        options.insert("multiple", if multiple { "true" } else { "false" });

        // Call OpenFile method
        let response: zbus::zvariant::OwnedValue = proxy.call_method(
            "OpenFile",
            &("", options)  // parent_window, options
        ).await?;

        // Parse response URIs
        // org.freedesktop.portal.FileChooser returns array of file:// URIs
        
        Ok(vec![std::path::PathBuf::from("/home/user/file.txt")])
    }

    /// Save file picker dialog
    pub async fn save_file(&self, title: &str, current_name: &str) -> Result<std::path::PathBuf> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
        ).await?;

        let mut options = HashMap::new();
        options.insert("title", title);
        options.insert("current_name", current_name);

        // Call SaveFile method
        let response: zbus::zvariant::OwnedValue = proxy.call_method(
            "SaveFile",
            &("", options)
        ).await?;

        Ok(std::path::PathBuf::from("/home/user/output.txt"))
    }
}
```

**FLUI Widget Integration:**

```rust
// crates/flui_widgets/src/linux/file_dialog.rs

use flui_view::View;

pub struct FileDialog {
    title: String,
    mode: FileDialogMode,
}

pub enum FileDialogMode {
    Open { multiple: bool },
    Save { default_name: String },
}

impl FileDialog {
    pub async fn show(&self) -> Result<Vec<std::path::PathBuf>> {
        let portal = FileChooserPortal::new()?;
        
        match &self.mode {
            FileDialogMode::Open { multiple } => {
                portal.open_file(&self.title, *multiple).await
            }
            FileDialogMode::Save { default_name } => {
                Ok(vec![portal.save_file(&self.title, default_name).await?])
            }
        }
    }
}
```

**Crate Assignment:**
- `flui-platform` (portal D-Bus interface)
- `flui_widgets` (dialog widget)

---

### 4.2 Screen Capture Portal ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Stable, PipeWire integration  
**Impact:** Secure screen recording/streaming

#### Overview

Screen capture portal provides permission-based screen recording for Wayland.

**Features:**
- Window or monitor selection
- Permission dialogs
- PipeWire stream for frames
- Cursor capture option

**Implementation:**

```rust
// crates/flui-platform/src/platforms/linux/portal/screen_cast.rs

use zbus::{Connection, Proxy};

pub struct ScreenCastPortal {
    connection: Connection,
}

impl ScreenCastPortal {
    /// Start screen capture session
    pub async fn start_screen_cast(&self) -> Result<u32> {
        let proxy = Proxy::new(
            &self.connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.ScreenCast",
        ).await?;

        // Create session
        let session: zbus::zvariant::OwnedObjectPath = proxy.call_method(
            "CreateSession",
            &(HashMap::<&str, &str>::new(),)
        ).await?;

        // Select sources (monitors, windows)
        proxy.call_method(
            "SelectSources",
            &(session.clone(), HashMap::from([
                ("types", "1"),  // 1 = monitor, 2 = window
                ("multiple", "false"),
                ("cursor_mode", "2"),  // 2 = embedded cursor
            ]))
        ).await?;

        // Start streaming
        let response: zbus::zvariant::OwnedValue = proxy.call_method(
            "Start",
            &(session, "")  // session, parent_window
        ).await?;

        // Extract PipeWire node ID from response
        let node_id: u32 = 42;  // Parse from response

        Ok(node_id)
    }

    /// Connect to PipeWire stream with node ID
    pub fn connect_stream(&self, node_id: u32) -> Result<PipeWireScreenCapture> {
        // Use PipeWire to receive frames
        PipeWireScreenCapture::new(node_id)
    }
}
```

**Crate Assignment:** `flui-platform` (linux portal screen cast)

---

## 5. Linux Kernel 6.12+ Features

### 5.1 Linux Kernel 6.12 LTS ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Released November 2024, LTS until end of 2026+  
**Impact:** Real-time support, modern scheduler, DRM improvements

#### Overview

Linux Kernel 6.12 is designated **Long Term Support (LTS)** and will be supported until at least the end of December 2026.

**Key Features:**
- **PREEMPT_RT** - Real-time kernel support (mainline)
- **sched_ext** - Extensible scheduler framework
- **DRM Panic QR Codes** - Kernel panic info as QR codes
- **Improved hardware support** - Newer GPUs, CPUs, peripherals

#### Real-Time Support (PREEMPT_RT)

**Use Case for FLUI:**
- Low-latency audio (music apps, games)
- Consistent frame timing (no frame drops)
- Predictable input response

**Configuration:**

```bash
# Enable PREEMPT_RT kernel
# Ubuntu: install linux-realtime package
sudo apt install linux-realtime

# Arch Linux: linux-rt AUR package
yay -S linux-rt

# Check if running RT kernel
uname -a | grep PREEMPT_RT
```

**Thread Priority:**

```rust
// crates/flui-platform/src/platforms/linux/realtime.rs

use libc::{pthread_self, pthread_setschedparam, sched_param, SCHED_FIFO};

pub fn set_realtime_priority(priority: i32) -> Result<()> {
    unsafe {
        let thread = pthread_self();
        let param = sched_param {
            sched_priority: priority.clamp(1, 99),
        };

        let result = pthread_setschedparam(thread, SCHED_FIFO, &param);
        if result != 0 {
            return Err(anyhow::anyhow!("Failed to set RT priority: {}", result));
        }
    }

    Ok(())
}

/// Set render thread to high priority
pub fn configure_render_thread() -> Result<()> {
    // Priority 80-90 for render thread (high but not critical)
    set_realtime_priority(85)?;
    
    tracing::info!("Render thread configured for real-time scheduling");
    Ok(())
}
```

**Crate Assignment:** `flui-platform` (linux realtime)

---

### 5.2 sched_ext - Extensible Scheduler ⭐⭐⭐

**Priority:** MEDIUM  
**Status:** New in Linux 6.12  
**Impact:** Custom schedulers for specific workloads

#### Overview

`sched_ext` allows loading **custom CPU schedulers** as BPF programs.

**Use Cases:**
- Gaming-optimized schedulers (lower latency)
- Multi-threaded rendering optimization
- Background task deprioritization

**Example Schedulers:**
- **scx_rustland** - Rust-based gaming scheduler
- **scx_lavd** - Latency-aware scheduler
- **scx_bpfland** - General-purpose BPF scheduler

**Detection:**

```rust
// crates/flui-platform/src/platforms/linux/scheduler.rs

pub fn is_sched_ext_available() -> bool {
    std::path::Path::new("/sys/kernel/sched_ext").exists()
}

pub fn get_active_scheduler() -> Option<String> {
    std::fs::read_to_string("/sys/kernel/sched_ext/state")
        .ok()
        .map(|s| s.trim().to_string())
}
```

**Crate Assignment:** `flui-platform` (linux scheduler detection)

---

## 6. Rust GUI Ecosystem on Linux

### 6.1 COSMIC Desktop Environment ⭐⭐⭐⭐⭐

**Priority:** CRITICAL (reference implementation)  
**Status:** Stable release December 11, 2025  
**Impact:** First major Rust-based desktop, uses Iced

#### Overview

System76's **COSMIC** desktop environment is the first major desktop environment built entirely in Rust, using the **Iced** GUI framework.

**Technology Stack:**
- **Iced** - Elm-inspired GUI framework
- **Smithay** - Wayland compositor library
- **wgpu** - GPU rendering (same as FLUI!)
- **cosmic-text** - Text rendering

**Relevance to FLUI:**
- Proves Rust GUI viability at scale
- Demonstrates wgpu performance
- Shares many dependencies (wgpu, cosmic-text)
- Reference for Wayland integration

**Lessons from COSMIC:**

1. **wgpu is production-ready** for desktop UI
2. **cosmic-text** provides excellent text rendering (FLUI should consider integration)
3. **Wayland compositor** can be built in Rust (Smithay library)
4. **Iced architecture** works for complex applications

**Potential Integration:**

```rust
// crates/flui_painting/src/text/cosmic_text_integration.rs

use cosmic_text::{Buffer, FontSystem, SwashCache};

pub struct CosmicTextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl CosmicTextRenderer {
    pub fn render_text(&mut self, text: &str, font_size: f32) -> TextLayout {
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, font_size));
        buffer.set_text(&mut self.font_system, text, Attrs::new(), Shaping::Advanced);
        
        // Layout and shape text
        buffer.shape_until_scroll(&mut self.font_system);

        // Rasterize glyphs
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let image = self.swash_cache.get_image(&mut self.font_system, glyph.cache_key);
                // Upload to GPU texture atlas
            }
        }

        TextLayout { /* ... */ }
    }
}
```

**Crate Assignment:** `flui_painting` (cosmic-text integration)

---

### 6.2 Iced Framework ⭐⭐⭐⭐

**Priority:** HIGH (reference for architecture)  
**Status:** Mature, used by COSMIC  
**Impact:** Proven Elm-inspired architecture

#### Overview

**Iced** is a cross-platform Rust GUI framework using Elm architecture (similar to Flutter's approach).

**Architecture:**
```
Application State → View (immutable) → Runtime → Renderer (wgpu)
```

**Similarities to FLUI:**
- wgpu-based rendering
- Reactive state management
- Cross-platform (Linux, Windows, macOS, Web)
- Widget composition

**Key Differences:**
- Iced uses Elm architecture, FLUI uses Flutter's three-tree
- Iced has built-in widget library, FLUI is more modular
- Iced focuses on desktop, FLUI targets mobile too

**What FLUI Can Learn:**
- Widget event handling patterns
- Accessibility integration strategies
- wgpu optimization techniques
- Cross-platform window management

---

### 6.3 egui Framework ⭐⭐⭐

**Priority:** MEDIUM (reference for immediate mode)  
**Status:** Mature, widely used  
**Impact:** Immediate-mode alternative approach

#### Overview

**egui** is an immediate-mode GUI library (like Dear ImGui).

**Key Features:**
- No macros, no DSL - pure Rust
- Very small binaries (WASM-optimized)
- Immediate mode (redraw every frame)
- Good for tools, debug UIs, prototyping

**Use Case for FLUI:**
- Developer tools (FLUI DevTools)
- Debug overlays
- Profiling UI

**Integration Example:**

```rust
// crates/flui_devtools/src/inspector_ui.rs

use egui::Context;

pub struct FluiInspector {
    egui_ctx: Context,
}

impl FluiInspector {
    /// Render debug overlay with egui
    pub fn render_overlay(&mut self, tree_stats: &TreeStats) {
        egui::Window::new("FLUI Inspector").show(&self.egui_ctx, |ui| {
            ui.heading("Render Tree Stats");
            ui.label(format!("Nodes: {}", tree_stats.node_count));
            ui.label(format!("Layout time: {:?}", tree_stats.layout_time));
            ui.label(format!("Paint time: {:?}", tree_stats.paint_time));

            ui.separator();

            if ui.button("Trigger Rebuild").clicked() {
                // Send rebuild signal
            }
        });
    }
}
```

**Crate Assignment:** `flui_devtools` (egui for debug UI)

---

## 7. Implementation Priority Matrix

### Critical Path Features (Q1 2026)

1. **Wayland Platform Support** ⭐⭐⭐⭐⭐
   - **Effort:** High (6-8 weeks)
   - **Impact:** CRITICAL (X11 deprecated)
   - **Crate:** `flui-platform` (linux wayland)

2. **Vulkan 1.4 Renderer** ⭐⭐⭐⭐⭐
   - **Effort:** High (6-8 weeks)
   - **Impact:** CRITICAL (modern GPU API)
   - **Crate:** `flui_engine` (vulkan backend)

3. **XDG File Picker Portal** ⭐⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** HIGH (essential for file I/O)
   - **Crate:** `flui-platform` + `flui_widgets`

### High Priority Features (Q2 2026)

4. **NVIDIA Explicit Sync** ⭐⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** CRITICAL (NVIDIA Wayland stability)
   - **Crate:** `flui-platform` (wayland nvidia)

5. **PipeWire Audio Backend** ⭐⭐⭐⭐
   - **Effort:** Medium (3-4 weeks)
   - **Impact:** HIGH (modern audio)
   - **Crate:** `flui-platform` (pipewire)

6. **Fractional Scaling** ⭐⭐⭐⭐
   - **Effort:** Medium (2 weeks)
   - **Impact:** HIGH (HiDPI displays)
   - **Crate:** `flui-platform` (wayland scaling)

### Medium Priority Features (Q3 2026)

7. **Variable Refresh Rate (VRR)** ⭐⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** MEDIUM (gaming, smooth scrolling)
   - **Crate:** `flui-platform` (wayland vrr)

8. **Screen Capture Portal** ⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** MEDIUM (screen recording apps)
   - **Crate:** `flui-platform` (portal screen cast)

9. **cosmic-text Integration** ⭐⭐⭐⭐
   - **Effort:** Medium (3 weeks)
   - **Impact:** HIGH (better text rendering)
   - **Crate:** `flui_painting`

10. **Real-Time Thread Priority** ⭐⭐⭐
    - **Effort:** Low (1 week)
    - **Impact:** MEDIUM (low-latency apps)
    - **Crate:** `flui-platform` (linux realtime)

### Low Priority Features (Q4 2026)

11. **PiP Protocol Support** ⭐⭐⭐
    - **Effort:** Low (1 week)
    - **Impact:** LOW (niche use case)
    - **Crate:** `flui-platform` (wayland protocols)

12. **Pointer Warp (Gaming)** ⭐⭐
    - **Effort:** Low (1 week)
    - **Impact:** LOW (FPS games only)
    - **Crate:** `flui-platform` (wayland pointer)

13. **sched_ext Detection** ⭐⭐
    - **Effort:** Low (1 week)
    - **Impact:** LOW (future optimization)
    - **Crate:** `flui-platform` (scheduler)

14. **egui DevTools Integration** ⭐⭐⭐
    - **Effort:** Medium (2 weeks)
    - **Impact:** MEDIUM (developer experience)
    - **Crate:** `flui_devtools`

---

## 8. Testing Strategy

### Distribution Testing Matrix

| Distribution | Wayland Compositor | Mesa Version | Kernel | Priority |
|-------------|-------------------|--------------|--------|----------|
| **Ubuntu 24.04+** | Mutter (GNOME) | 25.x | 6.12+ | ⭐⭐⭐⭐⭐ |
| **Fedora 40+** | Mutter (GNOME) | 25.x | 6.12+ | ⭐⭐⭐⭐⭐ |
| **Arch Linux** | User choice | 25.x latest | 6.12+ | ⭐⭐⭐⭐ |
| **Pop!_OS 24.04+** | COSMIC | 25.x | 6.12+ | ⭐⭐⭐⭐⭐ |
| **Debian 13 (Trixie)** | Mutter/Weston | 24.x → 25.x | 6.8+ | ⭐⭐⭐ |
| **openSUSE Tumbleweed** | KDE Plasma | 25.x | 6.12+ | ⭐⭐⭐⭐ |

### GPU Testing Matrix

| GPU | Driver | Vulkan Version | Priority |
|-----|--------|---------------|----------|
| **AMD RDNA2/3** | RADV (Mesa 25.x) | 1.4 | ⭐⭐⭐⭐⭐ |
| **AMD GCN 1.0/1.1** | RADV (Mesa 25.x) | 1.4 | ⭐⭐⭐ |
| **Intel Arc** | ANV (Mesa 25.x) | 1.4 | ⭐⭐⭐⭐ |
| **Intel Xe** | ANV (Mesa 25.x) | 1.4 | ⭐⭐⭐⭐⭐ |
| **NVIDIA RTX 40xx** | Proprietary 590+ | 1.4 | ⭐⭐⭐⭐⭐ |
| **NVIDIA GTX 16xx** | NVK (Mesa 25.x) | 1.4 | ⭐⭐⭐ |

### Automated Testing

```rust
// tests/linux/wayland_platform.rs

#[test]
fn test_wayland_connection() {
    let platform = WaylandPlatform::new().expect("Failed to connect to Wayland");
    assert!(platform.compositor.version() >= 4);
}

#[test]
fn test_vulkan_1_4_support() {
    let renderer = LinuxVulkanRenderer::new().expect("Failed to create Vulkan renderer");
    let (major, minor, _) = renderer.get_mesa_version();
    assert!(major >= 25, "Mesa 25.x required for Vulkan 1.4");
}

#[test]
fn test_fractional_scaling() {
    let platform = WaylandPlatform::new().unwrap();
    let window = platform.create_window("Test", 800, 600);
    
    // Simulate 1.5x scale factor
    let scale = 1.5;
    let logical_size = (800.0 / scale, 600.0 / scale);
    assert_eq!(logical_size, (533.33, 400.0));
}

#[test]
fn test_pipewire_audio() {
    let audio = PipeWireAudio::new().expect("PipeWire not available");
    
    // Play test tone
    let samples: Vec<f32> = (0..48000)
        .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin())
        .collect();
    
    audio.play_audio(&samples, 48000).expect("Audio playback failed");
}
```

### Continuous Integration

```yaml
# .github/workflows/linux_tests.yml
name: Linux Platform Tests

on: [push, pull_request]

jobs:
  test-linux:
    runs-on: ubuntu-24.04
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Install dependencies
        run: |
          sudo apt update
          sudo apt install -y \
            libwayland-dev \
            libvulkan-dev \
            mesa-vulkan-drivers \
            pipewire \
            libpipewire-0.3-dev
      
      - name: Run tests
        run: |
          cargo test --package flui-platform --features linux-wayland
          cargo test --package flui_engine --features vulkan
      
      - name: Check Vulkan support
        run: |
          vulkaninfo --summary
          # Verify Vulkan 1.4 support
```

---

## 9. Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Wayland compositor incompatibilities | Medium | High | Test on multiple compositors (Mutter, KWin, Sway, COSMIC) |
| NVIDIA driver regressions | Low | High | Support both proprietary (590+) and NVK drivers |
| Mesa version fragmentation | Medium | Medium | Require Mesa 25.0+, provide fallbacks for older features |
| PipeWire not available | Low | Medium | Fallback to ALSA for audio |
| XDG Portal missing | Low | High | Provide native file dialogs as fallback |
| Vulkan 1.4 unavailable | Low | High | Fallback to Vulkan 1.3, graceful degradation |

### Distribution Compatibility

- **Ubuntu 24.04+** - Full support (Wayland default, Mesa 25.x)
- **Fedora 40+** - Full support (bleeding edge)
- **Pop!_OS 24.04+ with COSMIC** - Full support (testing ground)
- **Debian 13** - Partial support (older Mesa initially)
- **Older LTS releases** - Limited support (X11 fallback may be needed)

### Timeline Risks

- **Q1 2026:** Wayland + Vulkan 1.4 implementation - **ON TRACK**
- **Q2 2026:** NVIDIA explicit sync, PipeWire integration - **MEDIUM RISK** (driver updates)
- **Q3 2026:** Advanced features (VRR, screen capture) - **LOW RISK**

---

## 10. Resource Requirements

### Engineering Team

- **1 Linux Platform Engineer** - Wayland, XDG portals, window management (full-time, 6 months)
- **1 Graphics Engineer** - Vulkan 1.4, Mesa integration (full-time, 4 months)
- **1 Audio Engineer** - PipeWire integration (part-time, 2 months)
- **1 QA Engineer** - Multi-distro testing, CI/CD (part-time, 6 months)

### Hardware

- **5 test systems** - Different GPUs (AMD RDNA3, Intel Arc, NVIDIA RTX 40xx, AMD GCN legacy, Intel Xe)
- **3 distributions** - Ubuntu 24.04, Fedora 40, Pop!_OS with COSMIC
- **CI/CD infrastructure** - GitHub Actions or GitLab CI (Linux runners)

### Budget Estimate

- **Engineering:** $200k - $400k (4 engineers × varying durations)
- **Hardware:** $4k (test systems)
- **CI/CD:** $1k/year (cloud runners)
- **Total:** $205k - $405k

---

## 11. Conclusion

Linux desktop platform has reached maturity in 2025-2026 with key improvements:

**Must-Have (2026):**
- Wayland platform support (X11 deprecated)
- Vulkan 1.4 renderer with Mesa 25.x
- XDG File Picker Portal (essential for Flatpak/Snap)
- NVIDIA explicit sync (stable Wayland + NVIDIA)

**High-Value (2026):**
- PipeWire audio backend (modern audio/video)
- Fractional scaling (HiDPI displays)
- cosmic-text integration (better text rendering)
- Screen capture portal (screen recording apps)

**Nice-to-Have (2027+):**
- VRR support (gaming, smooth scrolling)
- Real-time thread priority (low-latency)
- PiP protocol (video overlays)
- egui DevTools integration

**Recommended Timeline:**
- **Q1 2026:** Wayland platform, Vulkan 1.4, File Picker
- **Q2 2026:** NVIDIA explicit sync, PipeWire, Fractional scaling
- **Q3 2026:** VRR, Screen capture, cosmic-text
- **Q4 2026:** Polish, performance optimization, DevTools

This positions FLUI to fully leverage modern Linux desktop capabilities while maintaining compatibility with major distributions and GPU vendors.

---

**Next Steps:**
1. Review and approve this roadmap
2. Assign engineering resources
3. Set up multi-distribution testing environment
4. Begin Q1 2026 implementation (Wayland + Vulkan 1.4)

---

## Sources

- [2025 Wayland-NVIDIA GPU Compatibility Leap for Linux Users](https://www.webpronews.com/2025-wayland-nvidia-gpu-compatibility-leap-for-linux-users/)
- [Here's Our Prediction for the Future of Desktop Linux in 2026](https://itsfoss.com/news/linux-future-prediction-2026/)
- [Valve's Linux Efforts, Kernel Improvements & KDE Plasma Wayland Advancements Topped 2025 - Phoronix](https://www.phoronix.com/news/Top-Linux-News-2025)
- [Mesa 25.0 Linux Graphics Stack Brings Vulkan 1.4 Support on RADV, ANV, and NVK](https://9to5linux.com/mesa-25-0-linux-graphics-stack-brings-vulkan-1-4-support-on-radv-anv-and-nvk)
- [Mesa 25.2 Released With Many Improvements For RADV, Intel & NVK Drivers - Phoronix](https://www.phoronix.com/news/Mesa-25.2-Released)
- [Linux Kernel 6.12 Will Be LTS, Supported for Multiple Years](https://9to5linux.com/its-official-linux-kernel-6-12-will-be-lts-supported-for-multiple-years/)
- [PipeWire Is Doing An Excellent Job Handling Audio/Video Streams On The Linux Desktop - Phoronix](https://www.phoronix.com/news/PipeWire-State-2025)
- [A 2025 Survey of Rust GUI Libraries | boringcactus](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)
- [XDG Desktop Portal Documentation](https://flatpak.github.io/xdg-desktop-portal/)
