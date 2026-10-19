# Platform parity

> **Last reviewed:** 2026-10-17 · **Last updated:** 2026-10-18 · **Change:** minor (macOS, iOS and Android window lifecycle, Web page lifecycle) ·
> **Target:** every environment below is a ready platform

What every environment FLUI supports must do, and where each backend stands today. An environment is
a **ready platform** when its native backend has every row of the core and system-service sections
and each row has passed an executed run there. The ranked work list is [`gaps.md`](gaps.md).

Linux runs on the winit fallback backend until the native Linux backend lands, so the Wayland and X11
columns describe what winit delivers today. The headless backend is the test double: ✅ there means a
test can drive the row.

| Mark | Meaning |
|---|---|
| ✅ | Implemented and passed an executed run on that environment; the note or [`BETA.md`](BETA.md) names the run |
| 🟡 | Implemented but never run there, or partial; the note says which |
| ❌ | Missing: no code path, a stub, a mock, or a default that pretends to succeed |
| — | Not applicable: the environment has no such concept |
| ❔ | Not audited yet |

**Overall: ~27% (✅ = 1, 🟡 = ½, over applicable cells of the seven user environments) ·
455–755 h to full (estimated agent hours, ❔ rows excluded; the native Linux backend is counted once,
in [`gaps.md`](gaps.md), not per row).**

When work lands, update the row here and its entry in [`gaps.md`](gaps.md), and bump this file's
status line and revision history. A ✅ needs an executed run on that environment; compile-only code is
🟡.

Sources: the per-backend audit with `file:line` evidence
([`research/2026-10-14-platform-backend-audit.md`](research/2026-10-14-platform-backend-audit.md)),
and the checklists drawn from other frameworks, with the native API for each environment:
[desktop](research/2026-10-14-platform-checklist-desktop.md) and
[mobile and Web](research/2026-10-14-platform-checklist-mobile-web.md).

## Summary

Counted over every row of the core and system-service sections.

| Environment | Backend | ✅ | 🟡 | ❌ | — | ❔ | Score | Ready |
|---|---|---|---|---|---|---|---|---|
| Windows | Win32 (native) | 9 | 30 | 35 | 17 | 2 | 32% | no |
| macOS | AppKit (native) | 9 | 26 | 39 | 17 | 2 | 29% | no |
| Wayland | winit (fallback) | 6 | 22 | 40 | 23 | 2 | 24% | no |
| X11 | winit (fallback) | 8 | 22 | 37 | 24 | 2 | 28% | no |
| iOS | UIKit (native) | 7 | 16 | 41 | 29 | 0 | 23% | no |
| Android | android-activity (native) | 10 | 14 | 40 | 29 | 0 | 27% | no |
| Web | wasm canvas (native) | 7 | 17 | 34 | 34 | 1 | 26% | no |
| Headless | test double | 33 | 0 | 55 | 5 | 0 | 38% | — |

## Core

### App lifecycle

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Event loop, startup and wake-ups | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Runs: `device windows-input`, the macOS live check, `live-smoke` (Xvfb and `--wayland`), the iOS Simulator, the Android emulator, the Web counter | 0 |
| Quit, exit request, last-window policy | 🟡 | ✅ | ✅ | ✅ | — | 🟡 | 🟡 | ✅ | macOS Cmd+Q and close ran; `live-smoke` checks the exit code; iOS has no quit API | 2–3 |
| Close request with veto | 🟡 | ✅ | 🟡 | 🟡 | — | — | ❌ | ✅ | Web has no `beforeunload` | 2–3 |
| Reopen with no windows (Dock click) | — | ✅ | — | — | — | — | — | ❌ | `device macos-reopen`; headless can't drive it | 1 |
| Focus and active state | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Android follows `onWindowFocusChanged` alone, and focus no longer stands for the Activity being resumed. Galaxy S21 run (2026-10-18, SM-G991U, Android 15, release probe printing the signals): the notification shade gave Inactive then Resumed, Home gave Inactive, Hidden, Paused, and returning gave Hidden, Inactive, Resumed | 2–4 |
| Visibility, minimize, occlusion | 🟡 | ✅ | ❌ | ✅ | ✅ | 🟡 | 🟡 | ✅ | Windows sees minimize and restore only; winit never reports occlusion on Wayland; Android dispatches `onStart` and `onStop` as visibility; Home and return ran on the Galaxy S21 above. Runs: macOS minimize and hide, the X11 cover window in `live-smoke`, iOS Home and return | 6–10 |
| Suspend, resume, GPU-surface loss | — | — | — | — | ✅ | 🟡 | 🟡 | ✅ | Android dispatches `onResume` and `onPause` as the execution state, apart from focus (the shade left it running, Home suspended it on the Galaxy S21 above); Web: `freeze` and `resume` set the execution state (run against a fake page), and `webglcontextlost`, `webglcontextrestored` and a lost WebGPU device rebuild the renderer at once, without a frame (compile-checked, its decisions unit-tested; not yet in Chrome) | 4–8 |
| Sleep, wake, screen lock | ❌ | 🟡 | ❌ | ❌ | — | — | — | ❌ | macOS delivers `NSWorkspace` will-sleep and did-wake and the distributed `screenIsLocked`/`screenIsUnlocked`; type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736) lists the run. The other backends do not deliver them yet | 3–5 |
| Session end (log off, shutdown) with veto | ❌ | 🟡 | ❌ | ❌ | 🟡 | 🟡 | 🟡 | ❌ | macOS asks the hook on `NSWorkspaceWillPowerOffNotification`, holds the log out with `NSTerminateLater` while a block is held (it replies on release or after 4 s) and lets it pass otherwise; iOS tells the hook `Ending` from `applicationWillTerminate:`, which iOS does not promise; both type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736) lists the run. Android sends the end notice only, from `onStop` and `onSaveInstanceState` (once per start-to-stop cycle, since a Back that finishes the Activity saves no state), with no veto; on the Galaxy S21 above it arrived on each Home, so it repeats and does not mean the app is over. Web reports `pagehide` as the session ending and a restored cached page as cancelling it, without a veto and with writes best effort; run against a fake page, not yet in Chrome. The other backends do not handle it yet | 4–6 |
| Memory pressure | ❌ | 🟡 | ❌ | ❌ | 🟡 | 🟡 | — | ❌ | macOS maps the dispatch memory-pressure source (warn is `Moderate`, critical is `Critical`) and iOS maps `applicationDidReceiveMemoryWarning:` to `Critical`; type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736) lists the run. Android gives `Critical` for `onLowMemory` and reads `lastTrimLevel` through JNI on `LowMemory`, `Pause`, `Stop` and `Resume` (NativeActivity has no `onTrimMemory`), so a level is noticed at the next of those callbacks, not when the system sets it. Galaxy S21 run above, `am send-trim-memory`: RUNNING_MODERATE and BACKGROUND gave `Moderate`, RUNNING_CRITICAL and COMPLETE gave `Critical`, each once; `onLowMemory` was not run (adb cannot raise it) | 3–5 |
| Bring the app forward, request attention | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Windows `activate` re-foregrounds whatever is already in front; Wayland needs an `xdg-activation` token | 3–5 |
| Single instance, forward arguments | ❌ | — | ❌ | ❌ | — | — | — | ❌ | macOS enforces one instance itself | 3–5 |
| Deep links, URL schemes, file associations | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ | `on_open_urls` is never called, at cold start or while running; the Web entry URL is under history routing | 12–20 |
| State restoration | — | ❌ | — | — | ❌ | ❌ | ❌ | ❌ | macOS window restoration, iOS scene restoration, Android state after process death, Web `history.state` | 8–12 |
| App identity (AUMID, bundle ID, `.desktop` file) | ❔ | ❔ | ❔ | ❔ | — | — | — | — | Notifications, URL schemes, the tray, jump lists and portals need it; `flui-cli` packaging not audited | ❔ |

### Windows

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Open and close a window | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | iOS: the scene supplies the window; Android: one native window | 0 |
| Multiple windows | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | — | ✅ | No iPad scenes, no Android multi-window | 8–14 |
| Show without a blank first frame | 🟡 | ✅ | ❌ | 🟡 | — | — | — | ✅ | Windows and X11 show the window at creation; Wayland refuses `show`. macOS run: deferred first reveal | 3–5 |
| Title | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ | No API for apps; winit can't read the title back | 2–3 |
| Minimize, maximize, restore, state readback | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | ✅ | No API for apps; no readback on Linux; Wayland never reports minimized. macOS reads the mode from `NSWindow` and carries out every mode command, type-checked only ([#1736](https://github.com/vanyastaff/flui/issues/1736)); an earlier macOS run covered minimize and hide | 3–5 |
| Fullscreen | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ | winit is borderless only; Web republishes the state on `fullscreenchange`; mobile immersive mode is under System UI | 2–4 |
| Position, size, move and resize events | 🟡 | 🟡 | 🟡 | ❌ | — | — | 🟡 | ✅ | No `set_position` in the contract; winit `bounds()` and `resize` are missing; Wayland can't read or set a global position by design. macOS reads and sets the outer top-left position, content size and limits through `NSWindow` (type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736)) | 4–8 |
| Size constraints, resizable and other flags | ❌ | 🟡 | 🟡 | 🟡 | — | — | — | ✅ | Windows ignores min/max size and resizable (no `WM_GETMINMAXINFO`); no runtime setter anywhere. macOS sets and reads content size limits, resizable and decorated through `NSWindow` (type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736)) | 3–5 |
| Live resize without jitter | ❔ | ❔ | ❔ | ❔ | — | — | ❔ | — | Not audited; Windows has no `WM_ENTERSIZEMOVE` handling | ❔ |
| Save and restore window placement | ✅ | 🟡 | ❌ | ❌ | — | — | — | ✅ | `AppConfig::restore_placement`. Windows opens at the remembered position, size and mode (content size, `SetWindowPlacement`) and ran in a real window. macOS opens at the position, in the mode and returning to the `normal_geometry` of the options, reads the display by its UUID (`persistent_key`) and measures display bounds from the top-left corner; type-checked only, [#1736](https://github.com/vanyastaff/flui/issues/1736) lists the run. winit ignores a position, mode and normal geometry at open until it gains placement support, and no backend but headless, Windows and macOS reports a stable display key to match the display by | 3–5 |
| Undecorated windows, custom title bar | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Undecorated Windows windows can't be dragged or resized; no caption hit-testing, drag regions, snap layouts or system window menu; compositors without server-side decorations need client-side ones | 12–20 |
| Backdrop material, transparency, corners | 🟡 | ❌ | ❌ | ❌ | — | — | — | ❌ | Windows forces Mica at creation; macOS Liquid Glass sits behind a `&mut` extension nothing can reach | 6–10 |
| Owned, modal and popup windows; always on top | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Not in the contract, so menus, tooltips and dropdowns can't leave the window; Wayland needs `xdg_popup` | 10–16 |
| Window icon | ❌ | — | ❌ | ❌ | — | — | — | ❌ | `WM_SETICON`, `xdg-toplevel-icon`, `_NET_WM_ICON` | 1–2 |
| Exclude a window from screen capture | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ | `SetWindowDisplayAffinity`, `NSWindow.sharingType`, iOS capture detection, Android `FLAG_SECURE` | 2–4 |
| Native views inside the UI (web view, map, video) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | Not in the contract; it shapes compositing, hit-testing and accessibility | 30–60 |

### Displays and metrics

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Displays: list, work area, current display, changes | ✅ | 🟡 | ✅ | ✅ | 🟡 | ✅ | ✅ | ✅ | Headless injects displays, changes and window moves. Windows keys each monitor by adapter and target (`QueryDisplayConfig`), names the window's display through `MonitorFromWindow`, and delivers `WM_DISPLAYCHANGE`, DPI and work-area changes. Windows run (2026-10-18, Windows 11, one 3440×1440 monitor, `examples/displays.rs` watching): a refresh switch, a scale change to 125% and a resolution change to 2560×1440 each published a new snapshot with the same `DisplayId`. Windows two-monitor run (2026-10-18, Windows 11, `examples/displays.rs` watching, a second 3440×1440 monitor connected live): the new monitor appeared as a new snapshot revision with `id 2` at (3440, 0) and 60.001 Hz while `id 1` stayed at 100 Hz; dragging the window to monitor 2 raised `on_display_changed` and the window then named `DisplayId 2`, and dragging it back named `DisplayId 1`; rearranging monitor 2 to the left, at (-3440, 0), kept `id 2`; changing the refresh rates live, monitor 1 to 144 Hz and monitor 2 to 165 Hz, published each change with the ids unchanged. Disconnecting a monitor on Windows was not run (removing an output was run on X11, below). macOS keys each display by its UUID, resamples after `NSApplicationDidChangeScreenParametersNotification` and raises the window's display observation from `windowDidChangeScreen:` and backing-property changes; iOS numbers each retained `UIScreen`, observes screen and scene notifications and names the window's screen; both are type-checked only; no Apple code runs in CI. The Web lists the one screen, or every screen once the Window Management permission is already granted (it is read, never requested), keys a listed screen by its JS object, and resamples on a pixel-ratio, `window.screen` or screen-list change. Web run (2026-10-18, Windows 11, Chrome, one 3440×1440 monitor, `browser_display_probe`): with access already granted the display kept id 1 while its name changed from "Browser Screen" to "34HC5CUR P"; page zoom 0.9, 1.0, 1.1 and 1.25 changed the scale each time while the bounds stayed 3440×1440; more than one screen was not run (one monitor). Android reads every display through JNI (`DisplayManager.getDisplays`: real size, density, the current mode's rate, `getDisplayId` as the key) and resamples on `ConfigChanged`, on resume and on the choreographer's refresh-rate callback (API 30+; below it, only the first two). Android run (2026-10-18, Galaxy S21 SM-G991U, Android 15 / API 35, release probe): 1080×2400, scale 2.8125, 8 333 µs (120 Hz); the adaptive panel dropped to 16 666 µs when idle and returned to 120 Hz on touch, through the choreographer callback; Settings → Motion smoothness Standard gave 16 666 µs and Adaptive 8 333 µs, with the same `id=1`; more than one display (an external or virtual screen) was not run. Linux reads the outputs from the compositor on a connection of its own, kept current by a thread that wakes the owner: Wayland `wl_output` v2–4 with `zxdg_output_v1` (applied at `done`; keyed by the v4 connector name), X11 RandR 1.5 monitors (keyed by the first output's XID); the winit window names its display by its monitor's native number (the X11 CRTC, the `wl_output` global name). X11 runs (2026-10-18, WSL Ubuntu 24.04 on Windows 11, `examples/displays.rs` watching): XWayland under WSLg read `rdp-0`, 3440×1440, 59.936 Hz, and the window matched its output by CRTC and fired the display hint; live RandR changes under an Xorg dummy server published a new snapshot each time with the same `id 1`: 60.020 → 75.025 → 85.024 Hz, mode 1600×900, output off (no displays) and back on. Wayland run (same session): WSLg's weston read `weston rdp`, 3440×1440, 60.000 Hz, and a headless weston read `headless` 1280×720. Wayland live run (2026-10-18, `examples/displays.rs` watching, winit/Linux under WSLg, WSL Ubuntu 24.04 on Windows 11 with two 3440×1440 monitors): revision 1 was `weston rdp` at 640×480 while WSLg was starting; revision 2 was the same output grown live to 3440×1440 at (3440, 0), 60.000 Hz, keeping `id 1`; revision 3 added a second `wl_output` live, `weston rdp` 3440×1440 at (0, 0), as `id 2`: both outputs carry the same name and still get distinct ids. A Windows scale change was not seen: WSLg restarts its weston on one, ending the probe's connection, so the scale cannot be observed live through WSLg. Two outputs were present but no window was moved between them. A window that has painted no frame is on no output yet, so it names no display until one enters (winit sends no event for that; the event loop looks again when it goes idle). A move between two Wayland outputs of equal scale is not reported by winit, and a move between two monitors was not run on either protocol | 2–3 |
| Scale factor and DPI changes | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | ✅ | ✅ | Web watches `(resolution: Xdppx)` and re-arms it after each change; page zoom changed the scale at every step in the Web run above. Android run: emulator at density 420 | 2–4 |
| Refresh rate and frame pacing | ✅ | 🟡 | ✅ | ✅ | 🟡 | ✅ | ✅ | ✅ | Windows reads the active mode's rational rate. Windows run (same session): 60 Hz reported as 16 666 µs, a switch to 144 Hz reported 6 944 µs and raised the window's display observation. Windows two-monitor run (2026-10-18, a second 3440×1440 monitor connected live, `examples/displays.rs` watching): monitor 1 at 100 Hz and the new monitor 2 at 60.001 Hz were told apart by id; changing the rates live, monitor 1 to 144 Hz and monitor 2 to 165 Hz, published each change with the ids unchanged, and the window's display followed the monitor it was dragged to (`DisplayId 2`, then back to `DisplayId 1`). macOS reads the display's current-mode rate. iOS asks the display link for a 60 Hz to maximum frame-rate range (iOS 15 and later; `preferredFramesPerSecond` before), writes `CADisableMinimumFrameDurationOnPhone` into the app's `Info.plist`, and reports the cadence the link measures once it leaves a 5% band; both are type-checked only; no Apple code runs in CI. The Web estimates the period from `requestAnimationFrame` timestamps (a trimmed mean over 30 intervals, published only after it has settled), so it is an estimate of the cadence the browser presents at. Web run (same session): 144 Hz read as 6.938 ms (144.14 Hz) about 300 ms after load; a monitor change to 100 Hz read 99.67 Hz; the re-run published 100→144 Hz once, at 6.955 ms (143.79 Hz), with no value in between, and 100 Hz read 99.96 Hz. Android reads the current mode's rate and follows an adaptive switch; the runtime asks for the display's highest rate while an animation runs or a pointer is down, released 250 ms after (`ANativeWindow_setFrameRate`, API 30+), and the idle loop waits one display period instead of 16 ms. Android run (same session): a touch requested the highest mode rate, 120 Hz in Adaptive and 60 Hz in Standard, and `rate=0.0` after the 250 ms hold; touch then Home requested at 28.637, Paused at 28.749 and released at 28.924, and nothing stayed held across the resume. Linux reads the mode's rate: Wayland's `wl_output.mode` refresh in millihertz (zero or less is unknown), X11's `dot_clock / (htotal × vtotal)` as `xrandr` computes it (double scan and interlace counted, zero totals unknown). Linux runs (2026-10-18, same session): XWayland under WSLg read 59.936 Hz; under an Xorg dummy server `xrandr` changes published 60.020, 75.025 and 85.024 Hz at the same display id; Wayland read 60.000 Hz from WSLg's weston and a headless weston, and the live WSLg run (2026-10-18) followed an output growing from 640×480 to 3440×1440 at 60.000 Hz and a second output added, with ids kept and distinct, so Wayland is ✅ | 1–2 |
| GPU surface | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | Rendered in every run above | 0 |
| Safe area | — | ❌ | — | — | ✅ | 🟡 | ❌ | ✅ | macOS ignores the notch in fullscreen; Web doesn't read `env(safe-area-inset-*)`. iOS run: `device ios-safe-area-check` | 2–4 |
| Soft-keyboard insets | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| System-gesture exclusion | — | — | — | — | ❌ | ❌ | — | ❌ | `set_input_regions` arrives with the input stack | 0–1 |
| Display cutout, fold posture | — | — | — | — | — | 🟡 | ❌ | ❌ | Android folds the cutout into the safe area; no fold posture | 3–5 |

### Input

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Mouse: buttons, hover, capture | ✅ | ✅ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | ✅ | The iPad pointer has no hover or scroll. Runs: `device windows-input`, macOS CGEvent clicks, X11 `live-smoke`, the Web counter | 3–5 |
| Touch | 🟡 | — | 🟡 | 🟡 | ✅ | ✅ | 🟡 | ✅ | Runs: `device ios-input-check`, an Android emulator tap | 2–4 |
| Pen and stylus | 🟡 | ❌ | ❌ | ❌ | 🟡 | 🟡 | 🟡 | ✅ | macOS doesn't translate tablet events; winit has no pen | 8–12 |
| Precise scrolling with phases and momentum | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | Windows has no precision-touchpad phases (its wheel smoke passed); iOS has no trackpad scrolling; Web has no phases | 6–10 |
| Trackpad pinch and rotate | ❌ | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | Windows and Web don't decode Ctrl+wheel pinch; winit emits these on macOS only | 6–10 |
| Pointer lock and warp | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| Keyboard: keys, modifiers, repeat, dead keys | ✅ | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | iOS handles no hardware keyboard. Windows run: Tab and Enter | 4–6 |
| Keyboard layout and its changes | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Web hard-codes `en-US` | 4–6 |
| Cursor shapes | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | 🟡 | ✅ | No iPad pointer styles or Android `PointerIcon`; no custom image or hide-until-move | 3–5 |
| System back and history | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android predictive back (from targetSdk 36, `KEYCODE_BACK` isn't delivered), the iOS edge swipe, the Web History and Navigation APIs | 10–16 |
| Browser gesture suppression | — | — | — | — | — | — | ❌ | — | No `touch-action` or `overscroll-behavior` on the canvas | 1 |
| Input device availability | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Static tables today; per-window `InputCapabilities` arrives with the input stack | 0–1 |

### Text input

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| IME composition and candidate placement | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | ❌ | ✅ | Windows TSF and macOS `NSTextInputClient` haven't run with a real input method; no `UITextInput`, no `InputConnection` (NativeActivity has no IME), no hidden input on the Web | 30–50 |
| Soft keyboard show and hide | — | — | — | — | ❌ | ❌ | ❌ | ❌ | | 3–5 |
| Input purpose and hints (password, number, autocorrect) | ❌ | ❌ | ❌ | — | ❌ | ❌ | ❌ | ❌ | Not in the contract: `InputScope`, secure input, `text-input-v3` content type, `UITextInputTraits`, `EditorInfo`, `inputmode` | 6–10 |
| System edit menu | — | — | — | — | ❌ | ❌ | — | ❌ | `UIEditMenuInteraction` (its Paste avoids the iOS paste prompt), Android floating `ActionMode` | 4–6 |
| Spell check | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ | `ISpellChecker`, `NSSpellChecker`, `UITextChecker`, `SpellCheckerSession` | 6–10 |
| Autofill (passwords, one-time codes) | — | — | — | — | ❌ | ❌ | ❌ | ❌ | | 6–10 |
| Handwriting into text fields | ❌ | — | — | — | ❌ | ❌ | — | ❌ | Windows Ink, iPad Scribble, Android stylus handwriting | 6–10 |

### Accessibility

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Accessibility tree, actions, announcements | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | ❌ | ✅ | The desktop adapters sit behind the `a11y` feature, off by default; the runs read one control through UI Automation and AX, not a Narrator or VoiceOver session; announcements not audited; the Web needs its own ARIA DOM (AccessKit has no web adapter) | 30–50 |

### Clipboard and drag and drop

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Plain-text clipboard | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ❌ | 🟡 | ✅ | Inert on pure Wayland without data-control; Android is an in-process mock; Web reads only its own writes | 6–10 |
| Rich clipboard (images, HTML, files) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 8–12 |
| Drop into the app | ❌ | ❌ | 🟡 | 🟡 | ❌ | ❌ | ❌ | ❌ | winit delivers file URIs, and `flui-runtime` discards them | 12–20 |
| Drag out of the app | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 10–16 |

### System preferences

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Dark mode and its changes | ✅ | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ✅ | Every window seeds `MediaQueryData::platform_brightness` and follows changes. Sources: Windows `AppsUseLightTheme` (the title bar follows it), macOS `effectiveAppearance`, Linux the Settings portal's `color-scheme`, iOS `userInterfaceStyle`, Android night mode, Web `prefers-color-scheme`. Runs: on Windows 11 the same probe, as a native Win32 window, followed the user switching the app mode from light to dark and back without restarting, its title bar with it (`DWMWA_USE_IMMERSIVE_DARK_MODE` read back 0 and 1); on a Galaxy S21 (Android 15, release APK) a probe painting the surface from `platform_brightness` started light in light mode, then turned dark in the same process when the user switched dark mode on. No live flip on the other environments | 1–2 |
| Accent colour | ❌ | ❌ | ❌ | ❌ | — | ❌ | — | ❌ | Not in the contract | 2–4 |
| Text scale | 🟡 | — | ❌ | ❌ | 🟡 | 🟡 | ❌ | ❌ | iOS and Android go through native text sizing; Linux doesn't read `text-scaling-factor` | 4–6 |
| Reduce motion | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | Headless can't inject preferences | 1–2 |
| Contrast | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 3–5 |
| Bold text | — | — | — | — | 🟡 | 🟡 | — | ❌ | | 0–1 |
| Languages and regional formats | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Only Windows reads the languages; no regional formats (24-hour clock, first weekday) anywhere | 4–6 |
| Gesture and wheel settings | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | ❌ | ❌ | The Windows wheel smoke passed; the timings never ran | 4–6 |
| Power state (battery, low-power mode, thermal) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 3–5 |

### System UI

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Status and navigation bars, immersive mode | — | — | — | — | 🟡 | ✅ | ❌ | ❌ | Status-bar icon style from `AnnotatedRegion<SystemUiOverlayStyle>` (ADR-0232): Android ran on an API-35 phone (SystemUI received `APPEARANCE_LIGHT_STATUS_BARS` set and cleared on toggle); iOS type-checked only; no navigation-bar or immersive control; headless records the style. Android enforces edge-to-edge from targetSdk 35; Web `theme-color` | 6–10 |
| Orientation lock | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android 16 ignores it on large screens | 2–3 |
| Haptics | — | ❌ | — | — | ❌ | ❌ | ❌ | ✅ | Only the headless fake exists; macOS Force Touch trackpads | 4–6 |
| System sounds, alert beep | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ | | 2–3 |

### Menus

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| App menu bar | — | ❌ | — | — | ❌ | — | — | ❌ | A macOS app without one has no Quit or Edit menu; iPadOS 26 has a menu bar; elsewhere the app draws its own | 10–16 |
| Native context menus | ❌ | ❌ | — | — | — | — | — | ❌ | The mobile system edit menu is under Text input | 6–10 |
| Dock menu, jump list, recent documents | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Linux: desktop actions in the `.desktop` file | 4–6 |

## System services

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Open a URL | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | 🟡 | ✅ | Windows and Web only, and nothing calls it | 3–5 |
| Open a file, reveal it in the file manager | 🟡 | ❌ | 🟡 | 🟡 | — | — | — | ❌ | Linux reveal opens the parent folder without selecting the file | 2–4 |
| Move to trash | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | | 1–2 |
| Open, save and folder dialogs | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Windows has no filters, title or owner window; every other backend answers "cancelled" | 12–20 |
| Message box | ❌ | ❌ | — | — | ❌ | ❌ | ❌ | ❌ | Linux has no native one: the app draws it | 3–5 |
| Share | ❌ | ❌ | — | — | ❌ | ❌ | ❌ | ❌ | | 4–6 |
| App folders and storage | ✅ | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ✅ | `AppConfig::with_storage_dir` gives widgets storage: the user's data directories on desktop and iOS, the app's `files` and `no_backup` on Android, IndexedDB on the Web, where a write based on a version compares and replaces in one readwrite transaction, so two tabs never both succeed from one base. Runs: a probe counting its launches through `LifecycleContext::storage` read back 1 then 2 across restarts on Windows 11 (`%APPDATA%`) and on a Galaxy S21 (Android 15, release APK, data cleared first, process force-stopped between launches). The Web store's own tests ran on Chromium 152 against its IndexedDB; no Web app has run the probe. Android `app_path` still reports a fixed path | 1–2 |
| Tray and status icon | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Linux uses StatusNotifierItem | 6–10 |
| Local notifications | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Needs app identity | 12–20 |
| Taskbar and Dock progress, badge | 🟡 | ❌ | ❌ | ❌ | ❌ | — | ❌ | ❌ | Windows `ITaskbarList3` sits in an extension nothing can reach; Linux `LauncherEntry`; Web Badging API | 4–6 |
| Global shortcuts | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Wayland only through the GlobalShortcuts portal | 4–6 |
| Launch at login | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | | 2–3 |
| Keep the screen awake | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 3–5 |
| Secure storage | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ | Credential Manager or DPAPI, Keychain, Secret Service, Keystore | 6–10 |

## Capabilities

Separate packages on the capability seam. They don't decide whether an environment is ready, and none
exists yet.

| Capability | Environments |
|---|---|
| Location | all but Headless |
| Camera, photo and file pickers on mobile | iOS, Android, Web |
| Motion sensors | iOS, Android, Web |
| Biometrics, passkeys | all but Linux and Headless |
| Push notifications | all but Headless |
| NFC | iOS, Android, Web (Chrome Android) |
| In-app purchase and review | iOS, Android |
| App shortcuts (quick actions) | iOS, Android, Web |
| Printing | Windows, macOS, Linux, iOS, Android, Web |
| Screen capture | Windows, macOS, Linux |
| Colour and font pickers | Windows, macOS |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | major | First version: rows from the backend audit and the desktop and mobile/Web checklists (Electron, Tauri v2, winit, GPUI, Flutter, Compose); ✅ only where `BETA.md` records a run |
| 2026-10-17 | minor | Dark mode implemented on every backend, with change delivery; headless can inject it; the Windows and Android cells ran live |
| 2026-10-17 | minor | Storage wired on every backend (the Web through IndexedDB); the Windows and Android cells ran live |
| 2026-10-17 | minor | The Web store moves to IndexedDB, and writes based on a version work there |
| 2026-10-18 | minor | Windows displays and refresh rate: native keys, the window's display, change delivery and the active-mode rate; the Windows cells ran live |
| 2026-10-18 | minor | macOS and iOS displays and refresh rate: UUID keys, screen notifications, iOS ProMotion and measured period; type-checked only, the cells stay 🟡 |
| 2026-10-18 | minor | Web displays and refresh rate: an estimated period from animation frames, pixel-ratio and screen changes, screens listed when access is already granted; the Web cells ran live in Chrome |
| 2026-10-18 | minor | Android displays and refresh rate: real displays and rate read through JNI, a frame-rate request while the app animates or is touched; the Android cells ran live on a Galaxy S21 (API 35) |
| 2026-10-18 | minor | Linux displays and refresh rate: the outputs read from the compositor (Wayland `wl_output`/`zxdg_output_v1`, X11 RandR 1.5) on a thread that wakes the owner; the X11 cells ran live (XWayland under WSLg, `xrandr` changes under an Xorg dummy server), the Wayland cells read values live and stay 🟡 |
| 2026-10-18 | minor | Windows two-monitor run (`examples/displays.rs`, a second 3440×1440 monitor connected live): a new snapshot with `id 2`, the window naming `DisplayId 2` after a drag and `DisplayId 1` back, the id kept when the monitor moved left, and live rate changes (144 Hz, 165 Hz) published with ids unchanged; the display cells keep their marks; a Windows disconnect was not run; the overall figure recounted from the tables (~26%, 54 ✅, 143 🟡, 477 applicable cells), the summary table brought in line, and the hours of the two display rows lowered to the runs still owed |
| 2026-10-18 | minor | Wayland displays and refresh rate ran live (`examples/displays.rs` under WSLg: an output grown from 640×480 to 3440×1440 keeping `id 1`, then a second same-named output added as `id 2`); the two Wayland cells become ✅ and the overall figure is recounted: 56 ✅, 141 🟡, 477 applicable cells, (56 + 70.5) / 477 = 26.5%, ~27%; a Wayland scale change cannot be observed through WSLg and a window moved between the two outputs was not run |
| 2026-10-18 | minor | macOS and iOS window lifecycle: macOS reads and sets window state through `NSWindow`, delivers sleep, wake, lock and memory pressure, and holds a session end with `NSTerminateLater`; iOS maps the memory warning and `applicationWillTerminate:`; display bounds and persistent keys on macOS; type-checked only, the cells stay 🟡 until [#1736](https://github.com/vanyastaff/flui/issues/1736) runs |
| 2026-10-18 | minor | Web page lifecycle: `freeze` and `resume` set the execution state, `pagehide` is reported as the session ending (and a page restored from the back/forward cache as cancelling it), `fullscreenchange` republishes the window state, and a lost WebGL context or WebGPU device rebuilds the renderer at once; the page events ran against a fake page and the renderer rebuild is compile-checked, so the cells are 🟡; the two cells it moves from ❌ and — to 🟡 leave the overall figure at ~27% |
