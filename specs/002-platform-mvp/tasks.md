# Tasks: flui-platform MVP Completion

**Input**: Design documents from `/specs/002-platform-mvp/`
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅
**Branch**: `002-platform-mvp`

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story (US1–US6)
- Exact file paths from plan.md project structure

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependencies, module scaffolding, re-exports

- [x] T001 Add new dependencies to `crates/flui-platform/Cargo.toml`: `cursor-icon` (if needed), ensure `windows` features include `Win32_Graphics_DirectWrite`, `Win32_Globalization`; add `oneshot` for Task channel
- [x] T002 [P] Create module file `crates/flui-platform/src/task.rs` with `Task<T>` struct, `Priority` enum, `TaskLabel` newtype
- [x] T003 [P] Create module file `crates/flui-platform/src/cursor.rs` with `CursorStyle` enum (21 variants)
- [x] T004 [P] Create types section in `crates/flui-platform/src/traits/input.rs` for `DispatchEventResult` struct
- [x] T005 Update `crates/flui-platform/src/lib.rs` to declare and re-export new modules (`task`, `cursor`)
- [x] T006 Update `crates/flui-platform/src/traits/mod.rs` to re-export new types (`DispatchEventResult`, expanded traits)

**Checkpoint**: `cargo check -p flui-platform` compiles with new stubs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Value types, callback struct, and core abstractions that ALL user stories depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T007 [P] Implement `CursorStyle` enum (~21 variants) with `Default` in `crates/flui-platform/src/cursor.rs` per `contracts/value-types.rs`
- [x] T008 [P] Implement `WindowAppearance` enum (Light, Dark, VibrantLight, VibrantDark) with `Default` in `crates/flui-platform/src/traits/window.rs`
- [x] T009 [P] Implement `WindowBackgroundAppearance` enum (Opaque, Transparent, Blurred, MicaBackdrop, MicaAltBackdrop) with `Default` in `crates/flui-platform/src/traits/window.rs`
- [x] T010 [P] Implement `WindowBounds` enum (Windowed, Maximized, Fullscreen) in `crates/flui-platform/src/traits/window.rs`
- [x] T011 [P] Implement `DispatchEventResult` struct (propagate, default_prevented) with `Default` in `crates/flui-platform/src/traits/input.rs`
- [x] T012 [P] Implement `ClipboardItem` struct in `crates/flui-platform/src/traits/platform.rs` (text + metadata, alongside existing Clipboard trait)
- [x] T013 [P] Implement `PathPromptOptions` struct (files, directories, multiple) in `crates/flui-platform/src/traits/platform.rs`
- [x] T014 [P] Implement `FontMetrics`, `LineLayout`, `ShapedRun`, `ShapedGlyph`, `FontId`, `GlyphId`, `Font`, `FontWeight`, `FontStyle`, `FontRun` types in `crates/flui-platform/src/traits/platform.rs` per `contracts/text-system-trait.rs`
- [x] T015 Implement `WindowCallbacks` struct in `crates/flui-platform/src/shared/handlers.rs` with all 9 callback fields using `parking_lot::Mutex<Option<Box<dyn FnMut/FnOnce + Send>>>` per research.md R1 pattern
- [x] T016 Add `dispatch_*` helper methods to `WindowCallbacks` (dispatch_input, dispatch_resize, dispatch_moved, dispatch_close, dispatch_should_close, dispatch_active_status, dispatch_hover_status, dispatch_appearance_changed, dispatch_request_frame) using take/restore pattern

**Checkpoint**: `cargo check -p flui-platform` compiles. All value types have `Debug`, `Clone` (where appropriate), `Default`. WindowCallbacks compiles with all 9 fields.

---

## Phase 3: US1 — Per-Window Callback Event Delivery (Priority: P1) 🎯 MVP

**Goal**: PlatformWindow delivers input, resize, focus, and lifecycle events via per-window callbacks so upper layers can dispatch events to the correct window.

**Independent Test**: Create window, register callbacks, simulate messages, verify callbacks fire with correct data.

### Implementation for US1

- [x] T017 Expand `PlatformWindow` trait in `crates/flui-platform/src/traits/window.rs` with 9 callback registration methods: `on_input`, `on_request_frame`, `on_resize`, `on_moved`, `on_close`, `on_should_close`, `on_active_status_change`, `on_hover_status_change`, `on_appearance_changed` per `contracts/platform-window-trait.rs`
- [x] T018 Add `WindowCallbacks` field to `WindowsWindow` in `crates/flui-platform/src/platforms/windows/window.rs` and implement all 9 callback registration methods (delegate to `WindowCallbacks` setters)
- [x] T019 Wire `WM_MOUSEMOVE`, `WM_LBUTTONDOWN/UP`, `WM_RBUTTONDOWN/UP`, `WM_MOUSEWHEEL` in `crates/flui-platform/src/platforms/windows/platform.rs` to call `WindowCallbacks::dispatch_input()` with `PlatformInput::Pointer(PointerEvent)`
- [x] T020 Wire `WM_KEYDOWN`, `WM_KEYUP`, `WM_CHAR`, `WM_SYSKEYDOWN/UP` in `crates/flui-platform/src/platforms/windows/platform.rs` to call `WindowCallbacks::dispatch_input()` with `PlatformInput::Keyboard(KeyboardEvent)`
- [x] T021 Wire `WM_SIZE` in `crates/flui-platform/src/platforms/windows/platform.rs` to call `WindowCallbacks::dispatch_resize()` with new `Size<Pixels>` and scale factor
- [x] T022 Wire `WM_MOVE` to call `WindowCallbacks::dispatch_moved()`
- [x] T023 Wire `WM_CLOSE` to call `dispatch_should_close()` first — if true, call `dispatch_close()` and `DestroyWindow`; if false, suppress close
- [x] T024 Wire `WM_SETFOCUS`/`WM_KILLFOCUS` to call `WindowCallbacks::dispatch_active_status(true/false)`
- [x] T025 Wire `WM_MOUSELEAVE` (TrackMouseEvent) to call `WindowCallbacks::dispatch_hover_status(true/false)`
- [x] T026 Wire `WM_SETTINGCHANGE` to call `WindowCallbacks::dispatch_appearance_changed()`
- [x] T027 Wire `WM_PAINT` to call `WindowCallbacks::dispatch_request_frame()`

**Checkpoint**: WindowsPlatform window delivers all 9 callback types. Manual test: create window, register `on_input`, move mouse → callback fires.

---

## Phase 4: US2 — Window Control Methods (Priority: P1) 🎯 MVP

**Goal**: PlatformWindow provides methods for controlling window state (title, bounds, minimize, maximize, fullscreen, appearance) so flui-app can manage windows without platform-specific code.

**Independent Test**: Create window, call set_title/minimize/maximize/toggle_fullscreen, verify state via query methods.

### Implementation for US2

- [x] T028 Expand `PlatformWindow` trait in `crates/flui-platform/src/traits/window.rs` with 12 query methods: `bounds`, `content_size`, `window_bounds`, `is_maximized`, `is_fullscreen`, `is_active`, `is_hovered`, `mouse_position`, `modifiers`, `appearance`, `display`, `get_title` per `contracts/platform-window-trait.rs`
- [x] T029 Expand `PlatformWindow` trait with 10 control methods: `set_title`, `activate`, `minimize`, `maximize`, `restore`, `toggle_fullscreen`, `resize`, `close`, `request_redraw` (already exists), `set_background_appearance` per `contracts/platform-window-trait.rs`
- [x] T030 Implement 12 query methods on `WindowsWindow` in `crates/flui-platform/src/platforms/windows/window.rs` using Win32 APIs: `GetWindowRect`, `GetClientRect`, `IsZoomed`, `IsIconic`, `GetForegroundWindow`, `GetCursorPos`, `ScreenToClient`, `GetKeyboardState`, `DwmGetWindowAttribute`
- [x] T031 Implement 10 control methods on `WindowsWindow` in `crates/flui-platform/src/platforms/windows/window.rs` using Win32 APIs: `SetWindowTextW`, `SetForegroundWindow`, `ShowWindow(SW_MINIMIZE/SW_MAXIMIZE/SW_RESTORE)`, `SetWindowPos`, `DestroyWindow`, `DwmSetWindowAttribute`
- [x] T032 Implement `toggle_fullscreen()` on `WindowsWindow` — save/restore window placement, set `WS_POPUP` + monitor bounds for fullscreen, restore `WS_OVERLAPPEDWINDOW` for windowed (use existing fullscreen logic if present)
- [x] T033 Implement `set_background_appearance()` on `WindowsWindow` — wire Mica/MicaAlt/Transparent backdrop via `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` (existing Mica code may be reusable from `window_ext.rs`)
- [x] T034 Track `is_hovered` state in `WindowsWindow` using `TrackMouseEvent(TME_LEAVE)` — set flag on mouse enter, clear on `WM_MOUSELEAVE`
- [x] T035 Track `modifiers` state in `WindowsWindow` — update on `WM_KEYDOWN/UP` for Shift/Ctrl/Alt/Meta, expose via `modifiers()` method

**Checkpoint**: All PlatformWindow query/control methods work on Windows. `cargo test -p flui-platform` passes. set_title → get_title roundtrip works.

---

## Phase 5: US3 — Platform Trait Expansion (Priority: P2)

**Goal**: Platform trait provides app-level services (activation, cursor, file dialogs, keyboard layout, URL opening) so flui-app can offer these to widget developers.

**Independent Test**: Call each Platform method, verify platform-specific behavior.

### Implementation for US3

- [x] T036 Expand `Platform` trait in `crates/flui-platform/src/traits/platform.rs` with ~16 new methods per `contracts/platform-trait.rs`: `activate`, `hide`, `hide_other_apps`, `unhide_other_apps`, `window_appearance`, `should_auto_hide_scrollbars`, `set_cursor_style`, `write_to_clipboard(ClipboardItem)`, `read_from_clipboard() -> Option<ClipboardItem>`, `open_url`, `on_open_urls`, `keyboard_layout`, `on_keyboard_layout_change`, `compositor_name`
- [x] T037 Provide default implementations for methods that are no-ops on most platforms: `hide`, `hide_other_apps`, `unhide_other_apps`, `should_auto_hide_scrollbars` (false), `compositor_name` ("")
- [x] T038 Implement `activate()` on `WindowsPlatform` in `crates/flui-platform/src/platforms/windows/platform.rs` using `SetForegroundWindow` for the active window
- [x] T039 Implement `set_cursor_style()` on `WindowsPlatform` using `SetCursor` with `LoadCursorW` mapped from `CursorStyle` enum (Arrow→IDC_ARROW, IBeam→IDC_IBEAM, etc.)
- [x] T040 Implement `window_appearance()` on `WindowsPlatform` by reading `AppsUseLightTheme` from Windows registry (`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize`)
- [x] T041 Implement `open_url()` on `WindowsPlatform` using `ShellExecuteW` with `"open"` verb
- [x] T042 Implement `prompt_for_paths()` on `WindowsPlatform` using `IFileOpenDialog` COM API — returns `Task<Result<Option<Vec<PathBuf>>>>` (spawn on STA thread)
- [x] T043 Implement `prompt_for_new_path()` on `WindowsPlatform` using `IFileSaveDialog` COM API — returns `Task<Result<Option<PathBuf>>>` (spawn on STA thread)
- [x] T044 Implement `write_to_clipboard(ClipboardItem)` / `read_from_clipboard()` on `WindowsPlatform` via default trait methods delegating to Clipboard trait
- [x] T045 Implement `keyboard_layout()` on `WindowsPlatform` using `GetKeyboardLayoutNameW`
- [x] T046 Implement `on_keyboard_layout_change()` on `WindowsPlatform` — listen for `WM_INPUTLANGCHANGE` in window proc, dispatch to registered callback

**Checkpoint**: All new Platform methods implemented for Windows. `open_url`, `set_cursor_style`, `window_appearance` verified manually.

---

## Phase 6: US4 — Task\<T\> Async Abstraction (Priority: P2)

**Goal**: Executors return `Task<T>` implementing `Future` with priority-based scheduling so flui-scheduler and flui-app can spawn async work and await results.

**Independent Test**: Spawn Task<i32>, await it, verify result. Test detach(). Test priority ordering.

### Implementation for US4

- [x] T047 Implement `Task<T>` in `crates/flui-platform/src/task.rs`: `TaskState<T>` enum (Ready/Spawned), `Task::ready(val)` constructor, `Task::detach(self)`, `impl Future for Task<T>` per `contracts/task-types.rs` and research.md R2
- [x] T048 Implement `Priority` enum (High, Medium, Low) with `Default = Medium` in `crates/flui-platform/src/task.rs`
- [x] T049 [P] Implement `TaskLabel` newtype (`&'static str`) in `crates/flui-platform/src/task.rs` for debug/tracing identification
- [x] T050 Refactor `BackgroundExecutor` in `crates/flui-platform/src/executor.rs`: change `spawn()` to return `Task<T>`, add `spawn_with_priority(Priority, Future) -> Task<T>`, add `timer(Duration) -> Task<()>`, add `block()`
- [x] T051 Refactor `ForegroundExecutor` in `crates/flui-platform/src/executor.rs`: change `spawn()` to return `Task<T>`, `drain_tasks()` and `pending_count()` retained
- [x] T052 `block()` utility on BackgroundExecutor for synchronous test usage (blocks current thread on runtime)
- [x] T053 Update all existing callers — examples updated to use async spawn pattern, PlatformExecutor trait kept for Box<dyn FnOnce> fire-and-forget

**Checkpoint**: `Task<42>.await == 42`. `spawn(async { 1 + 1 }).await == 2`. `detach()` doesn't block. `cargo test -p flui-platform` passes.

---

## Phase 7: US6 — Headless Platform Testing Support (Priority: P2)

**Goal**: HeadlessPlatform supports all PlatformWindow callbacks with programmatic event injection and all Platform services with mocks so tests run without a display server.

**Independent Test**: Headless mode: create window, inject event, verify callback fires.

### Implementation for US6

- [x] T054 Add `WindowCallbacks` field to headless window struct in `crates/flui-platform/src/platforms/headless/platform.rs` and implement all 9 callback registration methods (same as T018 but for headless)
- [x] T055 Implement all 12 PlatformWindow query methods on headless window with configurable mock state (bounds, title, focused, visible, maximized, fullscreen, active, hovered, mouse_position, modifiers, appearance, display)
- [x] T056 Implement all 10 PlatformWindow control methods on headless window — update internal mock state (e.g., `set_title` updates stored title, `minimize` sets `is_minimized` flag, `maximize` sets `is_maximized` flag) and fire appropriate callbacks
- [x] T057 Add event injection methods to headless window: `inject_event(PlatformInput)` → fires `on_input`, `simulate_resize(width, height)` → fires `on_resize`, `simulate_focus(bool)` → fires `on_active_status_change`, `simulate_close()` → fires `on_should_close` then `on_close`
- [x] T058 Implement all new Platform trait methods on `HeadlessPlatform`: `activate` (no-op), `set_cursor_style` (store last), `window_appearance` (return configurable), `open_url` (store URLs), `keyboard_layout` (return "en-US"), `write_to_clipboard`/`read_from_clipboard` (in-memory store)
- [x] T059 Headless uses `TestExecutor` implementing `PlatformExecutor` trait — `Task<T>` returned by `BackgroundExecutor`/`ForegroundExecutor` concrete types, not trait object
- [x] T060 Verified `HeadlessPlatform` has ZERO `unimplemented!()` or `todo!()` calls — all trait methods fully implemented

**Checkpoint**: `cargo test -p flui-platform` all headless tests pass. No `unimplemented!()` in headless code. Event injection → callback roundtrip works.

---

## Phase 8: US5 — Text System with DirectWrite Backend (Priority: P3)

**Goal**: PlatformTextSystem provides real font enumeration, text measurement, and line layout via DirectWrite so flui-rendering can compute accurate text layout.

**Independent Test**: Load system fonts, measure "Hello World", verify measurement accuracy. Layout line with mixed runs.

### Implementation for US5

- [x] T061 Create `crates/flui-platform/src/platforms/windows/text_system.rs` with `DirectWriteTextSystem` struct owning `IDWriteFactory5` and `IDWriteFontCollection1`
- [x] T062 Implement `DirectWriteTextSystem::new()` — create `IDWriteFactory5` via `DWriteCreateFactory`, get system `IDWriteFontCollection1`
- [x] T063 Implement `PlatformTextSystem::all_font_names()` — iterate `IDWriteFontCollection1::GetFontFamilyCount()` + `GetFontFamily()` + `GetFamilyNames()`, return `Vec<String>`
- [x] T064 Implement `PlatformTextSystem::font_id()` — resolve `Font` descriptor (family + weight + style) to `FontId` via `IDWriteFontCollection1::FindFamilyName()` + `GetFontFamily()` + `GetFirstMatchingFont()`
- [x] T065 Implement `PlatformTextSystem::font_metrics()` — get `IDWriteFontFace3` from font, call `GetMetrics()`, convert to `FontMetrics` struct (ascent, descent, line_gap, etc.)
- [x] T066 Implement `PlatformTextSystem::glyph_for_char()` — call `IDWriteFontFace3::GetGlyphIndices()` for single character, return `Option<GlyphId>`
- [x] T067 Implement `PlatformTextSystem::layout_line()` — create `IDWriteTextFormat` with font/size, create `IDWriteTextLayout`, call `GetMetrics()` for width/height, `GetLineMetrics()` for ascent/descent, return `LineLayout`
- [x] T068 Implement `PlatformTextSystem::add_fonts()` — stub for post-MVP (custom font loading via IDWriteInMemoryFontFileLoader deferred)
- [x] T069 Wire `DirectWriteTextSystem` into `WindowsPlatform::text_system()` — replace existing stub `PlatformTextSystem` with `Arc<DirectWriteTextSystem>`
- [x] T070 Update headless `PlatformTextSystem` to return reasonable mock data: `all_font_names() → vec!["Mock Sans"]`, `font_metrics() → hardcoded metrics`, `layout_line() → width = chars * font_size * 0.6`

**Checkpoint**: `all_font_names()` returns installed fonts including "Segoe UI". `layout_line("Hello", 16.0, runs)` returns non-zero width. `font_metrics()` returns valid ascent/descent.

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Integration verification, cleanup, and quality assurance across all stories

- [x] T071 `WindowsWindow` already implements `HasWindowHandle + HasDisplayHandle` from `raw-window-handle 0.6` — verified existing impl
- [x] T072 [P] `window.rs` Window trait exists with full API — PlatformWindow is the low-level trait, Window is the high-level abstraction
- [x] T073 [P] `PlatformDisplay` has id, name, bounds, usable_bounds, scale_factor, refresh_rate, is_primary, logical_size — uuid/default_bounds deferred (non-essential)
- [x] T074 `cargo clippy -p flui-platform -- -D warnings` passes clean
- [x] T075 `cargo test -p flui-platform --lib` — 75 passed, 0 failed, 2 ignored
- [x] T076 `cargo build --workspace` passes clean
- [x] T077 `cargo build --examples -p flui-platform` — all 6 examples build (updated for new APIs)
- [x] T078 [P] All new public types have `///` doc comments (Task, Priority, TaskLabel, PathPromptOptions, font types)
- [x] T079 Build, test, example commands all pass
- [x] T080 API surface audit: PlatformWindow=38 methods (≥30), Platform=36 methods (≥35), HeadlessPlatform=0 unimplemented!()

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (Setup) ─────────────────────────────► Phase 2 (Foundational)
                                                        │
                                                        ▼
                                    ┌───────────────────┼───────────────────┐
                                    │                   │                   │
                                    ▼                   ▼                   ▼
                             Phase 3 (US1)       Phase 4 (US2)       Phase 6 (US4)
                             Callbacks P1        Controls P1         Task<T> P2
                                    │                   │                   │
                                    └───────┬───────────┘                   │
                                            ▼                               │
                                     Phase 5 (US3)                          │
                                     Platform P2 ◄──────────────────────────┘
                                            │              (prompt_for_paths needs Task<T>)
                                            ▼
                                     Phase 7 (US6)
                                     Headless P2
                                     (must impl ALL new methods)
                                            │
                                            ▼
                                     Phase 8 (US5)
                                     DirectWrite P3
                                            │
                                            ▼
                                     Phase 9 (Polish)
```

### Critical Path

1. **Phase 1** → **Phase 2** → **Phase 3 (US1)** → **Phase 4 (US2)** — this is the MVP path (P1 stories)
2. **Phase 6 (US4)** can start in parallel with Phase 3/4 (independent: task.rs + executor.rs)
3. **Phase 5 (US3)** depends on US1 (callbacks for `on_keyboard_layout_change`), US2 (window control for `activate`), and US4 (`Task<T>` for `prompt_for_paths`)
4. **Phase 7 (US6)** must come after ALL trait expansions are finalized (US1–US4)
5. **Phase 8 (US5)** can start after Phase 2 but benefits from US6 headless being ready for testing

### Parallel Opportunities

- **Phase 2**: All T007–T014 value types can be implemented in parallel (different types, different locations)
- **Phase 3 + Phase 6**: US1 (callbacks) and US4 (Task<T>) touch different files — can run in parallel
- **Phase 4**: T030 (query methods) and T031 (control methods) can be partially parallelized
- **Phase 9**: T072, T073, T078 are independent of each other

### Within Each Phase

- Trait expansion BEFORE implementation (define interface, then implement)
- Windows implementation BEFORE headless (headless mirrors Windows API surface)
- Core implementation before edge cases
- Phase complete → checkpoint verification before moving on

---

## Implementation Strategy

### MVP First (US1 + US2 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational value types + WindowCallbacks
3. Complete Phase 3: US1 — Per-window callbacks wired in WndProc
4. Complete Phase 4: US2 — Window control + query methods
5. **STOP and VALIDATE**: `cargo test -p flui-platform`, manual window test
6. This delivers a complete PlatformWindow with events + control — enough for flui-app integration

### Full MVP (All Stories)

7. Complete Phase 6: US4 — Task<T> (parallel-safe with Phase 3/4)
8. Complete Phase 5: US3 — Platform trait expansion
9. Complete Phase 7: US6 — Headless updates
10. Complete Phase 8: US5 — DirectWrite text system
11. Complete Phase 9: Polish and verify all success criteria

---

## Task Summary

| Phase | Story | Priority | Tasks | Key Files |
|-------|-------|----------|-------|-----------|
| 1 | — | — | T001–T006 (6) | Cargo.toml, lib.rs, mod.rs |
| 2 | — | — | T007–T016 (10) | cursor.rs, traits/window.rs, traits/input.rs, traits/platform.rs, shared/handlers.rs |
| 3 | US1 | P1 | T017–T027 (11) | traits/window.rs, windows/window.rs, windows/events.rs |
| 4 | US2 | P1 | T028–T035 (8) | traits/window.rs, windows/window.rs |
| 5 | US3 | P2 | T036–T046 (11) | traits/platform.rs, windows/platform.rs, windows/clipboard.rs |
| 6 | US4 | P2 | T047–T053 (7) | task.rs, executor.rs |
| 7 | US6 | P2 | T054–T060 (7) | headless/platform.rs |
| 8 | US5 | P3 | T061–T070 (10) | windows/text_system.rs, headless/platform.rs |
| 9 | — | — | T071–T080 (10) | cross-cutting |
| **Total** | | | **80 tasks** | |

---

## Notes

- [P] tasks = different files, no dependencies — safe for parallel agent execution
- Each user story independently testable after its phase completes
- Commit after each task or logical group
- Stop at any checkpoint to validate
- Constitution compliance: no `unwrap()`, use `tracing`, `unsafe` only in Win32 FFI with `// SAFETY:` comments
- All callbacks use `&self` (not `&mut self`) with interior mutability per FR-010
