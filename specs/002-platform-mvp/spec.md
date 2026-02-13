# Feature Specification: flui-platform MVP Completion

**Feature Branch**: `002-platform-mvp`
**Created**: 2026-02-13
**Status**: Draft
**Input**: Bring flui-platform to GPUI-level completeness as the foundation for all upper framework layers.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Per-Window Callback Event Delivery (Priority: P1)

As a framework developer integrating flui-app with flui-platform, I need PlatformWindow to deliver input, resize, focus, and lifecycle events via per-window callbacks so that the framework can dispatch events to the correct window without centralized routing.

**Why this priority**: This is the architectural foundation. Without per-window callbacks, no upper layer (flui-app, flui-interaction, flui-scheduler) can receive events from the platform. GPUI's proven pattern: platform invokes framework via callbacks registered on PlatformWindow.

**Independent Test**: Create a WindowsPlatform window, register on_input/on_resize/on_close callbacks, simulate Win32 messages, verify callbacks fire with correct W3C event data.

**Acceptance Scenarios**:

1. **Given** a PlatformWindow with `on_input` callback registered, **When** a WM_MOUSEMOVE message arrives, **Then** the callback fires with a `PlatformInput::Pointer(PointerEvent)` containing correct logical coordinates.
2. **Given** a PlatformWindow with `on_resize` callback registered, **When** WM_SIZE arrives, **Then** the callback fires with new `Size<Pixels>` and current scale factor.
3. **Given** a PlatformWindow with `on_close` callback registered, **When** WM_CLOSE arrives, **Then** `on_should_close` is queried first; if it returns true, `on_close` fires and window is destroyed.
4. **Given** a PlatformWindow with `on_active_status_change` callback, **When** WM_SETFOCUS/WM_KILLFOCUS arrives, **Then** the callback fires with `true`/`false`.
5. **Given** a HeadlessPlatform window, **When** `inject_event(PlatformInput)` is called, **Then** the registered `on_input` callback fires with the injected event.

---

### User Story 2 - Window Control Methods (Priority: P1)

As a framework developer, I need PlatformWindow to provide methods for controlling window state (title, bounds, minimize, maximize, fullscreen, appearance) so that flui-app can manage windows without platform-specific code.

**Why this priority**: Window control is required alongside callbacks to complete the PlatformWindow contract. Without set_title, minimize, maximize, the framework cannot manage window state.

**Independent Test**: Create a window, call set_title/minimize/maximize/toggle_fullscreen, verify state changes via query methods (is_maximized, is_fullscreen, bounds).

**Acceptance Scenarios**:

1. **Given** a PlatformWindow, **When** `set_title("Hello")` is called, **Then** `get_title()` returns "Hello" and the native title bar updates.
2. **Given** a normal PlatformWindow, **When** `minimize()` is called, **Then** `is_minimized()` returns true and `on_resize` callback does NOT fire (minimized windows skip rendering).
3. **Given** a PlatformWindow, **When** `toggle_fullscreen()` is called, **Then** `is_fullscreen()` returns true, bounds match monitor size, and `on_resize` fires with new size.
4. **Given** a PlatformWindow, **When** `mouse_position()` is called, **Then** returns current cursor position in logical pixels relative to window content area.
5. **Given** a PlatformWindow, **When** `modifiers()` is called, **Then** returns current keyboard modifier state (Shift, Ctrl, Alt, Meta).

---

### User Story 3 - Platform Trait Expansion (Priority: P2)

As a framework developer, I need the Platform trait to provide app-level services (activation, cursor style, file dialogs, keyboard layout, URL opening) so that flui-app can offer these to widget developers.

**Why this priority**: These platform services are needed for a complete application but not for the minimal event loop. Can be added after the core window contract is solid.

**Independent Test**: Call each Platform method and verify platform-specific behavior (Windows: ShellExecuteW for open_url, IFileOpenDialog for prompt_for_paths, SetCursor for cursor style).

**Acceptance Scenarios**:

1. **Given** a Platform, **When** `activate(true)` is called, **Then** the application comes to foreground (Windows: SetForegroundWindow).
2. **Given** a Platform, **When** `set_cursor_style(CursorStyle::IBeam)` is called, **Then** the system cursor changes to text selection cursor.
3. **Given** a Platform, **When** `prompt_for_paths(PathPromptOptions { files: true, .. })` is called, **Then** a native file dialog opens and returns selected paths.
4. **Given** a Platform, **When** `open_url("https://example.com")` is called, **Then** the default browser opens the URL.
5. **Given** a Platform, **When** `window_appearance()` is called, **Then** returns Light or Dark based on Windows system theme.

---

### User Story 4 - Task<T> Async Abstraction (Priority: P2)

As a framework developer, I need executors that return `Task<T>` implementing `Future` with priority-based scheduling so that flui-scheduler and flui-app can spawn async work and await results.

**Why this priority**: The current fire-and-forget `spawn(FnOnce)` cannot return results. Task<T> is needed for flui-scheduler integration and any async platform operation (file dialogs, clipboard, etc.).

**Independent Test**: Spawn a Task<i32> on BackgroundExecutor, await it, verify result. Spawn with Priority::High, verify it runs before Priority::Low tasks.

**Acceptance Scenarios**:

1. **Given** a BackgroundExecutor, **When** `spawn(async { 42 })` is called, **Then** returns `Task<i32>` that resolves to 42 when awaited.
2. **Given** a ForegroundExecutor, **When** `spawn(async { "hello" })` is called, **Then** the task runs on the main thread and returns "hello".
3. **Given** multiple tasks with different priorities, **When** all are spawned, **Then** High priority tasks complete before Medium, Medium before Low.
4. **Given** a BackgroundExecutor, **When** `timer(Duration::from_millis(100))` is called, **Then** returns a Task that completes after ~100ms.
5. **Given** a Task, **When** `detach()` is called, **Then** the task continues running but the handle is dropped without blocking.

---

### User Story 5 - Text System with DirectWrite Backend (Priority: P3)

As a framework developer, I need PlatformTextSystem to provide real font enumeration, text measurement, and line layout via DirectWrite (Windows) so that flui-rendering can compute accurate text layout.

**Why this priority**: Text layout is critical for the rendering pipeline but can use approximate measurements initially. Full DirectWrite integration is needed before widgets can display real text.

**Independent Test**: Load system fonts, measure "Hello World" text, verify measurement is within 5% of expected size. Layout a line with mixed font runs, verify glyph positions.

**Acceptance Scenarios**:

1. **Given** a PlatformTextSystem on Windows, **When** `all_font_names()` is called, **Then** returns a list of all installed system fonts including "Segoe UI".
2. **Given** a PlatformTextSystem, **When** `font_metrics(font_id)` is called, **Then** returns accurate FontMetrics (ascent, descent, line_gap, units_per_em).
3. **Given** a PlatformTextSystem, **When** `layout_line("Hello", 16.0, runs)` is called, **Then** returns a LineLayout with accurate glyph positions and total width.
4. **Given** a PlatformTextSystem, **When** `add_fonts(vec![font_bytes])` is called, **Then** the font is loadable by name in subsequent calls.

---

### User Story 6 - Headless Platform Testing Support (Priority: P2)

As a test author, I need HeadlessPlatform to support all PlatformWindow callbacks with programmatic event injection and all Platform services with mocks so that framework tests run without a display server.

**Why this priority**: Testing infrastructure is critical for CI. Every new trait method must have a headless implementation.

**Independent Test**: In headless mode: create window, inject pointer event, verify on_input callback fires. Inject resize, verify on_resize fires. All without any OS windowing.

**Acceptance Scenarios**:

1. **Given** a HeadlessPlatform window, **When** `inject_pointer_event(event)` is called, **Then** the registered `on_input` callback fires.
2. **Given** a HeadlessPlatform window, **When** `simulate_resize(800, 600)` is called, **Then** `on_resize` callback fires with Size(800, 600).
3. **Given** a HeadlessPlatform, **When** `set_cursor_style(IBeam)` is called, **Then** the call succeeds silently (no-op but no error).
4. **Given** a HeadlessPlatform, **When** `prompt_for_paths()` is called, **Then** returns a configurable mock result (not a real dialog).

---

### Edge Cases

- What happens when a callback is registered after window creation? Must still work.
- What happens when `on_should_close` returns false? Window must NOT be destroyed.
- What happens when resize events arrive faster than rendering? Must coalesce per `should_coalesce_pointer_moves`.
- What happens when a Task is dropped before completion? Must not leak resources.
- What happens when the last Arc<PlatformWindow> is dropped? Native window must be destroyed.
- What happens when text measurement is called with an empty string? Must return zero-size rect.
- What happens when `toggle_fullscreen` is called on a minimized window? Must restore first, then fullscreen.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: PlatformWindow MUST support per-window callback registration for: `on_input`, `on_request_frame`, `on_resize`, `on_moved`, `on_close`, `on_should_close`, `on_active_status_change`, `on_hover_status_change`, `on_appearance_changed`.
- **FR-002**: PlatformWindow MUST provide window control methods: `set_title`, `activate`, `minimize`, `maximize`, `zoom`, `toggle_fullscreen`, `resize`, `set_background_appearance`.
- **FR-003**: PlatformWindow MUST provide state query methods: `bounds`, `content_size`, `is_maximized`, `is_fullscreen`, `is_active`, `is_hovered`, `mouse_position`, `modifiers`, `appearance`, `display`.
- **FR-004**: Platform trait MUST provide: `activate`, `hide`, `window_appearance`, `set_cursor_style`, `should_auto_hide_scrollbars`, `open_url`, `prompt_for_paths`, `prompt_for_new_path`, `keyboard_layout`, `on_keyboard_layout_change`, `on_open_urls`.
- **FR-005**: Clipboard MUST support rich content via `ClipboardItem` with text + metadata entries (not just plain String).
- **FR-006**: Executors MUST return `Task<T>` implementing `Future<Output = T>` with `detach()` and priority-based scheduling.
- **FR-007**: PlatformTextSystem MUST provide: `add_fonts`, `all_font_names`, `font_id`, `font_metrics`, `glyph_for_char`, `layout_line`.
- **FR-008**: WindowsPlatform MUST wire Win32 WndProc messages to per-window callbacks via the existing event conversion system.
- **FR-009**: HeadlessPlatform MUST implement all new trait methods with mock/injectable behavior.
- **FR-010**: All callbacks MUST use `&self` (not `&mut self`) on PlatformWindow, using interior mutability (Cell/RefCell/Mutex) for callback storage.
- **FR-011**: Event delivery MUST use W3C-compliant types from `ui-events` and `keyboard-types` crates.
- **FR-012**: PlatformWindow MUST implement `HasWindowHandle + HasDisplayHandle` from `raw-window-handle` for GPU surface creation.
- **FR-013**: All new public types and traits MUST have `///` doc comments per constitution.
- **FR-014**: Platform event loop MUST use on-demand rendering (ControlFlow::Wait pattern) per constitution.

### Key Entities

- **PlatformWindow**: Cross-platform window abstraction with callbacks, control, and state query.
- **Platform**: Application-level platform services (lifecycle, cursor, dialogs, clipboard, executors, text).
- **Task<T>**: Future-based async task handle with priority and detach support.
- **PlatformTextSystem**: Font enumeration, metrics, glyph lookup, and line layout.
- **ClipboardItem**: Rich clipboard content with multiple entries (text, metadata).
- **CursorStyle**: Enum of ~20 cursor styles (Arrow, IBeam, PointingHand, resize variants, etc.).
- **WindowAppearance**: Light/Dark/VibrantLight/VibrantDark theme detection.
- **WindowBackgroundAppearance**: Opaque/Transparent/Blurred/Mica backdrop styles.
- **DispatchEventResult**: Callback return type indicating if event was handled and if default should be prevented.
- **PathPromptOptions**: File dialog configuration (files, directories, multiple selection).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: PlatformWindow trait has >= 30 methods (callbacks + control + queries), matching GPUI's core surface.
- **SC-002**: Platform trait has >= 35 methods covering app lifecycle, cursor, dialogs, keyboard, clipboard.
- **SC-003**: All WindowsPlatform WndProc messages are wired to per-window callbacks (pointer, keyboard, resize, focus, close, move).
- **SC-004**: Task<T> implements Future and supports spawn/await/detach with Priority (High/Medium/Low).
- **SC-005**: PlatformTextSystem on Windows provides real font enumeration and text measurement via DirectWrite (not stubs).
- **SC-006**: HeadlessPlatform implements 100% of trait methods (no unimplemented!() calls).
- **SC-007**: `cargo test -p flui-platform` passes with >= 70% coverage (constitution requirement).
- **SC-008**: `cargo clippy -p flui-platform -- -D warnings` passes with zero warnings.
- **SC-009**: All existing examples compile and run (zero broken examples).
- **SC-010**: flui-platform public API is sufficient for flui-app to replace direct winit dependency (verified by API surface comparison).
