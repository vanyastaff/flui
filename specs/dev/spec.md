# Feature Specification: flui-platform MVP - Cross-Platform Abstraction Layer

**Feature Branch**: `dev`
**Created**: 2026-01-26
**Status**: Planning
**Input**: "find out and learn docs about flui-platform and check .flutter and .gpui to make plan to mvp ready crate with support cross platform"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Native Window Creation and Management (Priority: P1) 🎯 MVP

**Goal**: Developers can create and manage native windows on Windows and macOS with full lifecycle control.

**Why this priority**: Window management is the foundation of any UI framework. Without windows, no rendering or interaction is possible.

**Independent Test**: Create a simple application that opens a window, resizes it, changes modes (minimize/maximize/fullscreen), and responds to close events. Must work identically on Windows and macOS.

**Acceptance Scenarios**:

1. **Given** application starts, **When** `platform.open_window()` is called, **Then** native OS window appears with specified size and title
2. **Given** window is open, **When** user clicks close button, **Then** `on_window_event(CloseRequested)` callback fires and window can be destroyed gracefully
3. **Given** window is Normal mode, **When** `window.set_mode(Maximized)` is called, **Then** window maximizes to full screen bounds
4. **Given** window is created, **When** DPI scaling changes (moving between monitors), **Then** `on_window_event(ScaleFactorChanged)` fires with new scale
5. **Given** window is visible, **When** `window.request_redraw()` is called, **Then** `on_window_event(RedrawRequested)` fires immediately

---

### User Story 2 - Text System Integration for Rendering (Priority: P1) 🎯 MVP

**Goal**: Text can be measured, shaped, and rendered using platform-native text systems (DirectWrite on Windows, Core Text on macOS).

**Why this priority**: Text rendering is critical for any UI. Without text measurement, layout calculations are impossible.

**Independent Test**: Load a font, measure text bounds for various strings (ASCII, Unicode, CJK), verify metrics match platform expectations. Must integrate with flui_painting for rendering.

**Acceptance Scenarios**:

1. **Given** platform initialized, **When** `text_system.default_font_family()` is called, **Then** returns platform-appropriate font ("Segoe UI" on Windows, "SF Pro Text" on macOS)
2. **Given** font loaded, **When** text is measured with font size 16pt, **Then** returns bounding box in logical pixels
3. **Given** text with emoji/CJK characters, **When** measured, **Then** returns correct width considering character composition
4. **Given** text shaped, **When** glyph positions requested, **Then** returns array of positioned glyphs for rendering
5. **Given** text system available, **When** font family requested that doesn't exist, **Then** falls back to default font gracefully

---

### User Story 3 - Cross-Platform Event Handling (Priority: P1) 🎯 MVP

**Goal**: Mouse, keyboard, and window events are captured and dispatched using W3C-standard event types across all platforms.

**Why this priority**: User interaction is fundamental. Consistent event handling ensures predictable behavior across platforms.

**Independent Test**: Capture mouse clicks, keyboard presses, window resize/move events. Verify event data (position, modifiers, key codes) matches W3C specifications on both Windows and macOS.

**Acceptance Scenarios**:

1. **Given** window has focus, **When** user clicks left mouse button, **Then** `PointerEvent::Down(Primary)` fires with logical pixel coordinates
2. **Given** window visible, **When** user presses 'A' key with Ctrl held, **Then** `KeyboardEvent` fires with `Key::Character("a")` and `Modifiers::CONTROL`
3. **Given** window is resized by user, **When** drag completes, **Then** `WindowEvent::Resized` fires with new logical size
4. **Given** window receives mouse movement, **When** cursor moves 10px, **Then** `PointerEvent::Move` contains delta in `PixelDelta` units
5. **Given** multi-touch device, **When** two fingers touch screen, **Then** separate `PointerEvent` for each touch point with unique pointer ID

---

### User Story 4 - Headless Platform for CI/Testing (Priority: P2)

**Goal**: Run all platform API tests in CI without GPU or display server, enabling automated testing and benchmarking.

**Why this priority**: Automated testing is essential for code quality. Headless platform enables CI/CD without complex virtualization.

**Independent Test**: Run full test suite with `FLUI_HEADLESS=1` environment variable. All platform API calls succeed with mock implementations. No window actually opens.

**Acceptance Scenarios**:

1. **Given** `FLUI_HEADLESS=1` set, **When** `current_platform()` called, **Then** returns `HeadlessPlatform` implementation
2. **Given** headless mode, **When** `open_window()` called, **Then** returns mock window with configurable size but no OS window
3. **Given** headless clipboard, **When** text written then read, **Then** returns same text (in-memory storage)
4. **Given** headless executor, **When** task spawned, **Then** executes immediately on calling thread (no async overhead)
5. **Given** headless platform, **When** tests run in parallel, **Then** no race conditions or shared state issues

---

### User Story 5 - Display/Monitor Enumeration (Priority: P2)

**Goal**: Query all connected displays/monitors with DPI-aware bounds, scale factors, and refresh rates for correct multi-monitor rendering.

**Why this priority**: Multi-monitor setups are common. Proper display enumeration prevents rendering artifacts and scaling issues.

**Independent Test**: Connect multiple monitors with different DPIs. Enumerate displays, verify bounds don't overlap incorrectly, move window between monitors, verify scale factor updates.

**Acceptance Scenarios**:

1. **Given** two monitors connected, **When** `platform.displays()` called, **Then** returns array with two `PlatformDisplay` objects
2. **Given** primary monitor queried, **When** `platform.primary_display()` called, **Then** returns display marked as primary by OS
3. **Given** 4K monitor (2x scale), **When** `display.scale_factor()` called, **Then** returns 2.0
4. **Given** display bounds queried, **When** `display.usable_bounds()` called, **Then** excludes taskbar/menu bar areas
5. **Given** monitor unplugged, **When** displays re-enumerated, **Then** removed display no longer in list

---

### User Story 6 - Async Executor System (Priority: P3)

**Goal**: Background executor for CPU/IO tasks (file loading, network) and foreground executor for UI thread tasks with clean async/await integration.

**Why this priority**: Async operations are common (asset loading, network requests). Proper executor prevents blocking UI thread.

**Independent Test**: Spawn background task that sleeps 100ms, then posts result to foreground. Verify UI remains responsive, result arrives on correct thread.

**Acceptance Scenarios**:

1. **Given** background executor, **When** CPU-intensive task spawned, **Then** runs on worker thread pool (not UI thread)
2. **Given** foreground executor, **When** task spawned, **Then** executes on next UI event loop iteration
3. **Given** async file load on background executor, **When** completed, **Then** callback can safely update UI state
4. **Given** multiple background tasks, **When** spawned concurrently, **Then** execute in parallel across worker threads
5. **Given** foreground task queue, **When** drained in event loop, **Then** tasks execute FIFO order

---

### Edge Cases

- **What happens when window created with invalid size (0x0, negative)?** → Return error, document minimum size constraints (e.g., 1x1 logical pixels)
- **How does system handle opening 100+ windows?** → Platform limits (OS-dependent), document resource constraints, return error after limit
- **What if DPI scaling is fractional (1.25, 1.5)?** → Round scale factor to nearest 0.25, document precision limits
- **How to handle window events during window destruction?** → Queue events, drop on unmount, document lifecycle safety
- **What if text system requested before platform initialization?** → Panic with clear message "Platform not initialized", document initialization order
- **How to handle Unicode beyond BMP (emoji, rare CJK)?** → Use UTF-8 throughout, text system must support full Unicode 15.0

## Requirements *(mandatory)*

### Functional Requirements

**Window Management:**
- **FR-001**: System MUST create native OS windows with configurable size, title, decorations
- **FR-002**: System MUST support window modes: Normal, Minimized, Maximized, Fullscreen
- **FR-003**: System MUST emit events for window lifecycle: Created, Resized, Moved, FocusChanged, CloseRequested, Closed
- **FR-004**: System MUST handle DPI scaling changes (per-monitor DPI v2 on Windows, Retina on macOS)
- **FR-004a**: System MUST support wgpu surface lifecycle - create surface on window creation, recreate surface on window resize/DPI change, destroy surface on window close (integrated via raw-window-handle)
- **FR-005**: System MUST support multiple concurrent windows with independent event streams

**Text System:**
- **FR-006**: System MUST integrate DirectWrite (Windows) and Core Text (macOS) for glyph shaping
- **FR-007**: System MUST provide font enumeration and family lookup
- **FR-007a**: System MUST implement font fallback chain: (1) requested family, (2) platform default font (Segoe UI/SF Pro Text), (3) first available system font (guaranteed fallback - never error)
- **FR-008**: System MUST measure text bounds in logical pixels for layout calculations
- **FR-009**: System MUST support full Unicode 15.0 including: (1) grapheme cluster segmentation for emoji sequences (ZWJ, variation selectors), (2) Unicode normalization (NFC), (3) bidirectional text (Arabic/Hebrew RTL), (4) complex script shaping (Devanagari ligatures, Thai vowel placement), (5) automatic font fallback for missing glyphs
- **FR-010**: System MUST return glyph positions for rendering integration with flui_painting as Vec<GlyphPosition> where each glyph contains: glyph_id (u32), x_offset (Pixels), y_offset (Pixels), x_advance (Pixels) - all coordinates relative to baseline origin
- **FR-010a**: System MUST provide Canvas::draw_glyphs() integration contract - PlatformTextSystem::shape_glyphs() output MUST be directly compatible with flui_painting::Canvas::draw_glyphs() input without transformation

**Event Handling:**
- **FR-011**: System MUST use W3C-standard event types (ui-events crate) for cross-platform consistency
- **FR-012**: System MUST dispatch PointerEvent (mouse, touch, stylus) with logical pixel coordinates
- **FR-013**: System MUST dispatch KeyboardEvent with keyboard-types Key enum and Modifiers
- **FR-014**: System MUST convert OS-native events (WM_*, NSEvent, Wayland) to W3C events in platform layer
- **FR-015**: System MUST provide callback registration for event handling (decoupled from platform implementation)

**Platform Abstraction:**
- **FR-016**: System MUST provide Platform trait with lifecycle (run, quit), window management, executors, clipboard
- **FR-017**: System MUST implement WindowsPlatform (Win32 API), MacOSPlatform (AppKit), HeadlessPlatform (testing)
- **FR-018**: System MUST use type erasure (Box<dyn Trait>) for platform-agnostic code
- **FR-019**: System MUST use callback registry pattern (GPUI-inspired) for framework decoupling
- **FR-020**: System MUST provide `current_platform()` function with automatic platform detection (conditional compilation)

**Display/Monitor Management:**
- **FR-021**: System MUST enumerate all connected displays with unique DisplayId
- **FR-022**: System MUST provide display bounds in DevicePixels (physical coordinates) and logical size via scale_factor
- **FR-023**: System MUST distinguish primary display from secondary displays
- **FR-024**: System MUST provide usable bounds (excluding taskbars, menu bars, notches)
- **FR-025**: System MUST support refresh rate queries (default 60Hz)

**Executor/Async Runtime:**
- **FR-026**: System MUST provide BackgroundExecutor for CPU/IO-bound tasks (multi-threaded Tokio)
- **FR-027**: System MUST provide ForegroundExecutor for UI thread tasks (flume channel-based)
- **FR-028**: System MUST execute foreground tasks in event loop (drain_tasks() in message pump)
- **FR-029**: System MUST ensure thread safety (BackgroundExecutor Send+Sync, ForegroundExecutor !Send)
- **FR-030**: System MUST support async/await integration with tokio runtime

**Clipboard:**
- **FR-031**: System MUST support read/write of plain text (UTF-8)
- **FR-032**: System MUST use platform-native clipboard APIs (CF_UNICODETEXT on Windows, NSPasteboard on macOS)
- **FR-033**: System MUST handle clipboard ownership transfer correctly (Windows HGLOBAL lifetime)
- **FR-034**: System MUST be thread-safe (use Mutex for clipboard access)
- **FR-035**: System MUST provide has_text() for format detection

### Non-Functional Requirements

**Performance:**
- **NFR-001**: Text measurement latency MUST be <1ms for strings <100 characters (measured via criterion benchmarks)
- **NFR-002**: Event dispatch latency MUST be <5ms from OS event to callback invocation (measured via tracing timestamps)
- **NFR-003**: Display enumeration MUST complete in <10ms even with 4+ monitors connected (measured via benchmarks)
- **NFR-004**: Executor spawn overhead MUST be <100µs for both background and foreground executors (measured via microbenchmarks)
- **NFR-005**: Clipboard roundtrip (write→read) MUST complete in <1ms for 1KB UTF-8 text (measured via benchmarks)

**Reliability:**
- **NFR-006**: Window lifecycle MUST not leak memory during create/destroy cycles (verified via heap profiler: valgrind, heaptrack)
- **NFR-007**: All platform API tests MUST pass in headless mode without GPU or display server (enables CI/CD automation)
- **NFR-008**: Platform implementations MUST be thread-safe with no data races (verified via ThreadSanitizer, loom)
- **NFR-009**: Event handling MUST not drop events under high load (<1000 events/second typical UI interaction)

**Quality:**
- **NFR-010**: Code coverage MUST be ≥70% for platform implementation crates (per constitution Principle VI)
- **NFR-011**: All public APIs MUST have rustdoc with examples and panic conditions documented (per constitution Documentation Gate)
- **NFR-012**: Unsafe code MUST have explicit safety justification in comments and PR description (per constitution Principle V)
- **NFR-013**: All tracing must use structured logging via `tracing` crate, NEVER `println!` or `eprintln!` (per constitution Principle VI)

**Compatibility:**
- **NFR-014**: Windows platform MUST support Windows 10 (1809+) and Windows 11 with native Win32 API
- **NFR-015**: macOS platform MUST support macOS 11 (Big Sur) and later with native AppKit/Cocoa
- **NFR-016**: Event types MUST conform to W3C UI Events specification for cross-platform consistency
- **NFR-017**: Headless platform MUST provide identical API surface to native platforms (full contract test coverage)

**Scalability:**
- **NFR-018**: System MUST support creating and managing 100+ concurrent windows without performance degradation
- **NFR-019**: System MUST handle multi-monitor setups with up to 8 displays (tested with virtual displays)
- **NFR-020**: Background executor MUST scale to available CPU cores (tokio multi-threaded runtime)

### Key Entities

- **Platform**: Central abstraction providing lifecycle, window management, executors, text system, clipboard
- **PlatformWindow**: Native window handle with size, position, mode, focus, visibility operations
- **PlatformDisplay**: Monitor/screen with bounds, scale factor, refresh rate, primary flag
- **PlatformTextSystem**: Platform-native text measurement and glyph shaping (DirectWrite/Core Text) - provides font loading, text bounds calculation, and positioned glyphs (Vec<GlyphPosition>) for rendering
- **FontHandle**: Opaque handle to loaded font (platform-specific: IDWriteTextFormat on Windows, CTFont on macOS)
- **GlyphPosition**: Positioned glyph for rendering with glyph_id (u32), x_offset/y_offset/x_advance (Pixels) relative to baseline origin
- **PlatformExecutor**: Task scheduling (Background for worker threads, Foreground for UI thread)
- **Clipboard**: Text read/write interface with platform-native format handling
- **WindowEvent**: Lifecycle events (Created, Resized, Moved, FocusChanged, CloseRequested, etc.)
- **PlatformHandlers**: Callback registry for decoupling framework from platform (quit, window_event, open_urls)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Windows can be created and destroyed without leaks on Windows and macOS (verified via heap profiler)
- **SC-002**: NFR-001 verified - Text measurement latency <1ms for strings <100 ASCII characters
- **SC-003**: NFR-002 verified - Event dispatch latency <5ms from OS event to callback invocation
- **SC-004**: NFR-007 verified - Full test suite passes in headless mode without GPU (CI-friendly)
- **SC-005**: NFR-003 verified - Display enumeration completes in <10ms even with 4+ monitors
- **SC-006**: NFR-004 verified - Executor spawns tasks with <100µs overhead (microbenchmark verified)
- **SC-007**: NFR-005 verified - Clipboard roundtrip (write→read) completes in <1ms for 1KB UTF-8 text
- **SC-008**: NFR-010 verified - Code coverage ≥70% for platform implementations (per constitution requirement)
- **SC-009**: NFR-011 verified - Documentation complete for all public APIs with examples (per constitution requirement)
- **SC-010**: NFR-012 verified - No unsafe code violations without explicit justification in PR
