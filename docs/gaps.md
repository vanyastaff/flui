# Where FLUI falls short

> [!NOTE]
> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-22 · **Change:** minor (entry 30, panic containment not gated, closed by `cargo xtask panic-boundary`, its Apple runs a bullet on entry 4, and entries 31–40 renumbered 30–39; entry 2: the desktop accessibility adapters are always on; previously minor: a bullet on entry 15, an Android touch held through Home; before that minor: entry 23, preferences polled on Android, closed and entries 24–41 renumbered 23–40; entry 14 retitled now that every host reads contrast and languages; previously minor: entries 39–41: add-to-app embedding, gamepad and D-pad, rotary input; before that minor: entries 32–38: live resize, present-mode and tearing control, launch handoff, quality-bar partials, power-aware pacing, the locale edge, accessibility bridge fidelity; a bullet each on entries 1, 14, 18, 24, 25, 26, 27, 28, 29 and 30; before that major: entries 23–31: the quality bar — polling, presentation feedback, HDR and colour, sandboxing, secure entry, latency modes, distribution, activation, panic gating)

The work list. One entry per shortfall, ranked by how much it stops someone from choosing FLUI for a
production app on that environment. Each entry says what is missing, the evidence, who it hurts and
the parity document that tracks it. Every entry blocks a ready platform
([`platform-parity.md`](platform-parity.md)). Pick from the top, check open pull requests first
(`gh pr list`), and when a gap closes, update this file and the parity document in the same pull
request.

## At a glance

| # | Gap | Environments | Who it hurts |
|--:|:--|:--|:--|
| 1 | [No text input on mobile and the Web](#1-no-text-input-on-mobile-and-the-web) | iOS, Android, Web | Every app with a text field |
| 2 | [No accessibility on mobile and the Web](#2-no-accessibility-on-mobile-and-the-web) | iOS, Android, Web; desktop | Blind and low-vision users; anyone who must meet accessibility law |
| 3 | [No system back](#3-no-system-back) | Android, iOS, Web | Every multi-screen mobile or web app |
| 4 | [Most implemented rows never ran on their environment](#4-most-implemented-rows-never-ran-on-their-environment) | all | Every claim of support |
| 5 | [Native Linux host (Wayland and X11)](#5-native-linux-host-wayland-and-x11) | Wayland, X11 | Linux users |
| 6 | [Clipboard is broken on three environments](#6-clipboard-is-broken-on-three-environments) | Android, Wayland, Web; all for rich | Every app with copy and paste |
| 7 | [File dialogs, open URL, reveal, trash, message box and share exist on Windows only, or nowhere](#7-file-dialogs-open-url-reveal-trash-message-box-and-share-exist-on-windows-only-or-nowhere) | all | Every document app; any app with a link |
| 8 | [No native drag and drop](#8-no-native-drag-and-drop) | all | Document, media and file-manager apps |
| 9 | [Window control has run only on Windows and an X11 test server](#9-window-control-has-run-only-on-windows-and-an-x11-test-server) | desktop | Every desktop app |
| 10 | [No custom title bar, popups or owned windows](#10-no-custom-title-bar-popups-or-owned-windows) | desktop | Apps with modern chrome; every menu and dropdown |
| 11 | [No native menus](#11-no-native-menus) | macOS, iPadOS; Windows | Every macOS app |
| 12 | [Lifecycle holes](#12-lifecycle-holes) | all | Apps that save state or hold resources |
| 13 | [No deep links, URL schemes, file associations or single instance](#13-no-deep-links-url-schemes-file-associations-or-single-instance) | all | Apps opened from links, files or a second launch |
| 14 | [System preferences missing or never run](#14-system-preferences-missing-or-never-run) | all | Users who rely on accessibility settings or another locale |
| 15 | [Pointer and keyboard gaps](#15-pointer-and-keyboard-gaps) | various | Pen, trackpad and iPad users |
| 16 | [Mobile system UI](#16-mobile-system-ui) | iOS, Android, Web | Every mobile app |
| 17 | [Desktop shell services](#17-desktop-shell-services) | desktop | Background, messaging and utility apps |
| 18 | [Text services](#18-text-services) | all | Writing apps; sign-in forms; tablet users |
| 19 | [No native views inside the UI (web view, map, video)](#19-no-native-views-inside-the-ui-web-view-map-video) | all | Apps that embed a map, browser or player |
| 20 | [Window looks](#20-window-looks) | desktop; mobile for capture | Apps with a native look; apps showing sensitive data |
| 21 | [Unaudited rows](#21-unaudited-rows) | desktop, Web | Unknown |
| 22 | [Version floors not enforced, and one GPU backend per environment](#22-version-floors-not-enforced-and-one-gpu-backend-per-environment) | all | Users on older systems or GPUs without Vulkan or WebGPU |
| 23 | [No presentation feedback](#23-no-presentation-feedback) | all | Every animated app; every latency claim |
| 24 | [No HDR, wide gamut or colour management](#24-no-hdr-wide-gamut-or-colour-management) | all | Photo, video and design apps; every app on an HDR display |
| 25 | [Sandbox-aware file access is absent](#25-sandbox-aware-file-access-is-absent) | all desktop and mobile | Every app distributed through a store or Flatpak |
| 26 | [Secure text entry and concealed clipboard entries are absent](#26-secure-text-entry-and-concealed-clipboard-entries-are-absent) | all | Every sign-in form; password managers |
| 27 | [Input latency modes and high-rate sampling are partial](#27-input-latency-modes-and-high-rate-sampling-are-partial) | all but Windows and the Web | Drawing, games and anything that tracks a finger or pen |
| 28 | [Distribution is unaudited](#28-distribution-is-unaudited) | all | Everyone who ships |
| 29 | [Activation ignores the OS focus rules](#29-activation-ignores-the-os-focus-rules) | Windows, Wayland | Apps brought forward by a link, a second launch or a notification |
| 30 | [Live resize is unaudited and unscheduled](#30-live-resize-is-unaudited-and-unscheduled) | desktop, Web | Every user who resizes a window by dragging its edge |
| 31 | [No present-mode, queue-depth, VRR or tearing control](#31-no-present-mode-queue-depth-vrr-or-tearing-control) | desktop, iOS, Android | Games, drawing and latency-sensitive apps |
| 32 | [Launch handoff on mobile and the Web](#32-launch-handoff-on-mobile-and-the-web) | iOS, Android, Web | Every mobile and web app in its first second |
| 33 | [Quality-bar partials with no entry](#33-quality-bar-partials-with-no-entry) | all | Everyone who relies on a platform claim; maintainers who cannot replay a failure |
| 34 | [Power-aware pacing and throttling](#34-power-aware-pacing-and-throttling) | all | Every user on battery or a thermally limited device |
| 35 | [Locale edge: layout direction, per-app language, system fonts and fallback](#35-locale-edge-layout-direction-per-app-language-system-fonts-and-fallback) | all | Apps in right-to-left or non-Latin languages; users who set a language for one app |
| 36 | [Accessibility bridge fidelity (properties and announcements)](#36-accessibility-bridge-fidelity-properties-and-announcements) | desktop, iOS, Android, Web | Screen-reader users |
| 37 | [Add-to-app: no FLUI view inside an existing native app](#37-add-to-app-no-flui-view-inside-an-existing-native-app) | all | Teams adding FLUI to an app they already ship |
| 38 | [No gamepad, and no D-pad run on a TV](#38-no-gamepad-and-no-d-pad-run-on-a-tv) | all | TV, console-style and Steam Deck users; games |
| 39 | [No rotary input](#39-no-rotary-input) | Windows, Android; Apple later | Surface Dial and Wear OS users |

## Entries

### 1. No text input on mobile and the Web

> **Environments:** iOS, Android, Web · **Who it hurts:** Every app with a text field · **Parity:** [Text input](platform-parity.md#text-input)

- No IME connection, soft keyboard, keyboard insets or input hints.
- **Missing on iOS:** no `UITextInput`.
- **Runs on Android:** it serves fields through `FluiView`'s `InputConnection` (ADR-0247), run with Samsung Keyboard on a Galaxy S21 (composition, commit, Escape, the action key, the keyboard shown and hidden, and through a manual Android run a field submitted by Done while the keyboard still composed).
- **Missing on Android:** never run with Gboard, `CursorAnchorInfo` placement, keyboard insets, paste through `OnReceiveContentListener` or stylus handwriting.
- **Runs on the Web:** it now serves fields through `EditContext` or a hidden textarea, run only through Chrome's own input-method path in headless Chrome.
- **Missing on the Web:** no soft keyboard, insets or input hints.
- **Missing everywhere:** client queries (selected range, surrounding text, first rect) for dictation, reconversion and handwriting, and a session that survives a window move, resize, DPI change and focus loss, are rows of their own under [Text input](platform-parity.md#text-input). Other frameworks show the failures: macOS dictation silently drops text when `selectedRange` returns `NSNotFound` ([rust-windowing/winit#4666](https://github.com/rust-windowing/winit/issues/4666), [#3617](https://github.com/rust-windowing/winit/issues/3617)), a Korean input method stops editing inline after the window moves ([DioxusLabs/dioxus#5246](https://github.com/DioxusLabs/dioxus/issues/5246)), and the input-method output of egui carries a position only ([emilk/egui#7941](https://github.com/emilk/egui/issues/7941)).

### 2. No accessibility on mobile and the Web

> **Environments:** iOS, Android, Web; desktop · **Who it hurts:** Blind and low-vision users; anyone who must meet accessibility law · **Parity:** [Accessibility](platform-parity.md#accessibility)

- Desktop is always on but untested with a screen reader.
- AccessKit has iOS and Android adapters but none for the Web (it needs a parallel ARIA DOM).
- When they are wired, the semantics identifier (ADR-0240) must reach Android's `viewIdResourceName`, iOS's `accessibilityIdentifier` and a `data-flui-id` DOM attribute, as it reaches `AutomationId`, `AXIdentifier` and AT-SPI's id through `author_id` on the desktop.
- The runs read one control, not a Narrator, VoiceOver or Orca session.
- No locked adapter routes custom actions or `ShowContextMenu`, so gesture alternatives (ADR-0216) keep a control on every path.

### 3. No system back

> **Environments:** Android, iOS, Web · **Who it hurts:** Every multi-screen mobile or web app · **Parity:** [Input](platform-parity.md#input)

- From targetSdk 36 Android delivers neither `onBackPressed` nor `KEYCODE_BACK`, so only predictive back works.
- iOS needs its own edge swipe.
- The Web needs History or Navigation API routing.

### 4. Most implemented rows never ran on their environment

> **Environments:** all · **Who it hurts:** Every claim of support · **Parity:** [Summary](platform-parity.md#summary)

- 🟡 dominates every column of the parity summary.
- Device checks exist for a handful of rows, and the macOS and iOS drivers are Python and Swift.
- Display runs still owed:
  - the macOS and iOS display sources (type-checked, never run)
  - the macOS and iOS window lifecycle (window state and commands, sleep, lock, session end, memory pressure; [#1736](https://github.com/vanyastaff/flui/issues/1736))
  - a window moved between two monitors on every host but Windows
  - unplugging a monitor on Windows
- The panic boundary's Apple entries (AppKit and UIKit methods, blocks, GCD work items) are
  type-checked only: no panic has been seen contained on a Mac or an iPhone
  ([#1840](https://github.com/vanyastaff/flui/issues/1840)).
- A macOS display whose current mode reports 0 Hz (some built-in panels) has no period and paces at the 60 Hz default, because the `CVDisplayLink` fallback is not written.

### 5. Native Linux host (Wayland and X11)

> **Environments:** Wayland, X11 · **Who it hurts:** Linux users · **Parity:** [Summary](platform-parity.md#summary)

- It replaces the winit fallback.
- winit leaves the clipboard inert on pure Wayland, reports no occlusion on Wayland, has no pen, no pinch and no window bounds.
- winit reports no event when a window crosses to another Wayland output of equal scale (the `wl_surface.enter` events belong to its connection).
- FLUI asks every Wayland window's monitor again when the loop goes idle, never run with a window moved between two outputs.
- Under Hyprland (Omarchy's compositor) the host takes the decoration the compositor chooses through `zxdg_decoration_manager_v1` (no title bar of its own), presents an alpha surface so the compositor's blur applies, and scales through `wp_fractional_scale_v1` and `wp_viewporter`; none of it exists, and winit draws its own decorations there.

### 6. Clipboard is broken on three environments

> **Environments:** Android, Wayland, Web; all for rich · **Who it hurts:** Every app with copy and paste · **Parity:** [Clipboard](platform-parity.md#clipboard-and-drag-and-drop)

- No rich formats.
- Android is an in-process mock.
- Pure Wayland without data-control is inert.
- Web reads only its own writes.

### 7. File dialogs, open URL, reveal, trash, message box and share exist on Windows only, or nowhere

> **Environments:** all · **Who it hurts:** Every document app; any app with a link · **Parity:** [System services](platform-parity.md#system-services)

- Every non-Windows host answers a file dialog with "cancelled".
- `open_url` is real on Windows and Web only, and nothing calls it.

### 8. No native drag and drop

> **Environments:** all · **Who it hurts:** Document, media and file-manager apps · **Parity:** [Clipboard and drag and drop](platform-parity.md#clipboard-and-drag-and-drop)

- The core runs: in-window sessions, sessions that reach the other windows of their runtime on the source's capture where the host places its windows, escalation to an OS session, identity and delivery to receivers in `flui-runtime`, exercised headless (ADR-0211). The `Draggable` and `DropTarget` widgets run on it headless, with the cursor, Escape and the keyboard and assistive cut, copy and paste.
- Windows has OLE as both target and source (`IDropTarget` per window, `DoDragDrop` started on the owner window's next turn, frames for every window while its loop rests), run only within one process (`ole_drag_and_drop_on_real_windows`), never with Explorer, another app, a held mouse button, pen or touch.
- The Web takes and starts HTML drags (a drag starts only from a press the runtime lifted before the browser's threshold), run in headless Chrome with constructed events.
- Android takes and starts drags on `FluiView` (ADR-0247): on a Galaxy S21 a drag the probe started dropped its text on its own window, and one crossed into Chrome, which declined it; no drag from or into another app has completed.
- **Still missing:** `NSDraggingDestination` and every other native drag source and target.
- Planned in the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (ADR-0211; T28–T31, T35, T37 and the per-OS tickets).

### 9. Window control has run only on Windows and an X11 test server

> **Environments:** desktop · **Who it hurts:** Every desktop app · **Parity:** [Windows](platform-parity.md#windows)

- **Runs:** title, mode, fullscreen, position, size and constraint commands with a state readback ship on every desktop host ([ADR-0237](adr/ADR-0237-window-state-and-host-notices.md)).
- **Missing:** macOS carries them out and reads them back type-checked only, never run ([#1736](https://github.com/vanyastaff/flui/issues/1736)).
- X11 ran only on an Xvfb server with the test standing in for the window manager.
- winit cannot read a title back on Linux, so the snapshot holds the one last set.
- Wayland never reports minimized.
- Placement restore ran only on Windows; live resize is not audited.

### 10. No custom title bar, popups or owned windows

> **Environments:** desktop · **Who it hurts:** Apps with modern chrome; every menu and dropdown · **Parity:** [Windows](platform-parity.md#windows)

- Undecorated Windows windows can't be dragged or resized.
- No caption hit-testing or snap layouts.
- Menus, tooltips and dropdowns can't leave the window.

### 11. No native menus

> **Environments:** macOS, iPadOS; Windows · **Who it hurts:** Every macOS app · **Parity:** [Menus](platform-parity.md#menus)

- No macOS menu bar (no Quit or Edit menu).
- No native context menus, no Dock menu or jump list.

### 12. Lifecycle holes

> **Environments:** all · **Who it hurts:** Apps that save state or hold resources · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- Session end is delivered on Windows (checked with sent messages), macOS, iOS, Android, the Web and Linux (there as a notification).
- The Web reports page freeze, context loss and page hide only against a fake page, not yet from Chrome.
- Sleep, wake and lock reach Windows, macOS and Linux (Linux checked only against a stand-in for logind), and memory pressure Windows, macOS, iOS, Android and Linux.

### 13. No deep links, URL schemes, file associations or single instance

> **Environments:** all · **Who it hurts:** Apps opened from links, files or a second launch · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- `on_open_urls` is never called.
- Packaging (app identity) not audited.

### 14. System preferences missing or never run

> **Environments:** all · **Who it hurts:** Users who rely on accessibility settings or another locale · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Missing: regional formats everywhere, text scale on the Web and on Linux desktops other than GNOME, accent colour, gesture timings.
- Never run: every host reads contrast and the preferred languages, but macOS and iOS are type-checked only, the Web ran on Node rather than in a browser, and Linux ran against a stand-in portal rather than a desktop session.
- Windows text scale is applied linearly. WinUI scales larger text less by an undocumented curve, so large text grows more than in a WinUI control at the same setting.
- Gesture presets per platform and pointer kind, accessibility floors, per-display geometry and headless injection landed with the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (ADR-0212).
- Layout direction, per-app language, and system fonts and fallback are separate rows, tracked by [entry 35](#35-locale-edge-layout-direction-per-app-language-system-fonts-and-fallback).

### 15. Pointer and keyboard gaps

> **Environments:** various · **Who it hurts:** Pen, trackpad and iPad users · **Parity:** [Input](platform-parity.md#input)

- **Missing:** no pen on macOS or Linux; iPad pointer and hardware keyboard missing; no cursors on mobile; keyboard layout read on Windows only.
- Windows: the precision-touchpad pan and pinch (DirectManipulation) have never run on a touchpad, and no injector reaches it, so its golden recording can only come from a `native-capture` run on one (injected touch, pen, mouse and wheel already record theirs through a manual Windows run).
- Android: touchpad pinch and two-finger scroll and its pen hover and tilt ran only from `uinput` devices through a manual Android run, with no hardware touchpad or stylus. Its widgets ran with `uinput` touches, `input` stylus and mouse events and hardware key events through a manual Android run, with no physical keyboard, key repeat or dead key.
- Android: a touch held while the app goes to the background makes the system report it not responding ten seconds later. The loop reads its input queue only while the Activity is resumed, so the cancel the system sends for the window it took away is never answered (seen on a Galaxy S21 by holding a touch and pressing Home).
- Web: no precise-scroll phases, and its trackpad pinch (decoded from Control-held wheel and WebKit gesture events) ran only with constructed events in headless Chrome.
- The winit X11 and Wayland pointer rules (X11's first-touch cursor and fractional touchpad lines, a Wayland leave mid-press) ran only as unit rows, never on a native session with a touchscreen or touchpad.
- Pointer, scroll and pinch work is planned per OS in the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (T36 DirectManipulation is implemented and awaits a touchpad run; T40–T44).

### 16. Mobile system UI

> **Environments:** iOS, Android, Web · **Who it hurts:** Every mobile app · **Parity:** [System UI](platform-parity.md#system-ui)

- Status-bar icon style ran on Android, iOS is type-checked only.
- **Missing:** no navigation-bar or immersive control (Android enforces edge-to-edge from targetSdk 35); no orientation lock; no safe area on the Web or for the macOS notch.
- Haptics has both tiers and the frame queue (ADR-0210), but:
  - its macOS and iOS paths never ran on a device
  - the Web's ran only in headless Chrome (activation refused, then `Submitted`, no motor)
  - Android ran only feedback and amplitude waveforms (on a Galaxy S21 without primitives; envelopes and compositions never ran)
  - Windows pen haptics ran only on a host with no haptic pen, while Windows touchpad and mouse haptics (`InputHapticsManager`) wait for a `windows` crate that ships it ([interaction spec](plans/specs/flui-interaction/completeness/tasks.md))
- System gesture exclusion is implemented on Android and iOS; it ran on an Android device, never on iOS.

### 17. Desktop shell services

> **Environments:** desktop · **Who it hurts:** Background, messaging and utility apps · **Parity:** [System services](platform-parity.md#system-services)

- No tray, notifications, taskbar progress or badge, global shortcuts, launch at login, keep-awake or secure storage.

### 18. Text services

> **Environments:** all · **Who it hurts:** Writing apps; sign-in forms; tablet users · **Parity:** [Text input](platform-parity.md#text-input)

- No spell check, system edit menu, autofill or handwriting.
- Handwriting, dictation and reconversion also need the text-input client queries and the session that survives a window move, resize, DPI change and focus loss; those rows sit under [Text input](platform-parity.md#text-input) and are tracked with [entry 1](#1-no-text-input-on-mobile-and-the-web).

### 19. No native views inside the UI (web view, map, video)

> **Environments:** all · **Who it hurts:** Apps that embed a map, browser or player · **Parity:** [Windows](platform-parity.md#windows)

- Not in the contract; the design shapes compositing, hit-testing and accessibility.
- Needs an owner decision.

### 20. Window looks

> **Environments:** desktop; mobile for capture · **Who it hurts:** Apps with a native look; apps showing sensitive data · **Parity:** [Windows](platform-parity.md#windows)

- Backdrop materials, transparency, icon, capture exclusion.
- Windows forces Mica; macOS Liquid Glass unreachable; no window icon on Windows or Linux.

### 21. Unaudited rows

> **Environments:** desktop, Web · **Who it hurts:** Unknown · **Parity:** [Windows](platform-parity.md#windows)

- App identity and packaging, live resize.

### 22. Version floors not enforced, and one GPU backend per environment

> **Environments:** all · **Who it hurts:** Users on older systems or GPUs without Vulkan or WebGPU, who get a failure in the middle of start instead of a clear refusal · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- The floors are declared in `platform-support.toml` ([version map](version-map.md)), but only the Android `minSdk` is packaged from it and no host checks them at start.
- Android packages API 23 while rendering only through Vulkan (API 24).
- iOS and macOS bundles inherit whatever deployment target the build had.
- Windows links a 1607 API unconditionally; the Linux release glibc is the runner's.
- No GLES or WebGL2 fallback is compiled (ADR-0244).

### 23. No presentation feedback

> **Environments:** all · **Who it hurts:** Every animated app; every latency claim · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- Nothing reports when a frame reached the screen.
- No DXGI frame statistics, `addPresentedHandler`, `wp_presentation_time`, frame timelines or `requestAnimationFrame` timestamps feed the frame clock, so animations schedule blind and input-to-photon latency cannot be measured or regressed.
- A Web `requestAnimationFrame` timestamp is the end of the previous frame's rendering, not a presentation time, so the Web cell cannot reach ✅ through it.

### 24. No HDR, wide gamut or colour management

> **Environments:** all · **Who it hurts:** Photo, video and design apps; every app on an HDR display · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- The engine requests a surface colour capability per window (`AppConfig::surface_color`, [ADR-0256](adr/ADR-0256-surface-colour-is-requested-and-converted-at-presentation.md)) and converts to Display P3, scRGB, extended sRGB or HDR10 at presentation; no run on an HDR or P3 display backs it yet, and no Metal space has run.
- The platform layer has no headroom query or display-profile change; headroom comes from wgpu when the surface is configured.
- Sources are still sRGB: images with a profile, video and HDR images enter the working space as sRGB values.
- Android has public headroom APIs (`Display.getHdrSdrRatio`, API 34, with a listener for changes); the Web reports capability only and cannot observe headroom. HDR and SDR-white change events are a parity row of their own.

### 25. Sandbox-aware file access is absent

> **Environments:** all desktop and mobile · **Who it hurts:** Every app distributed through a store or Flatpak · **Parity:** [System services](platform-parity.md#system-services)

- No security-scoped bookmarks, portals, AppContainer-aware paths, Storage Access Framework or document-scoped URLs.
- `data_dirs` and the dialogs assume an unsandboxed process, so an App Store, Flatpak or MSIX build gets paths it cannot open.
- The manifests are part of the same gap: nothing generates macOS entitlements, Flatpak `finish-args`, Snap interfaces or MSIX capabilities from declared capabilities (the parity row "Sandbox and entitlement manifests generated from declared capabilities"). Tauri's autostart plugin shows the cost: it writes a launcher path that cannot run outside the Flatpak sandbox ([tauri-apps/plugins-workspace#3166](https://github.com/tauri-apps/plugins-workspace/issues/3166)).

### 26. Secure text entry and concealed clipboard entries are absent

> **Environments:** all · **Who it hurts:** Every sign-in form; password managers · **Parity:** [Text input](platform-parity.md#text-input)

- No `EnableSecureEventInput`, `isSecureTextEntry`, `IME_FLAG_NO_PERSONALIZED_LEARNING`, clipboard-history exclusion or sensitive clip flags.
- A password typed in FLUI reaches input-method learning and the clipboard manager.
- Android's `ClipDescription.EXTRA_IS_SENSITIVE` (API 33) is only a rendering hint, so the Android cell of the concealed-clipboard row can be at most partial by design.
- Platform-edge diagnostics have no redaction check: protected-field text, clipboard payloads and URLs with tokens must never reach the `tracing` targets or the `native-capture` journal, which will carry text and URLs once more event families are recorded (the [OWASP Logging Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html) lists them under "Data to exclude").

### 27. Input latency modes and high-rate sampling are partial

> **Environments:** all but Windows and the Web · **Who it hurts:** Drawing, games and anything that tracks a finger or pen · **Parity:** [Input](platform-parity.md#input)

- `InputLatency` is set by nobody.
- Android cannot honour it on `NativeActivity`.
- iOS coalesced and predicted touches and the Linux and macOS paths are not audited.
- Timestamp conversion is a hazard of its own: Win32 `GetMessageTime` is 32-bit milliseconds that wrap every 49.7 days, Wayland event times have an undefined base, and Android's event time is uptime milliseconds. Windows pointer ticks are rebased across the wrap (`NativeTime::TickMillis`); the Wayland, X11, Android and Web conversions are not audited.

### 28. Distribution is unaudited

> **Environments:** all · **Who it hurts:** Everyone who ships · **Parity:** [Distribution](platform-parity.md#distribution)

- Packages, signing, notarisation, identity.
- `flui build` stages unsigned `.app`s and a debug APK; no MSIX, Flatpak, AAB, notarisation or Authenticode.
- The identity that notifications, URL schemes, portals and jump lists need is not checked into the package.
- Entitlement and sandbox manifests (macOS and iOS entitlements, Flatpak `finish-args`, Snap interfaces, MSIX capabilities, Android permissions) are not generated from declared capabilities; the Android template declares `VIBRATE` itself.

### 29. Activation ignores the OS focus rules

> **Environments:** Windows, Wayland · **Who it hurts:** Apps brought forward by a link, a second launch or a notification · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- No `xdg-activation` token.
- Windows re-foregrounds whatever is already in front.
- macOS cooperative activation (`NSApplication.yieldActivation(to:)`, macOS 14) is documented by Apple, which also marks `activate(ignoringOtherApps:)` deprecated; neither is audited.

### 30. Live resize is unaudited and unscheduled

> **Environments:** desktop, Web · **Who it hurts:** Every user who resizes a window by dragging its edge · **Parity:** [Windows](platform-parity.md#windows)

- No environment has been audited: the row "Live resize without jitter" is ❔ on Windows, macOS, Wayland, X11 and the Web, and entries 9 and 21 mention it only in passing.
- Windows has no `WM_ENTERSIZEMOVE` handling outside the DirectManipulation tests. That message opens a modal loop inside `DefWindowProc` that lasts until the drag ends ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-entersizemove)); winit reports that its `ControlFlow` is ignored there and that continuous redraws make a drag choppy ([rust-windowing/winit#3272](https://github.com/rust-windowing/winit/issues/3272), [#4708](https://github.com/rust-windowing/winit/issues/4708)).
- Metal presents asynchronously to Core Animation transactions unless `CAMetalLayer.presentsWithTransaction` is set ([Apple](https://developer.apple.com/documentation/quartzcore/cametallayer/presentswithtransaction)); Wayland needs an `ack_configure` before the committing frame and frame callbacks to pace the redraw; X11 has `_NET_WM_SYNC_REQUEST`. None of these is audited here.
- The engine presents with `Fifo`, which blocks on vsync and ghosts a stale frame during the Windows modal resize loop; its own notes name `DXGI_SCALING_NONE` as the only real fix, which wgpu 30 does not expose. A resize-jitter probe saw no size-mismatched acquire in a 40-resize burst into the Metal swapchain; the compositor-side half is unobserved.
- It is among the most-reported defects of comparable frameworks ([emilk/egui#903](https://github.com/emilk/egui/issues/903) is a fast-resize flicker). A reference run should record its trade-off in the row note: Flutter's Windows embedder blocks the platform thread for up to 100 ms for a frame at the new size.
- Pass criteria: a scripted two-second border drag with every presented frame captured, content size equal to window size in every captured frame, callbacks counted during the modal loop, and the p99 frame time recorded.

### 31. No present-mode, queue-depth, VRR or tearing control

> **Environments:** desktop, iOS, Android · **Who it hurts:** Games, drawing and latency-sensitive apps · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- The engine always presents with `Fifo` and a frame latency of 2; nothing lets the platform layer or the app choose a present mode, a queue depth, variable-refresh behaviour or tearing.
- Presentation feedback (entry 23) is read-only and "Refresh rate and frame pacing" covers rates only; control over how frames are presented is the other half.
- Windows tearing needs `DXGI_FEATURE_PRESENT_ALLOW_TEARING` and `DXGI_PRESENT_ALLOW_TEARING`, which can only be used with sync interval 0 ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/variable-refresh-rate-displays)); Wayland has `wp_tearing_control_v1`, `fifo-v1` and `commit-timing-v1`.
- Queue depth costs latency: the default DXGI queue of up to three frames adds about 16–32 ms of input latency in another framework ([libsdl-org/SDL#15656](https://github.com/libsdl-org/SDL/issues/15656)).
- The Web has nothing to control: `requestAnimationFrame` is its only cadence.

### 32. Launch handoff on mobile and the Web

> **Environments:** iOS, Android, Web · **Who it hurts:** Every mobile and web app in its first second · **Parity:** [Windows](platform-parity.md#windows)

- The OS shows a splash before the first frame on these environments, and the first frame must match it: Android 12 and later through the `SplashScreen` API ([Android documentation](https://developer.android.com/develop/ui/views/launch/splash-screen)), the iOS launch screen, and the page background before a Web app's wasm loads.
- Nothing aligns them today: the generated Web page sets no background before the wasm loads and its manifest `theme_color` is `#FFFFFF`, the generated Android manifest sets no splash or window background, and iOS declares an empty `UILaunchScreen`.
- The parity row "Show without a blank first frame" covers the desktop only, where the window is created hidden and shown after the first presented frame; Electron does the same with a hidden window shown on `ready-to-show` and a `backgroundColor`.
- A dark-mode user should see the dark splash and the dark first frame, not a white flash between them.

### 33. Quality-bar partials with no entry

> **Environments:** all · **Who it hurts:** Everyone who relies on a platform claim; maintainers who cannot replay a failure · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- Honest absence: nine spellings of "not here" exist today, and the Android clipboard is an in-process mock (ADR-0152, ADR-0254). Other layers show what silent spellings cost: winit setters return `()` while their documentation lists platforms as unsupported, `tauri-plugin-geolocation` resolves to a zeroed position on the desktop, and `tauri-plugin-notification` reports a permission it did not check.
- Replay: pointer, key and wheel traces replay on the headless host; IME, lifecycle, preference and display recordings do not exist.
- The `native-capture` journal records pointers only, so a failure in any other event family cannot be replayed.
- Not every unsupported cell in the parity tables names its reason, and nothing checks the cells against the code's `Unsupported` reasons.

### 34. Power-aware pacing and throttling

> **Environments:** all · **Who it hurts:** Every user on battery or a thermally limited device · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Power state (battery, low-power mode, thermal) is read on no environment, and nothing acts on it: no row named a frame cap or a background pause before.
- The native signals exist: Apple's `ProcessInfo.thermalState` and `isLowPowerModeEnabled` with a change notification, Android's `PowerManager.getCurrentThermalStatus()` and `addThermalStatusListener`, Windows battery saver through `SYSTEM_POWER_STATUS.SystemStatusFlag` and `RegisterPowerSettingNotification`, and the Linux portal's `PowerProfileMonitor.power-saver-enabled`.
- Apple's `CADisplayLink.preferredFrameRateRange` is only a hint that Low Power Mode, a critical thermal state and accessibility settings can override, so an app that asks for 120 Hz still has to follow the state.
- The check forces each state and asserts a delivery within one event-loop turn and a frame-rate or work reduction.

### 35. Locale edge: layout direction, per-app language, system fonts and fallback

> **Environments:** all · **Who it hurts:** Apps in right-to-left or non-Latin languages; users who set a language for one app · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Entry 14 names languages and regional formats only. Layout direction, per-app language, and system fonts and fallback have no reading in the platform layer.
- Android 13 adds per-app language preferences and `setApplicationLocales` ([Android documentation](https://developer.android.com/guide/topics/resources/app-languages)); iOS has a per-app language setting; a right-to-left layout follows from the locale.
- Text layout resolves fonts through `fontdb` in `flui-painting`; DirectWrite `IDWriteFontFallback`, Android `SystemFonts` (API 29), CoreText and fontconfig are the platform answers to which font draws a character the requested family lacks.
- The check is a locale matrix (en-US, ar-EG for right-to-left, ja-JP, hi-IN with its numbering) on every OS: list order, first weekday, 12 or 24 hour clock, per-app override, and CJK and emoji text without missing-glyph boxes.

### 36. Accessibility bridge fidelity (properties and announcements)

> **Environments:** desktop, iOS, Android, Web · **Who it hurts:** Screen-reader users: a control announced without its value, hint or state, or a change nobody hears · **Parity:** [Accessibility](platform-parity.md#accessibility)

- Entry 2 is about missing adapters; a bridge that exists can still drop properties or announcements, and the two fail independently of the tree and actions.
- Another framework's Linux bridge drops a field's `value` and `hint` ([flutter/flutter#194638](https://github.com/flutter/flutter/issues/194638)) and gives no AT-SPI announcement for live regions, snack bars, alert dialogs or route changes, only for an explicit announcement call ([flutter/flutter#194045](https://github.com/flutter/flutter/issues/194045)).
- The parity rows "Properties (value, hint, description, state)" and "Announcements and live regions" are ❔ on the desktop adapters and ❌ where there is no adapter at all.
- Each needs a check with the real assistive technology on each OS that reads role, name, value, hint, state, actions, focus order and announcements; a harness that registers AT-SPI listeners and matches only strings carrying a nonce keeps any other source from producing a hit.

### 37. Add-to-app: no FLUI view inside an existing native app

> **Environments:** all · **Who it hurts:** Teams adding FLUI to an app they already ship · **Parity:** [Windows](platform-parity.md#windows)

- FLUI owns the process: the host creates the window and runs the loop (`NativeActivity` today and `FluiActivity` after ADR-0255 on Android, `UIApplicationMain` on iOS, the Win32 pump, `NSApplication`, the root element it creates on the Web). A view placed into a window the app already owns — a child `HWND`, an `NSView`, a `UIView` under a foreign view controller, an Android `View` in a foreign `Activity`, an element of a page the app renders — has no host mode, so the only way in is to hand FLUI the whole window.
- What exists: the renderer draws into a `SurfaceTarget` (`HasWindowHandle + HasDisplayHandle`, [ADR-0254](adr/ADR-0254-window-contract.md)), so the surface can be foreign; input, focus, IME, accessibility and lifecycle still assume the host's own window procedure, responder chain or `Activity`.
- Other frameworks ship it as their enterprise path: Flutter's add-to-app ([Flutter documentation](https://docs.flutter.dev/add-to-app)) with `FlutterEngine` and `FlutterViewController` or `FlutterFragment`, Compose's `ComposeView` in a View hierarchy, SwiftUI's `UIHostingController`, Qt's `QWidget::createWindowContainer`.
- The design question is who owns the loop: an embedded view is driven by the app's loop and receives events from the app's window, so the owner thread's one call point for user code ([ADR-0151 §10](adr/ADR-0151-platform-layer-boundary-and-names.md)) has to run inside a foreign callback, and ADR-0151 names an external embedder as the first reason to reopen the crate split.
- The check is one screen of FLUI inside a foreign app on each OS: keyboard focus moving into and out of the view, IME and the assistive technology reaching it, and the app's own lifecycle pausing and resuming it.

### 38. No gamepad, and no D-pad run on a TV

> **Environments:** all · **Who it hurts:** TV, console-style and Steam Deck users; games · **Parity:** [Input](platform-parity.md#input)

- No gamepad in the contract or on any host: no buttons, sticks, triggers or rumble from XInput or GameInput on Windows, `GCController` on macOS and iOS (and tvOS's remote), Android's `SOURCE_GAMEPAD` and `SOURCE_JOYSTICK` axes (`AXIS_X`, `AXIS_HAT_X`, `KEYCODE_BUTTON_*`), the Web Gamepad API (polled through `navigator.getGamepads()`, with only connect and disconnect events) or evdev on Linux (Wayland has no gamepad protocol; a Steam Deck under gamescope has a controller as its only input).
- Android's D-pad arrives: the `ui_events` key table maps `KEYCODE_DPAD_UP`, `DOWN`, `LEFT` and `RIGHT` to the arrow keys and `KEYCODE_DPAD_CENTER` to Enter, and `DefaultFocusTraversal` binds unmodified arrows to `DirectionalFocusIntent`, so a remote can move focus; it never ran on Android TV or Fire OS, and a TV app also needs the `leanback` feature declaration and a banner that the Android packaging does not write.
- Without a pointer (Android TV, tvOS, a console) focus navigation is the whole input model: every widget must be reachable by direction with a visible focus ring, which no run has shown.
- The check is a remote on an Android TV device and a controller on Windows and a Steam Deck: every control of the interaction widgets reached by direction and activated by the centre button, and a controller's sticks and buttons recorded and replayed.

### 39. No rotary input

> **Environments:** Windows, Android; Apple later · **Who it hurts:** Surface Dial and Wear OS users · **Parity:** [Input](platform-parity.md#input)

- No rotary vocabulary in the contract: a rotation in detents from a device of its own arrives nowhere — the Surface Dial through `RadialController` on Windows (per window, with the tool menu the OS draws), Wear OS's bezel or crown as a `MotionEvent` with `AXIS_SCROLL` from `SOURCE_ROTARY_ENCODER` ([Android documentation](https://developer.android.com/training/wearables/user-input/rotary-input)), the Digital Crown on watchOS and visionOS.
- Wear OS is a form factor of the Android host, not a column: its lists are expected to scroll by rotary input, so a watch app without it fails review on the first screen.
- The check is a Surface Dial on Windows and the rotary input of a Wear OS emulator turning a scroll view by detents, recorded and replayed.

## Revision history

<details>
<summary>Revision history</summary>

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | major | First version, from the first parity table |
| 2026-10-14 | minor | Gaps 9, 15, 16 and 17 point at the interaction completeness spec that plans or closes their input parts |
| 2026-10-16 | minor | Gap 16: Windows precision-touchpad scrolling and pinch implemented (DirectManipulation); no touchpad run yet |
| 2026-10-17 | minor | Gap 9: Windows OLE drag and drop as target and source; run within one process only |
| 2026-10-17 | minor | Gap 3: gesture alternatives keep a control path while adapters do not route custom actions |
| 2026-10-17 | minor | Gap 9: drags between the windows of one runtime stay on the source's capture where the host places its windows |
| 2026-10-17 | minor | Gap 16: Windows injected input records golden native recordings; the touchpad's must come from hardware |
| 2026-10-17 | minor | Gap 9: the `Draggable` and `DropTarget` widgets on the drag sessions, run headless |
| 2026-10-17 | minor | Gaps 1, 16, 17: a manual Android run on a Galaxy S21 runs touch, a `uinput` pen and touchpad, gesture exclusion, preferences and haptics; Android IME, drag and drop and handwriting wait for a Java side |
| 2026-10-17 | minor | Gaps 9 and 16: Web HTML drag and drop and trackpad pinch, run with constructed events in headless Chrome |
| 2026-10-17 | minor | Gaps 9 and 17: the Web's drags and Vibration API ran with trusted input in headless Chrome |
| 2026-10-17 | minor | Gap 1: the Web serves text fields through `EditContext` or a hidden textarea, run through Chrome's input-method path |
| 2026-10-17 | minor | Gaps 1 and 9: Android IME and drag and drop through `FluiView` (ADR-0247), run on a Galaxy S21 with Samsung Keyboard and a drag within the app |
| 2026-10-17 | minor | Dark mode leaves the list: every host reads the user's theme and reports its changes; its live runs belong to "never ran" |
| 2026-10-17 | minor | Storage leaves the list: `with_storage_dir` gives widgets storage on every host, run on Windows and Android |
| 2026-10-18 | minor | Displays and frame pacing narrows: Windows reports its refresh rate, the window's display and display changes, run live |
| 2026-10-18 | minor | Displays and frame pacing narrows again: macOS and iOS read displays from the OS, deliver changes and (iOS) request and measure ProMotion; type-checked only, no Apple code runs in CI |
| 2026-10-18 | minor | Displays and frame pacing narrows: the Web estimates its refresh period, follows pixel-ratio and screen changes and lists screens when access is already granted, run live in Chrome |
| 2026-10-18 | minor | Displays and frame pacing narrows: Android reads its displays and refresh rate through JNI and requests the highest rate while animating or touched, run live on a Galaxy S21; what remains is Wayland and X11 (with the native Linux host) and the Apple hosts not run |
| 2026-10-18 | minor | Displays and frame pacing narrows: Linux reads the outputs and refresh rates from the compositor (Wayland `wl_output`/`zxdg_output_v1`, X11 RandR 1.5) and winit windows name their display by native number; X11 ran live (XWayland under WSLg, `xrandr` changes under an Xorg dummy server); what remains is a live Wayland change, the Apple hosts not run and two-monitor moves not run; the same-scale Wayland move joins the native Linux host entry |
| 2026-10-18 | minor | Displays and frame pacing leaves the list: every host reads its displays and refresh rate and delivers changes, and a second monitor connected live on Windows was told apart by id, followed by the window and by live rate changes. What remains is runs, not code (the Apple hosts, a two-monitor move off Windows, a Windows monitor disconnect), so they join "Most implemented rows never ran on their environment" (entry 4); entries 18 to 22 are renumbered 17 to 21 |
| 2026-10-18 | minor | The live Wayland output change ran under WSLg (an output grown, a second one added); it leaves the display runs still owed in entry 4 |
| 2026-10-18 | minor | Gaps 1 and 16: a manual Android run runs the interaction widgets's widgets on a Galaxy S21 through the development agent; an editor action commits the keyboard's composition before its Enter |
| 2026-10-18 | minor | The interaction and display work merged: Dark mode, Storage and Displays left the list, so the interaction entries renumber (text input 1, accessibility 2, drag and drop 8, preferences 14, pointer 15, mobile system UI 16); haptic frame deadlines read the presentation's refresh period from the display snapshot |
| 2026-10-18 | minor | Apps controlling their windows narrows on macOS and iOS: window state and commands, sleep, lock, session end and memory pressure are written and type-checked, and [#1736](https://github.com/vanyastaff/flui/issues/1736) is the owed run; the entries stay open |
| 2026-10-19 | minor | Version floors and the GPU fallback join the list (ADR-0244) |
| 2026-10-19 | minor | Entry 22 narrows: the floors are declared in `platform-support.toml` and the [version map](version-map.md) is generated from it; enforcing them is still open |
| 2026-10-20 | minor | Entry 9 says what remains now that the window commands and state readback ship (ADR-0237): runs on macOS and on a real X11 window manager, a Linux title readback, live resize; entry 5 notes the idle check for a same-scale Wayland move; entry 4 names the 0 Hz macOS panel |
| 2026-10-20 | minor | Gap 1: Windows answers the TSF input scope (password and `TextInputPurpose`); no run with an input method yet, and Android, Web, iOS and Wayland still read none of it except Android's password variation |
| 2026-10-21 | minor | Gap 1: Android and Web read `TextInputPurpose` (the Android keyboard type, the Web `inputmode`); no device or browser run yet, and iOS and Wayland still read none of it |
| 2026-10-20 | minor | Entry 2 records the identifier obligation (ADR-0240) for the Android, iOS and Web adapters |
| 2026-10-21 | minor | `InputLatency` on `WindowOptions`: the web reads `pointerrawupdate` for a `Low` window (type-checked for wasm32, not run in a browser); Android is not wired because `NativeActivity` takes touch from the native input queue and the Java `requestUnbufferedDispatch` does not reach it, so it needs `GameActivity` or a polling mode first |
| 2026-10-20 | major | Entries 23–31 join from the platform-layer quality bar: preferences polled on macOS and Android, no presentation feedback, no HDR or colour management, no sandbox-aware file access, no secure text entry or concealed clipboard, partial latency modes, unaudited distribution, activation without OS focus rules, panic containment not gated. By impact 23 belongs beside 4 and 26 beside 7; the numbers are appended so earlier references stay valid |
| 2026-10-20 | minor | Restructured for reading: a summary table, one section per entry, the hours column removed |
| 2026-10-20 | minor | Entries 32–38 join from the quality-bar research: live resize unaudited and unscheduled, no present-mode, queue-depth, VRR or tearing control, launch handoff on mobile and the Web, quality-bar partials with no entry, power-aware pacing, the locale edge, accessibility bridge fidelity; entries 1, 14, 18, 24, 25, 26, 27, 28, 29 and 30 gain a bullet each (client queries and session survival, the locale rows, entitlement manifests, secrets in diagnostics, timestamp conversion, and the corrections to the presentation-feedback, HDR and activation evidence) |
| 2026-10-21 | minor | Entries 39–41 join: add-to-app embedding, gamepad and D-pad, rotary input |
| 2026-10-21 | minor | Entry 23 closes: Android preferences arrive through `FluiView`'s setting, contrast and inset callbacks, and the Galaxy S21 loop made no turn in 10 s idle, resumed or behind Home; entries 24–41 become 23–40. Entry 14 becomes "System preferences missing or never run" |
| 2026-10-22 | minor | Entry 2: the desktop accessibility adapters are always on (no `a11y` feature; AT-SPI comes with the Linux host), still unrun with a screen reader |
| 2026-10-22 | minor | Entry 30 closes: `cargo xtask panic-boundary` holds every OS entry into the core host (169: extern fns, Objective-C and COM methods, AccessKit handlers, blocks, delegates and GCD work items) to a constant or one call of `owner::panic_boundary::os_entry`; the AppKit, UIKit, Win32 and Android entries it found unguarded go through it now, and the Apple ones, type-checked only, join entry 4's runs still owed ([#1840](https://github.com/vanyastaff/flui/issues/1840)); entries 31–40 become 30–39 |
| 2026-10-26 | minor | Entry 15: the interaction layer has the pointer frame (`dispatch_pointer_frame_on`, `pointer_frame_end`); no backend reports frames yet. Follow-ups: Win32 `GetPointerFrameInfoHistory`, Android `ACTION_MOVE` with all pointers, Wayland `wl_touch.frame`, iOS touch sets |

</details>
