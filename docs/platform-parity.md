# Platform parity

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-14 · **Change:** minor (Windows DirectManipulation) ·
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

**Overall: ~25% (✅ = 1, 🟡 = ½, over applicable cells of the seven user environments) ·
470–780 h to full (estimated agent hours, ❔ rows excluded; the native Linux backend is counted once,
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
| Windows | Win32 (native) | 5 | 33 | 37 | 16 | 3 | 29% | no |
| macOS | AppKit (native) | 9 | 22 | 43 | 17 | 3 | 27% | no |
| Wayland | winit (fallback) | 4 | 23 | 41 | 24 | 2 | 23% | no |
| X11 | winit (fallback) | 6 | 23 | 38 | 25 | 2 | 26% | no |
| iOS | UIKit (native) | 7 | 14 | 42 | 31 | 0 | 22% | no |
| Android | android-activity (native) | 5 | 17 | 43 | 29 | 0 | 21% | no |
| Web | wasm canvas (native) | 4 | 20 | 34 | 35 | 1 | 24% | no |
| Headless | test double | 35 | 1 | 53 | 5 | 0 | 40% | — |

## Core

### App lifecycle

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Event loop, startup and wake-ups | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | Runs: `device windows-input`, the macOS live check, `live-smoke` (Xvfb and `--wayland`), the iOS Simulator, the Android emulator, the Web counter | 0 |
| Quit, exit request, last-window policy | 🟡 | ✅ | ✅ | ✅ | — | 🟡 | 🟡 | ✅ | macOS Cmd+Q and close ran; `live-smoke` checks the exit code; iOS has no quit API | 2–3 |
| Close request with veto | 🟡 | ✅ | 🟡 | 🟡 | — | — | ❌ | ✅ | Web has no `beforeunload` | 2–3 |
| Reopen with no windows (Dock click) | — | ✅ | — | — | — | — | — | ❌ | `device macos-reopen`; headless can't drive it | 1 |
| Focus and active state | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Android `is_focused()` is always true | 2–4 |
| Visibility, minimize, occlusion | 🟡 | ✅ | ❌ | ✅ | ✅ | ❌ | 🟡 | ✅ | Windows sees minimize and restore only; winit never reports occlusion on Wayland; Android never dispatches visibility. Runs: macOS minimize and hide, the X11 cover window in `live-smoke`, iOS Home and return | 6–10 |
| Suspend, resume, GPU-surface loss | — | — | — | — | ✅ | 🟡 | ❌ | ✅ | Android never dispatches execution-state changes; Web ignores page freeze, `webglcontextlost` and device loss | 4–8 |
| Sleep, wake, screen lock | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Not in the contract | 3–5 |
| Session end (log off, shutdown) with veto | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | No backend handles it, and `flui-app` can't register the hook (`OwnerPlatform` doesn't expose it) | 4–6 |
| Memory pressure | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ | Not in the contract; Android only logs `LowMemory` | 3–5 |
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
| Minimize, maximize, restore, state readback | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | ✅ | No API for apps; no readback on macOS or Linux; Wayland never reports minimized. macOS ran minimize and hide | 3–5 |
| Fullscreen | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ | winit is borderless only; Web doesn't listen for `fullscreenchange`; mobile immersive mode is under System UI | 2–4 |
| Position, size, move and resize events | 🟡 | 🟡 | 🟡 | ❌ | — | — | 🟡 | ✅ | No `set_position` in the contract; winit `bounds()` and `resize` are missing; Wayland can't read or set a global position by design | 4–8 |
| Size constraints, resizable and other flags | ❌ | 🟡 | 🟡 | 🟡 | — | — | — | ✅ | Windows ignores min/max size and resizable (no `WM_GETMINMAXINFO`); no runtime setter anywhere | 3–5 |
| Live resize without jitter | ❔ | ❔ | ❔ | ❔ | — | — | ❔ | — | Not audited; Windows has no `WM_ENTERSIZEMOVE` handling | ❔ |
| Save and restore window placement | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Needs the position and state API above | 3–5 |
| Undecorated windows, custom title bar | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Undecorated Windows windows can't be dragged or resized; no caption hit-testing, drag regions, snap layouts or system window menu; compositors without server-side decorations need client-side ones | 12–20 |
| Backdrop material, transparency, corners | 🟡 | ❌ | ❌ | ❌ | — | — | — | ❌ | Windows forces Mica at creation; macOS Liquid Glass sits behind a `&mut` extension nothing can reach | 6–10 |
| Owned, modal and popup windows; always on top | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ | Not in the contract, so menus, tooltips and dropdowns can't leave the window; Wayland needs `xdg_popup` | 10–16 |
| Window icon | ❌ | — | ❌ | ❌ | — | — | — | ❌ | `WM_SETICON`, `xdg-toplevel-icon`, `_NET_WM_ICON` | 1–2 |
| Exclude a window from screen capture | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ | `SetWindowDisplayAffinity`, `NSWindow.sharingType`, iOS capture detection, Android `FLAG_SECURE` | 2–4 |
| Native views inside the UI (web view, map, video) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | Not in the contract; it shapes compositing, hit-testing and accessibility | 30–60 |

### Displays and metrics

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Displays: list, work area, current display, changes | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | ✅ | No change events anywhere; Windows `display()` always returns the primary; Android hard-codes 1080×2340 @ 2.75 | 6–10 |
| Scale factor and DPI changes | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ✅ | Web misses a change of device pixel ratio alone. Android run: emulator at density 420 | 2–4 |
| Refresh rate and frame pacing | ❌ | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | ✅ | Windows and Android report no frame period (60 Hz default); iOS doesn't report ProMotion | 4–6 |
| GPU surface | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — | Rendered in every run above | 0 |
| Safe area | — | ❌ | — | — | ✅ | 🟡 | ❌ | ✅ | macOS ignores the notch in fullscreen; Web doesn't read `env(safe-area-inset-*)`. iOS run: `device ios-safe-area-check` | 2–4 |
| Soft-keyboard insets | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| System-gesture exclusion | — | — | — | — | 🟡 | 🟡 | — | ✅ | `SystemGestureExclusion` → `PlatformWindow::set_input_regions`: Android `setSystemGestureExclusionRects` (API 29+), iOS deferred screen edges; neither ran on a device, and the iOS path was not type-checked on the authoring host | 1–2 |
| Display cutout, fold posture | — | — | — | — | — | 🟡 | ❌ | ❌ | Android folds the cutout into the safe area; no fold posture | 3–5 |

### Input

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Mouse: buttons, hover, capture | ✅ | ✅ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | ✅ | The iPad pointer has no hover or scroll. Runs: `device windows-input`, macOS CGEvent clicks, X11 `live-smoke`, the Web counter | 3–5 |
| Touch | 🟡 | — | 🟡 | 🟡 | ✅ | ✅ | 🟡 | ✅ | Runs: `device ios-input-check`, an Android emulator tap | 2–4 |
| Pen and stylus | 🟡 | ❌ | ❌ | ❌ | 🟡 | 🟡 | 🟡 | ✅ | macOS doesn't translate tablet events; winit has no pen | 8–12 |
| Precise scrolling with phases and momentum | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | Windows: a DirectManipulation viewport per window delivers the precision touchpad's pan as phased `Scroll` with `Momentum*` inertia (implemented; its init, messages and teardown ran on a real window, but no precision touchpad was attached, so no manipulation has run; ✅ needs `device windows-input` on one, T39). Wheel notches stay unphased; iOS has no trackpad scrolling; Web has no phases. winit and AppKit share one phase grammar, and a two-finger pan arrives as phased `Scroll` | 6–10 |
| Trackpad pinch and rotate | 🟡 | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | Windows decodes the precision touchpad's pinch from its DirectManipulation viewport (scale only, no rotation; never run on a touchpad); the Web does not decode Ctrl+wheel pinch; winit emits these on macOS only. `PanZoom` carries only scale and rotation about a focal point | 6–10 |
| Pointer lock and warp | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| Keyboard: keys, modifiers, repeat, dead keys | ✅ | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | iOS handles no hardware keyboard. Windows run: Tab and Enter | 4–6 |
| Keyboard layout and its changes | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Web hard-codes `en-US` | 4–6 |
| Cursor shapes | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | 🟡 | ✅ | No iPad pointer styles or Android `PointerIcon`; no custom image or hide-until-move | 3–5 |
| System back and history | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android predictive back (from targetSdk 36, `KEYCODE_BACK` isn't delivered), the iOS edge swipe, the Web History and Navigation APIs | 10–16 |
| Browser gesture suppression | — | — | — | — | — | — | ❌ | — | No `touch-action` or `overscroll-behavior` on the canvas | 1 |
| Input device availability | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Per-window `InputCapabilities` with `Availability` (native, emulated, unavailable) and the devices present, as an initial snapshot then ordered changes; headless and winit report presence changes, the other backends only their initial table so far; none ran on its environment | 0–1 |

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
| Dark mode and its changes | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | 🟡 | ❌ | Windows always reports Dark (FLUI forces the DWM dark flag, then reads it back); macOS and Linux always report Light; iOS and Android don't read it; Web has no change listener | 4–8 |
| Accent colour | ❌ | ❌ | ❌ | ❌ | — | ❌ | — | ❌ | Not in the contract | 2–4 |
| Text scale | 🟡 | — | ❌ | ❌ | 🟡 | 🟡 | ❌ | ❌ | iOS and Android go through native text sizing; Linux doesn't read `text-scaling-factor` | 4–6 |
| Reduce motion | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | Headless injects preferences through `HeadlessHost::set_system_preferences`; no reduce-motion row drives it yet | 1–2 |
| Contrast | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 3–5 |
| Bold text | — | — | — | — | 🟡 | 🟡 | — | ❌ | | 0–1 |
| Languages and regional formats | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Only Windows reads the languages; no regional formats (24-hour clock, first weekday) anywhere | 4–6 |
| Gesture and wheel settings | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | ❌ | ✅ | The Windows wheel smoke passed; the timings never ran. Presets per platform and pointer kind with accessibility floors, geometry per display; Android also reads the haptics setting; headless rows drive the presets and per-display geometry | 4–6 |
| Haptics setting | ❔ | ❔ | — | — | — | 🟡 | — | ✅ | `SystemPreferences::haptics()`, unobserved where the OS publishes nothing; Android reads `HAPTIC_FEEDBACK_ENABLED` (never ran on a device); iOS has no public read API; Windows pen and macOS Force Touch settings not audited | 1–2 |
| Power state (battery, low-power mode, thermal) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 3–5 |

### System UI

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Status and navigation bars, immersive mode | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android enforces edge-to-edge from targetSdk 35; Web `theme-color` | 6–10 |
| Orientation lock | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android 16 ignores it on large screens | 2–3 |
| Haptics | ❌ | 🟡 | — | — | 🟡 | 🟡 | 🟡 | ✅ | Both tiers, requests and outcomes and the frame queue (ADR-0210); every backend's degradation table runs as pure functions in CI. macOS `NSHapticFeedbackManager`, iOS UIKit generators and Core Haptics with engine restart, Android `performHapticFeedback` and `Vibrator` envelopes, compositions and waveforms, Web Vibration API: compiled, never run on a device. Windows: the pen `SimpleHapticsController` is T38, `InputHapticsManager` is in no published `windows` crate. Headless rows run the queue, the conformance rows and the tables | 4–6 |
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
| App folders and storage | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | ✅ | `FileStore` exists, but `flui-app` never wires it (`host_storage` returns `None`); Android `app_path` is fake; Web needs OPFS or IndexedDB | 6–10 |
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
| 2026-10-14 | minor | Interaction input stack ([spec](plans/specs/flui-interaction/completeness/tasks.md)): input availability per window, the shared scroll-phase grammar and scale-and-rotation `PanZoom`, presets per pointer kind with per-display geometry, headless preference injection, system-gesture exclusion on Android and iOS; new row: haptics setting |
| 2026-10-14 | minor | Two-tier haptics (ADR-0210, T16): requests and outcomes, the frame queue, degradation tables and conformance rows; native paths on macOS, iOS, Android and the Web compiled, never run on a device |
| 2026-10-14 | minor | Windows precision touchpad (DirectManipulation, T36): trackpad pinch ❌ to 🟡, precise-scroll note updated; Windows score 28% to 29%, overall still ~25%; no touchpad run yet, so no ✅ |
