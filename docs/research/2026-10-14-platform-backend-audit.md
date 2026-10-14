# FLUI platform layer: what is implemented today, per backend

Snapshot: worktree `HEAD` 0656b4f (`origin/main` 399f661 differs only in animation and docs, so the
platform crates are identical). Read-only audit, nothing was built or run. The last section covers
the unmerged `origin/runtime/host-input` branch.

## Legend and conventions

| Code | Meaning |
|---|---|
| **R** | Real native implementation |
| **P** | Partial: real code, but something is missing or wrong (the note says what) |
| **S** | Stub, mock, fake data, inline/no-op stand-in, or a default body that pretends to succeed. In the Headless column, **S** means a test double you can drive (`simulate_*`, `inject_event`, recording) |
| **S0** | Headless or other stand-in with no driver: a fixed canned value |
| **A** | Absent: no code path. The trait default body runs (`None`, `Light`, no-op, zero) |
| **N** | Not meaningful in that environment |
| *(contract)* | The API has no trait method for it |

Columns: **Win** = `WindowsPlatform` (Win32), **mac** = `MacOSPlatform` (AppKit), **Linux** =
`WinitPlatform`. `current_platform()` picks winit on Linux (`crates/flui-platform/src/lib.rs:390-409`).
`LinuxPlatform` is a dead stub where every method is `unimplemented!()`
(`pf/linux/mod.rs:110-261`). Only its `accessibility.rs` (AT-SPI) is live, and only the winit window
uses it. `LinuxWindowExt` has no implementor. **iOS** = `IOSPlatform` (UIKit scenes), **Android** =
`AndroidPlatform` (android-activity NativeActivity), **Web** = `WebPlatform` (wasm32 canvas),
**Headless** = `HeadlessPlatform` + `MockWindow` (test double; `FLUI_HEADLESS` selects it).

Path abbreviations: `pf/` = `crates/flui-platform/src/platforms/`, `tr/` =
`crates/flui-platform/src/traits/`, `api/` = `crates/flui-platform-api/src/`, `app/` =
`crates/flui-app/src/app/`, `rt/` = `crates/flui-runtime/src/`.

"Consumed above" names the production consumer above the backend crate. "none" means no production
caller; tests and examples don't count.

---

## 1. Contract inventory (every method, with its default)

### `Platform` (`tr/platform.rs:124`, de facto sealed to in-crate backends)

| Method | Line | Default body |
|---|---|---|
| `background_executor()` | 130 | required |
| `run(self: Box<Self>, on_ready)` | 163 | required |
| `quit()` | 169 | required |
| `set_exit_policy_hook(hook)` | 191 | no-op (drops hook) |
| `request_exit_policy_reevaluation()` | 224 | no-op |
| `set_wake_deadline_hook(hook)` | 252 | no-op |
| `open_window(WindowOptions)` | 276 | required |
| `active_window()` | 279 | required |
| `preferences()` | 292 | `Ok(SystemPreferences::default())` (all unknown) |
| `displays()` / `primary_display()` | 299 / 302 | required |
| `clipboard()` | 307 | required |
| `data_transfer()` | 319 | required (backends without a transport return `NullDataTransferSource`) |
| `activate(ignoring_other_apps)` | 327 | no-op |
| `window_appearance()` | 334 | `WindowAppearance::default()` = Light |
| `write_to_clipboard(ClipboardItem)` / `read_from_clipboard()` | 341 / 349 | routed through `clipboard()` text only |
| `open_url(&str)` | 357 | no-op |
| `reveal_path(&Path)` / `open_path(&Path)` | 362 / 367 | no-op |
| `prompt_for_paths(PathPromptOptions)` | 377 | `Task::ready(Ok(None))`, which reads as "user cancelled" |
| `prompt_for_new_path(dir, name)` | 390 | `Task::ready(Ok(None))` |
| `keyboard_layout()` | 402 | `String::new()` |
| `on_keyboard_layout_change(cb)` | 407 | no-op |
| `on_quit(cb)` | 414 | required |
| `on_session_end(cb)` | 423 | no-op. **Not exposed on `OwnerPlatform` or `SharedPlatform`** (`tr/owner.rs`), so flui-app cannot reach it |
| `on_reopen(cb)` | 437 | no-op |
| `on_window_event(cb)` | 442 | required |
| `on_open_urls(cb)` | 445 | no-op |
| `capabilities()` | 452 | required (static tables, `tr/capabilities.rs`) |
| `name()` / `compositor_name()` | 455 / 458 | required / `""` |
| `app_path()` | 466 | required |

`PlatformExecutor` (`tr/platform.rs:473`): `spawn` (required), `is_on_executor` (default `false`).
`PathPromptOptions { files, directories, multiple }` has no filters, title, default directory or
parent window.

### `OwnerPlatform` / `SharedPlatform` / `PlatformProxy` (`tr/owner.rs`)

- **`OwnerPlatform`** (owner thread only, `!Send`): `on_wake` 100, `open_window` 110 (returns
  `WindowOpen::{Ready, Pending}`), `active_window` 116, `preferences` 124, `displays` 130,
  `primary_display` 136, `activate` 141, `window_appearance` 147, `keyboard_layout` 153, `quit` 158,
  `shared` 170, `proxy` 177, `text_store_host(window)` 193, `capture_text_sizing(window)` 201.
- **`SharedPlatform`** (`Send + Sync`): `background_executor` 254, `capabilities` 260, `name` 266,
  `compositor_name` 272, `app_path` 281, `clipboard` 287, `data_transfer` 293, `on_quit` 298,
  `on_reopen` 303, `on_window_event` 308, `set_exit_policy_hook` 315,
  `request_exit_policy_reevaluation` 326, `set_wake_deadline_hook` 334, `on_open_urls` 342,
  `on_keyboard_layout_change` 347, `open_url` 352, `reveal_path` 357, `open_path` 362,
  `prompt_for_paths` 370, `prompt_for_new_path` 381, `write_to_clipboard` 390,
  `read_from_clipboard` 396. (No `on_session_end`.)
- **`PlatformProxy`** (any thread): `wake` 534, `open_window` 554 (returns `PendingWindow`),
  `request_quit` 569, `is_owner_thread` 577. There is no redraw verb
  (`api/platform_window.rs:46-52`).

### `HostWindow` (`tr/host_window.rs:62`)

`capture_text_sizing(token)` 69 → default `Unsupported`. `accessibility()` 82 → default `None`.
`text_store_host(token)` 112 → default `None`.

### `PlatformWindow` (`api/platform_window.rs:156`; the line numbers below are absolute)

| Method | Default | | Method | Default |
|---|---|---|---|---|
| `id` | required | | `set_title` 357 | no-op |
| `physical_size` / `logical_size` / `scale_factor` | required | | `show` 368 | `Err(Unsupported)` |
| `gesture_geometry` 186 | `Ok(None)` | | `reveal_after_first_frame` 387 | no-op |
| `request_redraw` | required | | `activate` / `minimize` / `maximize` / `restore` / `toggle_fullscreen` 390-402 | no-op |
| `pre_present_notify` 256 | no-op | | `resize` 405 | no-op |
| `is_focused` / `is_visible` | required | | `close` 443 | no-op |
| `bounds` 267 | `Bounds::default()` (zero) | | `set_background_appearance` 446 | no-op |
| `content_size` 272 | `logical_size()` | | `set_cursor` 460 | required |
| `window_bounds` 277 | `Windowed(bounds())` | | `on_input` / `on_request_frame` / `on_resize` / `on_moved` / `on_close` / `on_should_close` / `on_active_status_change` 474-518 | drop callback |
| `is_maximized` / `is_fullscreen` 282/287 | `false` | | `safe_area_insets` 525 | `ZERO` |
| `is_active` 292 | `is_focused()` | | `on_safe_area_change` 531 | drop |
| `is_hovered` 297 | `false` | | `execution_state` 539 | `Running` |
| `mouse_position` 302 | origin | | `on_execution_state_change` 546 | drop |
| `modifiers` 307 | `NONE` | | `on_visibility_status_change` / `on_hover_status_change` / `on_appearance_changed` / `on_surface_status_change` 579-650 | drop |
| `appearance` 312 | Light | | `window_handle` / `display_handle` 725/750 | `Err(Unavailable)` |
| `display` 317 | `None` | | `as_any` 765 | required |
| `refresh_period` 327 | `None` | | | |
| `text_input` 336 | `None` | | | |
| `haptics` 345 | `None` | | | |
| `get_title` 350 | `""` | | | |

In-crate backends implement every callback setter through `shared::impl_window_callback_setters!`.
The callback is stored, but it only fires if the backend dispatches to it, and the matrix below
records whether it does. `WindowOptions` (`api/window.rs:43`) has `title, size, resizable, visible,
reveal, decorated, min_size, max_size`. It has **no position, initial state (maximized/fullscreen),
parent/owner, transparency, level or icon**.

### Smaller contracts

- `PlatformTextInput` (`api/text_input.rs:31`): `set_ime_allowed` 39, `set_ime_cursor_area` 44. This
  is the push model: the backend sends `PlatformInput::Ime(ImeEvent::{Enabled, Preedit, Commit,
  Disabled})`.
- `TextStoreHost` (`api/text_store/host.rs:27`): `focus_store` 30, `complete_composition` 49. This is
  the pull model (ADR-0135).
- `Clipboard` (`api/clipboard.rs:7`): `read_text` 9, `write_text` 12 (synchronous), `has_text` 15
  (default `read_text().is_some()`). `ClipboardItem` (50) holds text + metadata only.
- `DataTransferSource` (`api/data_transfer.rs:658`): `clipboard_offer` 663, `request` 669,
  `update_drop_feedback` 679, `conclude_drop` 684. `NullDataTransferSource` 693 is inert.
- `Storage` (`api/storage.rs:23`): `read(name, limit)` 29, `publish(name, bytes, mode)` 39 (async
  futures).
- `PlatformDisplay` (`api/display.rs:27`): `id`, `name`, `bounds`, `usable_bounds` (default =
  `bounds`), `scale_factor`, `refresh_rate` (default **60.0**), `is_primary`, `logical_size`.
- `PlatformHaptics` (`api/haptics.rs:71`): `perform(HapticFeedback)`, `as_any`.
- `PlatformAccessibility` (`crates/flui-semantics/src/platform.rs:64`): `publish(TreeUpdate)` 79,
  `is_active` 86, `set_activation_listener` 89, `set_action_listener` 92.
- `PlatformCapabilities` (`tr/capabilities.rs:13`): `platform_name`, `has_lifecycle_management`,
  `supports_multiple_windows`, `should_coalesce_pointer_moves`, `default_target_fps`,
  `suspend_rendering_in_background`, `supports_touch/mouse/keyboard`, `supports_stylus` (default
  false). Static per-family tables, consumed nowhere.
- `SystemPreferences` (`api/preferences.rs:264`): `text_scale`, `motion`, `high_contrast`,
  `text_weight`, `locales`, `gestures{double_click_interval, double_tap_interval,
  native_mouse_geometry, native_touch_geometry, long_press_timeout}`, `wheel{vertical,
  horizontal_characters}`. No appearance, accent colour or haptics field on main. Change delivery is
  not a callback: the backend wakes the owner (`OwnerSignal`), and flui-app re-reads `preferences()`
  (`app/runner/host.rs:165`).
- `PlatformInput` (`api/input.rs:88`): `Pointer(PointerEvent)`, `Keyboard(KeyEvent)`,
  `Ime(ImeEvent)`, `DragDrop(DragDropEvent)`. `PointerEvent`: `Down`, `ButtonChange`, `Up`, `Move`,
  `Cancel`, `Enter`, `Leave`, `Scroll`, `ScrollInertiaCancel`, `PanZoom`, `DeviceAdded`,
  `DeviceRemoved`. `PointerKind`: `Mouse`, `Touch`, `Pen{Tip, Eraser}`, `Trackpad`, `Unknown`.
- Legacy crate-local traits: `crate::window::Window` / `WindowManager` (`crates/flui-platform/src/window.rs:50,436`),
  `WindowsWindowExt`, `MacOSWindowExt` and `LinuxWindowExt`. All take `&mut self`, so they can't be
  called through `Arc<dyn HostWindow>`. The only route is `as_any` downcast + `Clone`. No consumer.

---

## 2. Feature matrix

### 2.1 App lifecycle and event loop

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Event loop run + bootstrap | R | R | R | R | R | R | S | flui-app runners | Win `pf/windows/platform.rs:2229`. mac `pf/macos/platform.rs:202`. Linux `pf/winit/platform.rs:2192`. iOS `pf/ios/platform.rs:388`. Android `pf/android/mod.rs:354`. Web calls `on_ready` synchronously, then starts RAF and never returns control (`pf/web/platform.rs:238-273`). Headless has no loop and calls `on_ready` inline (`pf/headless/platform.rs:253`) |
| `quit` + `on_quit` | R | R | R | R | R | R | S | flui-app (`direct.rs`, `host.rs`, `application_control.rs` via proxy) | Win `PostQuitMessage` (2272) and on_quit taken at 2246. mac `loop_control.rs:346`. Android/iOS use `OwnerSignal::request_quit` (`android/mod.rs:722`, `ios/platform.rs:424`) |
| Exit policy hook + re-evaluation | R | R | R | N | N | N | S | flui-app `runtime.rs`, `lifecycle.rs` | Win 2407/2413, mac 303/308, Linux 2229/2235. Headless parks the request (`HeadlessExitReevaluation::drive`) |
| Wake-deadline hook (gesture timers) | R | R | R | R | R | A | N | flui-app `runner/host.rs`, `android.rs`, `ios.rs` | Win 2288 bounds the message wait. mac actuates through `wake_pump` (336). Linux uses `ControlFlow::WaitUntil` (2245). Android 803, iOS 483. Web keeps the default no-op (RAF loop) |
| Owner wake + `PlatformProxy::wake/request_quit` | R | R | R | R | R | R | S | flui-app `runtime.rs:947`, `application_control.rs:152,237` | `OwnerSignal` / owner lanes |
| Cross-thread `PlatformProxy::open_window` | A | A | R | A | A | A | S | none (tests only) | Only winit has a lane (`pf/winit/platform.rs:2612`). Every other backend uses `SignalTransport`, which answers `Unsupported` (`crates/flui-platform/src/shared/owner_signal.rs:300-305`) |
| Reopen (dock click) | N | R | A | N | N | N | A | flui-app `runner/main_window.rs` | mac `applicationShouldHandleReopen` (`loop_control.rs:104-203`). winit stores the callback but never invokes it (`pf/winit/platform.rs:2420`) |
| Session end (log off / shutdown) | A | A | A | A | A | N | S0 | none | Contract only. No `WM_QUERYENDSESSION`/`WM_ENDSESSION` and no `applicationShouldTerminate` mapping. Not reachable from `OwnerPlatform`/`SharedPlatform`. Headless `simulate_session_end` never calls the hook (`pf/headless/platform.rs:222-225`). Its test is `#[ignore]` (`crates/flui-platform/tests/headless.rs:114`) |
| Deep links / URLs opened into the app (`on_open_urls`) | A | A | A | A | A | N | A | none | `handlers.open_urls` is never invoked by any backend. No `application:openURLs:`, `scene:openURLContexts:`, Android intent or Windows protocol activation |
| App activation (`Platform::activate`) | P | A | A | N | N | N | S0 | flui-app `runner/desktop.rs:483` | Win re-foregrounds the *current* foreground window, so it does nothing for a background app (`pf/windows/platform.rs:2431-2446`). mac only has per-window `activate` |
| Focus (`on_active_status_change`, `is_focused`) | R | R | R | R | P | R | S | flui-app → flui-runtime | Win `WM_SETFOCUS`/`KILLFOCUS` (`platform.rs:1886-1930`). mac `windowDidBecomeKey` (`window.rs:2322`). iOS scene active (`pf/ios/window.rs:773-808`). Android dispatches Gained/LostFocus (`android/mod.rs:569-580`) but `is_focused()` is always `true` (`android/window.rs:275`). Web focus/blur (`web/events.rs:308`) |
| Visibility / occlusion | P | R | P | R | P | R | S | flui-app (`window_install`, `desktop`, `ios`) → flui-runtime | Win only sees minimize/restore and `WM_SHOWWINDOW` (`platform.rs:1237,1418`); Windows has no occlusion events. mac `windowDidChangeOcclusionState`. Linux winit `Occluded` fires on X11 only when the window is fully obscured, never on Wayland (`winit/platform.rs:1379`). Android `is_visible` = native window present, and it never dispatches visibility. Web `visibilitychange` (`web/events.rs:51`) |
| Execution state (Running/Suspended/Detached) | N | N | N | R | P | A | S | flui-app (`desktop`, `ios`, `secondary_window`), `rt/presentation.rs` | iOS `pf/ios/window.rs:784,949`. Android answers the query from Pause/Resume (`window.rs:259`) but never dispatches `on_execution_state_change`. Web stores the callback (`web/window.rs:525`) and maps nothing (page freeze/hidden) |
| GPU surface lost/available | N | N | N | R | R | A | S | flui-app `surface_lifecycle.rs`, `raster_lane.rs` | iOS `ios/window.rs:796-799`. Android Init/TerminateWindow + Pause/Resume. Web doesn't observe `webglcontextlost` |
| Low-memory / memory pressure | A | A | A | A | P | A | A | *(contract)* | Android only logs `LowMemory` (`android/mod.rs:598`) |
| Global window-event stream (`on_window_event`) | R | A | R | A | A | P | S | none | Win dispatches through `ctx.dispatch_event` (`platform.rs:1069-1913`). mac stores the callback and never invokes it (`macos/platform.rs:321`). Web sends only `Created` + `RedrawRequested` (`web/platform.rs:166,330`). No production registration |

### 2.2 Windows

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Open / close / multiple windows | R | R | R | P | P | P | S | flui-app (`main_window`, `secondary_window`, `direct`, `android`) | iOS `open_window` → `Unavailable`; the scene supplies the one window (`ios/platform.rs:428-433`). Android ignores `WindowOptions` and keeps a single ANativeWindow (`android/mod.rs:735`). Web is a single canvas with `WindowId(0)` (`web/platform.rs:296`) |
| Close veto (`on_should_close`, `CloseRequested`) | R | R | R | N | N | A | S | flui-app `close_request.rs` | Win `WM_CLOSE` (`platform.rs:1060`). mac `windowShouldClose:`. Web has no `beforeunload` (the callback is only stored, `web/window.rs:509`) |
| Title get/set | R | R | P | N | N | R | S | none (`flui-widgets/src/app/widgets_app.rs:30` says FLUI has no title capability yet) | Win `SetWindowTextW` (`window.rs:1118`). Linux sets only; `get_title` returns `""`. iOS and Android return fixed strings (`ios/window.rs:970`, `android/window.rs:370`). Web uses `document.title` |
| Show / activate (focus request) | R | R | P | N | N | N | S | flui-app `main_window.rs` (reopen → show) | Win `window.rs:1131,1197`. mac 993, 1043. Linux `show` refuses on Wayland (`winit/window.rs:232`) and has no `activate` |
| Deferred reveal after first frame | A | R | A | N | N | N | A | flui-app `first_reveal.rs` | mac `window.rs:1024`. Win and Linux show the window at creation (blank-frame flash) |
| Minimize / maximize / restore | R | R | R | N | N | N | S | none | Win `window.rs:1207-1227`. mac 1059-1086. Linux `winit/window.rs:240-248` |
| State queries (`is_maximized`, `is_fullscreen`, `window_bounds`) | R | A | A | N | N | P | S | none | Win `window.rs:1001-1024`. Web's fullscreen flag is set optimistically |
| Fullscreen | R | R | R | N | N | P | S | none | Linux is borderless only (`winit/window.rs:253`). Web flips its state without a `fullscreenchange` listener (`web/window.rs:461-472`). Android immersive mode is absent |
| Position / bounds / moved | R | R | P | N | N | P | S | none | **No `set_position` in the contract and no position in `WindowOptions`.** Win `bounds` 978 and `WM_MOVE` → `on_moved` (`platform.rs:1460-1490`). Linux has `on_moved` but `bounds()` stays at the zero default. Web's origin is always 0,0 |
| Programmatic resize | R | R | A | N | N | A | S | none | Win `SetWindowPos` (`window.rs:1239`). mac `window.rs:1117` |
| Min/max size constraints | A | R | R | N | N | N | S | `WindowOptions` only | Win ignores `min_size`/`max_size`. There's no `WM_GETMINMAXINFO`, and `set_min_size` just logs "not yet implemented" (`window.rs:1861-1873`). mac `window.rs:383-390`, Linux `winit/platform.rs:548-555`. **No runtime setter in the contract** |
| Resizable flag | A | R | R | N | N | N | S | `WindowOptions` only | Win always creates `WS_OVERLAPPEDWINDOW` (`window.rs:219-225`) |
| Decorations on/off | P | R | R | N | N | N | S | `WindowOptions` only | Win undecorated windows are `WS_POPUP` with no hit-testing for drag or resize (`window.rs:219-225`) |
| Custom title bar (client-drawn caption, drag regions, native caption buttons) | A | A | A | N | N | N | A | *(contract)* | No `WM_NCCALCSIZE`/`WM_NCHITTEST`. mac's transparent title bar exists only inside the `LiquidGlassConfig` ext |
| Backdrop material (`set_background_appearance`) | R | A | A | N | N | S0 | S0 | none | Win `DWMWA_SYSTEMBACKDROP_TYPE` (`window.rs:1311`); creation forces Mica (`window.rs:505-511`); `Transparent` maps to "none". mac Liquid Glass is only reachable through `&mut` `MacOSWindowExt` (`macos/window_ext.rs:58`). Web is a no-op (`web/window.rs:478`) |
| Per-OS extras (taskbar progress, snap layouts, corners, caption colour, window level, tabbing, tiling) | P | P | A | N | N | N | N | none | Ext traits only (`pf/windows/window_ext.rs:53-176`, `pf/macos/window_ext.rs:58-181`, `window_tiling.rs`, `window_manager.rs`). They take `&mut self` and are unreachable through `Arc<dyn HostWindow>` |
| **Appearance (light/dark) + change** | **P** | **A** | **P** | **A** | **A** | P | S0 | flui-app `runner/desktop.rs:382-399` (desktop runner only; mobile and web runners never seed brightness) | **Win:** `appearance()` reads back `DWMWA_USE_IMMERSIVE_DARK_MODE`, which FLUI itself forces to 1 at creation (`window.rs:513-520`, called at 291), so it reports **Dark regardless of system theme**. `Platform::window_appearance` reads `AppsUseLightTheme` correctly (`platform.rs:2451`), but nothing calls it. The change event fires on every `WM_SETTINGCHANGE` (1934). **mac:** no override → always Light, and `effectiveAppearance` isn't observed. **Linux:** winit `Window::theme()` is unsupported on X11 and override-only on Wayland → Light, and `ThemeChanged` never fires on Linux (`winit/window.rs:167`, `platform.rs:1374`). **iOS:** `traitCollectionDidChange` only invalidates text sizing (`ios/window.rs:93`). **Android:** `uiMode` isn't read. **Web:** reads `prefers-color-scheme` (`web/window.rs:436`) without a change listener |
| Hover (pointer entered/left window) | R | R | R | N | N | R | A | flui-app `runner/desktop.rs:371` | Win `window.rs:1035`, mac `mouseEntered/Exited`, Linux `CursorEntered/Left`, Web `pointerenter/leave` (`web/events.rs:313`). Headless has no driver |
| Cursor icon | R | R | R | N | A | R | S | `rt/presentation.rs:592-601` (MouseTracker) | Win `window.rs:960` + `WM_SETCURSOR` (`platform.rs:1581`). mac uses NSCursor (`window.rs:1167`). Android returns `Unsupported` (`android/window.rs:284`) even though Android has `PointerIcon` for mouse and stylus. iOS `Unsupported` (`ios/window.rs:965`); the iPad pointer is absent. Web uses CSS `cursor` (`web/window.rs:482`) |
| Pointer/modifier state queries | R | A | A | N | S0 | A | S | none | Win `window.rs:1040,1059`. Android `mouse_position` is always the origin (`window.rs:374`) |

### 2.3 Displays and metrics

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Display enumeration | R | R | P | P | **S** | P | S | none | Win `EnumDisplayMonitors` (`pf/windows/display.rs`) without refresh rate (default 60). Linux `usable_bounds` = full bounds (`winit/display.rs:22`). iOS main screen only. **Android is hard-coded 1080×2340 @ 2.75** (`android/mod.rs:862-886`). Web is a single `screen` |
| Window's current display (`PlatformWindow::display`) | P | A | A | A | A | R | S | none | **Win always returns the primary display** and ignores the `MonitorFromWindow` result (`window.rs:1091-1110`) |
| Display hotplug / change notification | A | A | A | A | A | A | A | *(contract)* | |
| DPI / scale factor + change | R | R | R | R | R | P | S | flui-app `on_resize` → flui-runtime | Win per-monitor v2 + `WM_DPICHANGED` (`platform.rs:805,1497`). mac `windowDidChangeBackingProperties`. Linux `ScaleFactorChanged` (1284). iOS layout/trait metrics (`ios/window.rs:144-160`). Android `ConfigChanged` (`mod.rs:582-596`). Web re-reads DPR on resize and ResizeObserver, but a DPR-only change (moving to another monitor) isn't observed (`web/events.rs:66-85`) |
| Refresh rate / frame period (`refresh_period`) | **A** | R | R | P | A | N | S | flui-app `runner/desktop.rs:190-266` (falls back to `DEFAULT_DISPLAY_PERIOD`) | Win has no override and the display reports the 60 default (`api/display.rs:60`). mac `window.rs:871` via `display.rs:127`. Linux `winit/window.rs:187`. iOS paces frames with `CADisplayLink` (`ios/window.rs:171`) but doesn't report the ProMotion period |
| Compositor pacing (`pre_present_notify`) | N | N | R | N | N | N | S | flui-app `frame_pacing.rs` | Wayland frame callback (`winit/window.rs:195`) |
| Frame request (`request_redraw` / `on_request_frame`) | R | R | R | R | R | R | S | flui-app runners | Win `InvalidateRect`, mac `setNeedsDisplay`, iOS `CADisplayLink`, Android atomic + loop tick, Web RAF. Headless dispatches synchronously |
| Safe-area insets + change | N | A | N | R | R | A | S | `rt/media_query_root.rs:71`; flui-app `android.rs:307`, `ios.rs:414` | iOS `safeAreaInsetsDidChange` (`ios/window.rs:155,945`). Android uses `WindowInsets` systemBars + displayCutout via JNI (`android/safe_area.rs:84-111`). Web doesn't read `env(safe-area-inset-*)`. mac doesn't report the notch in fullscreen |
| Keyboard (IME) occlusion insets | N | N | N | A | A | A | A | *(contract)* | The contract explicitly says safe area "is not keyboard occlusion" (`api/platform_window.rs:367-372`). Android omits `Type.ime()` |
| Gesture geometry (native slop, double-click area) | R | A | A | A | R | A | A | `rt/ui_runtime/preferences.rs:181` → flui-interaction `GestureSettings` | Win DPI + system metrics (`window.rs:921`). Android `ViewConfiguration` (`android/window.rs:241`) |
| Native text sizing capture (nonlinear text scale) | N | N | N | R | R | A | A | flui-app `native_text_sizing.rs` | iOS `UIFontMetrics` per text style (`ios/text_sizing.rs`). Android `TypedValue` sp (`android/text_sizing.rs`). On desktop the `text_scale` preference stands in (Windows only) |
| GPU surface (raw-window-handle) | R | R | R | R | P | R | N | flui-engine via flui-app raster lane | Android answers `Ok` across a pause; that soundness caveat is documented (`api/platform_window.rs:553-568`). Headless is always `Unavailable` |

### 2.4 Input

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Pointer: mouse | R | R | R | P | R | R | S | `rt/ui_runtime/input.rs:271` → flui-interaction | Win `EnableMouseInPointer` + `WM_POINTER`/`WM_MOUSE*` (`platform.rs:805,1556-1650`). mac `view.rs:400-450`. iOS maps only indirect-pointer *touches* to Mouse (`ios/events.rs:226-234`); there's no hover or scroll for the iPad pointer |
| Pointer: touch | R | N | R | R | R | R | S | same | Win `WM_POINTER` (`events.rs:594-690`). iOS `touchesBegan…` (`ios/window.rs:120-140`) |
| Pointer: pen/stylus (pressure, tilt, eraser, hover) | R | **A** | **A** | R | R | R | S | same | mac doesn't translate tablet events. winit 0.30 has no pen. iOS Pencil + hover recognizer (`ios/window.rs:109`). Android `ToolType::Stylus/Eraser`, tilt, orientation (`android/input.rs`) |
| Device add/remove, cancel | R | P | R | P | R | P | S | same | Win `WM_POINTERDEVICECHANGE` (`platform.rs:1671`). The others have cancel only |
| Wheel / scroll (discrete + precise phases) | P | R | R | A | R | P | S | same → `flui-widgets/src/scroll/scrollable.rs` | Win `WM_MOUSEWHEEL/HWHEEL` (`events.rs:1544-1600`) without precision-touchpad phases or DirectManipulation. mac phases + momentum (`view.rs:456`). Android `AXIS_VSCROLL/HSCROLL` (`shared/android_scroll.rs`). Web `wheel` without phases (`web/events.rs:331`) |
| Trackpad pinch / rotate (`PanZoom`) | **A** | R | A | N | N | A | S | flui-interaction scale recognizer | mac `magnifyWithEvent`/`rotateWithEvent` (`view.rs:460-476`). Linux has the code path (`winit/platform.rs:1131`), but winit 0.30 only emits these events on macOS and iOS. Win precision-touchpad pinch arrives as Ctrl+wheel and isn't decoded; the same is true of Web |
| Force-touch pressure | N | R | A | N | N | N | A | flui-interaction | mac `pressureChangeWithEvent` (`view.rs:468`) |
| Keyboard events | R | R | R | **A** | R | R | S | `rt/ui_runtime/input.rs:289` → focus | Win `platform.rs:1751-1885`, `events.rs:1686-1744`. mac `view.rs:386-394`. Linux `shared/keyboard_adapter.rs` (`platform.rs:1306`). iOS has no `pressesBegan` or hardware keyboard handling. Android hardware `KeyEvent` (`android/input.rs:599`). Web keydown/keyup on a canvas with `tabindex=0` (`web/events.rs:271`, `window.rs:165`) |
| Keyboard layout id + change | R | A | A | A | A | S0 | S0 | none | Win `GetKeyboardLayoutNameW` (`platform.rs:2756`) + `WM_INPUTLANGCHANGE` (1942). Web is hard-coded `"en-US"` (`web/platform.rs:390`) |
| System back / navigation (Android back, predictive back, browser history) | N | N | N | N | A | A | A | *(contract)* | Android back arrives only as a raw key event |
| Drag-and-drop **in** | **A** | **A** | P | A | A | A | A | **none**: flui-runtime drops it ("routing not implemented yet", `rt/ui_runtime/input.rs:302-310`) | Win `NullDataTransferSource` (`platform.rs:2374`); there's no `IDropTarget` or `WM_DROPFILES`. mac Null (`macos/platform.rs:289`). Linux winit offers file URIs only, without drop feedback and with a heuristic burst boundary (`winit/data_transfer.rs:1-40,302-360`) |
| Drag-and-drop **out** (start a drag) | A | A | A | A | A | A | A | *(contract)* | |

### 2.5 Text input

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| IME push model (`PlatformTextInput` + `ImeEvent`) | N (uses TSF) | R | R | **A** | **A** | **A** | S | `rt/presentation.rs:214` → `flui-interaction/src/text_input.rs` → flui-widgets `EditableText` | mac `NSTextInputClient` (`macos/text_input.rs:584-623,727`). Linux winit `Ime` (`platform.rs:1358`; `WinitTextInput` `winit/window.rs:60`). iOS has no `UITextInput`/`UIKeyInput`. Android has no `InputConnection`. Web has no hidden input or composition events; keydown only carries `isComposing` (`web/events.rs:692`) |
| Text-services store (pull model, `TextStoreHost`) | R | A | A | A | A | A | S | flui-app `runner/mod.rs` → `rt/presentation.rs` | Win TSF (`pf/windows/text_services/mod.rs:772`, `window.rs:885`). Headless uses `with_text_store_host` (`headless/platform.rs:205`) |
| Soft keyboard show/hide | N | N | N | A | A | A | N | *(contract)* | `set_ime_allowed` is the only hook, and no mobile or web backend implements it |
| Input purpose hints (number/email/password), autocorrect, spellcheck, dictation | A | A | A | A | A | A | A | *(contract)* | |

### 2.6 Accessibility

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Accessibility tree (`PlatformAccessibility`) | R | R | R | **A** | **A** | **A** | S | flui-app `runner/mod.rs` (reads `HostWindow::accessibility` once) → flui-semantics | All three desktop adapters sit behind the **`a11y` feature, which is off by default** (`crates/flui-platform/Cargo.toml:359`). Win `accesskit_windows` (`windows/accessibility.rs:165`). mac `accesskit_macos` (196). Linux `accesskit_unix`/AT-SPI (`linux/accessibility.rs:125`, wired only for `cfg(linux)` in `winit/window.rs:325`, so winit on Win/mac has none). Headless `FakeAccessibility` (`headless/platform.rs:1488`) |

### 2.7 Clipboard

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Clipboard text | R | R | P | R | **S** | P | S | flui-app `runner/host.rs:310` → `UiRuntime` → `flui-interaction/src/clipboard.rs:21` (`ClipboardHandle`) → widgets | Win `windows/clipboard.rs:59`. mac NSPasteboard (`macos/clipboard.rs:141`). Linux arboard (X11/XWayland); it's inert on pure Wayland without data-control (`winit/clipboard.rs`). iOS UIPasteboard (`ios/clipboard.rs:46`). **Android `MockClipboard` is in-process only** (`android/mod.rs:827-860`). Web: `writeText` is fire-and-forget, and `read_text` returns only this app's cached write (`web/clipboard.rs:36-58`) |
| Rich clipboard (image/HTML/files/custom MIME) | A | A | A | A | A | A | A | none | `ClipboardItem` holds text + metadata only (`api/clipboard.rs:50`). Every `clipboard_offer()` is `None`, winit included (`winit/data_transfer.rs:303`) |

### 2.8 System services

| Feature | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| `open_url` | R | A | A | A | A | R | S | none | Win `ShellExecuteW` (`platform.rs:2512`). Web `window.open(_blank)` (`web/platform.rs:384`). Headless records the URLs (418) |
| `reveal_path` / `open_path` | R | A | P | N | N | N | A | none | Win explorer `/select` + `ShellExecuteW` (2531/2552). Linux spawns `xdg-open`, and reveal opens the parent directory without selecting the file (`winit/platform.rs:2459-2461,2489`). Native mac has neither (winit-on-mac uses `open -R`) |
| File open/save dialogs | P | S | S | S | S | S | S0 | none | Win `IFileOpenDialog`/`IFileSaveDialog` on an STA thread (`platform.rs:2575,2661`), with no filters, title or owner window. Every other backend keeps the default `Task::ready(Ok(None))` (`tr/platform.rs:377-397`), **indistinguishable from the user cancelling** |
| Background executor | R | R | R | R | S | P | S | none (flui-app `AppRuntime` owns the real pools; `crates/flui-platform/src/executor.rs` calls this a compatibility surface "slated for removal") | Win/mac/Linux use the shared tokio `BackgroundExecutor`. iOS uses a GCD global queue (`ios/executor.rs:21`). Android runs work inline on the caller (`android/mod.rs:819-825`). Web runs inline (single thread) |
| Persistent storage / app data dirs | R | R | R | P? | A? | A | N | **none**: `app/storage_host.rs:16-28` always returns `None` ("no file store is wired yet"), so `LifecycleContext::storage` is `None` | The `FileStore` building block (`crates/flui-platform/src/storage/`, `storage` feature) uses the `dirs` crate. No type implements the `Storage` trait (only `flui-testing`'s `MemoryStorage`). iOS and Android paths come from `dirs` and are untested. wasm32 is an `Unavailable` stub (`storage/mod.rs:73-110`) |
| `app_path` | R | R | R | R | S | P | S0 | none | Android is hard-coded `/data/local/tmp` (`android/mod.rs:810`). Web returns `location.href` |
| `active_window` query | P | R | R | R | P | R | S | none | Win `GetForegroundWindow` can return another process's HWND (`platform.rs:2308`). Android returns `WindowId(0)` while its window's id is `WindowId(1)` (`mod.rs:759` vs `window.rs:211`) |
| Haptics (`PlatformWindow::haptics`) | N | A | N | **A** | **A** | A | S | `rt/presentation.rs:1202` (`perform_haptic_feedback`, via `rt/ui_runtime/frame_clock.rs:403`) | No real backend overrides `haptics()`. Only `FakeHaptics` exists (`headless/platform.rs:1616`). `UIFeedbackGenerator`, `View.performHapticFeedback`/Vibrator, `NSHapticFeedbackManager` and `navigator.vibrate` aren't wired |
| Capabilities descriptor | S0 | S0 | S0 | S0 | S0 | S0 | S0 | none | Static tables (`tr/capabilities.rs`). Win carries a "TODO: Return actual capabilities" (`platform.rs:2383`). Removed on the branch |

### 2.9 System preferences (`SystemPreferences` + change delivery)

| Field / mechanism | Win | mac | Linux | iOS | Android | Web | Headless | Consumed above | Notes / citations |
|---|---|---|---|---|---|---|---|---|---|
| Change delivery | R | P | P | R | P | R | S0 | flui-app `runner/host.rs:165` → `rt/ui_runtime/preferences.rs` | Win uses a hidden receiver window (`WM_SETTINGCHANGE`) with retry (`windows/preferences/receiver.rs`). mac polls every 500 ms (`macos/preferences.rs:19`). Linux subscribes to the XDG portal `SettingChanged`, for motion keys only (`winit/preferences.rs:118-136`). iOS uses `NSNotificationCenter` (`ios/preferences.rs`). Android: `ConfigChanged` + 500 ms polling. Web: `matchMedia` change (`web/preferences.rs:52`). Headless: `preferences()` isn't overridden, so it's always empty with no injection hook |
| `text_scale` | R | A | A | P (via text sizing) | P (via text sizing) | A | S0 | `rt/ui_runtime/text_preparation.rs` | Win `windows/preferences.rs:391`. Linux doesn't read GNOME `text-scaling-factor`. Web doesn't read the browser font size |
| `motion` (reduce motion) | R | R | R | R | R | R | S0 | `rt/ui_runtime/preferences.rs` | Win 395. mac `accessibilityDisplayShouldReduceMotion`. Linux portal `reduced-motion` / GNOME `enable-animations`. Android `android/preferences.rs:418`. Web `prefers-reduced-motion` (`web/platform.rs:229`) |
| `high_contrast` | R | A | A | A | A | A | S0 | same | Win 396. None of these is read: mac increase-contrast, Linux portal `contrast`, iOS darker colours, Android high-contrast text, Web `prefers-contrast`/`forced-colors` |
| `text_weight` (bold text) | N | N | N | R | R | N | S0 | same | iOS `ios/preferences.rs:101`. Android `fontWeightAdjustment` (`android/preferences.rs:421`) |
| `locales` | R | A | A | A | A | A | S0 | same | Win `GetUserPreferredUILanguages` (`windows/preferences/languages.rs`, 397). `navigator.languages`, Android `Configuration.getLocales` and `NSLocale.preferredLanguages` aren't read |
| Double-click interval | R | R | A | N | N | A | S0 | `flui-interaction/src/settings.rs` | Win 400. mac `NSEvent.doubleClickInterval` |
| Double-tap / long-press timeouts | A | N | A | A | R | A | S0 | `flui-interaction/src/settings.rs`, `recognizers/long_press.rs` | Android `ViewConfiguration` (`android/preferences.rs:413-414`) |
| Native mouse/touch geometry (snapshot) | R | A | A | A | R | A | S0 | via `gesture_geometry` | Win 403 (mouse). Android 415 (touch) |
| Wheel lines/characters | R | A | A | N | P | N | S0 | `flui-widgets/src/scroll/scrollable.rs` | Win 411-412. Android scroll factors are applied during event translation and aren't in the snapshot |
| Accent colour, dark-mode-as-preference, font family | A | A | A | A | A | A | A | *(contract)* | Appearance is per window only (see 2.2) |

---

## 3. Biggest gaps per backend

- **Windows:** no drag and drop at all (Null source). Appearance always reports Dark because FLUI forces the DWM dark flag and then reads it back. `display()` always returns the primary display. No refresh period (pacing falls back to the default). Min/max/resizable options are ignored. Undecorated windows can't be dragged or resized, and there's no custom title bar. No touchpad pinch or precise scroll phases. Session end isn't handled. `Platform::activate` does nothing.
- **macOS:** appearance and dark mode are absent (always Light, no change event). Drag and drop is absent. No pen/tablet. No `open_url`, reveal/open path or file dialogs; the dialogs silently report "cancelled". No `on_window_event` delivery. No keyboard layout. No locales or high contrast. `is_maximized`/`is_fullscreen`/`display` keep their defaults.
- **Linux (winit):** no `open_url` or dialogs. Appearance is effectively always Light and never changes (winit limits). Clipboard is inert on pure Wayland. No pinch, rotate or pen. Occlusion works only on X11. `bounds()`, `resize()`, `get_title()` and `activate` are missing. `show` is unsupported on Wayland. Preferences cover motion only. Accessibility needs the `a11y` feature. File drops only, without feedback.
- **iOS:** no keyboard events, IME/text input, soft keyboard, haptics, accessibility, appearance, deep links or keyboard insets. `open_window` is unsupported (scene-driven). The ProMotion refresh period isn't reported. No trackpad scroll. No `open_url`.
- **Android:** the clipboard is a mock, displays are fake constants, `app_path` is fake and the executor runs inline. No IME/text input, soft keyboard, haptics, accessibility, appearance, deep links, back-button contract or keyboard insets. `is_focused` is always true. Visibility and execution-state changes are never dispatched. The cursor is unsupported.
- **Web:** no IME/composition. Clipboard reads only its own writes. No drag and drop, accessibility, safe area or close veto. Appearance changes and DPR changes on a monitor move aren't observed. Keyboard layout is hard-coded to "en-US". Execution state is never set. Dialogs report "cancelled".
- **Headless:** it can't inject preferences, appearance changes, hover, keyboard-layout changes or session end (`simulate_session_end` is inert), and it has no data-transfer source. Everything else can be simulated.
- **Contract-wide (no backend can implement these without a new API):** window position, runtime min/max, parent/owner/modal windows, always-on-top, transparency, window icon. Drag-out and rich clipboard. Soft keyboard and input-purpose hints. Keyboard insets. System back. Display hotplug. Memory pressure. Native menus (app menu bar, context menu), tray/status icon, notifications, dock/taskbar badges, single-instance, wake-lock, orientation lock, status-bar style, share sheet, permissions, accent colour. `on_session_end` isn't reachable from `OwnerPlatform`/`SharedPlatform`.

---

## 4. Unmerged branch `origin/runtime/host-input` (db8211c, merge-base fb57300)

`git diff --stat origin/main...origin/runtime/host-input -- crates/flui-platform-api crates/flui-platform`:
70 files, +8668/−3370. These changes land in the rows above as follows (marked **branch**):

| Row affected | Change on the branch |
|---|---|
| Capabilities descriptor | **branch:** `Platform::capabilities()`, `SharedPlatform::capabilities()` and the whole `PlatformCapabilities` trait are removed (`tr/capabilities.rs` −218). In their place, each window delivers `PlatformInput::Capabilities(InputCapabilities)` (new `api/input_capabilities/`), first before any other input and again on every change. The fields are `pen`, `precise_scroll`, `trackpad_gestures`, `input_regions`, `outgoing_drag` (each `Native`/`Emulated`/`Unavailable`) plus `present: PointerKindSet`. Per-backend tables (`shared/input_availability.rs`): Win `pen`. mac `precise_scroll` + `trackpad_gestures`. iOS `pen` + `input_regions`. Android `pen` (+ `input_regions` at API ≥ 29). Web `pen`. winit by session: macOS precise + gestures, Wayland precise, Windows/X11 nothing. `outgoing_drag` is `Unavailable` everywhere. Headless is all `Native` |
| New row: input regions / system-gesture exclusion | **branch:** `PlatformWindow::set_input_regions(&[InputRegion])` (ADR-0217, new `api/input_regions.rs`). iOS **R** (`preferredScreenEdgesDeferringSystemGestures`), Android **R** (`View.setSystemGestureExclusionRects`, new `pf/android/gesture_exclusion.rs`, API 29+), Headless **S** (records). Win/mac/Linux/Web keep the default no-op. The framework now collects input regions from layout and hands them to the window |
| Preferences: haptics enabled | **branch:** a new `SystemPreferences::haptics: Option<bool>` (ADR-0212 §1). Only Android produces it (`android/preferences.rs` `with_haptics`). The haptics *output* row stays A on every real backend |
| Pointer rows (mouse/touch/pen/device/cancel) | **branch:** a new shared pointer assembly (`shared/pointer_assembly/{registry,buttons,scroll,pan_zoom,clock,batch}.rs`, +~2100 lines). The Windows, winit, Android, iOS, macOS and Web event translators are rewritten onto it. One `InputSequenceId` per contact, event-time deadlines, `EventTime::checked_add`. Cell values don't change: mac and Linux still have no pen, and nothing new decodes Win/Web Ctrl+wheel pinch |
| Wheel/scroll + PanZoom | **branch:** `PanZoom` becomes pinch/rotate only; two-finger pan moves to phased `Scroll` (`ScrollPhase`) on macOS and winit. Same cells |
| `on_input` thread contract | **branch:** `on_input` loses `+ Send`. It is owner-thread-only and refuses registration off-thread (may capture `Rc`). A reentrant key event is answered `DispatchEventResult::DEFERRED_UNHANDLED` and delivered once after the running callback returns (ADR-0207 §7) |
| Drag-and-drop | **branch:** `TransferActions` becomes a `bitflags` type. No new transport, so drop-in and drop-out cells don't change |
| Gesture settings (consumer side) | **branch (interaction):** per-pointer-kind gesture presets, `HitSlop`/`ContentShape`, One Euro smoothing after recognition, and native touch slops projected through the touch profile only. These consume the existing `gesture_geometry`/preferences rows; no new platform producer |

Rows the branch does **not** touch: windows, appearance, displays, refresh rate, IME, accessibility,
clipboard, dialogs, open_url, storage, session end, deep links and haptics output.
