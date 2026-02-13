# Research: flui-platform MVP Completion

**Date**: 2026-02-13 | **Branch**: `002-platform-mvp`

## R1: Per-Window Callback Architecture

### Decision: Mutex-based take/restore pattern with per-window callback storage

### Rationale

GPUI uses `Cell<Option<Box<dyn FnMut>>>` which is single-threaded only. FLUI needs thread-safe callbacks because PlatformWindow is `Send + Sync`. The take/restore pattern with `parking_lot::Mutex` achieves:

- Thread safety (required by `Send + Sync` on PlatformWindow)
- Reentrancy safety (lock released before callback invocation)
- No deadlocks (callback never called while lock is held)
- Zero-cost when callback is None (fast path)

### Pattern

```rust
pub struct WindowCallbacks {
    pub on_input: Mutex<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>>,
    pub on_request_frame: Mutex<Option<Box<dyn FnMut() + Send>>>,
    pub on_resize: Mutex<Option<Box<dyn FnMut(Size<Pixels>, f32) + Send>>>,
    pub on_moved: Mutex<Option<Box<dyn FnMut() + Send>>>,
    pub on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    pub on_should_close: Mutex<Option<Box<dyn FnMut() -> bool + Send>>>,
    pub on_active_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,
    pub on_hover_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,
    pub on_appearance_changed: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

// Safe dispatch: lock → take → unlock → call → lock → restore → unlock
fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
    let cb = self.callbacks.on_input.lock().take();
    let result = if let Some(mut cb) = cb {
        let r = cb(event);
        *self.callbacks.on_input.lock() = Some(cb);
        r
    } else {
        DispatchEventResult::default()
    };
    result
}
```

### Alternatives Considered

- **Cell-based (GPUI pattern)**: Rejected because PlatformWindow must be `Send + Sync` for cross-platform support. Cell is not Send.
- **Arc<Mutex<>> with global handlers**: Current FLUI approach. Rejected for per-window callbacks because all callbacks share one lock (contention). Keep for platform-level callbacks only.
- **RefCell**: Rejected because it panics on borrow conflicts from WndProc reentrancy.
- **RwLock**: Rejected — callbacks need write access (FnMut), so RwLock degrades to Mutex anyway.

---

## R2: Task<T> Implementation Strategy

### Decision: Tokio-based wrapper now, async_task migration path for later

### Rationale

FLUI already uses tokio 1.43 as a workspace dependency. Wrapping `tokio::task::JoinHandle<T>` provides Task<T> with minimal new dependencies. GPUI's async_task approach is superior long-term but requires a custom dispatcher per platform.

### Pattern

```rust
#[must_use]
pub struct Task<T>(TaskState<T>);

enum TaskState<T> {
    Ready(Option<T>),
    Spawned(tokio::task::JoinHandle<T>),
}

impl<T> Task<T> {
    pub fn ready(val: T) -> Self;
    pub fn detach(self);
}

impl<T: Send + 'static> Future for Task<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T>;
}
```

Priority is stored as metadata but routing to priority-aware thread pools is deferred. Tokio's fair scheduler handles Medium/Low adequately for now.

### Alternatives Considered

- **async_task crate (GPUI pattern)**: Better for custom platform dispatchers (Windows ThreadPool, macOS GCD). Deferred to post-MVP when platform dispatcher trait is needed.
- **No Task<T> (keep fire-and-forget)**: Rejected because platform operations (file dialogs, clipboard) need to return results asynchronously.
- **futures::channel::oneshot**: Simpler but doesn't integrate with executor priority or provide detach().

---

## R3: DirectWrite Text Backend

### Decision: Use `windows` crate (0.59) for IDWriteFactory5 + IDWriteTextLayout

### Rationale

The `windows` crate already in Cargo.toml provides complete DirectWrite bindings. GPUI's implementation proves the pattern works with IDWriteFactory5 (Windows 10+). The `directwrite` crate is alpha and less complete.

### Minimum API Surface for MVP

1. **Font enumeration**: `IDWriteFontCollection1::GetFontFamilyCount()` + `GetFontFamily()` + `GetFamilyNames()`
2. **Font metrics**: `IDWriteFontFace3::GetMetrics()` → ascent, descent, line_gap, units_per_em
3. **Glyph lookup**: `IDWriteFontFace3::GetGlyphIndices()` → char to glyph ID
4. **Text measurement**: `IDWriteTextLayout::GetMetrics()` → width, height
5. **Line layout**: `IDWriteTextLayout::GetLineMetrics()` + custom `IDWriteTextRenderer` for glyph runs

### Required Cargo.toml Features

```toml
windows = { version = "0.59", features = [
    "Win32_Graphics_DirectWrite",   # Core DirectWrite
    "Win32_Globalization",          # GetUserDefaultLocaleName
    # Existing features already present
] }
```

### Deferred to Post-MVP

- Glyph rasterization (`CreateGlyphRunAnalysis` + `CreateAlphaTexture`) — needed by flui-engine, not platform layer
- Font fallback chains (`IDWriteFontFallbackBuilder`) — only needed for complex text (CJK, emoji)
- Custom font loading (`InMemoryFontFileReference`) — only needed for bundled fonts
- Color emoji support (`TranslateColorGlyphRun`) — nice-to-have

### Alternatives Considered

- **directwrite crate (0.3.0-alpha4)**: Higher-level but alpha, less features. Rejected.
- **cosmic-text**: Already in workspace for engine layer. Not a platform-level API — it's a layout engine that sits above DirectWrite. Could be used as alternative to DirectWrite for layout, but GPUI proves DirectWrite works better for platform-native text.
- **swash + parley**: Modern Rust text stack. Considered for future but adds new dependencies.

---

## R4: PlatformWindow Trait Surface Design

### Decision: Merge control/query methods into PlatformWindow, keep Window trait as optional higher-level wrapper

### Rationale

GPUI has one PlatformWindow trait with ~50 methods. FLUI currently has PlatformWindow (8 methods) + Window trait (30+ methods). The split creates confusion about which to use. Merging critical methods into PlatformWindow ensures platform implementations provide everything the framework needs.

### Methods to Add to PlatformWindow

**Callbacks (9 methods):**
- `on_input(&self, Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>)`
- `on_request_frame(&self, Box<dyn FnMut() + Send>)`
- `on_resize(&self, Box<dyn FnMut(Size<Pixels>, f32) + Send>)`
- `on_moved(&self, Box<dyn FnMut() + Send>)`
- `on_close(&self, Box<dyn FnOnce() + Send>)`
- `on_should_close(&self, Box<dyn FnMut() -> bool + Send>)`
- `on_active_status_change(&self, Box<dyn FnMut(bool) + Send>)`
- `on_hover_status_change(&self, Box<dyn FnMut(bool) + Send>)`
- `on_appearance_changed(&self, Box<dyn FnMut() + Send>)`

**Control (10 methods):**
- `set_title(&self, title: &str)`
- `activate(&self)`
- `minimize(&self)`
- `maximize(&self)`
- `restore(&self)`
- `toggle_fullscreen(&self)`
- `resize(&self, size: Size<Pixels>)`
- `set_background_appearance(&self, WindowBackgroundAppearance)`
- `close(&self)`
- `set_edited(&self, edited: bool)` (default: no-op)

**Query (12 methods):**
- `bounds(&self) -> Bounds<Pixels>`
- `content_size(&self) -> Size<Pixels>`
- `window_bounds(&self) -> WindowBounds`
- `is_maximized(&self) -> bool`
- `is_fullscreen(&self) -> bool`
- `is_active(&self) -> bool`
- `is_hovered(&self) -> bool`
- `mouse_position(&self) -> Point<Pixels>`
- `modifiers(&self) -> Modifiers`
- `appearance(&self) -> WindowAppearance`
- `display(&self) -> Option<Arc<dyn PlatformDisplay>>`
- `get_title(&self) -> String`

**Total: 31 new + 6 existing = 37 methods**

### Alternatives Considered

- **Keep separate Window trait**: Creates duplication. Framework has to decide which to use. Window trait stays as optional convenience wrapper with builder pattern.
- **Single mega-trait**: GPUI approach. We're closer to this now but keep Window as separate higher-level abstraction that wraps PlatformWindow.

---

## R5: Platform Trait Expansion

### Decision: Add core platform services, skip macOS-specific and rare features

### Methods to Add

**App lifecycle (4 methods):**
- `activate(&self, ignoring_other_apps: bool)` (default: no-op)
- `hide(&self)` (default: no-op)
- `hide_other_apps(&self)` (default: no-op)
- `unhide_other_apps(&self)` (default: no-op)

**Appearance (2 methods):**
- `window_appearance(&self) -> WindowAppearance`
- `should_auto_hide_scrollbars(&self) -> bool` (default: false)

**Cursor (1 method):**
- `set_cursor_style(&self, style: CursorStyle)`

**Clipboard — enhanced (2 methods, replace existing):**
- `write_to_clipboard(&self, item: ClipboardItem)`
- `read_from_clipboard(&self) -> Option<ClipboardItem>`

**File operations (4 methods):**
- `prompt_for_paths(&self, options: PathPromptOptions) -> Task<Result<Option<Vec<PathBuf>>>>`
- `prompt_for_new_path(&self, dir: &Path, name: Option<&str>) -> Task<Result<Option<PathBuf>>>`
- `open_url(&self, url: &str)`
- `on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>)`

**Keyboard (3 methods):**
- `keyboard_layout(&self) -> String`
- `on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>)`
- `compositor_name(&self) -> &'static str` (default: "")

**Total: ~16 new methods**

### Skipped (Not Needed for MVP)

- `restart()` — app restart, rarely used
- `register_url_scheme()` — deep link registration, post-MVP
- `set_menus() / get_menus()` — macOS app menu, FLUI uses widget-based menus
- `set_dock_menu()` — macOS dock, platform-specific
- `write_credentials() / read_credentials()` — separate crate concern
- `screen_capture_sources()` — feature-gated even in GPUI
- `path_for_auxiliary_executable()` — rare utility

---

## R6: New Types Required

### CursorStyle Enum
~20 variants matching GPUI: Arrow, IBeam, Crosshair, ClosedHand, OpenHand, PointingHand, resize variants, OperationNotAllowed, DragLink, DragCopy, ContextualMenu, None.

### WindowAppearance Enum
Light, Dark, VibrantLight, VibrantDark. Default: Light.

### WindowBackgroundAppearance Enum
Opaque, Transparent, Blurred, MicaBackdrop, MicaAltBackdrop. Default: Opaque.

### WindowBounds Enum
Windowed(Bounds<Pixels>), Maximized(Bounds<Pixels>), Fullscreen(Bounds<Pixels>).

### DispatchEventResult Struct
```rust
pub struct DispatchEventResult {
    pub propagate: bool,
    pub default_prevented: bool,
}
```

### ClipboardItem Struct
```rust
pub struct ClipboardItem {
    pub entries: Vec<ClipboardEntry>,
}

pub enum ClipboardEntry {
    String(ClipboardString),
    // Image and ExternalPaths deferred to post-MVP
}

pub struct ClipboardString {
    pub text: String,
    pub metadata: Option<String>,
}
```

### PathPromptOptions Struct
```rust
pub struct PathPromptOptions {
    pub files: bool,
    pub directories: bool,
    pub multiple: bool,
}
```

### FontMetrics Struct (for PlatformTextSystem)
```rust
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub cap_height: f32,
    pub x_height: f32,
}
```

### LineLayout Struct
```rust
pub struct LineLayout {
    pub font_size: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub runs: Vec<ShapedRun>,
    pub len: usize,
}
```

---

## R7: Constitution Compliance Verification

| Rule | Status | Evidence |
|------|--------|---------|
| `unsafe` only in flui-platform | PASS | All unsafe in Win32 FFI with SAFETY comments |
| No unwrap() in library code | PASS | All errors via anyhow::Result |
| tracing for logging | PASS | No println/dbg |
| On-demand rendering | PASS | ControlFlow::Wait, render on dirty |
| >=70% test coverage | PENDING | Will measure after implementation |
| No Arc<Mutex> for tree structures | N/A | Platform layer uses Mutex for callbacks, not tree nodes |
| Strict DAG | PASS | flui-platform depends only on flui-types |
| Platform code confined to flui-platform | PASS | All Win32/DirectWrite code in platform crate |
