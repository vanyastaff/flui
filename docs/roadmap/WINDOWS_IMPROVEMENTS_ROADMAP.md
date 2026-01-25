# Windows Platform Improvements Roadmap (2024-2026)

**Document Version:** 1.0  
**Last Updated:** January 25, 2026  
**Research Date:** January 25, 2026  

## Overview

This document outlines modern Windows features and APIs (2024-2026) that could enhance FLUI platform. Based on research of Windows 11 24H2 (2024 Update), Windows 11 25H2 (2025 Update), Windows App SDK 1.6-1.8, and DirectX 12 improvements.

---

## 1. DirectX 12 Latest Features (2024-2025) - GPU Rendering

**Priority:** ⭐⭐⭐⭐⭐ (Critical for performance)  
**Target Crate:** `flui_engine` or new `flui-directx`  
**Windows Requirement:** Windows 11 22H2+

### Features

#### Work Graphs (2024)
- **What:** GPU-side scheduling primitives for complex compute work
- **Benefit:** GPU can spawn and manage work chains without CPU bottleneck
- **Use Case:** GPU-driven UI rendering pipelines
- **API:** Direct3D 12 Work Graphs
- **Requirements:** DirectX 12 Ultimate compatible GPU

**Implementation:**
```rust
// crates/flui-directx/src/work_graphs.rs
pub struct WorkGraphRenderer {
    device: ID3D12Device,
    work_graph_program: ID3D12StateObject,
    backing_memory: ID3D12Resource,
}

impl WorkGraphRenderer {
    pub fn dispatch_gpu_driven_render(&self, root_nodes: &[WorkNode]) {
        // GPU spawns child render tasks autonomously
        // No CPU dispatches for every draw call
    }
}
```

#### Shader Execution Reordering (SER) (2025)
- **What:** Runtime reordering of shading work for coherence
- **Benefit:** Improved performance for divergent, raytraced workloads
- **Use Case:** Complex UI effects with ray tracing
- **API:** Direct3D 12 SER

**Implementation:**
```rust
// crates/flui-directx/src/shader_execution_reordering.rs
pub struct SERShader {
    shader_module: ID3D12PipelineState,
}

impl SERShader {
    pub fn hint_reorder(&self, coherence_hint: CoherenceHint) {
        // Shader code can request runtime reordering
        // Improves ray tracing performance significantly
    }
}
```

#### Cooperative Vectors (2025)
- **What:** Hardware acceleration for vector-math patterns (ML)
- **Benefit:** Fast AI/ML operations in shaders
- **Use Case:** Neural rendering, AI effects in UI
- **API:** Direct3D 12 Cooperative Vectors

**Implementation:**
```rust
// crates/flui-directx/src/ml_acceleration.rs
pub struct MLShaderOps {
    cooperative_vector_support: bool,
}

impl MLShaderOps {
    pub fn matrix_multiply_accelerated(&self, a: Matrix, b: Matrix) -> Matrix {
        // Use hardware-accelerated ML ops in shader
        // 10-100x faster than generic shader code
    }
}
```

### Implementation Plan

**Phase 1:** DirectX 12 Ultimate Foundation
- Upgrade from DirectX 11/12 to DirectX 12 Ultimate
- Add Work Graphs support for GPU-driven rendering
- Benchmark performance vs current approach

**Phase 2:** SER Integration
- Implement Shader Execution Reordering for UI effects
- Add ray-traced shadows/reflections to UI
- Profile divergent workload performance

**Phase 3:** ML Acceleration
- Cooperative Vectors for neural rendering
- AI-based upscaling (similar to DLSS/FSR)
- On-GPU text rendering with ML

### Resources
- [DirectX 12 at Ten Years](https://windowsforum.com/threads/directx-12-at-ten-years-evolution-and-the-future-of-windows-graphics.394477/)
- [Microsoft celebrates 10 years of DirectX 12](https://www.pcgamer.com/hardware/microsoft-celebrates-10-years-of-directx-12-a-decade-of-updates-to-the-low-level-graphics-api-but-its-still-not-the-master-of-all-things-rendering/)

---

## 2. Windows App SDK 1.6-1.8 (2024-2025)

**Priority:** ⭐⭐⭐⭐ (High - Modern Windows APIs)  
**Target Crate:** `flui-platform`  
**Windows Requirement:** Windows 10 1809+

### Features

#### Native AOT Support (SDK 1.6)
- **What:** Ahead-of-Time compilation for .NET apps
- **Benefit:** 50% faster startup, 8x smaller package size
- **Use Case:** FLUI apps using .NET bindings
- **API:** Windows App SDK Native AOT

**Note:** This is primarily for .NET apps. For Rust FLUI, we already have native compilation.

#### Enhanced Package Management (SDK 1.6)
- **What:** Package removal, provisioning, update detection
- **Benefit:** Better app lifecycle management
- **Use Case:** FLUI app deployment and updates
- **API:** Windows App SDK Package Management APIs

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/package_manager.rs
pub struct PackageManager {
    // Detect pending updates
    // Remove old versions
    // Provision for deployment
}

impl PackageManager {
    pub fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
        // Use Windows App SDK Package APIs
    }
    
    pub fn apply_update(&self, update: UpdateInfo) -> Result<()> {
        // Seamless app updates
    }
}
```

#### TabView with Tear-Out Tabs (SDK 1.6)
- **What:** Draggable tabs that can be torn out into new windows
- **Benefit:** Modern browser-like tab experience
- **Use Case:** Multi-document FLUI apps
- **API:** WinUI 3 TabView.CanTearOutTabs

**Implementation:**
```rust
// crates/flui_widgets/src/windows/tab_view.rs
pub struct TabView {
    tabs: Vec<Tab>,
    can_tear_out: bool,
}

impl TabView {
    pub fn enable_tear_out(&mut self) {
        // Native Windows 11 tab tearing
        self.can_tear_out = true;
    }
    
    pub fn on_tab_torn_out(&self, tab: Tab) -> WindowHandle {
        // Create new window with torn-out tab
    }
}
```

#### WebView2 SDK Flexibility (SDK 1.6)
- **What:** NuGet reference for WebView2 instead of embedded version
- **Benefit:** Choose newer WebView2 versions
- **Use Case:** Embedded web content in FLUI
- **API:** Microsoft.Web.WebView2 NuGet

#### Dynamic Dependencies API (SDK 1.7)
- **What:** Delegates to Windows 11 native implementation
- **Benefit:** Better performance and robustness
- **Use Case:** Framework package dependencies
- **API:** Windows App SDK Dynamic Dependencies
- **Requirements:** Windows 11 24H2+ (10.0.26100.0)

#### ContentIsland API (SDK 1.7)
- **What:** New hosting scenarios, enhanced rendering, synchronization
- **Benefit:** Advanced composition and interop
- **Use Case:** FLUI rendering to external surfaces
- **API:** Microsoft.UI.Content namespace

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/content_island.rs
pub struct ContentIsland {
    island: IContentIsland,
}

impl ContentIsland {
    pub fn create_for_hwnd(hwnd: HWND) -> Self {
        // Create content island for rendering
    }
    
    pub fn sync_with_compositor(&self) {
        // Synchronize with DWM compositor
    }
}
```

#### Windows ML Expanded Support (SDK 1.8)
- **What:** ML on Windows 10 1809+, Windows Server 2019+
- **Benefit:** AI features on older Windows versions
- **Use Case:** Neural rendering, AI effects
- **API:** Windows ML with AMD MiGraphX support

**Implementation:**
```rust
// crates/flui-ml/src/windows_ml.rs
pub struct WindowsMLEngine {
    device: LearningModelDevice,
    session: LearningModelSession,
}

impl WindowsMLEngine {
    pub fn infer(&self, input: Tensor) -> Result<Tensor> {
        // Run ML inference for UI effects
        // AMD GPU acceleration via MiGraphX
    }
}
```

#### LanguageModel API (SDK 1.8)
- **What:** Phi Silica for text generation with content moderation
- **Benefit:** Local AI text generation
- **Use Case:** Smart autocomplete, AI assistants in FLUI apps
- **API:** LanguageModel API

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/language_model.rs
pub struct LanguageModel {
    model: ILanguageModel,
}

impl LanguageModel {
    pub async fn generate_text(&self, prompt: &str) -> Result<String> {
        // Local AI text generation with Phi Silica
        // Built-in content moderation
    }
}
```

#### New File Picker API (SDK 1.8)
- **What:** Elevated process support, simplified WinUI 3 usage
- **Benefit:** Easier file dialogs, works with elevated apps
- **Use Case:** File open/save in FLUI apps
- **API:** Microsoft.Windows.Storage.Pickers

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/file_picker.rs
pub struct FilePicker {
    picker: IFilePicker,
    window_id: WindowId,
}

impl FilePicker {
    pub fn new_for_window(window_id: WindowId) -> Self {
        // No need for HWND, just WindowId
        // Works in elevated processes
    }
    
    pub async fn pick_single_file(&self) -> Result<Option<PathBuf>> {
        // Modern async file picker
    }
}
```

### Resources
- [What's new in Windows App SDK 1.6](https://blogs.windows.com/windowsdeveloper/2024/09/04/whats-new-in-windows-app-sdk-1-6/)
- [Windows App SDK 1.7 release notes](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-7)
- [Windows App SDK 1.6 release notes](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-6)
- [Windows App SDK stable channel](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/stable-channel)

---

## 3. Auto HDR and Display Improvements (2024-2025)

**Priority:** ⭐⭐⭐⭐ (High - Visual quality)  
**Target Crate:** `flui-platform` + `flui_engine`  
**Windows Requirement:** Windows 11 22H2+

### Features

#### Auto HDR for DirectX 11/12
- **What:** Automatic HDR conversion for SDR content
- **Benefit:** Enhanced visual quality on HDR displays
- **Use Case:** FLUI UI rendering on HDR monitors
- **API:** Auto HDR (system-level)

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/hdr.rs
pub struct HDRDisplay {
    monitor: HMONITOR,
    hdr_capable: bool,
    auto_hdr_enabled: bool,
}

impl HDRDisplay {
    pub fn enable_auto_hdr_for_window(&self, hwnd: HWND) -> Result<()> {
        // Opt-in to Auto HDR
        // System automatically upgrades SDR → HDR
    }
    
    pub fn get_hdr_metadata(&self) -> HDRMetadata {
        // Max/min luminance, color gamut
    }
}
```

#### Automatic HDR Switching
- **What:** HDR video streaming even when HDR is off
- **Benefit:** Seamless HDR content without manual settings
- **Use Case:** Video playback in FLUI apps
- **API:** System > Display > HDR automatic switching

#### HDR Color Space
- **What:** Wide color gamut support (DCI-P3, Rec. 2020)
- **Benefit:** Accurate color reproduction
- **Use Case:** Design/creative FLUI applications

**Implementation:**
```rust
// crates/flui_types/src/color.rs
pub enum ColorSpace {
    sRGB,        // Standard
    DisplayP3,   // Wide gamut
    Rec2020,     // HDR wide gamut
    AdobeRGB,    // Professional
}

impl Color {
    pub fn from_hdr(r: f32, g: f32, b: f32, nits: f32) -> Self {
        // HDR color with brightness in nits
    }
}
```

### Fixed Issues (February 2025)
- ✅ Auto HDR game crashes fixed (KB5051987)
- ✅ Oversaturated graphics fixed
- ✅ Color accuracy improved
- ✅ USB audio device issues resolved

### Resources
- [Use Auto HDR for better gaming in Windows](https://support.microsoft.com/en-us/windows/use-auto-hdr-for-better-gaming-in-windows-0cce8402-3de5-4512-a742-e027ca7aa79c)
- [Microsoft fixes Auto HDR bugs](https://overclock3d.net/news/software/microsoft-finally-fixes-windows-11-24h2s-auto-hdr-bugs/)
- [February 2025 Windows 11 Update](https://windowsforum.com/threads/february-2025-windows-11-update-fixes-for-auto-hdr-audio-and-usb-issues.351894/)

---

## 4. Snap Layouts Improvements (24H2+)

**Priority:** ⭐⭐⭐ (Medium - Window management)  
**Target Crate:** `flui-platform`  
**Windows Requirement:** Windows 11 24H2+

### Features

#### Inline Snap Education
- **What:** Contextual messages when accidentally triggering snap
- **Benefit:** Better user discoverability of snap features
- **Use Case:** Help users learn window tiling
- **API:** System-level (Settings > System > Multitasking)

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/snap_layouts.rs
impl WindowsWindow {
    pub fn enable_snap_layouts(&self) -> Result<()> {
        // Opt-in to Windows 11 snap layouts
        // Show maximize button snap menu
    }
    
    pub fn get_snap_layout_recommendations(&self) -> Vec<SnapLayout> {
        // Query system for available snap zones
    }
}
```

#### Snap Bar (Top-Center Drag)
- **What:** Drag window to top-center to show snap bar
- **Benefit:** Quick access to snap layouts
- **Use Case:** User-friendly window organization

#### Snap Flyout (Maximize Button Hover)
- **What:** Hover maximize button to see snap options
- **Benefit:** Discoverable snap layouts
- **Use Case:** Multi-window FLUI apps

#### Keyboard Shortcuts Education
- **What:** Inline messages teach Win + Arrow keys
- **Benefit:** Power user efficiency
- **Shortcuts:**
  - `Win + Left/Right` - Snap left/right
  - `Win + Up` - Maximize
  - `Win + Down` - Restore/minimize

### Developer Integration
```rust
// crates/flui-platform/src/platforms/windows/window.rs
impl PlatformWindow for WindowsWindow {
    fn support_snap_layouts(&self) -> bool {
        // Return true to enable snap menu on maximize button
        true
    }
    
    fn on_snap_layout_changed(&self, layout: SnapLayout) {
        // Callback when user snaps window
    }
}
```

### Resources
- [Snap Your Windows - Microsoft Support](https://support.microsoft.com/en-us/windows/snap-your-windows-885a9b1e-a983-a3b1-16cd-c531795e6241)
- [Support snap layouts for desktop apps](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu)
- [Windows 11 improved window snapping](https://www.neowin.net/news/windows-11-to-get-improved-window-snapping-here-is-how-to-enable-it/)

---

## 5. Windows AI Foundry (24H2+)

**Priority:** ⭐⭐⭐ (Medium - AI features)  
**Target Crate:** New `flui-ai` or `flui-platform`  
**Windows Requirement:** Windows 11 24H2+

### Features

#### Windows ML API
- **What:** Built-in AI inferencing runtime
- **Benefit:** Cross-hardware AI (CPU, GPU, NPU)
- **Use Case:** On-device ML models for UI
- **Hardware:** AMD, Intel, Nvidia, Qualcomm

**Implementation:**
```rust
// crates/flui-ai/src/windows_ml.rs
pub struct AIInferenceEngine {
    device: LearningModelDevice,
    execution_provider: ExecutionProvider,
}

pub enum ExecutionProvider {
    CPU,
    GPU(GPUVendor),
    NPU,  // For Copilot+ PCs
}

impl AIInferenceEngine {
    pub fn infer_async(&self, model: &Model, input: Tensor) -> Future<Tensor> {
        // Run ML model on best available hardware
    }
}
```

#### App Actions API (24H2)
- **What:** Interface between apps and Click to Do
- **Benefit:** System-wide contextual actions
- **Use Case:** Text/image actions in FLUI apps
- **API:** App Actions API

**Implementation:**
```rust
// crates/flui-platform/src/platforms/windows/app_actions.rs
pub struct AppActions {
    actions: Vec<AppAction>,
}

pub struct AppAction {
    name: String,
    icon: Image,
    handler: Box<dyn Fn(ActionContext)>,
}

impl AppActions {
    pub fn register_text_action(&mut self, action: AppAction) {
        // Register action for selected text
        // Appears in Click to Do menu
    }
}
```

#### NPU Acceleration (Copilot+ PCs)
- **What:** 40+ TOPS neural processing unit
- **Benefit:** Ultra-fast AI inference
- **Use Case:** Real-time AI effects, Live Captions
- **Requirements:** Copilot+ PC hardware

**Implementation:**
```rust
// crates/flui-ai/src/npu.rs
pub struct NPUAccelerator {
    available: bool,
    tops: f32,  // Trillions of ops per second
}

impl NPUAccelerator {
    pub fn is_copilot_plus_pc(&self) -> bool {
        self.tops >= 40.0
    }
    
    pub fn run_on_npu(&self, model: &Model, input: Tensor) -> Result<Tensor> {
        // Ultra-fast inference on NPU
        // 10-100x faster than GPU for small models
    }
}
```

### Resources
- [What's new in Windows 11, version 24H2](https://learn.microsoft.com/en-us/windows/whats-new/whats-new-windows-11-version-24h2)
- [Build 2025: Windows 11 Gets New Developer Capabilities](https://www.thurrott.com/dev/321118/build-2025-windows-11-gets-new-developer-capabilities)

---

## 6. System Improvements (24H2+)

**Priority:** ⭐⭐ (Low - General improvements)  
**Target Crate:** `flui-platform`  
**Windows Requirement:** Windows 11 24H2+

### Features

#### Wi-Fi 7 Support (802.11be)
- **What:** Next-gen Wi-Fi standard
- **Benefit:** Faster data transfer, lower latency
- **Use Case:** Network-dependent FLUI apps
- **API:** Standard networking APIs

#### 7-Zip/TAR Archive Creation
- **What:** Native compression support
- **Benefit:** No external tools needed
- **Use Case:** File management in FLUI apps
- **API:** Windows Shell APIs

#### Checkpoint Cumulative Updates
- **What:** Differential updates based on previous cumulative
- **Benefit:** Faster updates, less bandwidth, smaller downloads
- **Use Case:** Better user experience
- **API:** System-level (Windows Update)

#### SSE4.2 and POPCNT Required
- **What:** New minimum CPU requirements
- **Benefit:** Use modern CPU instructions
- **Use Case:** Performance optimizations in FLUI
- **API:** x86-64-v2 instruction set

**Implementation:**
```rust
// crates/flui_types/src/simd.rs
#[cfg(target_feature = "sse4.2")]
pub fn vector_math_sse42(a: &[f32], b: &[f32]) -> Vec<f32> {
    // Use SSE4.2 for fast math
}

#[cfg(target_feature = "popcnt")]
pub fn count_bits(mask: u64) -> u32 {
    mask.count_ones()  // Uses POPCNT instruction
}
```

### Resources
- [Windows 11, version 24H2](https://en.wikipedia.org/wiki/Windows_11,_version_24H2)
- [What's new in Windows 11, version 24H2](https://learn.microsoft.com/en-us/windows/whats-new/whats-new-windows-11-version-24h2)

---

## Implementation Priority Matrix

| Feature | Priority | Effort | Impact | Crate | Windows Version |
|---------|----------|--------|--------|-------|-----------------|
| DirectX 12 Work Graphs | ⭐⭐⭐⭐⭐ | High | High | flui_engine | 11 22H2+ |
| Windows App SDK 1.8 APIs | ⭐⭐⭐⭐ | Medium | High | flui-platform | 10 1809+ |
| Auto HDR Support | ⭐⭐⭐⭐ | Low | Medium | flui-platform | 11 22H2+ |
| DirectX 12 SER | ⭐⭐⭐⭐ | Medium | Medium | flui_engine | 11 24H2+ |
| Snap Layouts | ⭐⭐⭐ | Low | Low | flui-platform | 11 24H2+ |
| Windows ML | ⭐⭐⭐ | Medium | Medium | flui-ai | 10 1809+ |
| Cooperative Vectors | ⭐⭐⭐ | Medium | Low | flui_engine | 11 25H2+ |
| TabView Tear-Out | ⭐⭐⭐ | Low | Low | flui_widgets | 10 1809+ |
| NPU Acceleration | ⭐⭐ | High | Low | flui-ai | 11 24H2+ |
| App Actions API | ⭐⭐ | Medium | Low | flui-platform | 11 24H2+ |

---

## Recommended Implementation Order

### Phase 1: Foundation (High Impact, Moderate Risk)
1. ✅ **Auto HDR Support** - Easy win, better visuals
2. ✅ **Snap Layouts Integration** - Simple API
3. ✅ **File Picker API (SDK 1.8)** - Critical for file dialogs

### Phase 2: App SDK Integration (High Impact, Medium Risk)
4. ✅ **Package Management APIs** - App lifecycle
5. ✅ **WebView2 Flexibility** - Modern web embedding
6. ✅ **TabView with Tear-Out** - Modern UI pattern

### Phase 3: Graphics (High Impact, High Risk)
7. ✅ **DirectX 12 Work Graphs** - GPU-driven rendering
   - Major architectural change
   - Significant performance gains
8. ✅ **DirectX 12 SER** - Ray tracing optimization
9. ✅ **Cooperative Vectors** - ML acceleration

### Phase 4: AI Features (Medium Impact, Medium Risk)
10. ✅ **Windows ML Integration** - On-device AI
11. ✅ **LanguageModel API** - Local text generation
12. ✅ **NPU Acceleration** - For Copilot+ PCs

---

## Architectural Considerations

### DirectX 12 vs wgpu

**Current:** FLUI likely uses wgpu (cross-platform)

**Option A: Keep wgpu** (Recommended)
```
flui_painting (Lyon CPU) → flui_engine (wgpu) → wgpu DirectX 12 backend
```
- ✅ Cross-platform (Windows, Linux, macOS)
- ✅ Community-maintained
- ⚠️ May lag behind DirectX 12 features

**Option B: Add Native DirectX 12** (Maximum Performance)
```
flui_painting → flui-directx (Native DirectX 12 API)
```
- ✅ Access to all DirectX 12 features (Work Graphs, SER, Cooperative Vectors)
- ✅ Maximum performance
- ❌ Windows-only
- ❌ More maintenance

**Recommendation:** Start with **Option A**, add **Option B** as optional feature:
```rust
[features]
default = ["wgpu-backend"]
directx12-native = ["native-dx12"]  # Windows-only optimizations
```

### Windows App SDK Integration

**Option A: Use windows-rs** (Current approach)
```rust
use windows::Win32::*;  // Raw Win32 APIs
```
- ✅ Direct Win32 access
- ✅ Full control
- ❌ More code to write

**Option B: Use Windows App SDK via C++/WinRT**
```rust
#[cxx::bridge]  // Use cxx for C++ interop
mod ffi {
    unsafe extern "C++" {
        include!("windows_app_sdk.h");
        fn create_file_picker() -> UniquePtr<FilePicker>;
    }
}
```
- ✅ Access to modern Windows App SDK
- ✅ WinUI 3 components
- ❌ Requires C++ build

**Recommendation:** Hybrid approach:
- Keep **windows-rs** for Win32 core
- Add **C++/WinRT bridge** for Windows App SDK 1.8 APIs

---

## Testing Requirements

### Windows Version Testing Matrix

| Feature | Win 10 1809+ | Win 11 22H2 | Win 11 24H2 | Win 11 25H2 |
|---------|--------------|-------------|-------------|-------------|
| Work Graphs | ❌ | ✅ | ✅ | ✅ |
| Auto HDR | ❌ | ✅ | ✅ | ✅ |
| SER | ❌ | ❌ | ✅ | ✅ |
| Cooperative Vectors | ❌ | ❌ | ❌ | ✅ |
| Windows App SDK 1.8 | ✅ | ✅ | ✅ | ✅ |
| Snap Layouts | ❌ | ✅ | ✅ Enhanced | ✅ |
| NPU Acceleration | ❌ | ❌ | ✅ Copilot+ | ✅ Copilot+ |

### Hardware Testing Matrix

| Feature | Intel CPU | AMD CPU | Intel GPU | AMD GPU | Nvidia GPU | NPU |
|---------|-----------|---------|-----------|---------|------------|-----|
| Work Graphs | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| SER | ✅ | ✅ | ⚠️ | ✅ | ✅ | ❌ |
| Cooperative Vectors | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| Windows ML | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| NPU Acceleration | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Copilot+ |

---

## Dependencies

### New Rust Crates Needed

```toml
# For DirectX 12
windows = "0.58"           # Already have, upgrade to latest
windows-implement = "0.58"
d3d12 = "0.8"              # Direct3D 12 bindings

# For Windows App SDK (via C++/WinRT)
cxx = "1.0"                # C++ interop
windows-app-sdk = { git = "..." }  # Custom wrapper

# For ML
windows-ml = "0.1"         # May need to create wrapper
onnxruntime = "1.17"       # ONNX runtime for Windows ML

# For HDR
windows-hdr = { path = "..." }  # Custom HDR utilities
```

### C++ Dependencies (for Windows App SDK)

```cmake
# CMakeLists.txt
find_package(Microsoft.WindowsAppSDK 1.8 REQUIRED)
find_package(Microsoft.Windows.CppWinRT REQUIRED)
```

---

## References

### Official Documentation
- [Windows 11 Release Information](https://learn.microsoft.com/en-us/windows/release-health/windows11-release-information)
- [Windows App SDK Stable Channel](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/stable-channel)
- [What's New - Windows SDK](https://learn.microsoft.com/en-us/windows/apps/windows-sdk/release-notes)
- [DirectX Developer](https://developer.nvidia.com/directx)

### Release Notes
- [Windows 11 24H2 What's New](https://learn.microsoft.com/en-us/windows/whats-new/whats-new-windows-11-version-24h2)
- [Windows App SDK 1.6](https://blogs.windows.com/windowsdeveloper/2024/09/04/whats-new-in-windows-app-sdk-1-6/)
- [Windows App SDK 1.7](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-7)
- [Windows App SDK 1.8 GitHub](https://github.com/microsoft/WindowsAppSDK/releases)

### Community Resources
- [DirectX 12 at Ten Years](https://windowsforum.com/threads/directx-12-at-ten-years-evolution-and-the-future-of-windows-graphics.394477/)
- [Windows 11 2024 Update XDA](https://www.xda-developers.com/windows-11-24h2/)
- [Best features in 2025 for Windows 11](https://www.windowscentral.com/microsoft/windows-11/best-features-microsoft-rolled-out-in-2025-for-windows-11-versions-25h2-and-24h2)

---

**Last Updated:** January 25, 2026  
**Next Review:** After Windows 12 announcement (expected 2026)
