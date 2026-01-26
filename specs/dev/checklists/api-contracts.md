# API Contract Completeness Checklist: flui-platform MVP

**Purpose**: Validate that all Platform trait methods, integration points, and test requirements are complete, unambiguous, and implementation-ready. Each "No" answer requires spec updates before implementation begins.

**Created**: 2026-01-26  
**Feature**: [flui-platform MVP](../spec.md)  
**Audience**: Author (pre-implementation validation), QA/Test Engineer (test strategy design)

---

## 🔴 CRITICAL: Text System Integration Requirements [CRITICAL - MVP BLOCKER]

### API Contract Completeness

- [ ] CHK001 - Are the exact method signatures for `PlatformTextSystem` trait fully specified (method names, parameters, return types, lifetimes)? [Completeness, Spec §FR-006 to FR-010]
- [ ] CHK002 - Is the contract for `text_system.default_font_family()` explicit about what happens on systems with no default font? [Edge Case, Gap]
- [ ] CHK003 - Are text measurement return values explicitly defined (logical pixels vs device pixels, coordinate space, baseline position)? [Clarity, Spec §FR-008]
- [ ] CHK004 - Is the glyph positioning data format specified (x/y offsets, advance widths, glyph IDs vs Unicode)? [Clarity, Spec §FR-010]
- [ ] CHK005 - Are font loading error conditions documented (missing font file, invalid format, permission denied)? [Completeness, Gap]
- [ ] CHK006 - Is the font fallback chain explicitly defined (primary font → fallback → system default → error)? [Clarity, Gap]

### Unicode & Complex Script Requirements [CRITICAL]

- [ ] CHK007 - Are requirements specified for emoji rendering (color vs monochrome, emoji sequences like 👨‍👩‍👧‍👦)? [Coverage, Spec §FR-009]
- [ ] CHK008 - Are requirements specified for right-to-left languages (Arabic, Hebrew) including bidirectional text? [Coverage, Gap]
- [ ] CHK009 - Are requirements specified for complex script shaping (Devanagari, Thai, Khmer with ligatures and diacritics)? [Coverage, Gap]
- [ ] CHK010 - Are zero-width joiner (ZWJ) and variation selector handling requirements defined? [Edge Case, Gap]
- [ ] CHK011 - Is the behavior for unsupported Unicode characters specified (replacement glyph, skip, error)? [Edge Case, Gap]
- [ ] CHK012 - Can "full Unicode 15.0 support" be objectively tested (which specific features required: normalization, grapheme clusters, script detection)? [Measurability, Spec §FR-009]

### DirectWrite Integration (Windows) [CRITICAL]

- [ ] CHK013 - Are DirectWrite initialization requirements specified (IDWriteFactory version, COM threading model)? [Completeness, Gap]
- [ ] CHK014 - Is the DirectWrite error handling strategy documented (HRESULT → Rust Result conversion)? [Completeness, Gap]
- [ ] CHK015 - Are DirectWrite font collection requirements defined (system fonts only vs custom fonts)? [Clarity, Gap]
- [ ] CHK016 - Is the text layout object lifecycle specified (when to create, cache, destroy IDWriteTextLayout)? [Completeness, Gap]
- [ ] CHK017 - Are DirectWrite rendering parameters documented (antialiasing mode, ClearType settings)? [Gap]

### Core Text Integration (macOS) [CRITICAL]

- [ ] CHK018 - Are Core Text initialization requirements specified (CTFramesetter vs CTLine usage)? [Completeness, Gap]
- [ ] CHK019 - Is the Core Text error handling strategy documented (NULL returns, CFError → Rust Result)? [Completeness, Gap]
- [ ] CHK020 - Are Core Text font descriptor requirements defined (CTFontDescriptor attributes)? [Clarity, Gap]
- [ ] CHK021 - Is the text layout object lifecycle specified (CTLine, CTRun memory management)? [Completeness, Gap]
- [ ] CHK022 - Are Core Text glyph attribute requirements documented (which CTRun attributes to extract)? [Gap]

### Performance Requirements [CRITICAL]

- [ ] CHK023 - Is the "<1ms for <100 char strings" requirement defined with exact measurement methodology (median, p95, p99)? [Measurability, Spec §NFR-001]
- [ ] CHK024 - Are performance requirements defined for worst-case scenarios (1000+ chars, mixed scripts, emoji-heavy text)? [Coverage, Gap]
- [ ] CHK025 - Is text measurement caching strategy specified (cache keys, invalidation, memory limits)? [Gap]
- [ ] CHK026 - Are performance requirements defined for font loading (first use vs cached)? [Gap]

### Failure Modes & Fallbacks [CRITICAL]

- [ ] CHK027 - Are all text system initialization failure modes documented (DirectWrite/CoreText unavailable)? [Completeness, Gap]
- [ ] CHK028 - Is the fallback behavior specified when requested font family doesn't exist? [Clarity, Spec §FR-008]
- [ ] CHK029 - Is the behavior specified when text measurement fails (invalid input, OOM, API error)? [Edge Case, Gap]
- [ ] CHK030 - Is the behavior specified when glyph shaping fails (unsupported script, missing glyphs)? [Edge Case, Gap]
- [ ] CHK031 - Are recovery requirements defined when text system becomes unavailable mid-session? [Exception Flow, Gap]

---

## 🔴 CRITICAL: Memory Safety Requirements [CRITICAL - CONSTITUTION REQUIREMENT]

### Window Lifecycle Safety

- [ ] CHK032 - Are window resource cleanup requirements explicit (HWND/NSWindow destruction, event handler deregistration)? [Completeness, Spec §FR-003]
- [ ] CHK033 - Is the behavior specified when events arrive for destroyed windows? [Edge Case, Spec §Requirements "How to handle window events during window destruction?"]
- [ ] CHK034 - Are requirements defined for detecting memory leaks in create/destroy cycles? [Measurability, Spec §NFR-006]
- [ ] CHK035 - Is the window handle lifetime relationship with Platform object documented (ownership, borrowing, Drop impl)? [Clarity, Gap]
- [ ] CHK036 - Are requirements specified for preventing use-after-free with window callbacks? [Completeness, Gap]

### Clipboard Handle Safety

- [ ] CHK037 - Is clipboard handle ownership documented (Windows HGLOBAL lifetime, NSPasteboard retain/release)? [Clarity, Spec §FR-033]
- [ ] CHK038 - Are requirements specified for clipboard data validity after read (copy vs reference semantics)? [Clarity, Gap]
- [ ] CHK039 - Is the behavior specified when clipboard access fails mid-operation (lock timeout, ownership conflict)? [Edge Case, Gap]
- [ ] CHK040 - Are thread safety requirements explicit for clipboard access (Mutex usage, deadlock prevention)? [Completeness, Spec §FR-034]

### Executor Thread Safety

- [ ] CHK041 - Is the Send+Sync contract for BackgroundExecutor explicitly documented with safety justification? [Clarity, Spec §FR-029]
- [ ] CHK042 - Is the !Send contract for ForegroundExecutor explicitly documented with reasoning? [Clarity, Spec §FR-029]
- [ ] CHK043 - Are requirements specified for preventing task queue races between threads? [Completeness, Gap]
- [ ] CHK044 - Is the behavior specified when foreground executor accessed from background thread (panic, error, deadlock prevention)? [Edge Case, Gap]

### FFI Safety Requirements

- [ ] CHK045 - Are all unsafe FFI calls to Win32/AppKit documented with safety invariants? [Completeness, Gap]
- [ ] CHK046 - Are requirements specified for raw pointer validity (lifetime, alignment, null checks)? [Completeness, Gap]
- [ ] CHK047 - Is the strategy documented for converting OS errors (HRESULT, NSError) to Rust Result without panic? [Clarity, Gap]
- [ ] CHK048 - Are requirements specified for preventing undefined behavior in C interop (repr(C), ABI compatibility)? [Completeness, Gap]

---

## 🟠 Platform Trait API Contracts [IMPORTANT - PRIMARY FOCUS]

### Core Platform Trait Methods

- [ ] CHK049 - Is the contract for `Platform::run()` explicit about when the closure is called (before/after event loop start)? [Clarity, Spec §FR-016]
- [ ] CHK050 - Is the behavior specified when `Platform::run()` is called twice on same instance? [Edge Case, Gap]
- [ ] CHK051 - Is the contract for `Platform::quit()` explicit about cleanup order (windows closed first, executors stopped, then exit)? [Clarity, Gap]
- [ ] CHK052 - Are requirements specified for graceful shutdown vs forced exit in `quit()`? [Completeness, Gap]
- [ ] CHK053 - Is the return type for `Platform::open_window()` explicitly defined (Result vs panic on error)? [Clarity, Spec §FR-001]
- [ ] CHK054 - Are all possible error conditions for `open_window()` documented (invalid size, OS limit reached, permission denied)? [Completeness, Gap]

### PlatformWindow Trait Methods

- [ ] CHK055 - Is the contract for `window.set_size()` explicit about coordinate space (logical pixels vs device pixels)? [Clarity, Gap]
- [ ] CHK056 - Is the behavior specified when `set_size()` called with invalid dimensions (0x0, negative, exceeds screen)? [Edge Case, Spec §Requirements "Edge Cases"]
- [ ] CHK057 - Is the contract for `window.set_mode()` explicit about animation behavior (instant vs animated transition)? [Clarity, Gap]
- [ ] CHK058 - Are mode transition constraints documented (can't fullscreen minimized window, must restore first)? [Completeness, Gap]
- [ ] CHK059 - Is the contract for `window.request_redraw()` explicit about timing (immediate, next frame, debounced)? [Clarity, Spec §FR-005]
- [ ] CHK060 - Is the behavior specified when `request_redraw()` called on invisible/minimized window? [Edge Case, Gap]
- [ ] CHK061 - Are `window.close()` requirements explicit about cleanup order (callbacks, resources, OS handle)? [Completeness, Gap]

### PlatformDisplay Trait Methods

- [ ] CHK062 - Is the contract for `display.scale_factor()` explicit about precision (f32 vs f64, rounding rules)? [Clarity, Spec §FR-022]
- [ ] CHK063 - Are fractional scale factors (1.25, 1.5) handling requirements specified? [Clarity, Spec §Requirements "Edge Cases"]
- [ ] CHK064 - Is the contract for `display.usable_bounds()` explicit about what's excluded (taskbar, menu bar, notch, dock)? [Clarity, Spec §FR-024]
- [ ] CHK065 - Is the behavior specified when display disconnected mid-session (bounds become invalid)? [Edge Case, Gap]
- [ ] CHK066 - Are refresh rate fallback requirements documented (default 60Hz when query fails)? [Completeness, Spec §FR-025]

### Callback Registry Pattern (PlatformHandlers)

- [ ] CHK067 - Is the callback registration ownership model explicit (Fn vs FnMut vs FnOnce, 'static lifetime)? [Clarity, Gap]
- [ ] CHK068 - Is the behavior specified when callback panics (catch_unwind, abort, propagate)? [Edge Case, Gap]
- [ ] CHK069 - Are callback deregistration requirements documented (manual unregister vs automatic on drop)? [Completeness, Gap]
- [ ] CHK070 - Is the callback invocation order specified when multiple handlers registered (FIFO, LIFO, unspecified)? [Clarity, Gap]

---

## 🟠 Integration Requirements [IMPORTANT - PRIMARY FOCUS]

### wgpu Surface Integration

- [ ] CHK071 - Is the handoff contract between PlatformWindow and wgpu surface creation explicit (raw-window-handle usage)? [Clarity, Spec §T006]
- [ ] CHK072 - Are requirements specified for surface configuration (format, present mode, alpha mode)? [Completeness, Gap]
- [ ] CHK073 - Is the behavior specified when surface creation fails (invalid handle, driver error)? [Edge Case, Gap]
- [ ] CHK074 - Are requirements defined for surface recreation on DPI change or window resize? [Completeness, Gap]
- [ ] CHK075 - Is the integration with wgpu::Instance initialization documented (which backends to enable)? [Gap]

### flui_painting Canvas Integration

- [ ] CHK076 - Is the handoff contract between PlatformTextSystem and Canvas::draw_text() explicit (glyph format, coordinate space)? [Clarity, Spec §FR-010]
- [ ] CHK077 - Are requirements specified for text rendering target (wgpu texture, CPU buffer, both)? [Completeness, Gap]
- [ ] CHK078 - Is the behavior specified when glyph rendering fails (missing glyph, texture atlas full)? [Edge Case, Gap]
- [ ] CHK079 - Are performance requirements defined for text-to-texture pipeline latency? [Gap]
- [ ] CHK080 - Is the integration test for text system + Canvas roundtrip specified with acceptance criteria? [Completeness, Spec §T035, T045]

### flui_interaction Event Propagation

- [ ] CHK081 - Is the handoff contract between PlatformWindow events and flui_interaction hit testing explicit? [Clarity, Gap]
- [ ] CHK082 - Are requirements specified for event coordinate transformation (device pixels → logical pixels → local coordinates)? [Completeness, Gap]
- [ ] CHK083 - Is the event propagation order documented (capture phase, target phase, bubble phase)? [Clarity, Gap]
- [ ] CHK084 - Is the behavior specified when event handler modifies event (preventDefault, stopPropagation)? [Completeness, Gap]
- [ ] CHK085 - Are requirements defined for event batching/coalescing (multiple mousemove events)? [Gap]

### Event Type Consistency (W3C Compliance)

- [ ] CHK086 - Is the exact W3C UI Events specification version referenced (Level 1, Level 2, draft)? [Traceability, Spec §FR-011]
- [ ] CHK087 - Are all PointerEvent fields documented (pointerId, pointerType, pressure, tiltX/Y, twist)? [Completeness, Spec §FR-012]
- [ ] CHK088 - Are all KeyboardEvent fields documented (code, key, location, repeat, isComposing)? [Completeness, Spec §FR-013]
- [ ] CHK089 - Is the mapping from OS-native events to W3C events exhaustively documented (WM_* → PointerEvent, NSEvent → KeyboardEvent)? [Completeness, Spec §FR-014]
- [ ] CHK090 - Are edge cases for W3C event conversion specified (unmappable keys, synthetic events, accessibility tools)? [Coverage, Gap]

---

## 🟠 Test Coverage Strategy [IMPORTANT - PRIMARY FOCUS]

### Contract Test Requirements

- [ ] CHK091 - Are contract tests specified for every Platform trait method with identical behavior across Windows/macOS/Headless? [Completeness, Spec §T009, T023]
- [ ] CHK092 - Is the contract test methodology explicit (identical inputs → identical outputs, error cases match)? [Clarity, Spec §T009]
- [ ] CHK093 - Are requirements specified for contract test coverage of edge cases (invalid inputs, boundary conditions)? [Coverage, Spec §T043]
- [ ] CHK094 - Is the contract test failure reporting clear (which platform diverged, expected vs actual)? [Clarity, Gap]
- [ ] CHK095 - Are requirements specified for contract tests to catch platform-specific bugs (Windows-only crash)? [Coverage, Gap]

### Integration Test Requirements

- [ ] CHK096 - Are integration tests specified for all cross-crate handoffs (platform → wgpu, platform → flui_painting, platform → flui_interaction)? [Completeness, Spec §T010, T035, T080]
- [ ] CHK097 - Is the integration test environment specified (headless mode, mock GPU, real hardware)? [Clarity, Gap]
- [ ] CHK098 - Are requirements specified for integration test isolation (no shared state between tests)? [Completeness, Spec §T068]
- [ ] CHK099 - Is the integration test performance target specified (full suite <30s in headless mode)? [Measurability, Spec §T072]
- [ ] CHK100 - Are requirements specified for testing multi-platform scenarios (window move between displays, DPI change)? [Coverage, Spec §T079, T080]

### Edge Case Test Coverage

- [ ] CHK101 - Are test requirements specified for all documented edge cases in spec (invalid window size, missing fonts, etc.)? [Completeness, Spec §Requirements "Edge Cases"]
- [ ] CHK102 - Are test requirements specified for Unicode edge cases (emoji sequences, RTL, complex scripts)? [Coverage, Spec §CHK007-CHK011]
- [ ] CHK103 - Are test requirements specified for lifecycle edge cases (events during destruction, rapid create/destroy)? [Coverage, Spec §CHK033]
- [ ] CHK104 - Are test requirements specified for resource exhaustion (100+ windows, OOM, OS limits)? [Coverage, Spec §T108]
- [ ] CHK105 - Are test requirements specified for concurrent access scenarios (multiple threads, race conditions)? [Coverage, Spec §T068]

### 70% Coverage Achievability

- [ ] CHK106 - Is the 70% coverage target broken down by module (window.rs: 80%, text_system.rs: 75%, etc.)? [Clarity, Spec §NFR-010]
- [ ] CHK107 - Are requirements specified for measuring coverage (cargo-tarpaulin, llvm-cov, exclude unsafe blocks)? [Clarity, Spec §T103]
- [ ] CHK108 - Are test gaps identified that prevent reaching 70% coverage (untested error paths, platform-specific code)? [Gap, Spec §T104]
- [ ] CHK109 - Is the strategy documented for testing platform-specific code (conditional compilation, mock traits)? [Clarity, Gap]
- [ ] CHK110 - Are requirements specified for maintaining coverage over time (CI enforcement, coverage ratcheting)? [Completeness, Gap]

### Test Execution Requirements

- [ ] CHK111 - Are headless mode test requirements explicit (FLUI_HEADLESS=1 env var, no GPU, mock all OS calls)? [Completeness, Spec §FR-017, T069]
- [ ] CHK112 - Are CI test execution requirements specified (GitHub Actions, timeout limits, artifact collection)? [Completeness, Spec §T071]
- [ ] CHK113 - Are requirements specified for test flakiness prevention (deterministic timing, no sleep(), retries)? [Completeness, Gap]
- [ ] CHK114 - Are requirements specified for test parallelization (isolated state, thread-safe fixtures)? [Completeness, Spec §T068]
- [ ] CHK115 - Is the test failure debugging strategy documented (tracing output, heap dumps, core dumps)? [Gap]

---

## 🟡 Performance & Non-Functional Requirements [MEDIUM PRIORITY]

### Performance Requirement Measurability

- [ ] CHK116 - Can the "<1ms text measurement" requirement be objectively measured with specific benchmark implementation? [Measurability, Spec §NFR-001]
- [ ] CHK117 - Can the "<5ms event dispatch" requirement be objectively measured with tracing timestamps? [Measurability, Spec §NFR-002]
- [ ] CHK118 - Can the "<10ms display enumeration" requirement be objectively measured with criterion benchmark? [Measurability, Spec §NFR-003]
- [ ] CHK119 - Can the "<100µs executor spawn" requirement be objectively measured with microbenchmark? [Measurability, Spec §NFR-004]
- [ ] CHK120 - Can the "<1ms clipboard roundtrip" requirement be objectively measured with benchmark? [Measurability, Spec §NFR-005]

### Performance Acceptance Criteria

- [ ] CHK121 - Are benchmark statistical requirements specified (sample size, confidence intervals, outlier handling)? [Clarity, Gap]
- [ ] CHK122 - Are performance degradation thresholds specified (10% regression fails CI, 5% warning)? [Completeness, Gap]
- [ ] CHK123 - Are performance requirements defined for worst-case scenarios (slow hardware, high load, resource contention)? [Coverage, Gap]
- [ ] CHK124 - Is the performance baseline documented for comparison (hardware specs, OS version, measurement methodology)? [Gap]

### Reliability & Quality Requirements

- [ ] CHK125 - Can memory leak prevention be objectively tested with heap profiler (valgrind, heaptrack methodology)? [Measurability, Spec §NFR-006]
- [ ] CHK126 - Can thread safety be objectively verified with ThreadSanitizer or loom testing strategy? [Measurability, Spec §NFR-008]
- [ ] CHK127 - Can event handling reliability (<1000 events/s) be objectively tested with stress test? [Measurability, Spec §NFR-009]
- [ ] CHK128 - Are requirements specified for graceful degradation when performance targets missed? [Completeness, Gap]

---

## 🟡 Cross-Platform Consistency [MEDIUM PRIORITY]

### Platform API Equivalence

- [ ] CHK129 - Are requirements specified for API surface equivalence (all platforms implement identical trait methods)? [Completeness, Spec §FR-017, NFR-017]
- [ ] CHK130 - Are requirements specified for behavioral equivalence (same inputs → same outputs across platforms)? [Consistency, Spec §T061]
- [ ] CHK131 - Are requirements specified for error condition equivalence (same errors returned across platforms)? [Consistency, Gap]
- [ ] CHK132 - Are requirements specified for event data equivalence (W3C compliance ensures consistency)? [Consistency, Spec §FR-011]

### Platform-Specific Divergence Handling

- [ ] CHK133 - Are acceptable platform divergences explicitly documented (macOS has Cmd key, Windows has Win key)? [Clarity, Gap]
- [ ] CHK134 - Is the strategy documented for handling platform-specific features (Windows transparency vs macOS vibrancy)? [Clarity, Gap]
- [ ] CHK135 - Are requirements specified for detecting unintentional platform divergence in CI? [Completeness, Gap]

---

## 🟢 Documentation & Examples [SUPPORTING]

### API Documentation Completeness

- [ ] CHK136 - Are rustdoc requirements specified for all public traits (Platform, PlatformWindow, PlatformDisplay, etc.)? [Completeness, Spec §NFR-011, T111]
- [ ] CHK137 - Are rustdoc requirements specified for all public types (WindowId, WindowMode, WindowEvent, etc.)? [Completeness, Spec §NFR-011]
- [ ] CHK138 - Are requirements specified for documenting panic conditions (when methods panic vs return error)? [Completeness, Spec §NFR-011]
- [ ] CHK139 - Are requirements specified for documenting safety invariants (unsafe code justification)? [Completeness, Spec §NFR-012]
- [ ] CHK140 - Are requirements specified for usage examples in rustdoc (simple example per public API)? [Completeness, Spec §NFR-011]

### Usage Examples & Quickstart

- [ ] CHK141 - Are requirements specified for minimal example (T003: examples/minimal_window.rs with tracing)? [Completeness, Spec §T003]
- [ ] CHK142 - Are requirements specified for event handling example (T062: examples/event_handling.rs)? [Completeness, Spec §T062]
- [ ] CHK143 - Are requirements specified for text measurement example (T045: examples/text_measurement.rs)? [Completeness, Spec §T045]
- [ ] CHK144 - Are requirements specified for display enumeration example (T082: examples/displays.rs)? [Completeness, Spec §T082]
- [ ] CHK145 - Are requirements specified for executor example (T094: examples/executor.rs)? [Completeness, Spec §T094]

---

## Summary Statistics

**Total Items**: 145  
**Critical Items**: 48 (CHK001-CHK048)  
**Important Items**: 67 (CHK049-CHK115)  
**Medium Priority**: 20 (CHK116-CHK135)  
**Supporting Items**: 10 (CHK136-CHK145)

**Pass Criteria**: All CRITICAL items must be "Yes" before implementation begins. IMPORTANT items should be addressed during implementation planning. MEDIUM items tracked but not gating.

**Next Steps**:
1. Review each "No" answer and identify spec gaps
2. Update spec.md with missing requirements
3. Update plan.md with architectural decisions
4. Update tasks.md with additional test tasks
5. Re-run this checklist until all CRITICAL items pass

---

## Notes

- Mark items complete with `[x]` as spec updates made
- Add comments inline for findings or decisions
- Link to specific spec sections when updating requirements
- Each "No" answer should have corresponding GitHub issue or spec TODO
- Use this checklist during architecture review before Phase 3 implementation begins
