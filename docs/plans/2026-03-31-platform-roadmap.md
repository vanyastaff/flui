# flui-platform Roadmap Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Clean up Platform trait (breaking changes), fix critical bugs, implement Web/WASM platform, fix Winit backend, and establish cross-platform foundation for macOS/Linux/Mobile.

**Architecture:** Hybrid strategy — Windows native (already complete), Web standalone (WASM + web-sys), macOS/Linux via fixed Winit backend. Text system unified on cosmic-text (PlatformTextSystem removed). Font loading through flui-assets.

**Tech Stack:** Rust, wgpu 25.x, winit 0.30, web-sys, wasm-bindgen, cosmic-text, tokio, flume, flui-assets

---

## Phase 1: Platform Trait Cleanup (Breaking Changes)

### Task 1: Remove PlatformTextSystem from Platform trait

**Problem:** `PlatformTextSystem` duplicates cosmic-text. Two text stacks = measurement inconsistency.
cosmic-text + fontdb handles font discovery, measurement, and shaping. glyphon handles rendering.
flui-assets handles font loading. Platform has zero text responsibilities.

**Files:**
- Modify: `crates/flui-platform/src/traits/platform.rs` — remove `text_system()` method and all font types
- Modify: `crates/flui-platform/src/platforms/windows/platform.rs` — remove `text_system` field and impl
- Modify: `crates/flui-platform/src/platforms/headless/platform.rs` — remove text_system impl
- Modify: `crates/flui-platform/src/platforms/winit/platform.rs` — remove text_system impl
- Modify: `crates/flui-platform/src/platforms/macos/platform.rs` — remove text_system impl
- Modify: `crates/flui-platform/src/platforms/linux/mod.rs` — remove text_system impl
- Modify: `crates/flui-platform/src/platforms/web/platform.rs` — remove text_system impl
- Modify: `crates/flui-platform/src/lib.rs` — remove text-related re-exports
- Delete: `crates/flui-platform/src/platforms/windows/text_system.rs` — DirectWriteTextSystem
- Modify: `crates/flui-platform/Cargo.toml` — remove DirectWrite feature flags if unused elsewhere

**Step 1: Remove `text_system()` from Platform trait**

In `traits/platform.rs`, remove:
```rust
// DELETE these lines:
fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
```

Remove the entire `PlatformTextSystem` trait, `FontId`, `GlyphId`, `Font`, `FontWeight`, `FontStyle`, `FontRun`, `FontMetrics`, `LineLayout`, `ShapedRun`, `ShapedGlyph`, `TextSystemError` types. These now live in cosmic-text's types.

**Step 2: Remove all platform text_system implementations**

Delete `text_system()` impl from every platform:
- `WindowsPlatform` — remove the `text_system` field and its initialization
- `HeadlessPlatform` — remove mock text system
- `WinitPlatform` — remove SimpleTextSystem
- All stubs (macOS, Linux, Web, Android, iOS)

**Step 3: Delete DirectWriteTextSystem**

Delete `crates/flui-platform/src/platforms/windows/text_system.rs` entirely.
Remove `mod text_system;` from `crates/flui-platform/src/platforms/windows/mod.rs`.

**Step 4: Clean up re-exports**

In `lib.rs`, remove: `FontId`, `FontMetrics`, `FontRun`, `FontStyle`, `FontWeight`, `GlyphId`, `LineLayout`, `PlatformTextSystem`, `ShapedGlyph`, `ShapedRun`, `TextSystemError` from the `pub use traits::` block.

**Step 5: Update Cargo.toml**

Remove DirectWrite feature flags from the windows dependencies if they're only used by text_system:
- `Win32_Graphics_DirectWrite` — check if used elsewhere before removing

**Step 6: Verify**

```bash
rtk cargo check -p flui-platform
rtk cargo test -p flui-platform --lib
```

Fix any downstream compilation errors. The text system tests in `tests/text_system.rs` and `tests/text_system_contracts.rs` should be deleted entirely.

**Step 7: Commit**

```
refactor(platform)!: remove PlatformTextSystem from Platform trait

BREAKING CHANGE: text_system() removed from Platform trait.
Text measurement/shaping now handled by cosmic-text (in flui-engine).
Font loading handled by flui-assets.
Removes ~800 lines of DirectWrite text system code.
```

---

### Task 2: Remove `request_frame()` from Platform trait

**Problem:** Duplicates `PlatformWindow::request_redraw()`. Doesn't know WHICH window to redraw. Unimplemented on Windows (stub). Confusing API.

**Files:**
- Modify: `crates/flui-platform/src/traits/platform.rs` — remove `request_frame()` method
- Modify: All platform implementations — remove `request_frame()` impls

**Step 1: Remove from trait**

In `traits/platform.rs`, remove:
```rust
// DELETE:
fn request_frame(&self);
```

**Step 2: Remove from all implementations**

Remove `request_frame()` from: WindowsPlatform, HeadlessPlatform, WinitPlatform, MacOSPlatform, LinuxPlatform, WebPlatform, AndroidPlatform, IOSPlatform.

**Step 3: Search for callers**

```bash
grep -rn "request_frame\(\)" crates/ --include="*.rs"
```

Update any callers to use `window.request_redraw()` instead.

**Step 4: Verify and commit**

```bash
rtk cargo check --workspace
```

```
refactor(platform)!: remove request_frame() from Platform trait

BREAKING CHANGE: Use PlatformWindow::request_redraw() instead.
request_frame() was unimplemented and duplicated per-window redraw.
```

---

### Task 3: Fix `run()` signature for ownership

**Problem:** `run(&self)` takes shared reference but winit's event loop requires ownership. WinitPlatform::run() panics.

**Files:**
- Modify: `crates/flui-platform/src/traits/platform.rs` — change `run(&self)` to `run(self: Box<Self>)`
- Modify: All platform implementations — update `run` signature
- Modify: All callers of `platform.run()`

**Step 1: Change trait signature**

```rust
// OLD:
fn run(&self, on_ready: Box<dyn FnOnce()>);

// NEW:
fn run(self: Box<Self>, on_ready: Box<dyn FnOnce()>);
```

**Step 2: Add double-call guard**

Since `run` now consumes `Box<Self>`, double-call is impossible by construction (ownership moved). No guard needed — the type system enforces it.

**Step 3: Update `current_platform()` return type**

`current_platform()` currently returns `Arc<dyn Platform>`. With `run(self: Box<Self>)`, we need `Box<dyn Platform>`:

```rust
pub fn current_platform() -> anyhow::Result<Box<dyn Platform>> {
    // ... platform selection ...
}
```

**Step 4: Update all platform implementations**

Change `fn run(&self, ...)` to `fn run(self: Box<Self>, ...)` in every platform:
- WindowsPlatform: move state out of Arc, run message loop
- HeadlessPlatform: call on_ready directly
- WinitPlatform: take ownership of event loop, call `event_loop.run()` — NO MORE PANIC

**Step 5: Update all callers**

```bash
grep -rn "\.run(" crates/ --include="*.rs"
```

Change from `platform.run(...)` to `platform.run(...)` (same call, but platform is now Box not Arc).

**Step 6: Verify and commit**

```bash
rtk cargo check --workspace
rtk cargo test -p flui-platform --lib
```

```
refactor(platform)!: change run(&self) to run(self: Box<Self>)

BREAKING CHANGE: Platform::run() now takes ownership.
Prevents double-call at compile time.
Fixes WinitPlatform::run() panic (winit needs event loop ownership).
current_platform() now returns Box<dyn Platform>.
```

---

### Task 4: Clean up WindowMode and WindowEvent

**Files:**
- Modify: `crates/flui-platform/src/traits/platform.rs`

**Step 1: Fix WindowMode::Fullscreen**

```rust
// OLD:
Fullscreen {
    restore_style: u32,        // Windows-specific leak
    restore_bounds: Bounds<DevicePixels>,
}

// NEW:
Fullscreen {
    restore_bounds: Bounds<DevicePixels>,
}
```

Move `restore_style` to Windows-specific code (store in `WindowContext` directly).

**Step 2: Remove deprecated WindowEvent variants**

```rust
// DELETE:
#[deprecated(note = "Use FocusChanged instead")]
Focused(WindowId),
#[deprecated(note = "Use FocusChanged instead")]
Unfocused(WindowId),
```

**Step 3: Update Windows fullscreen code**

In `windows/window.rs`, store `restore_style: u32` in `WindowContext` instead of `WindowMode`.

**Step 4: Verify and commit**

```
refactor(platform)!: clean up WindowMode and WindowEvent

BREAKING CHANGE: WindowMode::Fullscreen no longer has restore_style.
Removed deprecated Focused/Unfocused variants from WindowEvent.
```

---

### Task 5: Fix HeadlessPlatform capabilities() unsound unsafe

**Files:**
- Modify: `crates/flui-platform/src/platforms/headless/platform.rs`

**Step 1: Store capabilities as owned field**

```rust
struct HeadlessState {
    capabilities: DesktopCapabilities,  // owned, not computed on the fly
    // ... other fields ...
}

// In Platform impl:
fn capabilities(&self) -> &dyn PlatformCapabilities {
    // Return reference to owned field
    &self.capabilities
}
```

Remove the `unsafe` pointer cast.

**Step 2: Verify and commit**

```bash
rtk cargo test -p flui-platform --lib
```

```
fix(platform): remove unsound unsafe in HeadlessPlatform::capabilities()
```

---

### Task 6: Fix ForegroundExecutor per-task runtime

**Files:**
- Modify: `crates/flui-platform/src/executor.rs`

**Step 1: Replace per-task runtime with simple closure execution**

The ForegroundExecutor sends closures to the main thread via flume channel.
For simple futures (`async { 42 }`), we don't need a tokio runtime at all —
just `block_on` with a simple executor or use `futures_lite::future::block_on`.

```rust
pub fn spawn<R: Send + 'static>(
    &self,
    future: impl Future<Output = R> + Send + 'static,
) -> Task<R> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sender = self.sender.clone();

    if let Err(e) = sender.send(Box::new(move || {
        // Use the existing background runtime handle for polling
        // instead of creating a new runtime per task
        let result = futures_lite::future::block_on(future);
        let _ = tx.send(result);
    })) {
        tracing::error!("Failed to send task: {:?}", e);
    }

    Task::from_handle(tokio::task::spawn(async move {
        rx.await.expect("Foreground task dropped")
    }))
}
```

Or even simpler — if the future is just `async { value }`, `block_on` completes immediately.

**Step 2: Add `futures-lite` dependency if needed**

Check if `futures-lite` is available or use `pollster::block_on` (lighter weight).

**Step 3: Verify and commit**

```bash
rtk cargo test -p flui-platform --lib
```

```
perf(platform): remove per-task tokio runtime in ForegroundExecutor
```

---

## Phase 2: Web/WASM Platform

### Task 7: Create Web platform skeleton

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/platform.rs` — full WebPlatform implementation
- Create: `crates/flui-platform/src/platforms/web/window.rs` — WebWindow
- Create: `crates/flui-platform/src/platforms/web/events.rs` — DOM event conversion
- Create: `crates/flui-platform/src/platforms/web/clipboard.rs` — Navigator clipboard API
- Create: `crates/flui-platform/src/platforms/web/text.rs` — Canvas2D measureText (temporary until cosmic-text WASM works)
- Create: `crates/flui-platform/src/platforms/web/executor.rs` — wasm-bindgen-futures executor
- Modify: `crates/flui-platform/Cargo.toml` — add web-sys features

**Step 1: WebPlatform struct**

```rust
pub struct WebPlatform {
    canvas: web_sys::HtmlCanvasElement,
    windows: RefCell<HashMap<WindowId, WebWindow>>,
    handlers: RefCell<PlatformHandlers>,
    clipboard: Arc<WebClipboard>,
    capabilities: WebCapabilities,
}

impl Platform for WebPlatform {
    fn run(self: Box<Self>, on_ready: Box<dyn FnOnce()>) {
        // No event loop to "run" — browser owns the event loop
        // Just call on_ready and set up requestAnimationFrame
        on_ready();
    }

    fn quit(&self) {
        // Close the tab? Navigate away? No-op on web.
        tracing::warn!("quit() called on WebPlatform — no-op in browser");
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        // "Window" = canvas element. Web is single-window.
        let window = WebWindow::new(&self.canvas, options)?;
        Ok(Box::new(window))
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        vec![Arc::new(WebDisplay::new())] // screen object
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.clipboard.clone()
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str { "Web" }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        // beforeunload event
        *self.handlers.borrow_mut().quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        *self.handlers.borrow_mut().window_event = Some(callback);
    }

    // ... remaining methods with sensible defaults ...
}
```

**Step 2: Verify skeleton compiles**

```bash
# Cross-compile check (no actual WASM target needed for check)
rtk cargo check -p flui-platform --target wasm32-unknown-unknown
```

**Step 3: Commit**

```
feat(platform): web platform skeleton with canvas-based window
```

---

### Task 8: Web window and rendering surface

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/window.rs`

**Step 1: WebWindow struct**

```rust
pub struct WebWindow {
    canvas: web_sys::HtmlCanvasElement,
    window_id: WindowId,
    scale_factor: f64,
    callbacks: WindowCallbacks,
}

impl WebWindow {
    pub fn new(canvas: &HtmlCanvasElement, options: WindowOptions) -> Result<Self> {
        // Set canvas size from options
        let dpr = web_sys::window().unwrap().device_pixel_ratio();
        let w = options.size.width.0 as u32;
        let h = options.size.height.0 as u32;
        canvas.set_width((w as f64 * dpr) as u32);
        canvas.set_height((h as f64 * dpr) as u32);
        canvas.style().set_property("width", &format!("{}px", w))?;
        canvas.style().set_property("height", &format!("{}px", h))?;

        Ok(Self {
            canvas: canvas.clone(),
            window_id: WindowId(1), // Web = single window
            scale_factor: dpr,
            callbacks: WindowCallbacks::new(),
        })
    }
}

impl PlatformWindow for WebWindow {
    fn physical_size(&self) -> Size<DevicePixels> {
        Size::new(
            DevicePixels(self.canvas.width() as i32),
            DevicePixels(self.canvas.height() as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let phys = self.physical_size();
        Size::new(
            px(phys.width.0 as f32 / self.scale_factor as f32),
            px(phys.height.0 as f32 / self.scale_factor as f32),
        )
    }

    fn scale_factor(&self) -> f64 { self.scale_factor }

    fn request_redraw(&self) {
        // requestAnimationFrame
        // wasm_bindgen closure that calls dispatch_request_frame
    }

    fn is_focused(&self) -> bool {
        web_sys::window().unwrap()
            .document().unwrap()
            .has_focus().unwrap_or(false)
    }

    fn is_visible(&self) -> bool {
        !web_sys::window().unwrap()
            .document().unwrap()
            .hidden()
    }

    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // raw-window-handle web canvas support
        use raw_window_handle::{WebCanvasWindowHandle, RawWindowHandle};
        // ... construct handle from canvas ...
    }
}
```

**Step 2: Verify and commit**

```
feat(platform): web window with canvas element and DPR support
```

---

### Task 9: Web input events

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/events.rs`

**Step 1: Wire DOM events to PlatformInput**

```rust
pub fn setup_event_listeners(canvas: &HtmlCanvasElement, callbacks: &Arc<WindowCallbacks>) {
    // Pointer events
    setup_pointer_down(canvas, callbacks);
    setup_pointer_move(canvas, callbacks);
    setup_pointer_up(canvas, callbacks);
    setup_wheel(canvas, callbacks);

    // Keyboard events (on document, not canvas)
    setup_key_down(callbacks);
    setup_key_up(callbacks);

    // Resize (on window)
    setup_resize(callbacks);

    // Focus/blur
    setup_focus_blur(callbacks);
}

fn setup_pointer_down(canvas: &HtmlCanvasElement, callbacks: &Arc<WindowCallbacks>) {
    let cb = callbacks.clone();
    let closure = Closure::wrap(Box::new(move |event: web_sys::PointerEvent| {
        let input = pointer_event_to_platform_input(&event, PointerEventType::Down);
        cb.dispatch_input(input);
    }) as Box<dyn FnMut(_)>);
    canvas.add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref()).unwrap();
    closure.forget(); // leak — lives for app lifetime
}
```

**Step 2: DOM event → W3C PlatformInput conversion**

```rust
fn pointer_event_to_platform_input(event: &web_sys::PointerEvent, kind: PointerEventType) -> PlatformInput {
    // web_sys::PointerEvent already IS W3C — minimal conversion
    PlatformInput::Pointer(PointerEvent {
        // Map DOM fields to ui-events types
    })
}
```

**Step 3: Verify and commit**

```
feat(platform): web input event handling (pointer, keyboard, scroll)
```

---

### Task 10: Web clipboard

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/clipboard.rs`

**Step 1: Navigator Clipboard API**

```rust
pub struct WebClipboard;

impl Clipboard for WebClipboard {
    fn read_text(&self) -> Option<String> {
        // navigator.clipboard.readText() is async — can't use from sync trait
        // Fallback: use a cached value from last paste event
        None // TODO: async clipboard
    }

    fn write_text(&self, text: String) {
        if let Some(window) = web_sys::window() {
            if let Some(clipboard) = window.navigator().clipboard() {
                let _ = clipboard.write_text(&text);
            }
        }
    }

    fn has_text(&self) -> bool {
        true // Assume clipboard has content
    }
}
```

**Step 2: Commit**

```
feat(platform): web clipboard via Navigator API
```

---

### Task 11: Web executor (wasm-bindgen-futures)

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/executor.rs`

**Step 1: WASM executor**

```rust
pub struct WebExecutor;

impl PlatformExecutor for WebExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        wasm_bindgen_futures::spawn_local(async move {
            task();
        });
    }

    fn is_on_executor(&self) -> bool {
        true // WASM is single-threaded
    }
}
```

**Note:** On WASM, background executor = foreground executor (single-threaded). Both use `spawn_local`.

**Step 2: Commit**

```
feat(platform): web executor via wasm-bindgen-futures
```

---

### Task 12: Web cursor styles

**Files:**
- Modify: `crates/flui-platform/src/platforms/web/platform.rs`

**Step 1: CSS cursor mapping**

```rust
fn set_cursor_style(&self, style: CursorStyle) {
    let css_cursor = match style {
        CursorStyle::Arrow => "default",
        CursorStyle::IBeam => "text",
        CursorStyle::Crosshair => "crosshair",
        CursorStyle::PointingHand => "pointer",
        CursorStyle::ResizeLeftRight => "ew-resize",
        CursorStyle::ResizeUpDown => "ns-resize",
        CursorStyle::OperationNotAllowed => "not-allowed",
        // ... map all variants ...
    };
    self.canvas.style().set_property("cursor", css_cursor).ok();
}
```

**Step 2: Commit**

```
feat(platform): web cursor styles via CSS cursor property
```

---

### Task 13: Web integration test

**Files:**
- Create: `crates/flui-platform/tests/web_platform.rs`

**Step 1: Test that WebPlatform compiles and basic methods work**

This needs `wasm-pack test` or at minimum `cargo check --target wasm32-unknown-unknown`.

```bash
rtk cargo check -p flui-platform --target wasm32-unknown-unknown
```

**Step 2: Commit**

```
test(platform): verify web platform compiles for wasm32 target
```

---

## Phase 3: Fix Winit Backend

### Task 14: Fix WinitPlatform::run() with new signature

**Problem:** With `run(self: Box<Self>)`, winit can now take ownership of the event loop.

**Files:**
- Modify: `crates/flui-platform/src/platforms/winit/platform.rs`

**Step 1: Implement proper run()**

```rust
impl Platform for WinitPlatform {
    fn run(self: Box<Self>, on_ready: Box<dyn FnOnce()>) {
        let event_loop = self.event_loop; // take ownership — possible now!
        on_ready();
        event_loop.run(move |event, target| {
            match event {
                Event::WindowEvent { event, window_id } => {
                    self.handle_window_event(event, window_id, target);
                }
                Event::AboutToWait => {
                    self.drain_foreground_tasks();
                }
                _ => {}
            }
        }).unwrap();
    }
}
```

**Step 2: Wire winit events to PlatformInput**

Map `winit::event::WindowEvent` variants to `PlatformInput`:
- `CursorMoved` → `PointerEvent::Move`
- `MouseInput` → `PointerEvent::Down/Up`
- `MouseWheel` → `PointerEvent::Scroll`
- `KeyboardInput` → `KeyboardEvent`
- `Resized` → `WindowEvent::Resized`
- `ScaleFactorChanged` → `WindowEvent::ScaleFactorChanged`
- `Focused` → `WindowEvent::FocusChanged`
- `CloseRequested` → `WindowEvent::CloseRequested`

**Step 3: Winit text system — cosmic-text**

Since PlatformTextSystem is removed, no text system needed on the platform. cosmic-text handles everything in the engine layer.

**Step 4: Test on Windows (winit uses Win32 under the hood)**

```bash
rtk cargo test -p flui-platform --features winit-backend --lib
```

**Step 5: Commit**

```
fix(platform): fix WinitPlatform::run() — no more panic

Takes ownership via run(self: Box<Self>). Properly runs winit event loop.
Wire winit events to PlatformInput W3C types.
```

---

### Task 15: Winit clipboard (arboard)

**Files:**
- Modify: `crates/flui-platform/src/platforms/winit/platform.rs`

**Step 1: Use arboard for cross-platform clipboard**

```rust
pub struct WinitClipboard {
    clipboard: parking_lot::Mutex<arboard::Clipboard>,
}

impl Clipboard for WinitClipboard {
    fn read_text(&self) -> Option<String> {
        self.clipboard.lock().get_text().ok()
    }

    fn write_text(&self, text: String) {
        self.clipboard.lock().set_text(text).ok();
    }
}
```

`arboard` is already a dependency.

**Step 2: Commit**

```
feat(platform): winit clipboard via arboard (cross-platform)
```

---

## Phase 4: macOS Native Additions

### Task 16: Fix macOS capabilities panic

**Files:**
- Modify: `crates/flui-platform/src/platforms/macos/platform.rs`

**Step 1: Return DesktopCapabilities instead of panic**

```rust
fn capabilities(&self) -> &dyn PlatformCapabilities {
    &self.capabilities // stored as field
}
```

**Step 2: Commit**

```
fix(platform): fix macOS capabilities() panic
```

---

### Task 17: macOS-specific features via Winit

With Winit backend fixed (Task 14), macOS gets window management, events, and basic functionality for free. Only add native code for:

- macOS appearance detection (dark mode) — via `NSApp.effectiveAppearance`
- macOS native file dialogs — `NSOpenPanel` / `NSSavePanel` (or use `rfd` crate)
- macOS clipboard — handled by arboard (Task 15)

These are optional polish tasks, not MVP blockers.

**Step 1: Test Winit on macOS**

```bash
# On macOS machine:
cargo run -p flui-platform --example basic --features winit-backend
```

**Step 2: Commit**

```
test(platform): verify winit backend works on macOS
```

---

## Phase 5: Linux Polish

### Task 18: Linux via Winit

With Winit backend fixed, Linux gets everything for free:
- X11 + Wayland window management
- Keyboard/mouse events
- Display enumeration
- Clipboard via arboard

**Step 1: Test Winit on Linux**

```bash
cargo run -p flui-platform --example basic --features winit-backend
```

**Step 2: Commit**

```
test(platform): verify winit backend works on Linux (X11 + Wayland)
```

---

## Phase 6: Mobile (Future — not in this plan)

Deferred. Requires:
- Android: `android-activity` + winit Android backend + touch gestures + virtual keyboard
- iOS: UIKit + winit iOS backend + safe area insets + touch gestures

Estimated: 4+ weeks. Separate plan when needed.

---

## Execution Order and Dependencies

```
Task 1 (Remove PlatformTextSystem)  ← Breaking, do first
Task 2 (Remove request_frame)      ← Breaking, parallel with 1
Task 3 (Fix run() signature)       ← Breaking, parallel with 1-2
Task 4 (Clean WindowMode/Event)    ← Breaking, parallel with 1-3
Task 5 (Fix HeadlessPlatform)       ← Independent, any time
Task 6 (Fix ForegroundExecutor)     ← Independent, any time
--- Phase 1 complete: clean Platform trait ---
Task 7-12 (Web platform)           ← After Phase 1
Task 13 (Web test)                  ← After 7-12
--- Phase 2 complete: Web works ---
Task 14 (Fix Winit run)            ← After Task 3 (new run signature)
Task 15 (Winit clipboard)          ← After Task 14
--- Phase 3 complete: Winit works ---
Task 16 (macOS capabilities)       ← Independent
Task 17 (macOS test via Winit)     ← After Task 14
Task 18 (Linux test via Winit)     ← After Task 14
--- Phases 4-5 complete: macOS + Linux work ---
```

**Recommended parallel waves:**

1. **Wave 1:** Tasks 1-6 parallel (all Phase 1 cleanup — independent breaking changes)
2. **Wave 2:** Tasks 7-13 (Web platform — sequential within, depends on Phase 1)
3. **Wave 3:** Tasks 14-15 + 16 parallel (Winit fix + macOS fix)
4. **Wave 4:** Tasks 17-18 parallel (macOS + Linux testing)

**Total estimated effort:** 3-4 weeks
