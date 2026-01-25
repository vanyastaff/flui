# flui-platform Implementation Plan

**Crate:** `flui-platform`  
**Purpose:** Cross-platform window management, input handling, OS integration  
**Priority:** ⭐⭐⭐⭐⭐ CRITICAL (Foundation for all platforms)

---

## Overview

`flui-platform` is the foundational crate responsible for:
- Window creation and management
- Input event handling (keyboard, mouse, touch, gamepad)
- OS-specific integrations (file dialogs, notifications, system APIs)
- Platform abstractions (unified API across macOS, Windows, Linux, Android, Web)

**Architecture:**
```
flui-platform/
├── src/
│   ├── lib.rs              # Public API
│   ├── platform.rs         # Platform trait
│   ├── window.rs           # Window trait
│   ├── events.rs           # Event types
│   └── platforms/
│       ├── macos/          # macOS implementation
│       ├── windows/        # Windows implementation
│       ├── linux/          # Linux implementation
│       ├── android/        # Android implementation
│       └── web/            # Web (WASM) implementation
```

---

## Platform-Specific Features Matrix

| Feature | macOS | Windows | Linux | Android | Web | Priority |
|---------|-------|---------|-------|---------|-----|----------|
| **Window Management** | ✅ | ✅ | ✅ | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **Input Events** | ✅ | ✅ | ✅ | ✅ | ✅ | ⭐⭐⭐⭐⭐ |
| **File Picker** | NSPanel | WinUI 3 | XDG Portal | Embedded | Web API | ⭐⭐⭐⭐⭐ |
| **Notifications** | NSNotif | Toast | Portal | Android | Web Notif | ⭐⭐⭐⭐ |
| **HDR Support** | Metal | DX12 | Wayland | - | - | ⭐⭐⭐⭐ |
| **Accessibility** | NSAccess | UIA | AT-SPI | TalkBack | ARIA | ⭐⭐⭐⭐⭐ |
| **System Tray** | NSStatusItem | Tray | StatusNotifier | - | - | ⭐⭐⭐ |
| **Drag & Drop** | NSDrag | IDataObject | XDG DnD | ContentResolver | DataTransfer | ⭐⭐⭐⭐ |
| **Clipboard** | NSPasteboard | Clipboard | XDG Clipboard | ClipboardManager | Clipboard API | ⭐⭐⭐⭐⭐ |

---

## Q1 2026: Foundation (Weeks 1-12)

### macOS Platform Module

**Files:** `src/platforms/macos/`

#### 1.1 Window Management ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Dependencies:** None
- **Status:** Partially complete (NSView integration done)

**Tasks:**
- [x] NSWindow creation (DONE - Phase 8)
- [x] NSView integration (DONE - Phase 8.6)
- [x] **Liquid Glass Material** (DONE - 2026-01-25 Session)
  ```rust
  // src/platforms/macos/liquid_glass.rs
  pub enum LiquidGlassMaterial {
      Standard, Prominent, Sidebar, Menu, Popover, ControlCenter
  }
  impl MaterialExt for NSVisualEffectView {
      fn set_liquid_glass(&self, material: LiquidGlassMaterial);
  }
  ```
- [x] Window Tiling API (macOS Sequoia 15+) (DONE - 2026-01-25)
  ```rust
  // src/platforms/macos/window_tiling.rs
  pub struct TilingConfiguration {
      pub primary_position: TilePosition,
      pub split_ratio: f32,
      pub layout: TilingLayout,
  }
  // Supports SideBySide, TopBottom, Quarters layouts
  // 11 unit tests, comprehensive API
  ```
- [x] Multi-window support (DONE - 2026-01-25)
  ```rust
  // src/platforms/macos/window_manager.rs
  pub struct WindowManager {
      // Centralized window tracking and management
      // Window groups for tabbed windows
      // Cascade positioning, focus management
  }
  // 12 unit tests, thread-safe SharedWindowManager
  ```
- [ ] Window state management (minimize, maximize, fullscreen)

#### 1.2 Input Handling ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management
- **Status:** Partially complete (Phase 8.6)

**Tasks:**
- [x] Keyboard events (DONE - Phase 8.6)
- [x] Mouse events (DONE - Phase 8.6)
- [x] Scroll events (DONE - Phase 8.6)
- [ ] Touch events (trackpad gestures)
- [ ] Gamepad support (Game Controller framework)
- [ ] Pencil/Stylus input (for iPad support)

#### 1.3 File Picker ⭐⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] NSOpenPanel integration
  ```rust
  // src/platforms/macos/file_picker.rs
  pub struct FilePickerMacOS;
  impl FilePicker for FilePickerMacOS {
      async fn pick_file(&self, options: FilePickerOptions) -> Result<PathBuf>;
      async fn pick_files(&self, options: FilePickerOptions) -> Result<Vec<PathBuf>>;
      async fn save_file(&self, options: SaveFileOptions) -> Result<PathBuf>;
  }
  ```
- [ ] File type filtering
- [ ] Recent files integration
- [ ] iCloud Drive support

#### 1.4 Accessibility ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] NSAccessibility protocol implementation
  ```rust
  // src/platforms/macos/accessibility.rs
  pub struct AccessibilityNode {
      role: NSAccessibilityRole,
      label: String,
      value: Option<String>,
  }
  impl NSAccessibilityElement for FLUIView {
      fn accessibility_role(&self) -> NSAccessibilityRole;
      fn accessibility_label(&self) -> String;
  }
  ```
- [ ] VoiceOver support
- [ ] Keyboard navigation
- [ ] High contrast mode detection
- [ ] Reduced motion detection

**Deliverables:**
- Fully functional macOS platform module
- Liquid Glass materials integration
- Window tiling support
- Complete accessibility implementation

---

### Windows Platform Module

**Files:** `src/platforms/windows/`

#### 2.1 Window Management ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Dependencies:** None

**Tasks:**
- [ ] HWND creation and management
  ```rust
  // src/platforms/windows/window.rs
  pub struct WindowsWindow {
      hwnd: HWND,
      content_island: Option<ContentIsland>,
  }
  impl Window for WindowsWindow {
      fn create(options: WindowOptions) -> Result<Self>;
      fn set_title(&mut self, title: &str);
      fn set_size(&mut self, width: u32, height: u32);
  }
  ```
- [ ] **WinUI 3 Content Islands** (Windows App SDK 1.6+)
  ```rust
  // src/platforms/windows/content_island.rs
  pub struct ContentIsland {
      island: Microsoft::UI::Content::ContentIsland,
  }
  impl ContentIsland {
      pub fn attach_to_hwnd(&self, hwnd: HWND);
      pub fn set_content(&mut self, xaml_content: XamlRoot);
  }
  ```
- [ ] **Snap Layouts** (Windows 11 24H2+)
  ```rust
  // src/platforms/windows/snap_layouts.rs
  pub struct SnapLayout {
      layout_id: u32,
  }
  impl WindowExt for WindowsWindow {
      fn enable_snap_layouts(&self, layouts: Vec<SnapLayout>);
  }
  ```
- [ ] Multi-monitor support with DPI awareness
- [ ] Window chrome customization

#### 2.2 Input Handling ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] Win32 message loop
  ```rust
  // src/platforms/windows/events.rs
  pub struct EventLoop {
      handlers: PlatformHandlers,
  }
  impl EventLoop {
      pub fn run(&mut self) {
          // GetMessage/DispatchMessage loop
      }
      fn handle_wm_keydown(&self, wparam: WPARAM, lparam: LPARAM);
      fn handle_wm_mousemove(&self, wparam: WPARAM, lparam: LPARAM);
  }
  ```
- [ ] Keyboard events (WM_KEYDOWN, WM_CHAR)
- [ ] Mouse events (WM_MOUSEMOVE, WM_LBUTTONDOWN, etc.)
- [ ] Touch events (WM_TOUCH, WM_POINTER)
- [ ] Pen/Stylus input (Windows Ink)
- [ ] Gamepad support (XInput)

#### 2.3 File Picker ⭐⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] **WinUI 3 FilePicker** (Windows App SDK 1.6+)
  ```rust
  // src/platforms/windows/file_picker.rs
  pub struct FilePickerWindows;
  impl FilePicker for FilePickerWindows {
      async fn pick_file(&self, options: FilePickerOptions) -> Result<PathBuf> {
          // Use Windows.Storage.Pickers.FileOpenPicker
          let picker = FileOpenPicker::new();
          picker.ViewMode = PickerViewMode::List;
          // ...
      }
  }
  ```
- [ ] File type filters
- [ ] Recent files (Windows.Storage.AccessCache)
- [ ] Cloud storage integration (OneDrive)

#### 2.4 HDR Support ⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** Window Management

**Tasks:**
- [ ] Auto HDR detection
  ```rust
  // src/platforms/windows/hdr.rs
  pub struct HdrCapabilities {
      pub max_luminance: f32,
      pub supports_hdr10: bool,
      pub supports_dolby_vision: bool,
  }
  impl WindowExt for WindowsWindow {
      fn get_hdr_capabilities(&self) -> HdrCapabilities;
      fn enable_auto_hdr(&self);
  }
  ```
- [ ] DXGISwapChain HDR metadata
- [ ] Color space conversion

#### 2.5 Accessibility ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] UI Automation (UIA) provider
  ```rust
  // src/platforms/windows/accessibility.rs
  pub struct UiaProvider {
      patterns: Vec<UiaPattern>,
  }
  impl IRawElementProviderSimple for UiaProvider {
      fn GetPatternProvider(&self, pattern_id: i32) -> *mut IUnknown;
      fn GetPropertyValue(&self, property_id: i32) -> VARIANT;
  }
  ```
- [ ] Narrator support
- [ ] High contrast mode
- [ ] Keyboard navigation

**Deliverables:**
- Complete Windows platform module
- WinUI 3 Content Islands integration
- Snap Layouts support
- HDR capabilities
- Full accessibility via UIA

---

### Linux Platform Module

**Files:** `src/platforms/linux/`

#### 3.1 Wayland Window Management ⭐⭐⭐⭐⭐
- **Effort:** 4 weeks
- **Dependencies:** None

**Tasks:**
- [ ] Wayland client connection
  ```rust
  // src/platforms/linux/wayland/mod.rs
  pub struct WaylandPlatform {
      connection: Connection,
      compositor: wl_compositor::WlCompositor,
      xdg_wm_base: xdg_wm_base::XdgWmBase,
  }
  impl Platform for WaylandPlatform {
      fn create_window(&self, options: WindowOptions) -> Result<WaylandWindow>;
  }
  ```
- [ ] XDG Shell protocol
  ```rust
  // src/platforms/linux/wayland/window.rs
  pub struct WaylandWindow {
      surface: wl_surface::WlSurface,
      xdg_surface: XdgSurface,
      xdg_toplevel: XdgToplevel,
  }
  ```
- [ ] **Fractional Scaling** (wp_fractional_scale_v1)
  ```rust
  // src/platforms/linux/wayland/scaling.rs
  pub struct FractionalScaling {
      manager: WpFractionalScaleManagerV1,
  }
  impl FractionalScaling {
      fn get_scale_factor(&self, surface: &WlSurface) -> f64;
  }
  ```
- [ ] **NVIDIA Explicit Sync** (linux_drm_syncobj_v1)
  ```rust
  // src/platforms/linux/wayland/nvidia.rs
  pub struct ExplicitSync {
      syncobj_manager: WpLinuxDrmSyncobjManagerV1,
  }
  ```
- [ ] Multi-monitor support
- [ ] Window decorations (server-side vs client-side)

#### 3.2 X11 Fallback ⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] XCB connection
- [ ] Window creation via XCB
- [ ] ICCCM/EWMH compliance
- [ ] Multi-monitor (XRandR)

#### 3.3 Input Handling ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] Wayland input events (wl_seat, wl_keyboard, wl_pointer)
  ```rust
  // src/platforms/linux/wayland/input.rs
  impl wl_keyboard::EventHandler for WaylandPlatform {
      fn key(&mut self, key: u32, state: KeyState);
  }
  impl wl_pointer::EventHandler for WaylandPlatform {
      fn motion(&mut self, x: f64, y: f64);
      fn button(&mut self, button: u32, state: ButtonState);
  }
  ```
- [ ] Touch events (wl_touch)
- [ ] Tablet input (zwp_tablet_v2)

#### 3.4 XDG Desktop Portal ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] **File Picker Portal** (org.freedesktop.portal.FileChooser)
  ```rust
  // src/platforms/linux/portal/file_picker.rs
  pub struct FilePickerPortal {
      connection: zbus::Connection,
  }
  impl FilePicker for FilePickerPortal {
      async fn open_file(&self, title: &str) -> Result<Vec<PathBuf>> {
          let proxy = FileChooserProxy::new(&self.connection).await?;
          proxy.OpenFile("", options).await?
      }
  }
  ```
- [ ] **Screen Capture Portal** (org.freedesktop.portal.ScreenCast)
  ```rust
  // src/platforms/linux/portal/screen_cast.rs
  pub async fn start_screen_cast(&self) -> Result<u32> {
      // Returns PipeWire node ID
  }
  ```
- [ ] Notification Portal
- [ ] Settings Portal (theme detection)

#### 3.5 PipeWire Integration ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] **Audio Backend**
  ```rust
  // src/platforms/linux/pipewire/audio.rs
  pub struct PipeWireAudio {
      mainloop: pw::MainLoop,
      core: pw::Core,
  }
  impl AudioBackend for PipeWireAudio {
      fn play_audio(&self, samples: &[f32], sample_rate: u32);
  }
  ```
- [ ] Screen capture streams
- [ ] Microphone input

#### 3.6 Accessibility ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] AT-SPI (Assistive Technology Service Provider Interface)
  ```rust
  // src/platforms/linux/accessibility.rs
  pub struct AtSpiProvider {
      connection: zbus::Connection,
  }
  impl org.a11y.atspi.Accessible for AtSpiProvider {
      fn GetRole(&self) -> u32;
      fn GetState(&self) -> StateSet;
  }
  ```
- [ ] Orca screen reader support
- [ ] High contrast theme detection

**Deliverables:**
- Complete Wayland platform module with modern protocols
- X11 fallback for compatibility
- XDG Desktop Portal integration (file picker, screen cast)
- PipeWire audio backend
- AT-SPI accessibility

---

### Android Platform Module

**Files:** `src/platforms/android/`

#### 4.1 Window Management ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Dependencies:** None

**Tasks:**
- [ ] NativeWindow integration
  ```rust
  // src/platforms/android/window.rs
  pub struct AndroidWindow {
      native_window: NativeWindow,
      java_window: JObject,
  }
  impl Window for AndroidWindow {
      fn get_insets(&self) -> WindowInsets;  // Status bar, nav bar
  }
  ```
- [ ] **Edge-to-Edge Rendering** (API 35+)
  ```rust
  // src/platforms/android/edge_to_edge.rs
  pub struct WindowInsets {
      pub top: f32, pub bottom: f32, pub left: f32, pub right: f32,
  }
  impl AndroidWindow {
      fn enable_edge_to_edge(&self, env: &JNIEnv);
  }
  ```
- [ ] **Desktop Mode** (Android 16+ tablets)
  ```rust
  // src/platforms/android/desktop_mode.rs
  pub struct DesktopModeWindow {
      state: WindowState,  // Normal, Maximized, Minimized
  }
  impl DesktopModeWindow {
      fn is_desktop_mode(env: &JNIEnv) -> bool;
      fn on_resize(&mut self, new_bounds: Rect);
  }
  ```
- [ ] Multi-window support (split screen)

#### 4.2 Input Handling ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] Touch events (MotionEvent)
  ```rust
  // src/platforms/android/input.rs
  pub fn handle_touch_event(env: &JNIEnv, motion_event: JObject) {
      let action = env.call_method(motion_event, "getAction", "()I")?.i()?;
      let x = env.call_method(motion_event, "getX", "()F")?.f()?;
      // Dispatch to FLUI event system
  }
  ```
- [ ] Keyboard events (KeyEvent)
- [ ] Gamepad support (InputDevice)
- [ ] Stylus/Pen input

#### 4.3 File Picker ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] **Embedded Photo Picker** (Android 16+)
  ```rust
  // src/platforms/android/photo_picker.rs
  pub struct EmbeddedPhotoPicker {
      picker_view: JObject,
  }
  impl EmbeddedPhotoPicker {
      fn new(env: &JNIEnv, parent_view: &JObject) -> Result<Self>;
      fn get_selected_uris(&self, env: &JNIEnv) -> Result<Vec<String>>;
  }
  ```
- [ ] Document picker (ACTION_OPEN_DOCUMENT)
- [ ] SAF (Storage Access Framework)

#### 4.4 Progress Notifications ⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] **Progress Notifications** (Android 16+)
  ```rust
  // src/platforms/android/notifications.rs
  pub struct ProgressNotification {
      notification_id: i32,
      progress: f32,  // 0.0 - 1.0
      style: ProgressStyle,  // Linear, Circular
  }
  impl ProgressNotification {
      fn show(&self, env: &JNIEnv);
      fn update_progress(&mut self, progress: f32);
  }
  ```

#### 4.5 Memory Management ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] **16KB Page Size Support** (API 35+ CRITICAL)
  ```rust
  // src/platforms/android/memory.rs
  pub fn get_page_size() -> usize {
      unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
  }
  pub struct PageAlignedAllocator {
      page_size: usize,
  }
  ```
- [ ] JNI cache optimization
  ```rust
  // src/platforms/android/jni_cache.rs
  pub struct JniCache {
      classes: HashMap<&'static str, GlobalRef>,
      methods: HashMap<(&'static str, &'static str), JMethodID>,
  }
  ```

#### 4.6 Accessibility ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] TalkBack support (AccessibilityNodeProvider)
  ```rust
  // src/platforms/android/accessibility.rs
  pub struct AccessibilityNodeInfo {
      text: String,
      content_description: String,
      class_name: String,
  }
  ```

**Deliverables:**
- Complete Android platform module
- 16KB page size support (CRITICAL for API 35+)
- Embedded Photo Picker
- Desktop Mode support (tablets)
- Progress Notifications
- JNI optimization layer

---

### Web Platform Module

**Files:** `src/platforms/web/`

#### 5.1 Window Management ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** None

**Tasks:**
- [ ] Canvas element integration
  ```rust
  // src/platforms/web/window.rs
  pub struct WebWindow {
      canvas: HtmlCanvasElement,
      scale_factor: f64,
  }
  impl Window for WebWindow {
      fn get_canvas(&self) -> &HtmlCanvasElement;
      fn resize(&mut self, width: u32, height: u32);
  }
  ```
- [ ] Fullscreen API
- [ ] Resize observer
- [ ] Device pixel ratio handling

#### 5.2 Input Handling ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Dependencies:** Window Management

**Tasks:**
- [ ] Keyboard events (addEventListener)
  ```rust
  // src/platforms/web/input.rs
  pub fn setup_keyboard_events(canvas: &HtmlCanvasElement) {
      let on_keydown = Closure::wrap(Box::new(|event: KeyboardEvent| {
          // Convert to FLUI KeyEvent
      }) as Box<dyn FnMut(KeyboardEvent)>);
      canvas.add_event_listener_with_callback("keydown", on_keydown.as_ref())?;
      on_keydown.forget();
  }
  ```
- [ ] Mouse events (mouse, pointer)
- [ ] Touch events (touch, pointer)
- [ ] Gamepad API
- [ ] Pointer Lock API (for games)

#### 5.3 File Picker ⭐⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] File input element
  ```rust
  // src/platforms/web/file_picker.rs
  pub struct FilePickerWeb;
  impl FilePicker for FilePickerWeb {
      async fn pick_file(&self, options: FilePickerOptions) -> Result<Vec<u8>> {
          let input = document.create_element("input")?;
          input.set_attribute("type", "file")?;
          // Click input, wait for change event
      }
  }
  ```
- [ ] File System Access API (Chrome)
- [ ] Drag & Drop API

#### 5.4 Storage ⭐⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] **IndexedDB** for offline storage
  ```rust
  // src/platforms/web/storage.rs
  pub struct IndexedDbStorage {
      db: IdbDatabase,
  }
  impl Storage for IndexedDbStorage {
      async fn save(&self, key: &str, value: &[u8]) -> Result<()>;
      async fn load(&self, key: &str) -> Result<Option<Vec<u8>>>;
  }
  ```
- [ ] LocalStorage (for small data)
- [ ] Cache API

#### 5.5 Threading ⭐⭐⭐⭐⭐
- **Effort:** 3 weeks
- **Dependencies:** None

**Tasks:**
- [ ] **Web Workers** + SharedArrayBuffer
  ```rust
  // src/platforms/web/threading.rs
  pub struct WasmThreadPool {
      workers: Vec<Worker>,
      shared_memory: Option<SharedArrayBuffer>,
  }
  impl WasmThreadPool {
      fn new(num_threads: usize) -> Result<Self> {
          // Check cross-origin isolation
          if !is_cross_origin_isolated() {
              return Err(anyhow!("Requires COOP/COEP headers"));
          }
          // Create workers, allocate shared memory
      }
  }
  ```
- [ ] wasm-bindgen-rayon integration
- [ ] Cross-origin isolation detection

#### 5.6 Notifications ⭐⭐⭐
- **Effort:** 1 week
- **Dependencies:** None

**Tasks:**
- [ ] Web Notifications API
  ```rust
  // src/platforms/web/notifications.rs
  pub async fn show_notification(title: &str, body: &str) -> Result<()> {
      let permission = Notification::permission();
      if permission != "granted" {
          Notification::request_permission().await?;
      }
      Notification::new_with_options(title, &options)?;
      Ok(())
  }
  ```

**Deliverables:**
- Complete Web platform module with wasm-bindgen
- Canvas-based window management
- Web Workers + SharedArrayBuffer threading
- IndexedDB offline storage
- File picker and drag-drop
- Web Notifications

---

## Q2 2026: Advanced Features (Weeks 13-24)

### Cross-Platform Features

#### 6.1 Clipboard Abstraction ⭐⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Platforms:** All

**Tasks:**
- [ ] Unified Clipboard API
  ```rust
  // src/clipboard.rs
  pub trait Clipboard {
      fn read_text(&self) -> Result<String>;
      fn write_text(&self, text: &str) -> Result<()>;
      fn read_image(&self) -> Result<Vec<u8>>;
      fn write_image(&self, image: &[u8]) -> Result<()>;
  }
  ```
- [ ] macOS: NSPasteboard
- [ ] Windows: Clipboard Win32 API
- [ ] Linux: wl-clipboard (Wayland), xclip (X11)
- [ ] Android: ClipboardManager
- [ ] Web: Clipboard API

#### 6.2 Drag & Drop Abstraction ⭐⭐⭐⭐
- **Effort:** 2 weeks
- **Platforms:** All

**Tasks:**
- [ ] Unified DnD API
  ```rust
  // src/drag_drop.rs
  pub trait DragDropHandler {
      fn on_drag_enter(&mut self, data: DragData) -> DragEffect;
      fn on_drop(&mut self, data: DragData) -> Result<()>;
  }
  ```
- [ ] macOS: NSDraggingInfo
- [ ] Windows: IDataObject
- [ ] Linux: wl_data_device (Wayland)
- [ ] Android: DragEvent
- [ ] Web: DataTransfer API

#### 6.3 System Tray/Menu Bar ⭐⭐⭐
- **Effort:** 2 weeks
- **Platforms:** Desktop only (macOS, Windows, Linux)

**Tasks:**
- [ ] Unified System Tray API
  ```rust
  // src/system_tray.rs
  pub struct SystemTray {
      icon: Vec<u8>,
      menu: Vec<MenuItem>,
  }
  impl SystemTray {
      fn show(&self) -> Result<()>;
      fn set_tooltip(&mut self, tooltip: &str);
  }
  ```
- [ ] macOS: NSStatusItem
- [ ] Windows: Shell_NotifyIcon
- [ ] Linux: StatusNotifierItem (KDE), AppIndicator (GNOME)

---

## Q3 2026: Optimization & Polish (Weeks 25-36)

### Performance Optimization

#### 7.1 Event Loop Optimization ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**
- [ ] Batch event processing
- [ ] Event coalescing (mouse move, scroll)
- [ ] Priority queue for events
- [ ] Profiling and benchmarking

#### 7.2 Memory Optimization ⭐⭐⭐⭐
- **Effort:** 2 weeks

**Tasks:**
- [ ] Pool allocators for events
- [ ] Reduce allocations in hot paths
- [ ] Profile memory usage across platforms

#### 7.3 Platform-Specific Optimizations ⭐⭐⭐
- **Effort:** 2 weeks per platform

**Tasks:**
- [ ] macOS: Reduce NSAutoreleasePool overhead
- [ ] Windows: Optimize message loop
- [ ] Linux: Wayland event batching
- [ ] Android: JNI cache hit rate optimization
- [ ] Web: Minimize JS ↔ Rust crossing

---

## Q4 2026: Advanced Integrations (Weeks 37-48)

### AI/ML Integration

#### 8.1 On-Device ML ⭐⭐⭐
- **Effort:** 3 weeks

**Tasks:**
- [ ] macOS: Core ML integration
  ```rust
  // src/platforms/macos/ml.rs
  pub struct CoreMLModel {
      model: MLModel,
  }
  impl CoreMLModel {
      fn predict(&self, input: &[f32]) -> Result<Vec<f32>>;
  }
  ```
- [ ] Windows: Windows ML (DirectML)
  ```rust
  // src/platforms/windows/ml.rs
  pub struct WindowsMLEngine {
      device: LearningModelDevice,
  }
  ```
- [ ] Android: TensorFlow Lite
- [ ] Web: WebGPU compute shaders

---

## Testing Strategy

### Unit Tests
```rust
// tests/window_tests.rs
#[test]
fn test_window_creation() {
    let platform = Platform::new().unwrap();
    let window = platform.create_window(WindowOptions::default()).unwrap();
    assert!(window.width() > 0);
}

#[cfg(target_os = "macos")]
#[test]
fn test_liquid_glass_material() {
    let window = create_test_window();
    window.set_material(LiquidGlassMaterial::Sidebar);
    // Verify NSVisualEffectView material
}
```

### Integration Tests
- Window creation on all platforms
- Input event handling
- File picker functionality
- Clipboard operations
- Accessibility tree validation

### Platform-Specific Tests
- macOS: XCTest integration
- Windows: Windows App Certification Kit
- Linux: Wayland/X11 protocol conformance
- Android: Instrumentation tests
- Web: wasm-pack test (headless browsers)

---

## Dependencies

### Rust Crates
```toml
[dependencies]
# Cross-platform
raw-window-handle = "0.6"
parking_lot = "0.12"
tracing = "0.1"

# macOS
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
objc2-foundation = "0.2"
objc2-app-kit = "0.2"
core-graphics = "0.23"

# Windows
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "UI_Xaml_Controls",
    "Storage_Pickers",
] }

# Linux
[target.'cfg(target_os = "linux")'.dependencies]
wayland-client = "0.31"
wayland-protocols = { version = "0.31", features = ["client"] }
zbus = "4.0"  # D-Bus (XDG Portal)
pipewire = "0.8"

# Android
[target.'cfg(target_os = "android")'.dependencies]
ndk = "0.9"
jni = "0.21"

# Web
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = [
    "Window", "Document", "HtmlCanvasElement",
    "KeyboardEvent", "MouseEvent", "TouchEvent",
    "Worker", "MessageEvent", "SharedArrayBuffer",
    "IdbDatabase", "Notification",
] }
js-sys = "0.3"
```

---

## Milestones & Timeline

| Quarter | Milestone | Deliverables |
|---------|-----------|--------------|
| **Q1 2026** | Foundation | macOS, Windows, Linux, Android, Web basic window + input |
| **Q2 2026** | Features | File pickers, clipboard, drag-drop, accessibility |
| **Q3 2026** | Optimization | Performance tuning, memory optimization |
| **Q4 2026** | Advanced | ML integration, system tray, advanced protocols |

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Wayland protocol instability | Low | Medium | Fallback to X11, extensive testing |
| Android 16KB page size issues | Medium | High | Early testing on API 35+ devices |
| Cross-origin isolation (Web) | Medium | High | Documentation, dev server template |
| Windows WinUI 3 C++/WinRT interop | Medium | Medium | Use windows-rs bindings |
| macOS Liquid Glass API changes | Low | Medium | Monitor beta releases |

---

## Budget & Resources

**Engineering Team:**
- 2 Platform Engineers (full-time, 12 months)
- 1 QA Engineer (part-time, 12 months)

**Estimated Cost:** $400k - $600k

**Hardware:**
- macOS: Mac Mini M4, MacBook Pro (Intel + Apple Silicon)
- Windows: Surface Laptop, gaming desktop (AMD + NVIDIA GPUs)
- Linux: ThinkPad (AMD), desktop (NVIDIA)
- Android: Pixel 9, Pixel Tablet, Galaxy S25
- iOS: iPhone 16, iPad Pro

---

## Success Metrics

- ✅ All platforms compile without errors
- ✅ Window creation < 100ms on all platforms
- ✅ Input latency < 10ms (keyboard/mouse)
- ✅ 100% accessibility coverage (VoiceOver, Narrator, Orca)
- ✅ File picker works on all platforms
- ✅ Pass platform certification tests (macOS notarization, Windows App Cert Kit)
- ✅ Web bundle < 500KB (after wasm-opt + Brotli)
- ✅ Zero memory leaks in 24-hour stress test

---

**Next Steps:**
1. Review and approve this plan
2. Set up platform development environments
3. Create cross-platform abstractions (Window trait, Event types)
4. Begin Q1 2026 implementation (macOS Liquid Glass, Windows WinUI 3, Wayland)
