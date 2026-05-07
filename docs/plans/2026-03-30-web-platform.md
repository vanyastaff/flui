# Web Platform (WASM) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the web/WASM platform backend so FLUI apps can run in a browser with Canvas rendering, input handling, and requestAnimationFrame loop.

**Architecture:** WebPlatform implements the existing `Platform` trait using `wasm-bindgen` + `web-sys` for browser APIs. A `<canvas>` element serves as the rendering surface. The render loop uses `requestAnimationFrame`. Input arrives via DOM PointerEvent/KeyboardEvent listeners mapped to FLUI's W3C-based `PlatformInput`. Structure mirrors HeadlessPlatform (Arc<Mutex<State>> pattern) with real browser API calls.

**Tech Stack:** wasm-bindgen 0.2, web-sys 0.3, js-sys 0.3, wasm-bindgen-futures 0.4, console_error_panic_hook 0.1

---

## Phase Overview

| Phase | Task | Description |
|-------|------|-------------|
| 1 | 1-3 | Dependencies, WebPlatform scaffold, WebCapabilities |
| 2 | 4-6 | WebWindow (Canvas), WebDisplay, WebClipboard |
| 3 | 7-8 | WebTextSystem, WebExecutor |
| 4 | 9-10 | Input events, requestAnimationFrame loop |
| 5 | 11-12 | Platform::run() integration, flui-app run_web() |
| 6 | 13-14 | Web example, smoke test |

---

### Task 1: Add wasm32 dependencies to flui-platform

**Files:**
- Modify: `crates/flui-platform/Cargo.toml`

**Step 1: Add wasm32-specific dependencies**

Add after the Android dependencies block (line ~76):

```toml
# Web/WASM platform
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = [
    # Core DOM
    "Window", "Document", "Element", "HtmlCanvasElement", "HtmlElement",
    # Events
    "Event", "EventTarget", "AddEventListenerOptions",
    "PointerEvent", "KeyboardEvent", "WheelEvent", "MouseEvent",
    "FocusEvent", "UiEvent",
    # Rendering
    "CanvasRenderingContext2d", "DomRect",
    # Display
    "Screen", "MediaQueryList", "MediaQueryListEvent",
    # Observers
    "ResizeObserver", "ResizeObserverEntry", "ResizeObserverSize",
    # Animation
    "Performance",
    # Clipboard
    "Clipboard", "Navigator",
    # Visibility
    "VisibilityState",
    # Console
    "console",
] }
console_error_panic_hook = "0.1"
```

**Step 2: Add `web` feature to flui-platform**

In `[features]` section, add:

```toml
web = []
```

**Step 3: Verify Cargo.toml parses**

Run: `rtk cargo check -p flui-platform --features web`
Expected: Warning about unused feature, but no parse errors. (Will error on wasm32 target only deps since we're not compiling for wasm — that's OK, the TOML is validated.)

**Step 4: Commit**

```bash
rtk git add crates/flui-platform/Cargo.toml && rtk git commit -m "feat(web): add wasm-bindgen and web-sys dependencies"
```

---

### Task 2: Scaffold WebPlatform module structure

**Files:**
- Rewrite: `crates/flui-platform/src/platforms/web/mod.rs`
- Create: `crates/flui-platform/src/platforms/web/platform.rs`
- Create: `crates/flui-platform/src/platforms/web/window.rs`
- Create: `crates/flui-platform/src/platforms/web/display.rs`
- Create: `crates/flui-platform/src/platforms/web/clipboard.rs`
- Create: `crates/flui-platform/src/platforms/web/text_system.rs`
- Create: `crates/flui-platform/src/platforms/web/executor.rs`
- Create: `crates/flui-platform/src/platforms/web/events.rs`

**Step 1: Replace mod.rs with module declarations**

```rust
//! Web/WASM platform implementation
//!
//! Implements the Platform trait for web browsers via WebAssembly using
//! wasm-bindgen and web-sys for browser API access.

mod clipboard;
mod display;
mod events;
mod executor;
mod platform;
mod text_system;
mod window;

pub use platform::WebPlatform;
```

**Step 2: Create empty stub files for each module**

Each file starts with a module doc comment and `#![allow(unused)]` temporarily. The platform.rs should contain the `WebPlatform` struct definition with `WebState` inner state (following HeadlessPlatform pattern):

`platform.rs`:
```rust
//! Web platform core implementation

use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use wasm_bindgen::prelude::*;

use crate::{
    shared::PlatformHandlers,
    traits::*,
};

use super::{
    clipboard::WebClipboard,
    display::WebDisplay,
    executor::WebExecutor,
    text_system::WebTextSystem,
    window::WebWindow,
};

/// Web/WASM platform implementation
pub struct WebPlatform {
    state: Arc<Mutex<WebState>>,
}

struct WebState {
    capabilities: WebCapabilities,
    handlers: PlatformHandlers,
    foreground_executor: Arc<WebExecutor>,
    background_executor: Arc<WebExecutor>,
    text_system: Arc<WebTextSystem>,
    clipboard: Arc<WebClipboard>,
    window: Option<WebWindow>,
    is_running: bool,
}

// SAFETY: WebPlatform is single-threaded (WASM is single-threaded).
// wasm-bindgen types are !Send+!Sync but WASM has no threads.
unsafe impl Send for WebPlatform {}
unsafe impl Sync for WebPlatform {}

impl WebPlatform {
    /// Create a new Web platform instance
    pub fn new() -> Result<Self> {
        // Install panic hook for better error messages
        console_error_panic_hook::set_once();

        let state = WebState {
            capabilities: WebCapabilities,
            handlers: PlatformHandlers::new(),
            foreground_executor: Arc::new(WebExecutor::new()),
            background_executor: Arc::new(WebExecutor::new()),
            text_system: Arc::new(WebTextSystem::new()),
            clipboard: Arc::new(WebClipboard::new()),
            window: None,
            is_running: false,
        };

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
        })
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }
}
```

**Step 3: Create minimal stubs for other files**

`window.rs`, `display.rs`, `clipboard.rs`, `text_system.rs`, `executor.rs`, `events.rs` — each with the struct definition and a `new()` constructor, but with `unimplemented!()` for trait methods.

**Step 4: Verify compilation (must be on wasm32 target)**

This code is behind `#[cfg(target_arch = "wasm32")]`. Verify with:
```bash
rustup target add wasm32-unknown-unknown
rtk cargo check -p flui-platform --target wasm32-unknown-unknown
```

Note: This will likely fail due to non-wasm dependencies (tokio, parking_lot). We'll need conditional compilation — see Step 5 below.

**Step 5: Fix conditional compilation**

The `flui-platform` Cargo.toml has `tokio` and `num_cpus` as unconditional dependencies. These don't work on wasm32. Wrap them:

In `Cargo.toml` move tokio/num_cpus/flume under:
```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "sync", "time"] }
num_cpus = "1.16"
flume = "0.11"
waker-fn = "1.2"
```

Also check that `parking_lot` works on wasm32 (it does — it uses `wasm-bindgen` feature automatically).

**Step 6: Commit**

```bash
rtk git add -A crates/flui-platform/src/platforms/web/ crates/flui-platform/Cargo.toml
rtk git commit -m "feat(web): scaffold WebPlatform module structure with wasm32 conditional deps"
```

---

### Task 3: Implement WebExecutor

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/executor.rs`

**Step 1: Implement WebExecutor**

WASM is single-threaded, so both foreground and background executor just run tasks immediately (like HeadlessPlatform's TestExecutor). Future enhancement: use `wasm-bindgen-futures` to spawn async tasks.

```rust
//! Web executor implementation
//!
//! WASM is single-threaded, so tasks run immediately on the main thread.
//! Future: use Web Workers for true background execution.

use crate::traits::PlatformExecutor;

/// Web executor that runs tasks immediately (single-threaded WASM)
pub struct WebExecutor;

// SAFETY: WASM is single-threaded
unsafe impl Send for WebExecutor {}
unsafe impl Sync for WebExecutor {}

impl WebExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformExecutor for WebExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        task();
    }

    fn is_on_executor(&self) -> bool {
        true // Always on main thread in WASM
    }
}
```

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/executor.rs
rtk git commit -m "feat(web): implement WebExecutor (single-threaded WASM)"
```

---

### Task 4: Implement WebDisplay

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/display.rs`

**Step 1: Implement WebDisplay using Screen API**

```rust
//! Web display implementation using Screen API

use flui_types::geometry::{Bounds, DevicePixels, Point, Size, device_px};

use crate::traits::{DisplayId, PlatformDisplay};

/// Web display wrapping the browser's Screen API
pub struct WebDisplay {
    width: i32,
    height: i32,
    scale_factor: f64,
}

// SAFETY: WASM is single-threaded
unsafe impl Send for WebDisplay {}
unsafe impl Sync for WebDisplay {}

impl WebDisplay {
    /// Create from browser window dimensions
    pub fn from_browser() -> Self {
        let window = web_sys::window().expect("no global window");
        let screen = window.screen().expect("no screen");

        let width = screen.width().unwrap_or(1920);
        let height = screen.height().unwrap_or(1080);
        let scale_factor = window.device_pixel_ratio();

        Self {
            width,
            height,
            scale_factor,
        }
    }
}

impl PlatformDisplay for WebDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(0) // Single display in browser
    }

    fn name(&self) -> String {
        "Browser Screen".to_string()
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        Bounds::new(
            Point::new(device_px(0), device_px(0)),
            Size::new(device_px(self.width), device_px(self.height)),
        )
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        true
    }
}
```

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/display.rs
rtk git commit -m "feat(web): implement WebDisplay via Screen API"
```

---

### Task 5: Implement WebClipboard

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/clipboard.rs`

**Step 1: Implement using in-memory fallback**

The browser Clipboard API is async and requires user activation. For MVP, use an in-memory fallback (like HeadlessPlatform). Future enhancement: use `navigator.clipboard` with `wasm-bindgen-futures`.

```rust
//! Web clipboard implementation
//!
//! Uses in-memory storage for MVP. Future: navigator.clipboard API.

use parking_lot::Mutex;

use crate::traits::Clipboard;

/// Web clipboard (in-memory for MVP)
pub struct WebClipboard {
    content: Mutex<Option<String>>,
}

// SAFETY: WASM is single-threaded
unsafe impl Send for WebClipboard {}
unsafe impl Sync for WebClipboard {}

impl WebClipboard {
    pub fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for WebClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}
```

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/clipboard.rs
rtk git commit -m "feat(web): implement WebClipboard (in-memory MVP)"
```

---

### Task 6: Implement WebTextSystem

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/text_system.rs`

**Step 1: Implement using Canvas measureText**

Similar to HeadlessPlatform's MockTextSystem but uses Canvas 2D `measureText()` for real metrics when available, with estimated fallbacks.

```rust
//! Web text system using Canvas measureText
//!
//! Uses an offscreen Canvas 2D context for text measurement.
//! Font enumeration is limited on web — returns common web-safe fonts.

use std::borrow::Cow;

use crate::traits::{
    Font, FontId, FontMetrics, FontRun, FontWeight, GlyphId, LineLayout, PlatformTextSystem,
};

/// Web text system using Canvas 2D measureText
pub struct WebTextSystem {
    // Offscreen canvas context created lazily
}

// SAFETY: WASM is single-threaded
unsafe impl Send for WebTextSystem {}
unsafe impl Sync for WebTextSystem {}

impl WebTextSystem {
    pub fn new() -> Self {
        Self {}
    }

    fn get_context(&self) -> Option<web_sys::CanvasRenderingContext2d> {
        let document = web_sys::window()?.document()?;
        let canvas = document
            .create_element("canvas")
            .ok()?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .ok()?;
        canvas
            .get_context("2d")
            .ok()?
            .and_then(|ctx| ctx.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    }

    fn css_font_string(descriptor: &Font, size: f32) -> String {
        let style = match descriptor.style {
            crate::traits::FontStyle::Normal => "",
            crate::traits::FontStyle::Italic => "italic ",
            crate::traits::FontStyle::Oblique => "oblique ",
        };
        let weight = descriptor.weight.to_numeric();
        let family = if descriptor.family.is_empty() {
            "sans-serif"
        } else {
            &descriptor.family
        };
        format!("{style}{weight} {size}px {family}")
    }
}

impl PlatformTextSystem for WebTextSystem {
    fn add_fonts(&self, _fonts: Vec<Cow<'static, [u8]>>) -> anyhow::Result<()> {
        // TODO: Use CSS FontFace API to load custom fonts
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        // Web-safe fonts that are always available
        vec![
            "sans-serif".to_string(),
            "serif".to_string(),
            "monospace".to_string(),
            "Arial".to_string(),
            "Helvetica".to_string(),
            "Times New Roman".to_string(),
            "Courier New".to_string(),
        ]
    }

    fn font_id(&self, _descriptor: &Font) -> anyhow::Result<FontId> {
        // On web, we use CSS font strings directly — FontId is a placeholder
        Ok(FontId(0))
    }

    fn font_metrics(&self, _font_id: FontId) -> FontMetrics {
        // Estimated metrics (Canvas API doesn't expose full font metrics)
        FontMetrics {
            units_per_em: 1000,
            ascent: 800.0,
            descent: 200.0,
            line_gap: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            cap_height: 700.0,
            x_height: 500.0,
        }
    }

    fn glyph_for_char(&self, _font_id: FontId, ch: char) -> Option<GlyphId> {
        Some(GlyphId(ch as u32))
    }

    fn layout_line(&self, text: &str, font_size: f32, _runs: &[FontRun]) -> LineLayout {
        // Try to use Canvas measureText for accurate width
        let width = if let Some(ctx) = self.get_context() {
            let font_str = format!("{font_size}px sans-serif");
            ctx.set_font(&font_str);
            ctx.measure_text(text)
                .map(|m| m.width() as f32)
                .unwrap_or(text.chars().count() as f32 * font_size * 0.6)
        } else {
            text.chars().count() as f32 * font_size * 0.6
        };

        LineLayout {
            font_size,
            width,
            ascent: font_size * 0.8,
            descent: font_size * 0.2,
            runs: Vec::new(),
            len: text.len(),
        }
    }
}
```

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/text_system.rs
rtk git commit -m "feat(web): implement WebTextSystem via Canvas measureText"
```

---

### Task 7: Implement WebWindow

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/window.rs`

This is the core component — wraps a `<canvas>` element.

**Step 1: Implement WebWindow struct and PlatformWindow trait**

```rust
//! Web window implementation wrapping a Canvas element

use std::sync::Arc;

use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size, device_px, px};
use parking_lot::Mutex;
use wasm_bindgen::prelude::*;

use crate::{
    shared::WindowCallbacks,
    traits::{
        DisplayId, DispatchEventResult, PlatformDisplay, PlatformInput, PlatformWindow,
        WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowId,
    },
};

use super::display::WebDisplay;

/// Web window wrapping a Canvas element
pub struct WebWindow {
    id: WindowId,
    canvas: web_sys::HtmlCanvasElement,
    state: Arc<Mutex<WebWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    // Store closures to prevent them from being dropped
    _closures: Arc<Mutex<Vec<Closure<dyn FnMut(web_sys::Event)>>>>,
}

struct WebWindowState {
    title: String,
    width: f32,
    height: f32,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    fullscreen: bool,
}

// SAFETY: WASM is single-threaded
unsafe impl Send for WebWindow {}
unsafe impl Sync for WebWindow {}

impl WebWindow {
    /// Create a new WebWindow from an existing canvas element or create one
    pub fn new(id: WindowId, title: &str, width: f32, height: f32) -> anyhow::Result<Self> {
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no global window"))?;
        let document = window
            .document()
            .ok_or_else(|| anyhow::anyhow!("no document"))?;
        let scale_factor = window.device_pixel_ratio();

        // Try to find existing canvas with id "flui-canvas", or create one
        let canvas = match document.get_element_by_id("flui-canvas") {
            Some(el) => el
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| anyhow::anyhow!("element 'flui-canvas' is not a canvas"))?,
            None => {
                let canvas = document
                    .create_element("canvas")
                    .map_err(|e| anyhow::anyhow!("failed to create canvas: {:?}", e))?
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .map_err(|_| anyhow::anyhow!("failed to cast to canvas"))?;
                canvas.set_id("flui-canvas");
                document
                    .body()
                    .ok_or_else(|| anyhow::anyhow!("no body"))?
                    .append_child(&canvas)
                    .map_err(|e| anyhow::anyhow!("failed to append canvas: {:?}", e))?;
                canvas
            }
        };

        // Set canvas size (physical pixels for sharp rendering)
        let phys_width = (width * scale_factor as f32) as u32;
        let phys_height = (height * scale_factor as f32) as u32;
        canvas.set_width(phys_width);
        canvas.set_height(phys_height);

        // Set CSS size (logical pixels)
        let style = canvas.style();
        let _ = style.set_property("width", &format!("{width}px"));
        let _ = style.set_property("height", &format!("{height}px"));

        // Set page title
        document.set_title(title);

        let state = WebWindowState {
            title: title.to_string(),
            width,
            height,
            scale_factor,
            focused: true,
            visible: true,
            fullscreen: false,
        };

        Ok(Self {
            id,
            canvas,
            state: Arc::new(Mutex::new(state)),
            callbacks: Arc::new(WindowCallbacks::new()),
            _closures: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Get a reference to the canvas element (for GPU surface creation)
    pub fn canvas(&self) -> &web_sys::HtmlCanvasElement {
        &self.canvas
    }

    /// Get the WindowCallbacks for event dispatch from the events module
    pub fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }

    /// Store a JS closure to prevent it from being dropped
    pub fn keep_closure(&self, closure: Closure<dyn FnMut(web_sys::Event)>) {
        self._closures.lock().push(closure);
    }

    /// Update the tracked size (called from resize observer)
    pub fn update_size(&self, width: f32, height: f32) {
        let mut state = self.state.lock();
        state.width = width;
        state.height = height;
    }

    /// Update focus state
    pub fn update_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
    }
}

impl PlatformWindow for WebWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        Size::new(
            device_px((state.width * state.scale_factor as f32) as i32),
            device_px((state.height * state.scale_factor as f32) as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        Size::new(px(state.width), px(state.height))
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        self.callbacks.dispatch_request_frame();
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let state = self.state.lock();
        Bounds::new(
            Point::new(px(0.0), px(0.0)),
            Size::new(px(state.width), px(state.height)),
        )
    }

    fn content_size(&self) -> Size<Pixels> {
        self.logical_size()
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.state.lock();
        if state.fullscreen {
            WindowBounds::Fullscreen(Bounds::new(
                Point::default(),
                Size::new(px(state.width), px(state.height)),
            ))
        } else {
            WindowBounds::Windowed(Bounds::new(
                Point::default(),
                Size::new(px(state.width), px(state.height)),
            ))
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.state.lock().fullscreen
    }

    fn is_active(&self) -> bool {
        self.state.lock().focused
    }

    fn appearance(&self) -> WindowAppearance {
        // Check prefers-color-scheme
        let window = web_sys::window();
        if let Some(w) = window {
            if let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)") {
                if mql.matches() {
                    return WindowAppearance::Dark;
                }
            }
        }
        WindowAppearance::Light
    }

    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(WebDisplay::from_browser()))
    }

    fn get_title(&self) -> String {
        self.state.lock().title.clone()
    }

    fn set_title(&self, title: &str) {
        self.state.lock().title = title.to_string();
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            document.set_title(title);
        }
    }

    fn toggle_fullscreen(&self) {
        let canvas: &web_sys::Element = self.canvas.as_ref();
        let is_fullscreen = self.state.lock().fullscreen;
        if is_fullscreen {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                document.exit_fullscreen();
            }
        } else {
            let _ = canvas.request_fullscreen();
        }
        self.state.lock().fullscreen = !is_fullscreen;
    }

    fn close(&self) {
        self.callbacks.dispatch_close();
    }

    // ==================== Callback Registration ====================

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        *self.callbacks.on_input.lock() = Some(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_request_frame.lock() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        *self.callbacks.on_resize.lock() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_moved.lock() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.callbacks.on_close.lock() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        *self.callbacks.on_should_close.lock() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_active_status_change.lock() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_appearance_changed.lock() = Some(callback);
    }

    // ==================== GPU Surface Handles ====================

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{RawWindowHandle, WebCanvasWindowHandle, WindowHandle};

        // Create a handle from the canvas element
        // wgpu uses this to create a WebGPU/WebGL surface
        let handle = WebCanvasWindowHandle::new(
            std::num::NonZero::new(self.canvas.clone().into()).unwrap(),
        );
        let raw = RawWindowHandle::WebCanvas(handle);
        // SAFETY: The canvas element is valid for the lifetime of this borrow
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{DisplayHandle, RawDisplayHandle, WebDisplayHandle};

        let raw = RawDisplayHandle::Web(WebDisplayHandle::new());
        // SAFETY: Web display handle is always valid
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
```

**Note:** The `window_handle` implementation needs careful attention. The `raw-window-handle` 0.6 crate requires `WebCanvasWindowHandle` which takes a `NonZero<u32>` or a `JsValue`. Check the exact API at build time — it may require `raw-window-handle` feature `web` or different construction. The key is that wgpu can create a surface from the canvas.

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/window.rs
rtk git commit -m "feat(web): implement WebWindow wrapping Canvas element"
```

---

### Task 8: Implement events module (DOM → PlatformInput)

**Files:**
- Implement: `crates/flui-platform/src/platforms/web/events.rs`

**Step 1: Map DOM events to PlatformInput**

```rust
//! DOM event → PlatformInput mapping
//!
//! Sets up DOM event listeners on the canvas and converts browser events
//! to FLUI's W3C-based PlatformInput types.

use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{
    shared::WindowCallbacks,
    traits::PlatformInput,
};

use super::window::WebWindow;

/// Register all DOM event listeners on the canvas
pub fn register_event_listeners(window: &WebWindow) {
    let canvas = window.canvas();
    let callbacks = Arc::clone(window.callbacks());

    // --- Pointer events ---
    register_pointer_events(canvas, &callbacks, window);

    // --- Keyboard events ---
    register_keyboard_events(&callbacks, window);

    // --- Focus events ---
    register_focus_events(canvas, window);

    // --- Wheel events ---
    register_wheel_events(canvas, &callbacks, window);

    // Prevent default context menu on canvas
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
    });
    let _ = canvas.add_event_listener_with_callback("contextmenu", closure.as_ref().unchecked_ref());
    closure.forget(); // Leak — lives for app lifetime
}

fn register_pointer_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    _window: &WebWindow,
) {
    // Pointer down/move/up events
    for event_name in &["pointerdown", "pointermove", "pointerup"] {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let pe: web_sys::PointerEvent = e.unchecked_into();
            if let Some(input) = convert_pointer_event(&pe) {
                callbacks.dispatch_input(input);
            }
        });
        let _ = canvas
            .add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

fn register_keyboard_events(
    callbacks: &Arc<WindowCallbacks>,
    window: &WebWindow,
) {
    let browser_window = web_sys::window().expect("no global window");

    for event_name in &["keydown", "keyup"] {
        let callbacks = Arc::clone(callbacks);
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let ke: web_sys::KeyboardEvent = e.unchecked_into();
            if let Some(input) = convert_keyboard_event(&ke) {
                callbacks.dispatch_input(input);
            }
        });
        let _ = browser_window
            .add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

fn register_focus_events(
    canvas: &web_sys::HtmlCanvasElement,
    window: &WebWindow,
) {
    // Make canvas focusable
    canvas.set_tab_index(0);

    let window_clone = unsafe {
        // SAFETY: WASM is single-threaded, pointer remains valid
        &*(window as *const WebWindow)
    };

    // Focus
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(true);
        });
        let _ = canvas.add_event_listener_with_callback("focus", closure.as_ref().unchecked_ref());
        closure.forget();
    }

    // Blur
    {
        let callbacks = Arc::clone(window.callbacks());
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            callbacks.dispatch_active_status_change(false);
        });
        let _ = canvas.add_event_listener_with_callback("blur", closure.as_ref().unchecked_ref());
        closure.forget();
    }
}

fn register_wheel_events(
    canvas: &web_sys::HtmlCanvasElement,
    callbacks: &Arc<WindowCallbacks>,
    _window: &WebWindow,
) {
    let callbacks = Arc::clone(callbacks);
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        e.prevent_default();
        let we: web_sys::WheelEvent = e.unchecked_into();
        if let Some(input) = convert_wheel_event(&we) {
            callbacks.dispatch_input(input);
        }
    });

    let mut options = web_sys::AddEventListenerOptions::new();
    options.passive(false); // Need non-passive for preventDefault
    let _ = canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        closure.as_ref().unchecked_ref(),
        &options,
    );
    closure.forget();
}

// ==================== Event Conversion ====================

fn convert_pointer_event(pe: &web_sys::PointerEvent) -> Option<PlatformInput> {
    use ui_events::keyboard::Modifiers;
    use ui_events::pointer::{self, PointerEvent as UiPointerEvent};

    let event_type = pe.type_();
    let state = match event_type.as_str() {
        "pointerdown" => pointer::PointerState::Down,
        "pointermove" => pointer::PointerState::Moved,
        "pointerup" => pointer::PointerState::Up,
        _ => return None,
    };

    let mut modifiers = Modifiers::empty();
    if pe.shift_key() {
        modifiers |= Modifiers::SHIFT;
    }
    if pe.ctrl_key() {
        modifiers |= Modifiers::CONTROL;
    }
    if pe.alt_key() {
        modifiers |= Modifiers::ALT;
    }
    if pe.meta_key() {
        modifiers |= Modifiers::META;
    }

    let button = match pe.button() {
        0 => pointer::Button::Primary,
        1 => pointer::Button::Auxiliary,
        2 => pointer::Button::Secondary,
        3 => pointer::Button::Fourth,
        4 => pointer::Button::Fifth,
        _ => pointer::Button::Primary,
    };

    let pointer_event = UiPointerEvent {
        state,
        position: flui_types::geometry::Point::new(
            flui_types::geometry::px(pe.offset_x() as f32),
            flui_types::geometry::px(pe.offset_y() as f32),
        ),
        button,
        modifiers,
    };

    Some(PlatformInput::Pointer(pointer_event))
}

fn convert_keyboard_event(ke: &web_sys::KeyboardEvent) -> Option<PlatformInput> {
    use ui_events::keyboard::{self, KeyboardEvent as UiKeyboardEvent};

    let state = match ke.type_().as_str() {
        "keydown" => keyboard::KeyState::Down,
        "keyup" => keyboard::KeyState::Up,
        _ => return None,
    };

    let mut modifiers = keyboard_types::Modifiers::empty();
    if ke.shift_key() {
        modifiers |= keyboard_types::Modifiers::SHIFT;
    }
    if ke.ctrl_key() {
        modifiers |= keyboard_types::Modifiers::CONTROL;
    }
    if ke.alt_key() {
        modifiers |= keyboard_types::Modifiers::ALT;
    }
    if ke.meta_key() {
        modifiers |= keyboard_types::Modifiers::META;
    }

    let key = map_key_value(&ke.key());

    let keyboard_event = UiKeyboardEvent {
        state,
        key,
        code: ui_events::keyboard::Code::Unidentified,
        location: ui_events::keyboard::Location::Standard,
        modifiers,
        repeat: ke.repeat(),
        is_composing: ke.is_composing(),
    };

    Some(PlatformInput::Keyboard(keyboard_event))
}

fn convert_wheel_event(we: &web_sys::WheelEvent) -> Option<PlatformInput> {
    use crate::traits::ScrollDelta;

    let delta = ScrollDelta::Pixels(flui_types::geometry::Point::new(
        flui_types::geometry::px(we.delta_x() as f32),
        flui_types::geometry::px(we.delta_y() as f32),
    ));

    Some(PlatformInput::Scroll(delta))
}

/// Map DOM KeyboardEvent.key string to ui-events Key enum
fn map_key_value(key: &str) -> ui_events::keyboard::Key {
    use keyboard_types::NamedKey;
    use ui_events::keyboard::Key;

    match key {
        "Enter" => Key::Named(NamedKey::Enter),
        "Tab" => Key::Named(NamedKey::Tab),
        "Backspace" => Key::Named(NamedKey::Backspace),
        "Escape" => Key::Named(NamedKey::Escape),
        "ArrowUp" => Key::Named(NamedKey::ArrowUp),
        "ArrowDown" => Key::Named(NamedKey::ArrowDown),
        "ArrowLeft" => Key::Named(NamedKey::ArrowLeft),
        "ArrowRight" => Key::Named(NamedKey::ArrowRight),
        "Shift" => Key::Named(NamedKey::Shift),
        "Control" => Key::Named(NamedKey::Control),
        "Alt" => Key::Named(NamedKey::Alt),
        "Meta" => Key::Named(NamedKey::Meta),
        "Delete" => Key::Named(NamedKey::Delete),
        "Home" => Key::Named(NamedKey::Home),
        "End" => Key::Named(NamedKey::End),
        "PageUp" => Key::Named(NamedKey::PageUp),
        "PageDown" => Key::Named(NamedKey::PageDown),
        " " => Key::Named(NamedKey::Space),
        s if s.len() == 1 => Key::Character(s.to_string()),
        _ => Key::Unidentified,
    }
}
```

**Important note on PlatformInput:** Verify the actual `PlatformInput` enum variants before implementing. The pointer event struct may differ from what's shown here — check `crates/flui-platform/src/traits/input.rs` for exact types. The conversion functions must match the actual types used by FLUI.

**Step 2: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/events.rs
rtk git commit -m "feat(web): implement DOM event → PlatformInput conversion"
```

---

### Task 9: Implement Platform trait for WebPlatform

**Files:**
- Complete: `crates/flui-platform/src/platforms/web/platform.rs`

**Step 1: Implement all Platform trait methods**

Complete the `impl Platform for WebPlatform` block. Key method is `run()` which sets up the requestAnimationFrame loop:

```rust
impl Platform for WebPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.foreground_executor.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_state(|state| state.text_system.clone())
    }

    fn run(&self, on_ready: Box<dyn FnOnce()>) {
        tracing::info!("Starting web platform");

        self.with_state(|state| {
            state.is_running = true;
        });

        // Call on_ready synchronously — the browser event loop is already running
        on_ready();

        // Start requestAnimationFrame loop
        self.start_raf_loop();

        tracing::info!("Web platform ready");
    }

    fn quit(&self) {
        tracing::info!("Web platform quit requested");
        self.with_state(|state| {
            state.is_running = false;
            state.handlers.invoke_quit();
        });
    }

    fn request_frame(&self) {
        // In the browser, RAF is always running — this is a no-op hint
        // The RAF loop checks is_running to decide whether to render
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!(?options, "Creating web window (canvas)");

        let window = WebWindow::new(
            WindowId(0), // Single window in browser
            &options.title,
            options.size.width.0,
            options.size.height.0,
        )?;

        // Register DOM event listeners
        super::events::register_event_listeners(&window);

        // Notify window created
        self.with_state(|state| {
            state.handlers.invoke_window_event(
                crate::traits::WindowEvent::Created(WindowId(0)),
            );
        });

        Ok(Box::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
        Some(WindowId(0)) // Always the single canvas
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        Some(vec![WindowId(0)])
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        vec![Arc::new(WebDisplay::from_browser())]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(WebDisplay::from_browser()))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|state| state.clipboard.clone())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        // SAFETY: WebCapabilities is a ZST, reference is always valid
        &WebCapabilities
    }

    fn name(&self) -> &'static str {
        "Web (WASM)"
    }

    fn compositor_name(&self) -> &'static str {
        "Browser"
    }

    fn window_appearance(&self) -> WindowAppearance {
        if let Some(w) = web_sys::window() {
            if let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)") {
                if mql.matches() {
                    return WindowAppearance::Dark;
                }
            }
        }
        WindowAppearance::Light
    }

    fn open_url(&self, url: &str) {
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(url, "_blank");
        }
    }

    fn keyboard_layout(&self) -> String {
        // Not reliably available in browsers
        "en-US".to_string()
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.quit = Some(callback);
        });
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|state| {
            state.handlers.window_event = Some(callback);
        });
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        // Return the origin URL as the "app path"
        if let Some(w) = web_sys::window() {
            if let Ok(origin) = w.location().origin() {
                return Ok(std::path::PathBuf::from(origin));
            }
        }
        Ok(std::path::PathBuf::from("/"))
    }
}
```

**Step 2: Implement the RAF loop helper**

```rust
impl WebPlatform {
    fn start_raf_loop(&self) {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::JsCast;

        let state = Arc::clone(&self.state);

        // requestAnimationFrame recursive loop pattern
        let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let g = Rc::clone(&f);

        let window = web_sys::window().expect("no global window");

        *g.borrow_mut() = Some(Closure::new(move || {
            let is_running = state.lock().is_running;
            if !is_running {
                return;
            }

            // Request next frame before dispatching (ensures smooth loop)
            if let Some(w) = web_sys::window() {
                let _ = w.request_animation_frame(
                    f.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
                );
            }
        }));

        // Kick off the first frame
        let _ = window.request_animation_frame(
            g.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
        );
    }
}
```

**Step 3: Commit**

```bash
rtk git add crates/flui-platform/src/platforms/web/platform.rs
rtk git commit -m "feat(web): implement Platform trait for WebPlatform with RAF loop"
```

---

### Task 10: Update lib.rs and current_platform() for wasm32

**Files:**
- Modify: `crates/flui-platform/src/lib.rs`
- Modify: `crates/flui-platform/src/platforms/mod.rs` (if exists)

**Step 1: Ensure WebPlatform is exported**

Verify that `current_platform()` already handles `wasm32` (it does — line 358-370 in lib.rs). Just ensure the import path is correct:

```rust
#[cfg(target_arch = "wasm32")]
pub use platforms::web::WebPlatform;
```

**Step 2: Verify WebPlatform::new() is called correctly in current_platform()**

The existing code at line 369 calls `WebPlatform::new()?` — this should work with our new implementation.

**Step 3: Commit**

```bash
rtk git add crates/flui-platform/src/lib.rs
rtk git commit -m "feat(web): wire WebPlatform into current_platform() exports"
```

---

### Task 11: Implement run_web() in flui-app

**Files:**
- Modify: `crates/flui-app/src/app/runner.rs`
- Modify: `crates/flui-app/Cargo.toml`

**Step 1: Add wasm-bindgen dependency to flui-app**

In `crates/flui-app/Cargo.toml`:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
```

**Step 2: Implement run_web()**

Replace the stub `run_web()` function (lines 502-506) with the full implementation, mirroring `run_desktop()`:

```rust
#[cfg(target_arch = "wasm32")]
fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting web platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");
    let platform_inner = Arc::clone(&platform);

    platform.run(Box::new(move || {
        // 1. Open window (creates canvas)
        let options: WindowOptions = (&config).into();
        let window = platform_inner
            .open_window(options)
            .expect("Failed to create canvas window");

        // 2. Create GPU renderer
        // Note: On web, wgpu uses WebGPU or WebGL2 backend
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(async {
            let handle = PlatformWindowHandle(window.as_ref());
            Renderer::new(&handle).await
        });
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                platform_inner.quit();
                return;
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget
        mount_root(&root, phys_size.width.0 as f32, phys_size.height.0 as f32);

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // 5. Register input callback
        window.on_input(Box::new(move |input: PlatformInput| {
            AppBinding::instance().handle_input(input);
            DispatchEventResult {
                propagate: false,
                default_prevented: true,
            }
        }));

        // 6. Register frame callback
        let renderer_frame = Arc::clone(&renderer);
        window.on_request_frame(Box::new(move || {
            let binding = AppBinding::instance();

            if !binding.needs_redraw() && !binding.has_pending_work() {
                return;
            }

            let now = std::time::Instant::now();
            let scheduler = Scheduler::instance();
            let _frame_id = scheduler.handle_begin_frame(now);
            scheduler.handle_draw_frame();

            let mut r = renderer_frame.lock();
            binding.render_frame(&mut r);
        }));

        // 7. Register resize callback
        let renderer_resize = Arc::clone(&renderer);
        window.on_resize(Box::new(move |size, scale_factor| {
            let w = (size.width.0 * scale_factor) as u32;
            let h = (size.height.0 * scale_factor) as u32;
            renderer_resize.lock().resize(w, h);
            AppBinding::instance().request_redraw();
        }));

        // 8. Lifecycle
        platform_inner.on_quit(Box::new(|| {
            tracing::info!("Web platform quit");
            AppBinding::instance().transition_lifecycle(LifecycleEvent::Terminating);
        }));

        let platform_for_close = Arc::clone(&platform_inner);
        window.on_close(Box::new(move || {
            tracing::info!("Canvas window closed");
            platform_for_close.quit();
        }));

        // 9. Store window
        AppBinding::instance().set_window(window);

        tracing::info!("Web platform initialized");
    }));
}
```

**Step 3: Update the cfg dispatch in `run_app_with_config_impl`**

Make sure `run_web` receives both `root` and `config` (currently the stub only takes `config`):

```rust
#[cfg(target_arch = "wasm32")]
{
    run_web(root, config);
}
```

**Step 4: Commit**

```bash
rtk git add crates/flui-app/src/app/runner.rs crates/flui-app/Cargo.toml
rtk git commit -m "feat(web): implement run_web() in flui-app with GPU renderer"
```

---

### Task 12: Handle wasm32 compilation issues across workspace

**Files:**
- Possibly modify: `crates/flui-engine/Cargo.toml` (wgpu features for web)
- Possibly modify: `crates/flui-scheduler/src/lib.rs` (Instant on wasm)
- Possibly modify: various crates for `std::time::Instant` → `web-time`

**Step 1: Audit dependencies for wasm32 compatibility**

Key known issues on wasm32:
1. `std::time::Instant` doesn't work on wasm32 — need `web-time` or `instant` crate
2. `tokio` doesn't fully work on wasm32 — ensure it's not imported in web paths
3. `pollster::block_on` works on wasm32 but is synchronous
4. `parking_lot` works on wasm32 (no threads, so Mutex is trivial)
5. `wgpu` needs proper features for web: `webgpu`, `webgl`

**Step 2: Configure wgpu for web**

In workspace `Cargo.toml` or `flui-engine/Cargo.toml`, ensure wgpu has web features:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wgpu = { workspace = true, features = ["webgpu", "webgl"] }
```

**Step 3: Fix std::time::Instant if needed**

Add `web-time` crate for wasm32:
```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
web-time = "1.1"
```

Use a type alias:
```rust
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
```

**Step 4: Test full workspace compilation**

```bash
rtk cargo check --workspace --target wasm32-unknown-unknown
```

Fix all compilation errors. This is likely the most iterative task.

**Step 5: Commit**

```bash
rtk git add -A && rtk git commit -m "fix(web): resolve wasm32 compilation issues across workspace"
```

---

### Task 13: Create web example

**Files:**
- Create: `examples/web_demo/Cargo.toml`
- Create: `examples/web_demo/src/main.rs`
- Create: `examples/web_demo/index.html`
- Modify: `Cargo.toml` (add to workspace members)

**Step 1: Create example project**

`examples/web_demo/Cargo.toml`:
```toml
[package]
name = "flui-web-demo"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
flui-app = { path = "../../crates/flui-app", features = ["web"], default-features = false }
flui-view = { path = "../../crates/flui-view" }
wasm-bindgen = "0.2"
```

`examples/web_demo/src/lib.rs`:
```rust
use wasm_bindgen::prelude::*;
use flui_app::run_app;
use flui_view::{View, StatelessView, BuildContext};

#[derive(Clone)]
struct HelloWeb;

impl StatelessView for HelloWeb {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(HelloWeb)
    }
}

impl View for HelloWeb {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::StatelessElement::new(
            self,
            flui_view::element::StatelessBehavior,
        ))
    }
}

#[wasm_bindgen(start)]
pub fn start() {
    run_app(HelloWeb);
}
```

`examples/web_demo/index.html`:
```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>FLUI Web Demo</title>
    <style>
        body { margin: 0; overflow: hidden; background: #1a1a2e; }
        canvas { display: block; width: 100vw; height: 100vh; }
    </style>
</head>
<body>
    <canvas id="flui-canvas"></canvas>
    <script type="module">
        import init from './pkg/flui_web_demo.js';
        init();
    </script>
</body>
</html>
```

**Step 2: Build with wasm-pack**

```bash
cd examples/web_demo
wasm-pack build --target web --out-dir pkg
```

**Step 3: Serve and test**

```bash
# Simple HTTP server
python -m http.server 8080
# Open http://localhost:8080/index.html
```

**Step 4: Commit**

```bash
rtk git add examples/web_demo/ Cargo.toml
rtk git commit -m "feat(web): add web_demo example with wasm-pack build"
```

---

### Task 14: Verify end-to-end and write smoke tests

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/platform.rs` (add tests)

**Step 1: Write unit tests for WebPlatform (headless/mock)**

Since we can't run wasm32 unit tests easily, write cfg-gated tests that verify the logic:

```rust
#[cfg(test)]
mod tests {
    // Web platform tests require wasm32 target
    // Run with: wasm-pack test --headless --chrome

    #[test]
    fn test_web_capabilities() {
        use crate::traits::{PlatformCapabilities, WebCapabilities};

        let caps = WebCapabilities;
        assert_eq!(caps.platform_name(), "Web");
        assert!(caps.has_lifecycle_management());
        assert!(!caps.supports_multiple_windows());
        assert_eq!(caps.default_target_fps(), 60);
    }
}
```

**Step 2: Verify wasm-pack test works (optional, needs Chrome/Firefox)**

```bash
cd crates/flui-platform
wasm-pack test --headless --chrome
```

**Step 3: Commit**

```bash
rtk git add crates/flui-platform/
rtk git commit -m "test(web): add WebPlatform smoke tests"
```

---

## Known Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| `parking_lot::Mutex` on wasm32 | Works — single-threaded, no contention |
| `std::time::Instant` on wasm32 | Use `web-time` crate or `Performance.now()` |
| `raw-window-handle` Canvas API | Check 0.6 API — may need `WebCanvasWindowHandle` adjustments |
| wgpu WebGPU support | wgpu 25.x supports WebGPU/WebGL2 via feature flags |
| `tokio` on wasm32 | Gate all tokio usage behind `#[cfg(not(target_arch = "wasm32"))]` |
| `pollster::block_on` on wasm | Works synchronously — OK for init, not for async ops |
| DOM event types mismatch | Verify `ui-events` crate PointerEvent struct matches what we construct |

## Definition of Done

- [ ] `cargo check -p flui-platform --target wasm32-unknown-unknown` compiles
- [ ] `cargo check -p flui-app --target wasm32-unknown-unknown --features web --no-default-features` compiles
- [ ] `web_demo` example builds with `wasm-pack build --target web`
- [ ] Opening `index.html` in Chrome shows a canvas (even if blank — GPU init succeeds)
- [ ] Pointer and keyboard events logged to browser console
- [ ] No panics on load
