# Research: flui-platform Cross-Platform Abstraction

**Date**: 2026-01-26
**Feature**: flui-platform MVP with cross-platform support

## Research Questions and Findings

### Q1: What is the current state of flui-platform implementation?

**Decision**: Windows and macOS implementations are production-ready (10/10 and 9/10 respectively). Focus MVP on completing text system integration and macOS hardware testing.

**Rationale**:
- **Windows Platform**: 100% complete with Win32 API integration, Mica backdrop support, full window management, display enumeration, clipboard, and executors
- **macOS Platform**: 90% complete with AppKit/Cocoa integration, all features implemented but untested on hardware
- **Headless Platform**: 100% complete for CI/testing with 12 passing tests
- **Winit Platform**: Architecture complete but window creation needs channel-based approach (optional, not MVP blocker)
- **Other Platforms** (Linux, Android, iOS, Web): Stubs with roadmap, post-MVP

**Alternatives Considered**:
- Starting from scratch with only Winit for all platforms → Rejected because native implementations provide better control and are already 90%+ complete
- Focusing on Linux before completing Windows/macOS → Rejected because Windows/macOS cover 95% of desktop users

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\IMPLEMENTATION_STATUS.md` shows detailed completion status
- Windows tests all passing (clipboard, window management, event handling)
- macOS code compiles without errors, needs hardware verification

---

### Q2: How does Flutter handle cross-platform abstraction?

**Decision**: Adopt Flutter's binding mixin pattern as Rust trait composition for modularity.

**Rationale**:
Flutter uses **composable binding system** with mixins:
```dart
BindingBase → SchedulerBinding → GestureBinding → ServicesBinding → WidgetsBinding
```

Each binding handles one concern (scheduling, gestures, platform services, widgets). Rust equivalent uses trait composition:

```rust
trait BindingBase { /* lifecycle */ }
trait SchedulerBinding: BindingBase { /* frame scheduling */ }
trait GestureBinding: BindingBase { /* input handling */ }
```

**Key patterns to adopt**:
1. **Platform Channels**: Async message passing between framework and platform (method calls, events, streams)
2. **System Channels**: Standard APIs for clipboard, keyboard, lifecycle
3. **Event Resampling**: Touch events resampled at ~16.667ms for smooth 60fps
4. **Multi-Window Support**: `WindowController` + `WindowDelegate` pattern for lifecycle
5. **Lifecycle Observers**: Observer pattern for app state changes (resume, pause, detach)

**Alternatives Considered**:
- Monolithic binding class → Rejected because Flutter's modular approach allows independent testing and optional features
- Direct OS event loop without abstraction → Rejected because Flutter's success validates the abstraction layer approach

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\.flutter\src\foundation\binding.dart` - Binding architecture
- `C:\Users\vanya\RustroverProjects\flui\.flutter\src\services\platform_channel.dart` - Channel implementation
- `C:\Users\vanya\RustroverProjects\flui\.flutter\src\widgets\_window.dart` - Multi-window lifecycle

---

### Q3: What patterns from GPUI should flui-platform adopt?

**Decision**: Already adopted GPUI's core patterns (Platform trait, type erasure, callback registry, interior mutability). Missing per-window callbacks - add post-MVP.

**Rationale**:

**Already Adopted** (✅):
1. **Platform trait design** - Single trait for all platform operations, returns `Box<dyn PlatformWindow>`
2. **Type erasure** - `Arc<dyn PlatformTextSystem>`, `Box<dyn PlatformWindow>` for platform-agnostic code
3. **Callback registry** - `PlatformHandlers` with `on_quit`, `on_window_event`, `on_reopen`
4. **Interior mutability** - `Arc<Mutex<T>>` for thread-safe `&self` methods (improved from GPUI's `Rc<RefCell<T>>` with parking_lot)
5. **Executor pattern** - Background (tokio) + Foreground (flume channel) split

**Gaps** (⏳ Post-MVP):
1. **Per-window callbacks** - GPUI has `Callbacks` struct on windows for fine-grained handlers (`on_resize`, `on_input`, `on_moved`). flui-platform routes all events through platform-level `on_window_event`.
2. **Priority-based task scheduling** - GPUI executors support priority queues. flui uses simple FIFO.

**Alternatives Considered**:
- Implementing per-window callbacks now → Deferred to post-MVP because current platform-level routing works and adds complexity
- Priority queues → Deferred until performance profiling shows need

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\.gpui\src\platform.rs:166-279` - Platform trait
- `C:\Users\vanya\RustroverProjects\flui\.gpui\src\platform\linux\x11\window.rs:238-248` - Callbacks pattern
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\src\traits\platform.rs` - flui adoption

---

### Q4: What are the MVP blockers for cross-platform support?

**Decision**: Three critical tasks for MVP: (1) Text system integration, (2) Frame scheduling, (3) macOS hardware testing.

**Rationale**:

**Critical** (MVP Blockers):
1. **Text System Integration** (1-2 weeks):
   - Windows: DirectWrite for font loading, text shaping, glyph metrics
   - macOS: Core Text equivalent
   - Integration with flui_painting for rendering
   - **Blocker because**: No text = no UI labels, buttons, or any text rendering

2. **Frame Scheduling** (2-3 days):
   - Windows: SetTimer or manual frame timing
   - macOS: CVDisplayLink or NSTimer
   - 60 FPS limiting for animations
   - **Blocker because**: Continuous rendering needed for animations, games, live updates

3. **macOS Hardware Testing** (1-2 days with Mac hardware):
   - Verify clipboard on real macOS
   - Test window management, display enumeration
   - Profile performance
   - **Blocker because**: Untested code on 20%+ of desktop users is risky

**Important** (Post-MVP):
- WinitPlatform window creation (optional fallback for Linux development)
- Rich clipboard formats (images, HTML)
- Linux native implementation

**Alternatives Considered**:
- Shipping without text system → Rejected because text is fundamental to any UI
- Skipping macOS testing → Rejected because 20%+ market share requires quality assurance
- Implementing Linux before testing macOS → Rejected because macOS is closer to complete

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\src\platforms\windows\platform.rs:602` - DirectWrite TODO comment
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\src\platforms\macos\platform.rs:97` - Core Text TODO comment
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\IMPLEMENTATION_STATUS.md` - Status tracking

---

### Q5: What dependencies are needed for MVP completion?

**Decision**: Add raw-window-handle, waker-fn, and platform-specific text system crates.

**Rationale**:

**High Priority** (Add for MVP):
1. **raw-window-handle = "0.6"** - Required for wgpu/Vulkan integration, standard for window handle abstraction
2. **waker-fn = "1.2.0"** - Simplify async waker creation in executors
3. **Windows text system**:
   - Already using `windows = "0.52"` for Win32 API
   - DirectWrite interfaces included in windows crate
4. **macOS text system**:
   - Already using `cocoa = "0.26.0"` for AppKit
   - Core Text via Core Foundation FFI

**Already Added**:
- ✅ flume = "0.11" - Better MPSC for foreground executor (used by GPUI)
- ✅ tokio = "1.43" - Background executor runtime
- ✅ parking_lot = "0.12" - Fast mutexes (2-3x faster than std)

**Medium Priority** (Post-MVP):
- windows-registry = "0.5" - Read dark mode, theme, DPI settings
- open = "5.2.0" - Open URLs/files with default app

**Alternatives Considered**:
- Using custom window handle types → Rejected because raw-window-handle is industry standard
- Implementing text systems from scratch → Rejected because platform APIs are production-quality
- Using web_sys for text on all platforms → Rejected because native APIs provide better quality and performance

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\DEPENDENCY_RECOMMENDATIONS.md` - Dependency analysis
- `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\Cargo.toml` - Current dependencies

---

### Q6: How should text system integrate with flui_painting?

**Decision**: Text system provides glyph positions, flui_painting handles rendering via Canvas API.

**Rationale**:

**Architecture**:
```
Text Measurement (flui-platform) → Glyph Positions → Canvas Drawing (flui_painting) → GPU Upload (flui_engine)
```

**Integration Points**:
1. **Text System** (`flui-platform`):
   - Font loading: `load_font(family, weight, style) -> FontHandle`
   - Text measurement: `measure_text(text, font, size) -> Rect<Pixels>`
   - Glyph shaping: `shape_text(text, font, size) -> Vec<GlyphPosition>`

2. **Canvas API** (`flui_painting`):
   - Text drawing: `canvas.draw_text(text, position, paint)`
   - Glyph rendering: `canvas.draw_glyphs(glyphs, paint)`
   - Font management: `canvas.set_font(font_handle, size)`

3. **GPU Rendering** (`flui_engine`):
   - Glyph atlas: Rasterize glyphs to texture atlas
   - SDF rendering: Signed distance field for scalable text (via glyphon)
   - Tessellation: Convert glyph outlines to triangles (via lyon)

**Alternatives Considered**:
- Text system handles rendering directly → Rejected because violates separation of concerns (platform vs rendering)
- Using cosmic-text for all platforms → Deferred to V2 because native platform APIs provide better quality for MVP
- Skipping glyph shaping → Rejected because complex scripts (Arabic, Devanagari) require proper shaping

**Evidence**:
- `C:\Users\vanya\RustroverProjects\flui\.flutter\src\painting\text_painter.dart` - Flutter's text painting approach
- `C:\Users\vanya\RustroverProjects\flui\.gpui\src\text_system.rs` - GPUI text system architecture
- Current flui_painting Canvas API in `C:\Users\vanya\RustroverProjects\flui\crates\flui_painting\src\canvas.rs`

---

### Q7: What testing strategy ensures MVP quality?

**Decision**: Unit tests (70% coverage target), integration tests (cross-crate), contract tests (Platform trait), headless CI.

**Rationale**:

**Test Pyramid**:
1. **Unit Tests** (70% coverage for platform crates per constitution):
   - Window creation/destruction
   - Event parsing (OS events → W3C events)
   - Clipboard roundtrip
   - Executor task spawning
   - Display enumeration

2. **Integration Tests** (tests/ directory):
   - Text system integration with flui_painting
   - Window lifecycle with event callbacks
   - Multi-monitor window movement
   - Async executor integration

3. **Contract Tests** (Platform trait compliance):
   - Each platform implementation must pass identical test suite
   - Ensures consistent behavior across Windows/macOS/Headless

4. **Headless CI**:
   - `FLUI_HEADLESS=1 cargo test --workspace`
   - No GPU or display server required
   - Enables GitHub Actions CI

**Test Infrastructure**:
- GestureRecorder/Player for deterministic interaction tests
- Mock windows in headless mode
- Tracing instrumentation for timing verification

**Alternatives Considered**:
- 100% test coverage → Rejected because platform-specific code is harder to test (per constitution allows 70%)
- Testing only on one platform → Rejected because cross-platform consistency requires testing all implementations
- Manual testing only → Rejected because CI is critical for preventing regressions

**Evidence**:
- Constitution requirement: ≥70% coverage for platform crates
- Existing headless tests in `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\src\platforms\headless\platform.rs`
- Test structure in `C:\Users\vanya\RustroverProjects\flui\crates\flui-platform\tests\`

---

## Summary

**MVP Path Forward**:
1. ✅ Windows Platform: Production-ready
2. ✅ macOS Platform: Code complete, needs hardware testing
3. ✅ Headless Platform: Complete for CI
4. ⏳ Text System: Critical MVP blocker (1-2 weeks)
5. ⏳ Frame Scheduling: Important for animations (2-3 days)
6. ⏳ macOS Testing: Quality assurance (1-2 days with hardware)

**Estimated MVP Completion**: 2-3 weeks

**Post-MVP Roadmap**:
- Rich clipboard (images, HTML): 1 week
- Per-window callbacks: 2-3 days
- Linux native: 3-4 weeks
- Android/iOS/Web: 4-8 weeks each

**Key Architectural Decisions**:
- Use native APIs (Win32, AppKit) instead of Winit-only for better control
- Adopt GPUI's proven patterns (Platform trait, callback registry, type erasure)
- Follow Flutter's modular binding approach via Rust trait composition
- W3C-standard events for cross-platform consistency
- Text measurement in platform layer, rendering in flui_painting
