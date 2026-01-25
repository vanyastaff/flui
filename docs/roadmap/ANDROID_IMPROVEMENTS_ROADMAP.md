# Android Platform Improvements Roadmap for FLUI

**Document Version:** 1.0  
**Last Updated:** January 25, 2026  
**Target Platform:** Android 15 (API 35) and Android 16 "Baklava" (API 36)  
**FLUI Crate:** `flui-platform` (android module), `flui_engine` (Vulkan backend)

---

## Executive Summary

This document outlines modern Android platform features and improvements relevant to FLUI framework implementation. Android has undergone significant architectural changes in 2024-2025:

- **Android 15** (Released October 15, 2024) - 16KB page size requirement, ANGLE as default OpenGL ES driver
- **Android 16 "Baklava"** (Stable June 10, 2025, Beta January 23, 2026) - Progress Notifications, Embedded Photo Picker, Desktop Mode

**Key Architectural Changes:**
1. **16KB Page Size Requirement** - Major memory management shift (from 4KB)
2. **ANGLE Driver Default** - OpenGL ES on Vulkan for 2025+ devices
3. **Desktop Mode** - Tablet desktop-like windowing (late 2025)
4. **Dual SDK Release** - Major + minor releases per year

---

## 1. Android 15 Features (API Level 35)

### 1.1 16KB Page Size Requirement ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Mandatory for API 35+ targeting (August 2025 deadline)  
**Impact:** Architectural change for memory management

#### Overview

Android 15 introduces 16KB page size support to improve performance and reduce memory overhead. Apps targeting API 35+ **MUST** support 16KB pages.

**Performance Impact:**
- 5-10% overall performance improvement
- Reduced memory fragmentation
- Better TLB efficiency
- Lower page fault overhead

#### Implementation Requirements

**NDK r26+ Required:**
```rust
// crates/flui-platform/src/platforms/android/memory.rs

/// Query page size at runtime
pub fn get_page_size() -> usize {
    unsafe {
        libc::sysconf(libc::_SC_PAGESIZE) as usize
    }
}

/// Page-aligned memory allocator
pub struct PageAlignedAllocator {
    page_size: usize,
}

impl PageAlignedAllocator {
    pub fn new() -> Self {
        Self {
            page_size: get_page_size(),
        }
    }

    pub fn allocate(&self, size: usize) -> *mut u8 {
        let aligned_size = (size + self.page_size - 1) & !(self.page_size - 1);
        unsafe {
            libc::memalign(self.page_size, aligned_size) as *mut u8
        }
    }
}
```

**Build System Updates:**
```toml
# Cargo.toml
[target.'cfg(target_os = "android")'.dependencies]
# NDK r26 minimum for 16KB page size
ndk = "0.9"  # Ensure NDK r26+
ndk-sys = "0.6"
```

**Testing Matrix:**
- 4KB page size devices (legacy)
- 16KB page size devices (Pixel 9+, 2025+ flagships)
- Runtime page size detection

**Crate Assignment:** `flui-platform` (android memory module)

---

### 1.2 ANGLE OpenGL ES Driver (Default 2025+) ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Default on Android 15+ devices shipping in 2025  
**Impact:** OpenGL ES rendering now runs on Vulkan backend

#### Overview

ANGLE (Almost Native Graphics Layer Engine) translates OpenGL ES calls to Vulkan. Starting in 2025, new Android devices ship with ANGLE as the default OpenGL ES driver.

**Benefits:**
- Better performance (Vulkan efficiency)
- Improved driver consistency
- Reduced driver bugs (single Vulkan backend)
- Better security (Vulkan validation layers)

**Implications for FLUI:**

If FLUI uses OpenGL ES (via `glutin` or similar):
- No code changes needed (ANGLE is transparent)
- Performance may improve automatically
- Vulkan backend benefits (better GPU scheduling)

**Recommended Approach:**

```rust
// crates/flui_engine/src/android/renderer.rs

pub enum AndroidBackend {
    /// Native Vulkan (preferred)
    Vulkan,
    /// OpenGL ES (via ANGLE, fallback)
    OpenGLES,
}

impl AndroidRenderer {
    pub fn new(window: &AndroidWindow) -> Result<Self> {
        // Prefer native Vulkan over ANGLE-translated OpenGL ES
        if Self::supports_vulkan() {
            Self::new_vulkan(window)
        } else {
            Self::new_opengl_es(window)  // ANGLE backend
        }
    }

    fn supports_vulkan() -> bool {
        // Check for Vulkan 1.1+ support
        unsafe {
            let instance = ash::Entry::load().unwrap()
                .create_instance(&vk::InstanceCreateInfo::default(), None);
            instance.is_ok()
        }
    }
}
```

**Crate Assignment:** `flui_engine` (android backend selection)

---

### 1.3 Edge-to-Edge Rendering (Default) ⭐⭐⭐

**Priority:** MEDIUM  
**Status:** Default for API 35+ apps  
**Impact:** Window content extends under system bars

#### Overview

Android 15 makes edge-to-edge rendering the default. Apps must handle insets properly to avoid content being obscured by status/navigation bars.

**Implementation:**

```rust
// crates/flui-platform/src/platforms/android/window.rs

use ndk::native_window::NativeWindow;
use jni::JNIEnv;

pub struct WindowInsets {
    pub top: f32,      // Status bar height
    pub bottom: f32,   // Navigation bar height
    pub left: f32,     // Left system bar
    pub right: f32,    // Right system bar
}

impl AndroidWindow {
    /// Enable edge-to-edge rendering
    pub fn enable_edge_to_edge(&self, env: &JNIEnv) {
        // WindowCompat.setDecorFitsSystemWindows(window, false)
        let window_compat = env.find_class("androidx/core/view/WindowCompat").unwrap();
        let method = env.get_static_method_id(
            window_compat,
            "setDecorFitsSystemWindows",
            "(Landroid/view/Window;Z)V"
        ).unwrap();
        
        env.call_static_method_unchecked(
            window_compat,
            method,
            &[self.java_window.into(), false.into()]
        ).unwrap();
    }

    /// Query system window insets
    pub fn get_insets(&self, env: &JNIEnv) -> WindowInsets {
        // WindowInsetsCompat.getInsets(view, WindowInsetsCompat.Type.systemBars())
        // Implementation via JNI calls
        WindowInsets {
            top: 24.0,     // Example values, query from Java
            bottom: 48.0,
            left: 0.0,
            right: 0.0,
        }
    }
}
```

**Crate Assignment:** `flui-platform` (android window insets)

---

### 1.4 FileIntegrityManager API ⭐⭐

**Priority:** LOW (security-focused apps only)  
**Status:** New in API 35  
**Impact:** Verify app asset integrity at runtime

#### Overview

FileIntegrityManager allows apps to verify that assets haven't been tampered with after installation.

**Use Case for FLUI:**
- Verify shader binaries haven't been modified
- Ensure font files are authentic
- Protect against asset injection attacks

**Implementation:**

```rust
// crates/flui_assets/src/android/integrity.rs

use jni::JNIEnv;
use jni::objects::JObject;

pub struct FileIntegrityVerifier {
    manager: JObject,
}

impl FileIntegrityVerifier {
    pub fn verify_asset(&self, env: &JNIEnv, path: &str) -> Result<bool> {
        // FileIntegrityManager.isApkVeritySupported()
        // FileIntegrityManager.setupFsVerity(file)
        
        let is_supported = env.call_method(
            self.manager,
            "isApkVeritySupported",
            "()Z",
            &[]
        )?.z()?;

        if !is_supported {
            return Ok(true);  // Skip verification if not supported
        }

        // Verify file integrity
        // Implementation via JNI
        Ok(true)
    }
}
```

**Crate Assignment:** `flui_assets` (android integrity checks)

---

### 1.5 Private Space (User Privacy) ⭐

**Priority:** LOW  
**Status:** New in API 35  
**Impact:** Separate workspace for sensitive apps

Not directly relevant to FLUI framework (user-facing feature), but worth noting for app developers using FLUI.

---

## 2. Android 16 "Baklava" Features (API Level 36)

### 2.1 Progress Notifications ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Stable June 10, 2025  
**Impact:** Native progress UI for long-running operations

#### Overview

Android 16 introduces `Notification.ProgressStyle` for rich progress notifications with circular/linear progress indicators.

**Use Case for FLUI:**
- Asset loading progress
- Shader compilation progress
- File download progress

**Implementation:**

```rust
// crates/flui-platform/src/platforms/android/notifications.rs

use jni::JNIEnv;
use jni::objects::JObject;

pub enum ProgressStyle {
    Linear,
    Circular,
}

pub struct ProgressNotification {
    notification_id: i32,
    title: String,
    progress: f32,  // 0.0 - 1.0
    style: ProgressStyle,
}

impl ProgressNotification {
    pub fn show(&self, env: &JNIEnv) {
        // NotificationCompat.Builder(context, CHANNEL_ID)
        //     .setSmallIcon(R.drawable.icon)
        //     .setContentTitle(title)
        //     .setProgress(100, (progress * 100) as i32, false)
        //     .setStyle(ProgressStyle.Linear)
        
        let builder = env.new_object(
            "androidx/core/app/NotificationCompat$Builder",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[/* context, channel_id */]
        ).unwrap();

        // Set progress
        env.call_method(
            builder,
            "setProgress",
            "(IIZ)Landroidx/core/app/NotificationCompat$Builder;",
            &[100.into(), (self.progress * 100.0) as i32.into(), false.into()]
        ).unwrap();

        // Show notification
        // Implementation via JNI
    }

    pub fn update_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
        // Update notification
    }

    pub fn dismiss(&self, env: &JNIEnv) {
        // NotificationManager.cancel(notification_id)
    }
}
```

**Crate Assignment:** `flui-platform` (android notifications)

---

### 2.2 Embedded Photo Picker ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Stable June 10, 2025  
**Impact:** Native photo picker in app UI (no activity launch)

#### Overview

Android 16 introduces embedded photo picker APIs that allow displaying the photo picker **inside your app's view hierarchy** instead of launching a separate activity.

**Benefits:**
- Better UX (no activity transition)
- Seamless integration in UI
- Custom layout embedding

**Implementation:**

```rust
// crates/flui-platform/src/platforms/android/photo_picker.rs

use jni::JNIEnv;
use jni::objects::{JObject, JValue};

pub struct EmbeddedPhotoPicker {
    picker_view: JObject,
}

impl EmbeddedPhotoPicker {
    /// Create embedded photo picker
    pub fn new(env: &JNIEnv, parent_view: &JObject) -> Result<Self> {
        // PhotoPicker.Builder(context)
        //     .setMaxItems(10)
        //     .setEmbedded(true)
        //     .build()
        
        let builder = env.new_object(
            "androidx/activity/result/contract/PhotoPicker$Builder",
            "(Landroid/content/Context;)V",
            &[/* context */]
        )?;

        env.call_method(
            builder,
            "setMaxItems",
            "(I)Landroidx/activity/result/contract/PhotoPicker$Builder;",
            &[10.into()]
        )?;

        env.call_method(
            builder,
            "setEmbedded",
            "(Z)Landroidx/activity/result/contract/PhotoPicker$Builder;",
            &[true.into()]
        )?;

        let picker_view = env.call_method(
            builder,
            "build",
            "()Landroid/view/View;",
            &[]
        )?.l()?;

        // Add to parent view
        env.call_method(
            parent_view,
            "addView",
            "(Landroid/view/View;)V",
            &[picker_view.into()]
        )?;

        Ok(Self { picker_view })
    }

    /// Get selected photos
    pub fn get_selected_uris(&self, env: &JNIEnv) -> Result<Vec<String>> {
        // PhotoPicker.getSelectedItems()
        // Returns List<Uri>
        let uris = env.call_method(
            self.picker_view,
            "getSelectedItems",
            "()Ljava/util/List;",
            &[]
        )?.l()?;

        // Convert to Vec<String>
        // Implementation details...
        Ok(vec![])
    }
}
```

**FLUI Widget Integration:**

```rust
// crates/flui_widgets/src/android/photo_picker.rs

use flui_view::View;

pub struct PhotoPickerView {
    max_items: usize,
    on_selection: Box<dyn Fn(Vec<String>)>,
}

impl View for PhotoPickerView {
    fn build(&self, ctx: &BuildContext) -> Box<dyn View> {
        // Create native embedded photo picker
        // Wrap in FLUI view hierarchy
    }
}
```

**Crate Assignment:** 
- `flui-platform` (JNI bridge)
- `flui_widgets` (widget wrapper)

---

### 2.3 Desktop Mode (Tablets) ⭐⭐⭐⭐

**Priority:** HIGH (for tablet support)  
**Status:** Scheduled late 2025  
**Impact:** Desktop-like windowing on tablets

#### Overview

Android 16 introduces Desktop Mode for tablets, allowing multiple resizable windows, taskbar, and desktop-like window management.

**Features:**
- Resizable, movable windows
- Window minimize/maximize/close
- Taskbar with app switcher
- Multi-window layout manager
- Drag-and-drop between windows

**Implications for FLUI:**

FLUI apps need to support:
1. **Arbitrary window sizes** (not just portrait/landscape)
2. **Window resize callbacks**
3. **Focus management** (multiple windows)
4. **Drag-and-drop** (between windows)

**Implementation:**

```rust
// crates/flui-platform/src/platforms/android/desktop_mode.rs

pub struct DesktopModeWindow {
    window_id: u32,
    bounds: Rect,
    state: WindowState,
}

pub enum WindowState {
    Normal,
    Maximized,
    Minimized,
    Fullscreen,
}

impl DesktopModeWindow {
    /// Check if running in desktop mode
    pub fn is_desktop_mode(env: &JNIEnv) -> bool {
        // WindowManager.getCurrentWindowMetrics().isDesktopMode()
        // API 36+ only
        true  // Placeholder
    }

    /// Handle window resize
    pub fn on_resize(&mut self, new_bounds: Rect) {
        self.bounds = new_bounds;
        // Trigger FLUI layout recalculation
    }

    /// Handle window state change
    pub fn on_state_change(&mut self, new_state: WindowState) {
        self.state = new_state;
        match new_state {
            WindowState::Minimized => {
                // Pause rendering
            }
            WindowState::Maximized => {
                // Fullscreen layout
            }
            _ => {}
        }
    }

    /// Enable drag-and-drop
    pub fn enable_drop_target(&self, env: &JNIEnv) {
        // View.setOnDragListener(listener)
    }
}
```

**Testing Requirements:**
- Pixel Tablet (2025 models with Desktop Mode)
- Samsung Galaxy Tab (2025+)
- Window resize stress testing
- Multi-window scenarios

**Crate Assignment:** `flui-platform` (android desktop mode)

---

### 2.4 Health Connect (FHIR Medical Records) ⭐

**Priority:** LOW (healthcare apps only)  
**Status:** Stable June 10, 2025  
**Impact:** Standardized medical record access

Not directly relevant to general FLUI applications, but worth noting for healthcare app developers.

---

### 2.5 APV Codec (Advanced Video) ⭐⭐

**Priority:** MEDIUM  
**Status:** Stable June 10, 2025  
**Impact:** Higher quality video encoding/decoding

**Details:**
- APV Codec with 422-10 Profile
- Professional video quality (10-bit, 4:2:2 chroma)
- Hardware acceleration on 2025+ SoCs

**Use Case for FLUI:**
- Video playback widgets
- Camera preview (high quality)
- Video editing UIs

**Crate Assignment:** `flui_media` (future crate for video playback)

---

## 3. NDK and Native Development Improvements

### 3.1 NDK r26 Requirements ⭐⭐⭐⭐⭐

**Priority:** CRITICAL  
**Status:** Required for 16KB page size support  
**Impact:** Minimum NDK version bump

**Changes:**
- 16KB page size support
- Improved Rust toolchain integration
- Better Vulkan validation layer support
- Updated LLVM 17.x (better code generation)

**Build Configuration:**

```toml
# .cargo/config.toml
[target.aarch64-linux-android]
linker = "aarch64-linux-android34-clang"  # API 34+
ar = "llvm-ar"

[target.armv7-linux-androideabi]
linker = "armv7a-linux-androideabi34-clang"
ar = "llvm-ar"

[build]
# Ensure NDK r26+
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

**Verification:**

```bash
# Check NDK version
$ $ANDROID_NDK_ROOT/source.properties
# Pkg.Revision = 26.0.0 or higher
```

**Crate Assignment:** Build system configuration

---

### 3.2 Vulkan 1.3 Support ⭐⭐⭐⭐

**Priority:** HIGH  
**Status:** Available on 2024+ Android devices  
**Impact:** Modern GPU features for rendering

**Features:**
- Dynamic rendering (no render passes)
- Synchronization2 (better GPU sync)
- Extended dynamic state
- Timeline semaphores

**Implementation:**

```rust
// crates/flui_engine/src/android/vulkan.rs

use ash::vk;

pub struct AndroidVulkanRenderer {
    instance: ash::Instance,
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
}

impl AndroidVulkanRenderer {
    pub fn new(window: &NativeWindow) -> Result<Self> {
        let entry = ash::Entry::linked();
        
        // Request Vulkan 1.3
        let app_info = vk::ApplicationInfo::builder()
            .api_version(vk::API_VERSION_1_3);

        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::builder()
                    .application_info(&app_info),
                None
            )?
        };

        // Check for dynamic rendering support
        let features13 = vk::PhysicalDeviceVulkan13Features::builder()
            .dynamic_rendering(true)
            .synchronization2(true);

        // Create device with Vulkan 1.3 features
        // ...

        Ok(Self { instance, device, physical_device })
    }

    /// Use dynamic rendering (no VkRenderPass)
    pub fn begin_rendering(&self, command_buffer: vk::CommandBuffer) {
        let rendering_info = vk::RenderingInfo::builder()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D { width: 1920, height: 1080 },
            })
            .layer_count(1);

        unsafe {
            self.device.cmd_begin_rendering(command_buffer, &rendering_info);
        }
    }
}
```

**Crate Assignment:** `flui_engine` (vulkan backend)

---

### 3.3 JNI Optimizations ⭐⭐⭐

**Priority:** MEDIUM  
**Status:** Ongoing improvements  
**Impact:** Faster Rust ↔ Java interop

**Best Practices:**

```rust
// crates/flui-platform/src/platforms/android/jni_cache.rs

use jni::{JNIEnv, objects::GlobalRef};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Cache JNI class references to avoid repeated lookups
pub struct JniCache {
    classes: HashMap<&'static str, GlobalRef>,
    methods: HashMap<(&'static str, &'static str), jni::objects::JMethodID>,
}

static JNI_CACHE: Lazy<Mutex<JniCache>> = Lazy::new(|| {
    Mutex::new(JniCache {
        classes: HashMap::new(),
        methods: HashMap::new(),
    })
});

impl JniCache {
    /// Get cached class reference
    pub fn get_class(env: &JNIEnv, name: &'static str) -> Result<GlobalRef> {
        let mut cache = JNI_CACHE.lock();
        
        if let Some(class_ref) = cache.classes.get(name) {
            return Ok(class_ref.clone());
        }

        // Cache miss - lookup and store
        let local_class = env.find_class(name)?;
        let global_class = env.new_global_ref(local_class)?;
        cache.classes.insert(name, global_class.clone());
        
        Ok(global_class)
    }

    /// Get cached method ID
    pub fn get_method(
        env: &JNIEnv,
        class_name: &'static str,
        method_name: &'static str,
        signature: &str
    ) -> Result<jni::objects::JMethodID> {
        let mut cache = JNI_CACHE.lock();
        let key = (class_name, method_name);

        if let Some(method_id) = cache.methods.get(&key) {
            return Ok(*method_id);
        }

        // Cache miss
        let class = Self::get_class(env, class_name)?;
        let method_id = env.get_method_id(class, method_name, signature)?;
        cache.methods.insert(key, method_id);

        Ok(method_id)
    }
}
```

**Performance Impact:**
- 10-50x faster repeated JNI calls
- Reduced GC pressure (fewer local refs)
- Lower latency for UI operations

**Crate Assignment:** `flui-platform` (android JNI cache)

---

## 4. Implementation Priority Matrix

### Critical Path Features (Q1 2026)

1. **16KB Page Size Support** ⭐⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** CRITICAL (mandatory for API 35+)
   - **Crate:** `flui-platform` (android memory)

2. **NDK r26 Migration** ⭐⭐⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** CRITICAL (enables 16KB pages)
   - **Crate:** Build system

3. **Embedded Photo Picker** ⭐⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** HIGH (major UX improvement)
   - **Crate:** `flui-platform` + `flui_widgets`

### High Priority Features (Q2 2026)

4. **Desktop Mode Support** ⭐⭐⭐⭐
   - **Effort:** High (4-6 weeks)
   - **Impact:** HIGH (tablet market)
   - **Crate:** `flui-platform` (android desktop)

5. **Vulkan 1.3 Renderer** ⭐⭐⭐⭐
   - **Effort:** High (6-8 weeks)
   - **Impact:** MEDIUM (performance, modern GPU)
   - **Crate:** `flui_engine` (vulkan backend)

6. **Progress Notifications** ⭐⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** MEDIUM (better UX)
   - **Crate:** `flui-platform` (android notifications)

### Medium Priority Features (Q3 2026)

7. **ANGLE Backend Optimization** ⭐⭐⭐⭐
   - **Effort:** Medium (2-3 weeks)
   - **Impact:** MEDIUM (automatic with ANGLE)
   - **Crate:** `flui_engine` (backend selection)

8. **Edge-to-Edge Insets** ⭐⭐⭐
   - **Effort:** Low (1 week)
   - **Impact:** MEDIUM (modern UI look)
   - **Crate:** `flui-platform` (android window)

9. **JNI Cache Layer** ⭐⭐⭐
   - **Effort:** Medium (2 weeks)
   - **Impact:** MEDIUM (performance)
   - **Crate:** `flui-platform` (jni optimization)

### Low Priority Features (Q4 2026)

10. **FileIntegrityManager** ⭐⭐
    - **Effort:** Low (1 week)
    - **Impact:** LOW (security niche)
    - **Crate:** `flui_assets`

11. **APV Codec Support** ⭐⭐
    - **Effort:** Medium (3-4 weeks)
    - **Impact:** LOW (video apps only)
    - **Crate:** `flui_media` (future)

---

## 5. Testing Strategy

### Device Testing Matrix

| Device Category | Models | Android Version | Page Size | Priority |
|----------------|--------|-----------------|-----------|----------|
| **Flagship Phone** | Pixel 9, Galaxy S25 | 15, 16 | 16KB | ⭐⭐⭐⭐⭐ |
| **Mid-range Phone** | Pixel 8a, OnePlus 12R | 15 | 4KB/16KB | ⭐⭐⭐⭐ |
| **Budget Phone** | Moto G (2025) | 15 | 4KB | ⭐⭐⭐ |
| **Tablet** | Pixel Tablet, Galaxy Tab S9 | 15, 16 | 16KB | ⭐⭐⭐⭐⭐ |
| **Foldable** | Pixel Fold 2, Galaxy Z Fold 6 | 15, 16 | 16KB | ⭐⭐⭐⭐ |
| **Desktop Mode** | Pixel Tablet (2025+) | 16 | 16KB | ⭐⭐⭐⭐ |

### Automated Testing

```rust
// tests/android/page_size.rs

#[test]
fn test_page_size_detection() {
    let page_size = get_page_size();
    assert!(page_size == 4096 || page_size == 16384);
}

#[test]
fn test_page_aligned_allocation() {
    let allocator = PageAlignedAllocator::new();
    let ptr = allocator.allocate(1000);
    
    let page_size = get_page_size();
    assert_eq!(ptr as usize % page_size, 0);  // Verify alignment
}

#[test]
fn test_desktop_mode_window_resize() {
    if !DesktopModeWindow::is_desktop_mode() {
        return;  // Skip on non-desktop devices
    }

    let mut window = DesktopModeWindow::new();
    window.on_resize(Rect::new(0, 0, 1920, 1080));
    assert_eq!(window.bounds.width, 1920);
}
```

### Continuous Integration

```yaml
# .github/workflows/android_tests.yml
name: Android Tests

on: [push, pull_request]

jobs:
  test-android:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        api-level: [34, 35, 36]  # Android 14, 15, 16
        arch: [x86_64, arm64-v8a]
    
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Android NDK r26
        run: |
          wget https://dl.google.com/android/repository/android-ndk-r26-linux.zip
          unzip android-ndk-r26-linux.zip
          export ANDROID_NDK_ROOT=$PWD/android-ndk-r26
      
      - name: Run tests on emulator
        uses: reactivecircus/android-emulator-runner@v2
        with:
          api-level: ${{ matrix.api-level }}
          arch: ${{ matrix.arch }}
          script: cargo test --target aarch64-linux-android
```

---

## 6. Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| 16KB page size compatibility issues | Medium | High | Extensive testing on 4KB/16KB devices |
| Desktop Mode API instability (late 2025) | High | Medium | Feature flag, fallback to traditional windowing |
| ANGLE performance regression | Low | Medium | Benchmark suite, native Vulkan preference |
| NDK r26 toolchain bugs | Low | High | Stay on stable NDK releases, test early |
| JNI overhead in UI thread | Medium | Medium | JNI caching, background thread offload |

### Timeline Risks

- **Q1 2026:** August 2025 deadline for API 35 targeting (16KB pages) - **ON TRACK**
- **Q2 2026:** Desktop Mode late 2025 release may slip - **MONITOR**
- **Q3 2026:** Vulkan 1.3 renderer complexity - **ALLOCATE 6-8 WEEKS**

---

## 7. Resource Requirements

### Engineering Team

- **1 Android Platform Engineer** - JNI bridge, window management (full-time, 6 months)
- **1 Graphics Engineer** - Vulkan 1.3 renderer (full-time, 4 months)
- **1 QA Engineer** - Device testing, CI/CD (part-time, 6 months)

### Hardware

- **5 test devices** - Pixel 9, Galaxy S25, Pixel Tablet, Pixel Fold 2, mid-range phone
- **1 Pixel Tablet (2025)** - Desktop Mode testing
- **CI/CD infrastructure** - Android emulator farm (cloud or on-prem)

### Budget Estimate

- **Engineering:** $180k - $360k (3 engineers × 4-6 months)
- **Hardware:** $5k (test devices)
- **CI/CD:** $2k/year (cloud emulators)
- **Total:** $187k - $367k

---

## 8. Conclusion

Android 15 and 16 introduce significant architectural improvements:

**Must-Have (2026):**
- 16KB page size support (mandatory)
- NDK r26 migration (mandatory)
- Embedded Photo Picker (major UX win)

**High-Value (2026):**
- Desktop Mode for tablets (growing market)
- Vulkan 1.3 renderer (performance)
- Progress Notifications (UX polish)

**Nice-to-Have (2027+):**
- ANGLE optimizations (automatic with new devices)
- FileIntegrityManager (security niche)
- APV Codec (video apps)

**Recommended Timeline:**
- **Q1 2026:** 16KB pages, NDK r26, Embedded Photo Picker
- **Q2 2026:** Desktop Mode, Vulkan 1.3
- **Q3 2026:** Progress Notifications, Edge-to-Edge, JNI cache
- **Q4 2026:** Polish, testing, performance optimization

This positions FLUI to fully leverage modern Android platform capabilities while maintaining compatibility with existing devices.

---

**Next Steps:**
1. Review and approve this roadmap
2. Assign engineering resources
3. Set up device testing lab
4. Begin Q1 2026 implementation (16KB pages)
