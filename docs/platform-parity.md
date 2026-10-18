# Platform parity

> **Last reviewed:** 2026-10-18 · **Last updated:** 2026-10-18 · **Change:** minor (the interaction gallery on an Android phone) ·
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
| Windows | Win32 (native) | 5 | 36 | 34 | 16 | 3 | 31% | no |
| macOS | AppKit (native) | 9 | 22 | 43 | 17 | 3 | 27% | no |
| Wayland | winit (fallback) | 4 | 23 | 41 | 24 | 2 | 23% | no |
| X11 | winit (fallback) | 6 | 23 | 38 | 25 | 2 | 26% | no |
| iOS | UIKit (native) | 7 | 14 | 42 | 31 | 0 | 22% | no |
| Android | android-activity (native) | 9 | 21 | 36 | 29 | 0 | 30% | no |
| Web | wasm canvas (native) | 4 | 24 | 30 | 35 | 1 | 28% | no |
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
| Safe area | — | ❌ | — | — | ✅ | ✅ | ❌ | ✅ | macOS ignores the notch in fullscreen; Web doesn't read `env(safe-area-inset-*)`. iOS run: `device ios-safe-area-check`. Android run: `device android-gallery` on a Galaxy S21 (portrait only), where the insets reach `SafeArea` as the status and navigation bars (28.4 and 48 logical px at 2.8125) and the gallery's tab strip and screens lie clear of them and take taps | 2–4 |
| Soft-keyboard insets | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| System-gesture exclusion | — | — | — | — | 🟡 | ✅ | — | ✅ | `SystemGestureExclusion` → `PlatformWindow::set_input_regions`: Android `setSystemGestureExclusionRects` (API 29+), iOS deferred screen edges. Android run: `device android-input` on a Galaxy S21 (Android 15) reads the probe's left-edge exclusion back from the window manager's display region. The iOS path never ran and was not type-checked on the authoring host | 1–2 |
| Display cutout, fold posture | — | — | — | — | — | 🟡 | ❌ | ❌ | Android folds the cutout into the safe area; no fold posture | 3–5 |

### Input

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Mouse: buttons, hover, capture | ✅ | ✅ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | ✅ | The iPad pointer has no hover or scroll. Runs: `device windows-input` (`SendInput` clicks, a double click, a chord, a Control-held click and both wheels, recorded and replayed as one mouse pointer), macOS CGEvent clicks, X11 `live-smoke`, the Web counter, and `device web-input` (trusted clicks, a double click, a drag and a secondary click over WebDriver BiDi in headless Chrome, recorded and replayed). Android runs an `input mouse` click on the gesture pad and a wheel notch over the collapsing header (`device android-gallery`); no hardware mouse, hover or capture | 3–5 |
| Touch | 🟡 | — | 🟡 | 🟡 | ✅ | ✅ | 🟡 | ✅ | Runs: `device windows-input` (a synthetic touch from `InjectSyntheticPointerInput` into a real window, recorded and replayed), `device ios-input-check`, an Android emulator tap, `device android-input` on a Galaxy S21 (`adb shell input` taps and swipes, recorded and replayed), and `device web-input` (two trusted touches in headless Chrome, recorded and replayed); `device android-gallery` drives the interaction gallery's widgets on the same phone with a `uinput` multi-touch screen (swipes, flings, a pull, pages, the sheet, double taps, a spread, a pinch, long presses and a carried drag), each outcome read through the development agent and the screen's pixels; `device web-gallery` drove the same widgets in headless Chrome with trusted touch swipes, drags, long presses, double taps, spreads and pinches, which the canvas keeps from the browser's own panning (`touch-action: none`) | 2–4 |
| Pen and stylus | 🟡 | ❌ | ❌ | ❌ | 🟡 | 🟡 | 🟡 | ✅ | macOS doesn't translate tablet events; winit has no pen. Windows runs a synthetic pen (pressure and tilt) from `InjectSyntheticPointerInput` through `device windows-input`, recorded and replayed. Android runs `device android-input` on a Galaxy S21: `input stylus` taps and a `uinput` pen the system's `InputReader` reads, hovering, touching with pressure and tilt and leaving under one pointer identity, recorded and replayed. No hardware pen has run on either (the S21 takes no S Pen); `device android-gallery` taps the gesture pad with `input stylus` after a mouse hovered on the same injected device, which the pointer assembly now ends as a hover rather than refusing the pen; Android has no property revisions, so `Revise` never comes from it. The Web reads pressure, spherical angles or the tilt planes, twist and coalesced and predicted samples; constructed pen events and a trusted BiDi pen stroke (pressure, angles, twist, recorded and replayed) ran in headless Chrome, no hardware pen | 8–12 |
| Precise scrolling with phases and momentum | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | Windows: a DirectManipulation viewport per window delivers the precision touchpad's pan as phased `Scroll` with `Momentum*` inertia (implemented; on a Windows host its init, messages, pump, own frame request, pacing timer, size-and-move pause and teardown ran on a real window and viewport with synthetic readings, but no precision touchpad was attached, so no hardware manipulation has run; no injector reaches the touchpad, and the native capture journals its viewport calls, so a `native-capture` run on one records the golden that certifies it). Wheel notches stay unphased; iOS has no trackpad scrolling; Web has no phases. winit and AppKit share one phase grammar, and a two-finger pan arrives as phased `Scroll`. Android (API 34+) turns a touchpad's two-finger scroll gesture into phased `Scroll` from its cursor; `device android-input` ran it on a Galaxy S21 from a `uinput` touchpad, not a hardware one; Android sends no touchpad momentum | 6–10 |
| Trackpad pinch and rotate | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | 🟡 | ✅ | Windows decodes the precision touchpad's pinch from its DirectManipulation viewport (scale only, no rotation; never run on a touchpad); Android (API 34+) decodes a touchpad's pinch gesture as `PanZoom` scale about its cursor, ran by `device android-input` on a Galaxy S21 from a `uinput` touchpad, not a hardware one (Android has no touchpad rotation); the Web decodes the Control-held wheel events Chromium and Firefox send for a pinch (`Emulated`, classified by a sequence's first event, End at a deadline on the event timeline, refused in favour of the browser's zoom) and WebKit's `gesture*` events (scale and rotation): run in headless Chrome with constructed events through `cargo xtask device web-input`, never with a trackpad or in Safari; winit emits these on macOS only. `PanZoom` carries only scale and rotation about a focal point | 6–10 |
| Pointer lock and warp | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 4–6 |
| Keyboard: keys, modifiers, repeat, dead keys | ✅ | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | iOS handles no hardware keyboard. Windows run: Tab and Enter. Android run: `device android-gallery` sends hardware key events (`input keyevent`, `keycombination`): Tab and Shift+Tab, Enter, Space, Escape, F5, the arrows, Home, + and -, Ctrl+X and Ctrl+V (a character key looked up without Control, so the shortcut sees `x`), and the Menu key (named `ContextMenu`, which opens a row's menu); no repeat, dead key or physical keyboard | 4–6 |
| Keyboard layout and its changes | 🟡 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Web hard-codes `en-US` | 4–6 |
| Cursor shapes | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | 🟡 | ✅ | No iPad pointer styles or Android `PointerIcon`; no custom image or hide-until-move | 3–5 |
| System back and history | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android predictive back (from targetSdk 36, `KEYCODE_BACK` isn't delivered), the iOS edge swipe, the Web History and Navigation APIs | 10–16 |
| Browser gesture suppression | — | — | — | — | — | — | ❌ | — | No `touch-action` or `overscroll-behavior` on the canvas | 1 |
| Input device availability | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Per-window `InputCapabilities` with `Availability` (native, emulated, unavailable) and the devices present, as an initial snapshot then ordered changes; headless and winit report presence changes, the other backends only their initial table so far; none ran on its environment | 0–1 |

### Text input

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| IME composition and candidate placement | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | Windows TSF and macOS `NSTextInputClient` haven't run with a real input method; no `UITextInput`; Android serves the focused field through `FluiView`'s `InputConnection` (ADR-0237: a mirror the input method reads and edits on the Java main thread, its edits replayed into the field's store in order): `device android-input` on a Galaxy S21 (Android 15) tapped three letters on Samsung Keyboard, which composed them into the store, kept a hardware Escape during the composition the input method's, committed on space and sent Done as Enter, and `device android-gallery` typed four letters into the gallery's field and pressed Done while the keyboard still composed them: the editor action commits the composition before its Enter, so the field submits what the keyboard shows; its candidates sit in the keyboard's own strip, and `CursorAnchorInfo` placement never ran, nor did Gboard; the Web serves the focused field through an `EditContext` on the canvas, or a hidden textarea without one, with Escape during a composition left to the input method: run in headless Chrome through Chrome's own input-method path (`device web-input`), never with an OS input method or a soft keyboard | 30–50 |
| Soft keyboard show and hide | — | — | — | — | ❌ | ✅ | ❌ | ❌ | Android shows Samsung Keyboard when a field takes focus and hides it when none does: `device android-input` on a Galaxy S21 reads `mInputShown` from `dumpsys input_method` | 3–5 |
| Input purpose and hints (password, number, autocorrect) | ❌ | ❌ | ❌ | — | ❌ | 🟡 | ❌ | ❌ | Android maps the store's status to `EditorInfo` (a protected field is a password the keyboard does not learn from, a multi-line field's Enter breaks lines; only the plain single-line field ran on a device); otherwise not in the contract: `InputScope`, secure input, `text-input-v3` content type, `UITextInputTraits`, `EditorInfo`, `inputmode` | 6–10 |
| System edit menu | — | — | — | — | ❌ | ❌ | — | ❌ | `UIEditMenuInteraction` (its Paste avoids the iOS paste prompt), Android floating `ActionMode` | 4–6 |
| Spell check | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ | `ISpellChecker`, `NSSpellChecker`, `UITextChecker`, `SpellCheckerSession` | 6–10 |
| Autofill (passwords, one-time codes) | — | — | — | — | ❌ | ❌ | ❌ | ❌ | | 6–10 |
| Handwriting into text fields | ❌ | — | — | — | ❌ | 🟡 | — | ❌ | Windows Ink, iPad Scribble; Android asks `startStylusHandwriting` (API 33) for a pen tip that touches down inside the focused field (ADR-0237), never run: the S21 takes no S Pen, though its Samsung Keyboard reports stylus handwriting support | 6–10 |

### Accessibility

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Accessibility tree, actions, announcements | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | ❌ | ✅ | The desktop adapters sit behind the `a11y` feature, off by default; the runs read one control through UI Automation and AX, not a Narrator or VoiceOver session; announcements not audited; the Web needs its own ARIA DOM (AccessKit has no web adapter) | 30–50 |
| Gesture alternatives (custom actions, context menu, keys, controls) | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | Behavioural widgets publish each gesture's intent as a custom action, keys and a visible control or a context menu (ADR-0216), checked headless by `assert_gesture_alternatives`; no locked AccessKit adapter routes custom actions or `ShowContextMenu`, so on every platform assistive technology reaches them only through the controls; the Win32 and AppKit menu-key decoders pass the conformance row, no native run | 10–16 |

### Clipboard and drag and drop

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Plain-text clipboard | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ❌ | 🟡 | ✅ | Inert on pure Wayland without data-control; Android is an in-process mock; Web reads only its own writes | 6–10 |
| Rich clipboard (images, HTML, files) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | | 8–12 |
| Drop into the app | 🟡 | ❌ | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ | `flui-runtime` delivers OS drags to the receivers under the pointer and answers the OS with their feedback (ADR-0211); Windows registers an OLE `IDropTarget` per window (text, HTML, files, media types, the session identity), run here only within one process (`ole_drag_and_drop_on_real_windows`: OLE's calls on a real window, and a real `DoDragDrop` dropping into one), never from Explorer or another app; winit delivers file URIs only, never run there; the Web takes HTML drags on its canvas (types and files at hover, every representation copied out at the drop, `dragover` answered from the cached feedback), run in headless Chrome with constructed drag events and a trusted BiDi drag of a page element onto the canvas through `cargo xtask device web-input`, never from the OS or another page; Android takes drags on `FluiView`'s `onDragEvent` (ADR-0237: an offer minted from the `ClipDescription` on entry, the drop answered from the cached feedback, text, HTML and URIs copied out at the drop, a URI's bytes read through the content resolver when requested), run by `device android-input` on a Galaxy S21 only with a drag the probe started dropping on its own window, never from another app; headless plays the OS through `HeadlessDragAndDrop` (`drag_and_drop_reaches_the_runtime`) | 12–20 |
| Drag out of the app | 🟡 | ❌ | ❌ | ❌ | ❌ | 🟡 | 🟡 | ✅ | Sessions run in their window on pointer capture and escalate to an OS session (ADR-0211); over another window of their runtime they stay on the source's capture where the backend places its windows (`PlatformWindow::window_at`: Windows, run here on two real windows by `ole_drag_and_drop_on_real_windows`, the routing itself run headless by `drag_and_drop_reaches_the_runtime`), and escalate elsewhere; Windows starts the OS session with `DoDragDrop` on the owner window's next turn and keeps every window of the thread painting while the loop rests, run here in one process with synthetic input (the deferred start, a second window's frames, a drop into a FLUI window), never with a held mouse button, pen or touch, nor into another app; OLE's loop follows the mouse alone, so a drag no mouse button holds, or a pen or touch drag, is refused `Unavailable` and stays in its window (`a_drag_the_os_loop_cannot_carry_stays_in_its_window`, against the thread's key state); the Web starts an HTML drag at the browser's `dragstart` only for a session the runtime started on that press before the browser's drag threshold (a long-press trigger), storing every representation then, and keeps any later one in its window, run in headless Chrome with constructed events and a trusted BiDi drag out of the canvas onto a page element, never into another page or app; Android starts `FluiView.startDragAndDrop` (API 24) with the whole clip when a drag leaves its window (ADR-0237), and on a Galaxy S21 `device android-input` dropped one on the probe's own window (so the view surface is valid under `NativeActivity`'s `takeSurface`); a manual run carried one into Chrome in a pop-up window, which declined it, and the source heard `Cancelled`; no other app has taken one; the other native backends have no drag source, report outgoing drags `Unavailable` and keep a drag in its window; headless starts sessions through `with_os_drag`, and the widget harness's window plays them for `Draggable` (`a_drag_leaving_the_window_escalates_to_the_os_when_it_may_reach_it`) | 10–16 |

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
| Gesture and wheel settings | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | ❌ | ✅ | The Windows wheel smoke passed; the timings never ran. Presets per platform and pointer kind with accessibility floors, geometry per display; Android also reads the haptics setting; headless rows drive the presets and per-display geometry. Android run: `device android-input` on a Galaxy S21 changes the long-press timeout with `settings put` and reads it back from `SystemPreferences`, with the double-tap timeout, touch and double-tap slop and fling limits from `ViewConfiguration`; its wheel factors were not checked | 4–6 |
| Haptics setting | ❔ | ❔ | — | — | — | ✅ | — | ✅ | `SystemPreferences::haptics()`, unobserved where the OS publishes nothing; Android reads `HAPTIC_FEEDBACK_ENABLED`, run by `device android-input` on a Galaxy S21 (switched off and on with `settings put`); iOS has no public read API; Windows pen and macOS Force Touch settings not audited | 1–2 |
| Power state (battery, low-power mode, thermal) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | Not in the contract | 3–5 |

### System UI

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| Status and navigation bars, immersive mode | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android enforces edge-to-edge from targetSdk 35; Web `theme-color` | 6–10 |
| Orientation lock | — | — | — | — | ❌ | ❌ | ❌ | ❌ | Android 16 ignores it on large screens | 2–3 |
| Haptics | 🟡 | 🟡 | — | — | 🟡 | 🟡 | 🟡 | ✅ | Both tiers, requests and outcomes and the frame queue (ADR-0210); every backend's degradation table runs as pure functions in CI. macOS `NSHapticFeedbackManager`, iOS UIKit generators and Core Haptics with engine restart, Android envelopes (API 36) and compositions: compiled, never run on a device. The Web Vibration API ran in headless Chrome through `device web-input`: refused before the first trusted click, `Submitted` after it; no motor vibrated. Android `performHapticFeedback` and the `Vibrator` waveform ran on a Galaxy S21 (Android 15, amplitude control, no primitives) through `device android-input`: the feedback and a continuous pattern play as the probe's touch vibrations, a two-tap pattern falls back as the table says; patterns need the `VIBRATE` permission the CLI template declares, and fall back without it. Windows plays on the pen in range of the window through its `SimpleHapticsController`; a real window's actuator ran on a Windows host with no haptic pen (every request `Unavailable`), so no pen has vibrated yet. `InputHapticsManager` (touchpad, mouse) is in no published `windows` crate and stays `Unavailable`. Headless rows run the queue, the conformance rows and the tables | 4–6 |
| System sounds, alert beep | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ | | 2–3 |

### Menus

| Feature | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless | Notes | Hours |
|---|---|---|---|---|---|---|---|---|---|---|
| App menu bar | — | ❌ | — | — | ❌ | — | — | ❌ | A macOS app without one has no Quit or Edit menu; iPadOS 26 has a menu bar; elsewhere the app draws its own | 10–16 |
| Native context menus | ❌ | ❌ | — | — | — | — | — | ❌ | The mobile system edit menu is under Text input; the in-app, theme-free `ContextMenuArea` draws its own menu on every platform, run headless only | 6–10 |
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
| 2026-10-16 | minor | Windows precision touchpad (DirectManipulation, T36): trackpad pinch ❌ to 🟡, precise-scroll note updated; Windows score 28% to 29%, overall still ~25%; no touchpad run yet, so no ✅ |
| 2026-10-17 | minor | Windows pen haptics (ADR-0210): haptics ❌ to 🟡; Windows score stays 29%; no haptic pen has run, so no ✅ |
| 2026-10-17 | minor | Windows OLE drag and drop (ADR-0211): drop into the app and drag out of the app ❌ to 🟡; Windows score 29% to 31%, overall still ~25%; run here only within one process, so no ✅ |
| 2026-10-17 | minor | Drags between the windows of one runtime stay on the source's capture where the backend places its windows (Windows, headless); no status changes: the Windows query ran on real windows, the routing only headless |
| 2026-10-17 | minor | `device windows-input` records golden native recordings of injected touch, pen, mouse and wheel input (ADR-0215 §8); mouse, touch and pen notes name the run; symbols unchanged, and the precision touchpad still needs a hardware run |
| 2026-10-17 | minor | `Draggable` and `DropTarget` on the drag sessions (ADR-0211): drag out of the app notes the headless widget row; no status changes, no native run |
| 2026-10-17 | minor | `device android-input` on a Galaxy S21 (Android 15): system-gesture exclusion and the haptics setting 🟡 to ✅; trackpad pinch ❌ to 🟡 (a `uinput` touchpad, no hardware one); touch, pen, precise scrolling, gesture settings and haptics notes name the run; Android score 21% to 23%, overall still ~25%; Android drag and drop and IME stay ❌ (no Java side) |
| 2026-10-17 | minor | Web input in a real Chrome (`cargo xtask device web-input`): trackpad pinch ❌ to 🟡 (ctrl+wheel and WebKit gesture events), wheel defaults follow the dispatch result, pen tilt planes; Web score 24% to 25%; constructed events only, no trackpad, pen or Safari, so no ✅ |
| 2026-10-17 | minor | Web HTML drag and drop (ADR-0211 §11): drop into the app and drag out of the app ❌ to 🟡; Web score 25% to 27%; constructed drag events in headless Chrome, so no ✅ |
| 2026-10-17 | minor | `device web-input` drives the web backend over WebDriver BiDi with trusted input in headless Chrome: mouse, touch and pen recorded into the golden `web_bidi_mouse_pen_touch`, the Vibration API before and after user activation, and HTML drags in both directions; notes updated, no status changes (headless Chrome, no hardware) |
| 2026-10-17 | minor | Web text input (ADR-0090): IME composition ❌ to 🟡 (`EditContext`, a hidden textarea without it); Web score 27% to 28%; Chrome's input-method path in headless Chrome only, so no ✅ |
| 2026-10-17 | minor | Android `FluiView` (ADR-0237): soft keyboard ❌ to ✅ and IME composition ❌ to 🟡 (Samsung Keyboard composed, committed and kept Escape on a Galaxy S21; no candidate placement run), input hints and handwriting ❌ to 🟡 (never run on the device), drop into the app and drag out of the app ❌ to 🟡 (a drag dropped on its own window; none from or into another app); Android score 23% to 29% |
| 2026-10-18 | minor | `device web-gallery` drives every interaction gallery screen in headless Chrome with trusted mouse, wheel, key and touch input and Chrome's input method, reading outcomes from the semantics tree the page's development agent serves and from page captures; it found and fixed two Web defects (the canvas let the browser cancel moving touches; `request_redraw` ran frames inline, so an animation started from a click held the page's thread); notes updated, no status changes (headless Chrome, no hardware) |
| 2026-10-18 | minor | `device android-gallery` drives the interaction gallery on a Galaxy S21 (Android 15) through the development agent, 36 checks: safe area 🟡 to ✅ (portrait); touch, pen, mouse, keyboard and IME notes name the run; Android score 29% to 30%, overall still ~25% |
