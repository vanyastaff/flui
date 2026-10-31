# Where FLUI falls short

> [!NOTE]
> **Last reviewed:** 2026-10-21 · **Last updated:** 2026-10-30 · **Change:** minor (entries 5 and 6: the native X11 host owns and reads the `PRIMARY` selection beside `CLIPBOARD`, its text asked of the application at the loop's turn, run on Xvfb; previously minor: entry 40: on the Web, background compute runs inline on the UI thread; previously minor: entry 36: Windows has no scroll by position until `accesskit_windows` implements `ScrollPattern`; previously minor: entries 5 and 6: the native X11 host owns and reads the `CLIPBOARD` selection on a connection of its own, run on Xvfb; previously minor: entries 2 and 36: every Web canvas mirrors its tree as a parallel ARIA DOM, run in headless Chrome, and reports no screen reader's cursor; previously minor: entry 6: the primary selection is a capability of the seam, provided by the native Wayland host and run on sway 1.9; previously minor: entries 5 and 6: the native Wayland host reads and writes its seats' selection through `wl_data_device`, run on Weston 13; previously minor: entry 6: Android reads and writes the system `ClipboardManager` through JNI, type-checked; previously minor: entry 6: the Web reads the system clipboard asynchronously, unrun in a browser; previously minor: entries 2 and 36: every Android Activity window answers TalkBack through `accesskit_android`, explore by touch included, and reports TalkBack's cursor through a vendored accessor, type-checked, and the semantics identifier reaches `viewIdResourceName`; previously minor: entry 36: no assistive-technology focus reaches a widget, so a carousel's autoplay cannot pause on it; previously minor: entries 5 and 10: the native Wayland host draws its own frame where the compositor draws none, its title bar, buttons and resize margins run on Weston 13; previously minor: entry 9: shown, minimized and occluded are three facts of the window snapshot; what of occlusion no run covers; previously minor: presentation feedback on macOS: the AppKit view's own `CAMetalLayer` reports each shown drawable, run on an M1 at 100 Hz, macOS ❌ to ✅; iOS tried and left `NotYetOnThisHost` after a blank Simulator run; previously minor: entry 9: a hand run of the AppKit window on an M1, macOS 27.0, two defects fixed and two mode findings recorded; previously minor: entry 24: every display carries its colour profile, read on every host but X11 and published as a snapshot revision when it changes, and the engine sets a surface up for it; no HDR display ran; previously minor: entry 23: Windows and the native Wayland host report presentation, the frame clock reads it; Apple, Android and winit still do not; previously minor: entries 5 and 29: Windows activates under the foreground rules, flashing where refused and handing the right only to the handler a link starts; the native Wayland host asks with an `xdg-activation` token, which names no input until its seats are bound; previously minor: entry 5: the native X11 host has its input, run with XTest on Xvfb; previously minor: entries 5 and 9: the native X11 host passes the window conformance under openbox on Xvfb, its input, text, clipboard, drag and drop and selection still owed; previously minor: entries 4 and 9: the AppKit window ran the window conformance on an M1, macOS 27.0, its hand checks still owed; entry 9: two AppKit mode findings from review (a transient `Normal` publish; admission against the published mode), not yet fixed; previously minor: entry 20: the contract has a window material, drawn on Windows and on macOS (type-checked), so Mica is a choice there; previously minor: entry 9: no close request on the Web and Android; previously minor: entry 9: Android cannot close its window; entry 4: the browser run of the window snapshot is owed; previously minor: entry 20: the macOS backdrop material and Liquid Glass are reachable through `flui::native::macos`, unrun; previously minor: entry 30, panic containment not gated, closed by `cargo xtask panic-boundary`, its Apple runs a bullet on entry 4, and entries 31–40 renumbered 30–39; entry 2: the desktop accessibility adapters are always on; previously minor: a bullet on entry 15, an Android touch held through Home; before that minor: entry 23, preferences polled on Android, closed and entries 24–41 renumbered 23–40; entry 14 retitled now that every host reads contrast and languages; previously minor: entries 39–41: add-to-app embedding, gamepad and D-pad, rotary input; before that minor: entries 32–38: live resize, present-mode and tearing control, launch handoff, quality-bar partials, power-aware pacing, the locale edge, accessibility bridge fidelity; a bullet each on entries 1, 14, 18, 24, 25, 26, 27, 28, 29 and 30; before that major: entries 23–31: the quality bar — polling, presentation feedback, HDR and colour, sandboxing, secure entry, latency modes, distribution, activation, panic gating)

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
| 9 | [Window control has run only on Windows, an X11 test server and the AppKit conformance kit](#9-window-control-has-run-only-on-windows-an-x11-test-server-and-the-appkit-conformance-kit) | desktop | Every desktop app |
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
| 22 | [Version floors and the GPU fallback not run on devices](#22-version-floors-and-the-gpu-fallback-not-run-on-devices) | all | Users on older systems or GPUs without Vulkan or WebGPU |
| 23 | [No presentation feedback](#23-no-presentation-feedback) | iOS, Android, winit | Every animated app; every latency claim |
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
| 40 | [No compute lane off the UI thread on the Web](#40-no-compute-lane-off-the-ui-thread-on-the-web) | Web | Web apps that decode images or lay out large documents |

## Entries

### 1. No text input on mobile and the Web

> **Environments:** iOS, Android, Web · **Who it hurts:** Every app with a text field · **Parity:** [Text input](platform-parity.md#text-input)

- No IME connection, soft keyboard, keyboard insets or input hints.
- **Runs on iOS:** an invisible `UITextInput` responder (ADR-0255 decision 11) takes the system keyboard when a field takes focus and turns its edits into composition and commits; on the iOS 27.1 Simulator the Japanese Romaji and nine-key Pinyin keyboards composed `日本語` and `你好` into an `EditableText` (2026-10-28).
- **Missing on iOS:** the shared text-store host behind the responder (until it serves the field's text and layout, the responder answers `firstRectForRange:` and `caretRectForPosition:` with the field's caret area whatever the range, `selectionRectsForRange:` with no rects, and tells the field only edits at its caret), `UITextInteraction`'s selection UI, input hints, keyboard insets, hiding the keyboard checked on screen and a run on a device. The field draws kana and kanji as missing-glyph boxes on the Simulator: its font fallback has no CJK face there.
- **Runs on Android:** it serves fields through `FluiView`'s `InputConnection` (ADR-0247), run with Samsung Keyboard on a Galaxy S21 (composition, commit, Escape, the action key, the keyboard shown and hidden, and through a manual Android run a field submitted by Done while the keyboard still composed).
- **Missing on Android:** never run with Gboard, `CursorAnchorInfo` placement, keyboard insets, paste through `OnReceiveContentListener` or stylus handwriting.
- **Runs on the Web:** it now serves fields through `EditContext` or a hidden textarea, run only through Chrome's own input-method path in headless Chrome.
- **Missing on the Web:** no soft keyboard, insets or input hints.
- **Missing everywhere:** client queries (selected range, surrounding text, first rect) for dictation, reconversion and handwriting, and a session that survives a window move, resize, DPI change and focus loss, are rows of their own under [Text input](platform-parity.md#text-input). Other frameworks show the failures: macOS dictation silently drops text when `selectedRange` returns `NSNotFound` ([rust-windowing/winit#4666](https://github.com/rust-windowing/winit/issues/4666), [#3617](https://github.com/rust-windowing/winit/issues/3617)), a Korean input method stops editing inline after the window moves ([DioxusLabs/dioxus#5246](https://github.com/DioxusLabs/dioxus/issues/5246)), and the input-method output of egui carries a position only ([emilk/egui#7941](https://github.com/emilk/egui/issues/7941)).

### 2. No accessibility on mobile and the Web

> **Environments:** iOS, Android, Web; desktop · **Who it hurts:** Blind and low-vision users; anyone who must meet accessibility law · **Parity:** [Accessibility](platform-parity.md#accessibility)

- Desktop is always on but untested with a screen reader.
- **Runs on iOS:** every UIKit window answers UIAccessibility through `accesskit_ios`'s low-level adapter, and the semantics identifier reaches `accessibilityIdentifier`; on the iOS Simulator an XCUITest client read the tree, found a button by its identifier and pressed it (2026-10-28). No VoiceOver session ran, and no AccessKit action was sent.
- **Runs on Windows:** the semantics identifier reaches UI Automation's `AutomationId`: a `UIAutomationClient` read a counter's button by its identifier, invoked it twice and read the count (Windows 11, 2026-10-29). No Narrator session ran.
- **Android, type-checked:** every Activity window answers TalkBack through `accesskit_android`'s low-level adapter, which `FluiView`'s node provider forwards to on the Java main thread; a publish is applied there in order. No TalkBack session ran. Explore by touch comes from the native input queue, where NativeActivity takes it: a finger hovering over the touchscreen goes to the adapter, not to the app as input (ADR-0255 decision 3). Bounds are placed from the view's screen location, which a device run must check against the window surface the tree is drawn in.
- **Web, run in headless Chrome:** every canvas window mirrors its tree as a parallel ARIA DOM (ADR-0255 decision 18): one `flui-node` element per node, nested as the tree is, in a host `position: fixed` over the canvas with `pointer-events: none`, so the pointer still reaches the canvas. Until a screen reader activates a placeholder button only it finds, the page holds nothing else, since the browser never says one is running; nothing deactivates it. A click on an element reaches the composition root as the action its node supports, focus moves the tree's focus, and keys on an element reach the window as on the canvas. `flui-native`'s `web_browser` tests read the tree back from the DOM, click a node and keep the host over a canvas in a scrolled or scaled container (a rotated container is not followed). Values, scrolling, links and text fields are not mirrored yet, and no NVDA, VoiceOver or TalkBack session read it.
- The semantics identifier (ADR-0240) reaches `AutomationId`, `AXIdentifier`, AT-SPI's id, iOS's `accessibilityIdentifier`, Android's `viewIdResourceName` (type-checked, through a change to the vendored `accesskit_android`) and the Web's `data-flui-id` attribute through `author_id`.
- The runs read one control, not a Narrator, VoiceOver, Orca or TalkBack session.
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
  - the macOS window by hand: the AppKit window conformance kit passes on an M1, macOS 27.0,
    2026-10-27 (nextest flui-native 233/233; the fixture after review: appkit_window 3/3, nextest
    flui-native 237/237, 2026-10-27), and [#1736](https://github.com/vanyastaff/flui/issues/1736)
    still lists a drag, the y-axis and a second display; the green button, Cmd-M and the Dock;
    opening options; displays (UUID and bounds); sleep, lock and memory-pressure notices; session end
  - the iOS window lifecycle (the memory warning and `applicationWillTerminate:`)
  - a window moved between two monitors on every host but Windows
  - unplugging a monitor on Windows
  - in a browser, the page moved to another screen changing the window snapshot's `display`, and
    a `freeze`/`resume` and a canvas focus change each arriving as one `WindowHandler::state_changed`: the
    Node suite covers them against a fake page, no browser run yet
- The panic boundary's Apple entries (AppKit and UIKit methods, blocks, GCD work items) are
  type-checked only: no panic has been seen contained on a Mac or an iPhone
  ([#1840](https://github.com/vanyastaff/flui/issues/1840)).
- The macOS extension `flui::native::macos` is type-checked only: neither the backdrop material
  behind the content, nor Liquid Glass (`NSGlassEffectView`, macOS 26), nor the narrowing of an
  AppKit window to its view has run on a Mac
  ([#1840](https://github.com/vanyastaff/flui/issues/1840)).
- A macOS display whose current mode reports 0 Hz (some built-in panels) has no period and paces at the 60 Hz default, because the `CVDisplayLink` fallback is not written.

### 5. Native Linux host (Wayland and X11)

> **Environments:** Wayland, X11 · **Who it hurts:** Linux users · **Parity:** [Summary](platform-parity.md#summary)

- It replaces the winit fallback.
- **X11 has its own host now** (`X11Platform`): one libxcb connection read by a loop on the owner thread, which waits with no timeout but the application's wake deadline; windows that publish what the server and the window manager hold, take every window command, and honour what the window manager supports; the RandR displays, the Settings portal, logind and memory pressure, and an AT-SPI bridge per window. Ran on WSL Ubuntu 24.04 under Xvfb with openbox, 2026-10-27: the window conformance, a mode set before mapping, the window manager's close asked of the handler, a frame asked for on another thread waking an idle loop, the exit with the last window and a wake deadline delivered once. Its input is XInput 2 (pointers, touch, pens, unphased smooth scrolling), XKB (keys, compose, the layout and its changes) and the cursor theme; XTest's pointer, buttons, wheel and keys ran through it on the same server (2026-10-28) and replay from a native recording, while touch, pens and smooth scrolling are checked by the decoder alone, since Xvfb has no such device. Its clipboard owns and reads the `CLIPBOARD` selection on a connection and a thread of its own (ICCCM: `TARGETS`, `TIMESTAMP`, the text forms, `INCR` both ways); two of its clipboards exchanged text, large text included, through Xvfb (2026-10-30). Its primary selection is `PRIMARY` on the same connection: the application's selection is asked for its text at the loop's turn, only when another client reads it, and two of its clipboards exchanged it through Xvfb (2026-10-30). **Still missing on it:** text input, handing the clipboard to a clipboard manager on exit, and drag and drop, which come with the host's later parts; a run on a desktop X server with a touchscreen, a tablet and a touchpad.
- The X11 host's pen, touch, smooth scrolling, touchpad pinch (XInput 2.4, which Xvfb announces), a grab's cancel mid-press and the device hierarchy (a device unplugged and plugged back in) are tested only synthetically: their XInput 2 events are built by the decoder's tests and the conformance table, never produced by a server, since Xvfb has no touch screen, tablet or touchpad and XTest drives only the core pointer and keyboard. Swipes are not delivered, as on Wayland. `current_platform` picks it for an X11 session only when winit is not built, and `flui-app` builds winit on Linux.
- The native Wayland host has its foundation (`WaylandPlatform`): the loop, `xdg-shell` windows with the snapshot and commands, displays, preferences, lifecycle and AT-SPI, run against a headless Weston. Its windows ask the compositor for their frame (`xdg-decoration`), draw at a fractional scale (`wp_fractional_scale_v1`), are given no frame while suspended, and are exported for a portal's parent (`xdg-foreign` v2); the AT-SPI bridge follows the compositor's activation. A window moved between two outputs of one scale names the display it moved to, run 2026-10-28 on Weston 13 with two outputs (X11 backend on Xvfb). Weston offers none of `xdg-decoration`, `wp_fractional_scale_v1` and `xdg-foreign`; the three ran on a headless sway 1.9 at scale 1.5, 2026-10-28 (WSL Ubuntu 24.04): sway honoured a floating window's frame request and overruled a tiled one's, the buffer was drawn at 1.5, and a second client imported the export and saw it revoked with the window. A tiled window takes no `SetSize`, as the tiling holds its size. Under sway the window conformance still fails on `SetMode(Maximized)`, which sway (`xdg_wm_base` v2, no `wm_capabilities`) ignores and the window cannot know in advance. Its windows ask to be activated with an `xdg-activation` token, which names the last key, button, touch or keyboard focus the seats report (entry 29). Its seats deliver input: keyboard through XKB (layout, compose and client-side repeat, read through the keyboard the X11 host shares), pointer and wheel, touch, pens (`tablet-v2`), pinches (`pointer-gestures`) and cursors (`cursor-shape`); the cursor, a click, a wheel notch and a key ran from a Weston seat, while touch, pens and pinches have run through their translation tests only, with no device, and no cursor shape has run: Weston 13 has no `cursor-shape`, so the cursor ran from the cursor theme (`XCURSOR_THEME`, `XCURSOR_SIZE`), the fallback for such compositors. A tablet pad's buttons, rings and strips are not read. Where the compositor draws no frame, the window draws its own (entry 10). Missing there: text input, clipboard, drag and drop, the portal parent reaching a capability crate (the contract's window reference does not carry it yet), and a run under a desktop compositor (GNOME, KDE, Hyprland). `current_platform` picks winit while it is built, which `flui-app` does on Linux.
- A Wayland window opened with `visible: false` cannot be hidden: Wayland has no hide for a toplevel, so it stays unshown only until it draws its first frame, and no command shows or hides it later (`hides`/`shows` are `Unavailable`).
- winit leaves the clipboard inert on pure Wayland, reports no occlusion on Wayland, has no pen, no pinch and no window bounds.
- winit reports no event when a window crosses to another Wayland output of equal scale (the `wl_surface.enter` events belong to its connection).
- FLUI asks every Wayland window's monitor again when the loop goes idle, never run with a window moved between two outputs.
- Under Hyprland (Omarchy's compositor) the host takes the decoration the compositor chooses through `zxdg_decoration_manager_v1` (no title bar of its own), presents an alpha surface so the compositor's blur applies, and scales through `wp_fractional_scale_v1` and `wp_viewporter`. The native Wayland host does the decoration and the scale (not run under Hyprland); the alpha surface does not exist, and winit, which `flui-app` picks, draws its own decorations there.

### 6. Clipboard is broken on three environments

> **Environments:** Android, Wayland, Web; all for rich · **Who it hurts:** Every app with copy and paste · **Parity:** [Clipboard](platform-parity.md#clipboard-and-drag-and-drop)

- No rich formats.
- Android reads and writes the system `ClipboardManager` through JNI; type-checked, no device run yet.
- The native X11 host owns and reads the `CLIPBOARD` selection on a connection of its own, run between two of its clipboards on Xvfb (2026-10-30); what it owns is gone when the application exits, as it hands nothing to a clipboard manager yet.
- Pure Wayland without data-control is inert under winit, which `flui-app` picks on Linux; the native Wayland host reads and writes its seats' selection through `wl_data_device`, run between two clients of Weston 13 (2026-10-30), and its primary selection through `zwp_primary_selection_device_manager_v1`, run between two clients of sway 1.9 (2026-10-30); no widget claims or pastes the primary selection yet.
- The Web reads the system clipboard through `navigator.clipboard` asynchronously, falling back to its own writes where the browser refuses; no browser run yet.

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

### 9. Window control has run only on Windows, an X11 test server and the AppKit conformance kit

> **Environments:** desktop · **Who it hurts:** Every desktop app · **Parity:** [Windows](platform-parity.md#windows)

- **Runs:** title, mode, fullscreen, position, size and constraint commands with a state readback ship on every desktop host ([ADR-0237](adr/ADR-0237-window-state-and-host-notices.md)).
- **macOS:** the AppKit window conformance kit passes on an M1, macOS 27.0, 2026-10-27 (nextest flui-native 233/233); the fixture after review: appkit_window 3/3, nextest flui-native 237/237, 2026-10-27. A hand run on the same Mac, 2026-10-28, one display: every command and opening option, a title-bar drag (the position follows, y grows downward, the top stops under the menu bar at y = 30), the green button (full screen) and Option-click (zoom and back), the yellow button, a Dock click (`reopen`, then `Show` brings the window back), a corner resize held to the constraints, the title-bar close of one of two windows, a borderless window taking keys, a quit Apple Event ([#1736](https://github.com/vanyastaff/flui/issues/1736)). A minimized window read `visible: false` and `SetSize` went below the minimum; both are fixed. Still owed: a second display; sleep, lock and memory-pressure notices; session end.
  - **A window at its maximum size reads `Maximized`.** AppKit's `isZoomed` holds for any frame equal to the standard frame, which the window's maximum size bounds, so a window resized by hand or by `SetSize` to its constraints' maximum (900×700 on a 3440×1320 usable area) publishes `Maximized` with a `normal_geometry`, though it does not fill the usable area. Win32's `IsZoomed` stays false for a window dragged to its maximum. Telling a zoom from a frame that equals the standard one needs a decision on what the backend tracks.
  - **`normal_geometry` after a macOS tile is stale.** Dragging a window to the top edge fills the screen through the system's tiling, which no delegate method reports; the window reads `Maximized` with the `normal_geometry` recorded before its last move, while AppKit returns it to where it was dragged from.
  - **A transient `Normal` publish on leaving full screen.** `windowDidExitFullScreen:` publishes the window before AppKit puts back the frame it had before full screen; only the held mode waits for the owner lane's next turn. A window zoomed before full screen therefore publishes `Normal` for a moment before it publishes `Maximized`. The final state is correct, seen on an M1, macOS 27.0: without a subscriber command the window comes back maximized, and with a subscriber's `SetMode(Normal)` it ends normal and unzoomed. Proposed fix: defer the publish with the held mode. Reported on [#1736](https://github.com/vanyastaff/flui/issues/1736).
- X11 through winit ran only on an Xvfb server with the test standing in for the window manager. The native X11 host's window passes the window conformance under openbox on Xvfb (2026-10-27), the snapshot compared with what the server reads back; not yet under a desktop's window manager or on a HiDPI server.
- winit cannot read a title back on Linux, so the snapshot holds the one last set.
- Wayland never reports minimized.
- **Occlusion.** Every host keeps the window's show state (`visible`), its minimized mode and its occlusion (`occluded`) apart, and a minimized window stays shown; the conformance checks that without a readback of the host's own. What no run covers: a covering by another window on Win32, which reports none (a cloak and a locked session it does), on a native X11 window manager, which reports it only as an event the conformance cannot read back, and on AppKit by hand; Cmd-H on macOS takes `isVisible` from the application's windows, so a hidden application reads as hidden, not occluded.
- Placement restore ran only on Windows; live resize is not audited.
- **The snapshot against the window system** (`window_conformance`'s `the_snapshot_agrees_with_the_window_system`: metrics, focus, activation, visibility and capabilities read back by the fixture's own means) ran on headless, on Win32 and on AppKit (an M1, macOS 27.0, 2026-10-27).
  - winit: not run; the real-loop harness runs a scenario on a worker thread, while the fixture's `apply` and `settle` need owner turns between commands.
  - UIKit: not run, type-checked only, queued in [#1840](https://github.com/vanyastaff/flui/issues/1840).
  - Web: not run; the browser suite has no fixture that reads the canvas back apart from the window.
  - Android: not run; the conformance needs a device fixture.
- **Android cannot close its window:** `WindowCommand::Close` is `Unavailable` there (one native window, which the application does not finish), so a programmatic close answers `CloseRequestError::Refused`.
- **No close request on the Web and Android:** neither host ever asks a window handler's `close_requested` (a page's unload and an Activity's finish are not requests the application can refuse), so an application's close veto never runs there.

### 10. No custom title bar, popups or owned windows

> **Environments:** desktop · **Who it hurts:** Apps with modern chrome; every menu and dropdown · **Parity:** [Windows](platform-parity.md#windows)

- Undecorated Windows windows can't be dragged or resized.
- No caption hit-testing or snap layouts.
- Menus, tooltips and dropdowns can't leave the window.
- On Wayland, where the compositor draws no frame (GNOME, Weston) or leaves it to the client, the native host's window draws its own: a title bar with the title and the buttons `wm_capabilities` offers, margins it is resized from, moved and resized through `xdg_toplevel.move`/`resize` with the press's serial, and the compositor's window menu on a secondary press. It ran on Weston 13's desktop shell (X11 backend on Xvfb, XTest, 2026-10-28): the bar read back from the screen, a drag moved and a corner resized the window, the buttons maximized and closed it. **Still missing:** it is the host's own neutral frame, light or dark after the desktop, not the application's title bar (no caption hit-testing for a frame drawn by the interface, as on Windows above); no touch or pen drags it; no shadow is drawn; the window menu, tiled edges and a fractional scale have not run (Weston 13 has no window menu or tiling, sway draws its own frames), nor has GNOME. Its title is shaped over the bundled faces and the host's, read by a scan of its own, which an application's font collection does not share yet.

### 11. No native menus

> **Environments:** macOS, iPadOS; Windows · **Who it hurts:** Every macOS app · **Parity:** [Menus](platform-parity.md#menus)

- No macOS menu bar (no Quit or Edit menu), so the shortcuts its items carry do nothing: Ctrl-Cmd-F leaves a full-screen window full screen.
- No native context menus, no Dock menu or jump list.

### 12. Lifecycle holes

> **Environments:** all · **Who it hurts:** Apps that save state or hold resources · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- Session end is delivered on Windows (checked with sent messages), macOS, iOS, Android, the Web and Linux (there as a notification).
- The Web reports page freeze, context loss and page hide only against a fake page, not yet from Chrome.
- Sleep, wake and lock reach Windows, macOS and Linux (Linux checked only against a stand-in for logind), and memory pressure Windows, macOS, iOS, Android and Linux.
- A GPU device whose rebuild keeps failing is given up after about twelve seconds (ADR-0267). On the desktop the application gets it back with `AppHandle::retry_render_device`; Android and iOS have no such entry, so a given-up window there stays blank until the process restarts. No device loss has run on real hardware: the rows use the stand-in renderer.

### 13. No deep links, URL schemes, file associations or single instance

> **Environments:** all · **Who it hurts:** Apps opened from links, files or a second launch · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- No host reports an opened URL or file: the registration that would have heard one is gone, and `opened(urls, files)` joins the application handler with the activation feature.
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

- **Missing:** no pen on macOS or Linux; no iPad hardware keyboard; the iPad pointer (trackpad scroll, pinch, rotation, hover and the secondary button) is type-checked only, never run on an iPad; no cursors on mobile; keyboard layout read on Windows only.
- Windows: the precision-touchpad pan and pinch (DirectManipulation) have never run on a touchpad, and no injector reaches it, so its golden recording can only come from a `native-capture` run on one (injected touch, pen, mouse and wheel already record theirs through a manual Windows run).
- Android: touchpad pinch and two-finger scroll and its pen hover and tilt ran only from `uinput` devices through a manual Android run, with no hardware touchpad or stylus. Its widgets ran with `uinput` touches, `input` stylus and mouse events and hardware key events through a manual Android run, with no physical keyboard, key repeat or dead key.
- Android: a touch held while the app goes to the background makes the system report it not responding ten seconds later. The loop reads its input queue only while the Activity is resumed, so the cancel the system sends for the window it took away is never answered (seen on a Galaxy S21 by holding a touch and pressing Home).
- Web: no precise-scroll phases, and its trackpad pinch (decoded from Control-held wheel and WebKit gesture events) ran only with constructed events in headless Chrome.
- The winit X11 and Wayland pointer rules (X11's first-touch cursor and fractional touchpad lines, a Wayland leave mid-press) ran only as unit rows, never on a native session with a touchscreen or touchpad.
- Pointer frames (`PlatformInput::PointerFrame`): only Windows touch and pen report them, read with `GetPointerFrameInfo` and delivered as one frame after `SkipPointerFrameMessages`, and that path has run on synthetic data only: no two-finger touch run on a Windows touch machine yet. Wayland through winit frames the touches of one loop turn, because winit does not report `wl_touch.frame` (a turn holds whole frames but may hold several; no touchscreen run). Android frames the fingers of one `ACTION_MOVE` (cross-checked in CI, no run on a device), and iOS (a touch set) still reports each contact on its own, as frames of one.
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
- The contract's window material (`WindowCommand::SetMaterial`: `Opaque`, `System`) is drawn on Windows (Mica, build 22621 and later; a Win32 window still opens on it) and on macOS (`NSVisualEffectView`'s window-background material, type-checked only); Linux, Web and mobile report it `Unavailable`. The backdrop material and Liquid Glass of `flui::native::macos` ran on macOS 27 (an M1, 2026-10-27, `an_appkit_backdrop_goes_behind_the_content_and_comes_away`); the contract's material on macOS waits for its Mac run; no window icon on Windows or Linux.
- What the OS has and the host does not drive answers `NotYetOnThisHost`, not `NotOnThisPlatform`: the material on iOS (`UIVisualEffectView`), on Android from API 31 where the device blurs across windows, and on an AppKit window off the main lane; the macOS frame-rate hint from macOS 14 (`NSView.displayLink`); the pointer's shape on Android from API 24 (`PointerIcon`) and on an iPad from iOS 13.4 (`UIPointerStyle`). Below those versions, on an iPhone, or with the device's cross-window blur off, the answer is `NotOnThisDevice`. Only the rules' table tests ran (on Windows); none of the iOS, Android or macOS paths has run on a device.

### 21. Unaudited rows

> **Environments:** desktop, Web · **Who it hurts:** Unknown · **Parity:** [Windows](platform-parity.md#windows)

- App identity and packaging, live resize.

### 22. Version floors and the GPU fallback not run on devices

> **Environments:** all · **Who it hurts:** Users on older systems or GPUs without Vulkan or WebGPU, who get a failure in the middle of start instead of a clear refusal · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- The floors are declared in `platform-support.toml` ([version map](version-map.md)), the backends refuse a release below them at start, and `flui build` packages them, raised by an application's `[platforms.<environment>] min`: the APK's `minSdk` and the NDK level, the iOS and macOS deployment targets with `MinimumOSVersion` and `LSMinimumSystemVersion`. No bundle has run on its floor.
- Android packages API 23; below API 24, or on a device without a working Vulkan driver, the renderer falls back to GLES 3.0, which no device has run yet.
- The CLI writes no Windows application manifest (`supportedOS`), and an iOS app built through a hand-made Xcode project takes its deployment target from that project; only the start-up refusal holds the floor there.
- Windows links a 1607 API unconditionally; the Linux release glibc is the runner's.
- The GPU fallback (Vulkan to GLES 3.0, WebGPU to WebGL2, Direct3D 12 to WARP) is compiled and the engine's pixel suites pass on llvmpipe GL; only Linux has run it, and no browser has run WebGL2 or Windows WARP.

### 23. No presentation feedback

> **Environments:** iOS, Android, Linux on winit · **Who it hurts:** Every animated app; every latency claim · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- Windows (DXGI frame statistics, run) and the native Wayland host (`wp_presentation_time`, run under Weston 13) report when each frame reached the screen, on the event timeline; the frame clock keeps the report and bounds idle work by the next refresh. Headless injects one.
- macOS reports (run): the AppKit view is backed by its own `CAMetalLayer`, which wgpu draws into and whose `nextDrawable` adds `addPresentedHandler:` to each drawable; the display pass moved from `drawRect:` to `updateLayer`. On an M1, macOS 27.0, 100 Hz (2026-10-28): 29 reports for 30 presses of `counter`, the shown times a multiple of 9.999 ms apart, the handler 0.34–0.49 ms after the showing; the conformance row saw a report 13 ms before the next request. During a live resize Metal reports most drawables never shown (80 of 102); whether the window shows fewer frames there than with wgpu's own layer is not measured, and no one has looked at a resize by eye.
- iOS reports nothing (`NotYetOnThisHost`): a UIKit view whose layer class is a `CAMetalLayer` (FLUI's or the plain class) showed nothing on the iOS Simulator (iOS 27.1), while the same app with wgpu's sublayer draws; the simulator's drawables have no `addPresentedHandler:` either. A device run decides whether the layer class works there.
- Android reports nothing (`NotYetOnThisHost`): the time a frame was shown belongs to the Vulkan swap chain wgpu presents through, and wgpu reads none. The path left is wgpu reading `VK_GOOGLE_display_timing` (`vkGetPastPresentationTimingGOOGLE`) and handing it to the presenter as DXGI's statistics are handed over. The loop now runs its frames on the choreographer's callback (ADR-0269), and on API 33 its preferred frame timeline's `expectedPresentationTimeNanos` fills `FrameRequest::target_present`; that names when a frame is due on screen, never a report that it was shown. Type-checked only, not yet run on a device.
- winit, the Wayland and X11 columns' host until the native ones replace it, reports nothing. The Web (a `requestAnimationFrame` timestamp is a frame's start) and X11 (the Present extension reports a completion to the GPU driver's connection) have no presentation time a toolkit can read, and their windows say so.
- Animations still tick on the vsync instant, never the presentation time (ADR-0257 decision 2); nothing yet attributes a presentation to the produced frame it showed, so the frame telemetry's input-to-present histogram still ends at the submit.

### 24. No HDR, wide gamut or colour management

> **Environments:** all · **Who it hurts:** Photo, video and design apps; every app on an HDR display · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- The engine requests a surface colour capability per window (`AppConfig::surface_color`, [ADR-0256](adr/ADR-0256-surface-colour-is-requested-and-converted-at-presentation.md)) and converts to Display P3, scRGB, extended sRGB or HDR10 at presentation; no run on an HDR or P3 display backs it yet, and no Metal space has run.
- Every display carries its colour profile (`DisplayInfo::color`: `SurfaceColorSpace` and `Headroom`, or why there is none), read on Windows, macOS, iOS, Android, Wayland and the Web, and a profile change is a new snapshot revision; the engine sets a surface up for the colour of its window's display. Only an SDR Windows monitor has run; no HDR or P3 display, and the Apple, Android and Wayland reads are type-checked only.
- A profile change reconfigures the window's surface before its next frame (ADR-0256 §6, amended), but no host ran a real change: Windows, macOS and Wayland observe the change type-checked or synthetically only, iOS and Android re-read only on their other display events.
- X11 reads no colour (`_ICC_PROFILE` is not parsed). Windows has no broadcast for the SDR white level, so a moved slider is read at the next other display event. Android sets the window's `ColorMode` for the requested surface but does not listen to `Display.registerHdrSdrRatioChangedListener`: the listener needs a Java class in `FluiView`, so a moved HDR/SDR ratio is read only at the next configuration change or resume. The Web reports capability only and no headroom.
- Sources are still sRGB: images with a profile, video and HDR images enter the working space as sRGB values.

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
- iOS attaches the touches UIKit coalesced into a move and the ones it predicts (type-checked, no run on a device). Pencil force, altitude and azimuth that UIKit later revises (`touchesEstimatedPropertiesUpdated:`) are not reported: the pointer vocabulary has no revision of a delivered sample, so a stroke keeps the first estimate. The planned shape is a `#[non_exhaustive]` `PointerEvent::SampleRevised { pointer, sample_index, sensors }`, which lands with the first stroke or ink consumer; a scenario that covers Pencil estimates fails on this gap until then. Holding a reading back until its estimate settles was rejected: it adds the latency this entry is about.
- The Linux and macOS paths are not audited.
- Timestamp conversion is a hazard of its own: Win32 `GetMessageTime` is 32-bit milliseconds that wrap every 49.7 days, Wayland event times have an undefined base, and Android's event time is uptime milliseconds. Windows pointer ticks are rebased across the wrap (`NativeTime::TickMillis`); the native Wayland host reads a stamp through the same rule only when it lies within a second of `CLOCK_MONOTONIC` and stamps the event on arrival otherwise (Weston's X11 backend stamps on the X server's clock); the X11, Android and Web conversions are not audited.

### 28. Distribution is unaudited

> **Environments:** all · **Who it hurts:** Everyone who ships · **Parity:** [Distribution](platform-parity.md#distribution)

- Packages, signing, notarisation, identity.
- `flui build` stages unsigned `.app`s and a debug APK; no MSIX, Flatpak, AAB, notarisation or Authenticode.
- The identity that notifications, URL schemes, portals and jump lists need is not checked into the package.
- Entitlement and sandbox manifests (macOS and iOS entitlements, Flatpak `finish-args`, Snap interfaces, MSIX capabilities, Android permissions) are not generated from declared capabilities; the Android template declares `VIBRATE` itself.

### 29. Activation ignores the OS focus rules

> **Environments:** Windows, Wayland · **Who it hurts:** Apps brought forward by a link, a second launch or a notification · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- Windows follows the foreground rules: an activation the rules refuse flashes the window's taskbar button and leaves the foreground with the application the user works with, a right handed over (`AllowSetForegroundWindow`) brings it forward (run with two processes, 2026-10-28). Opening a link, file or folder hands the right only to the handler process the shell started, by its id; a handler already running (a browser, Explorer) is handed nothing and comes forward under the OS's own rules, which may leave it behind (the PID hand-over not run against a real handler). No second launch hands the right yet: that comes with single instance, in the activation spec (entry 13).
- The Win32 snapshot says `focused: true` and `active: true` for a window of a process the user is not working with: they are its thread's focus and activation, not the system's. `WM_ACTIVATEAPP` does not tell them apart (Windows sends it for the thread's own activation of a background process, seen in a two-process run, 2026-10-28), so the fix needs the foreground's changes (`EVENT_SYSTEM_FOREGROUND`) to reach each window, which the host does not watch yet.
- The native Wayland host asks with an `xdg-activation` token and uses the launcher's `XDG_ACTIVATION_TOKEN` for the first window it holds; a token names the last key, button, touch or keyboard focus its seats report, the serial a compositor weighs before it moves the focus (with no input yet, an activation asks for attention: sway marks the window urgent). CI covers only the refusal: Weston 13, which CI runs, has no `xdg_activation_v1`, so the whole path runs only on sway, locally ([testing](testing.md)). It is not the Linux host `current_platform` picks yet, and winit requests no token.
- macOS cooperative activation (`NSApplication.yieldActivation(to:)`, macOS 14) is documented by Apple, which also marks `activate(ignoringOtherApps:)` deprecated; neither is audited.

### 30. Live resize is unaudited and unscheduled

> **Environments:** desktop, Web · **Who it hurts:** Every user who resizes a window by dragging its edge · **Parity:** [Windows](platform-parity.md#windows)

- No environment has been audited: the row "Live resize without jitter" is ❔ on Windows, macOS, Wayland, X11 and the Web, and entries 9 and 21 mention it only in passing.
- Windows has no `WM_ENTERSIZEMOVE` handling outside the DirectManipulation tests. That message opens a modal loop inside `DefWindowProc` that lasts until the drag ends ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-entersizemove)); winit reports that its `ControlFlow` is ignored there and that continuous redraws make a drag choppy ([rust-windowing/winit#3272](https://github.com/rust-windowing/winit/issues/3272), [#4708](https://github.com/rust-windowing/winit/issues/4708)).
- Metal presents asynchronously to Core Animation transactions unless `CAMetalLayer.presentsWithTransaction` is set ([Apple](https://developer.apple.com/documentation/quartzcore/cametallayer/presentswithtransaction)); Wayland needs an `ack_configure` before the committing frame and frame callbacks to pace the redraw; X11 has `_NET_WM_SYNC_REQUEST`. None of these is audited here.
- macOS sets `presentsWithTransaction` on the view's layer for each display pass of a live resize and clears it after, so wgpu presents that drawable in the transaction that carries the new window frame; the frame renders inline in `updateLayer`, so the wait wgpu adds holds the main thread only while an edge is dragged. Type-checked, not looked at by eye.
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

- Honest absence: the nine spellings of "not here" are one now, `Unsupported` with its reason in every contract error (ADR-0151 §4). Other layers show what silent spellings cost: winit setters return `()` while their documentation lists platforms as unsupported, `tauri-plugin-geolocation` resolves to a zeroed position on the desktop, and `tauri-plugin-notification` reports a permission it did not check.
- Replay: pointer, key and wheel traces, system notices, session ends, reopens, input method events, reads of the system preferences and displays, and window snapshots replay on the headless host; close requests and closes do not.
- The `native-capture` journal records pointers, system notices, session ends, reopens, input method events, reads of the system preferences and displays, and window snapshots (titles redacted by default), not close requests or closes. An input method that edits the focused text store directly (TSF on Windows, the Android and web mirrors) delivers no event, so its edits are not journaled: the `ime` row family reserves a store-edit payload for them.
- Not every unsupported cell in the parity tables names its reason, and nothing checks the cells against the code's `Unsupported` reasons.

### 34. Power-aware pacing and throttling

> **Environments:** all · **Who it hurts:** Every user on battery or a thermally limited device · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Power state (battery, low-power mode, thermal) is read on no environment, and nothing acts on it: no row named a frame cap or a background pause before.
- The native signals exist: Apple's `ProcessInfo.thermalState` and `isLowPowerModeEnabled` with a change notification, Android's `PowerManager.getCurrentThermalStatus()` and `addThermalStatusListener`, Windows battery saver through `SYSTEM_POWER_STATUS.SystemStatusFlag` and `RegisterPowerSettingNotification`, and the Linux portal's `PowerProfileMonitor.power-saver-enabled`.
- Apple's `CADisplayLink.preferredFrameRateRange` is only a hint that Low Power Mode, a critical thermal state and accessibility settings can override, so an app that asks for 120 Hz still has to follow the state.
- iOS asks its display link for the screen's best rate only while a presentation animates or a pointer is down (the runtime's frame-rate demand, as on Android), and leaves a still screen to the system's rate. On a still screen the link pauses: it runs only while a frame is asked for, and a request from any thread resumes it on the main thread (type-checked, no device run). The Web's frame loop stands still the same way once the refresh estimate has its window of frames, and a request resumes it (run in the wasm32 suite on Node, not in a browser).
- The check forces each state and asserts a delivery within one event-loop turn and a frame-rate or work reduction.

### 35. Locale edge: layout direction, per-app language, system fonts and fallback

> **Environments:** all · **Who it hurts:** Apps in right-to-left or non-Latin languages; users who set a language for one app · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Entry 14 names languages and regional formats only. Layout direction, per-app language, and system fonts and fallback have no reading in the platform layer.
- Android 13 adds per-app language preferences and `setApplicationLocales` ([Android documentation](https://developer.android.com/guide/topics/resources/app-languages)); iOS has a per-app language setting; a right-to-left layout follows from the locale.
- Text layout resolves fonts through `fontdb` in `flui-painting`. DirectWrite `IDWriteFontFallback`, Android `fonts.xml` and `AFontMatcher` (API 29), CoreText and fontconfig are the platform answers to which font draws a character the requested family lacks. The collection puts such an answer (`SystemFallback`) ahead of FLUI's tables ([ADR-0271](adr/ADR-0271-the-host-feed-asks-the-platform-which-faces-fall-back.md)). DirectWrite gives it on Windows, CoreText on macOS and iOS, fontconfig on Linux and the BSDs, `AFontMatcher` on Android from API 29 and `fonts.xml` below it; Windows, Apple and Android are type-checked, not run. Nothing asks again when the locale changes (entry 14).
- The check is a locale matrix (en-US, ar-EG for right-to-left, ja-JP, hi-IN with its numbering) on every OS: list order, first weekday, 12 or 24 hour clock, per-app override, and CJK and emoji text without missing-glyph boxes.

### 36. Accessibility bridge fidelity (properties and announcements)

> **Environments:** desktop, iOS, Android, Web · **Who it hurts:** Screen-reader users: a control announced without its value, hint or state, or a change nobody hears · **Parity:** [Accessibility](platform-parity.md#accessibility)

- Entry 2 is about missing adapters; a bridge that exists can still drop properties or announcements, and the two fail independently of the tree and actions.
- Another framework's Linux bridge drops a field's `value` and `hint` ([flutter/flutter#194638](https://github.com/flutter/flutter/issues/194638)) and gives no AT-SPI announcement for live regions, snack bars, alert dialogs or route changes, only for an explicit announcement call ([flutter/flutter#194045](https://github.com/flutter/flutter/issues/194045)).
- The parity rows "Properties (value, hint, description, state)" and "Announcements and live regions" are ❔ on the desktop adapters and ❌ where there is no adapter at all.
- Each needs a check with the real assistive technology on each OS that reads role, name, value, hint, state, actions, focus order and announcements; a harness that registers AT-SPI listeners and matches only strings carrying a nonce keeps any other source from producing a hit.
- Assistive-technology focus reaches a widget only on iOS and Android, and no run has shown it. The model is there: `Semantics::on_assistive_focus_within` tells a subtree when the screen reader's cursor enters and leaves it, a `Carousel` pauses its autoplay on it, and `PresentationCapabilities::assistive_focus` says whether the platform reports the cursor (`each_native_adapter_says_whether_it_reports_the_assistive_cursor` in `flui-native`). On iOS the cursor comes through a hook FLUI adds to `accesskit_ios`, vendored in `third_party/` until upstream has one; it needs a VoiceOver run on a device. UI Automation, NSAccessibility and AT-SPI give an application no signal for Narrator's, VoiceOver's or Orca's cursor, and the browser gives a page none for any screen reader's, so those adapters say `Unreported`. On Android the cursor is read from `accesskit_android`, vendored in `third_party/` with one accessor added, after each action, hover and update FLUI hands the adapter; it needs a TalkBack run on a device. Where the cursor is not reported, a `Carousel` pauses on keyboard focus, pointer and the app leaving the front, and the pause control it draws is what meets WCAG 2.2.2.
- Windows has no scroll by position: `accesskit_windows` 0.35 implements no UI Automation `ScrollPattern`, so Narrator and NVDA scroll a `Scrollable` only through its position slider's `RangeValue` and directional steps. The semantics already publishes the position from the top or left edge and takes `SetScrollOffset` (ADR-0273); the adapter reads neither.

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

### 40. No compute lane off the UI thread on the Web

> **Environments:** Web · **Who it hurts:** Web apps that decode images or lay out large documents · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- `Background::compute` (ADR-0272) runs its job inline on the UI thread on wasm32, because `ExecutionServices` is sequential there (ADR-0047): a long decode blocks the frame it was started in. The answer still settles at the next owner turn, so widget code is the same everywhere.
- The lane belongs on Web workers: a worker pool that runs a `ComputeJob` needs shared memory (`SharedArrayBuffer`, which needs cross-origin isolation headers) or a job shape that crosses `postMessage` by value.
- The check is a browser run of a large image decode started through `Background::compute` with the frame clock showing no frame longer than its budget.

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
| 2026-10-26 | minor | Entry 15: `PlatformInput::PointerFrame` carries a frame from a backend; the Win32 touch and pen path reads it with `GetPointerFrameInfo` (synthetic data only, live touch run pending) |
| 2026-10-26 | minor | Entry 20: the macOS backdrop material and Liquid Glass come back through the escape hatch `flui::native::macos` (`AppKitWindow::set_backdrop`), type-checked only and listed in [#1840](https://github.com/vanyastaff/flui/issues/1840); Windows still forces Mica |
| 2026-10-26 | minor | Entry 9: the window snapshot holds every window fact, and the window conformance compares it with what the window system reads back; it ran on headless and Win32, and the winit, AppKit, UIKit, Web and Android runs are listed |
| 2026-10-27 | minor | Entries 4 and 9: the AppKit window conformance kit passes on an M1, macOS 27.0 (nextest flui-native 233/233), and entry 9 is retitled for it; the hand checks of [#1736](https://github.com/vanyastaff/flui/issues/1736) stay owed. The fixture after review: appkit_window 3/3, nextest flui-native 237/237, 2026-10-27. Entry 9 gains two AppKit mode findings from review: a transient `Normal` publish inside `windowDidExitFullScreen:` (the final state is correct, seen on an M1, macOS 27.0), and admission against the published mode rather than the queued one |
| 2026-10-27 | minor | Entry 20: the contract's window material (`WindowCommand::SetMaterial`, `Opaque` and `System`) replaces `WindowBackgroundAppearance`; Windows draws it from build 22621 and admission answers `NotOnThisDevice` below, which ran on Windows 11; macOS draws it type-checked only; every other host reports it `Unavailable` |
| 2026-10-27 | minor | Entries 5 and 9: the native X11 host (loop, windows, displays, preferences, host notices, AT-SPI) passes the window conformance under openbox on Xvfb; its input, text input, clipboard and drag and drop, its selection by `current_platform` and a desktop run are still owed |
| 2026-10-28 | minor | Entries 5 and 29: a Win32 activation the foreground rules refuse flashes the taskbar button and takes no foreground, a right handed over brings the window forward (run with two processes), and opening a link or file hands the right only to the handler process the shell started; the Win32 snapshot's thread-local focus of a background process is recorded, not fixed; `Platform::activate` no longer touches another application's window. The native Wayland host asks with an `xdg-activation` token (run on sway; Weston 13 has none), which names no user input until the seats are bound; a second launch handing the right waits for a single instance (entry 13) |
| 2026-10-28 | minor | Entry 5: the native X11 host has its input (XInput 2 pointers, touch, pens and unphased smooth scrolling, XKB keys, compose and layout, the cursor), XTest runs on Xvfb; its text input, clipboard and drag and drop, trackpad gestures, its selection by `current_platform` and a desktop run are still owed |
| 2026-10-28 | minor | Entry 5: the native Wayland host has its seats' input (XKB keys with compose and layout, pointer and wheel, touch, `tablet-v2`, `pointer-gestures`, `cursor-shape` or the cursor theme); a cursor, a click, a wheel notch and a key ran from a Weston seat; touch, pens and pinches tested by translation only |
| 2026-10-28 | minor | Entry 24: every display carries its colour profile (`DisplayInfo::color`), read on Windows (DXGI), macOS, iOS, Android, Wayland (`wp_color_management_v1`) and the Web, a change published as a snapshot revision, and the engine sets a surface up for it; what remains: a live reconfiguration, X11, the Android listener and `ColorMode`, an HDR display run, wide-gamut sources |
| 2026-10-28 | minor | Entry 23: frame requests carry when earlier frames reached the screen (`FrameRequest::presented`); Windows reads DXGI frame statistics and the native Wayland host `wp_presentation_time`, both run; the frame clock bounds idle work by the next reported refresh; macOS, iOS, Android and winit report nothing yet, and the Web and X11 have nothing to report |
| 2026-10-28 | minor | Entry 9: a hand run of the AppKit window on an M1, macOS 27.0: drag, the title-bar buttons, the Dock, a corner resize, a close of one of two windows and a borderless window ran; a minimized window read hidden and `SetSize` went below the minimum (both fixed); a window at its maximum size reads `Maximized`, and `normal_geometry` after a tile is stale |
| 2026-10-28 | minor | Entry 23: macOS reports when each frame reached the screen (the AppKit view's own `CAMetalLayer`, run on an M1); iOS left out after a blank Simulator run with such a layer, and macOS dropped from the entry's environments |
| 2026-10-28 | minor | Entry 9: the window snapshot keeps shown, minimized and occluded apart on every host (`visible`, `mode`, `occluded`, read together by `is_seen`); a minimized X11 window no longer reads hidden; a covering on Win32, a native X11 window manager and AppKit by hand has not run, and Cmd-H reads as hidden |
| 2026-10-29 | minor | Entry 36: no assistive-technology focus reaches a widget, so a `Carousel`'s autoplay cannot pause on it |
| 2026-10-30 | minor | Entry 2: the semantics identifier reaches Android's `viewIdResourceName` through a change to the vendored `accesskit_android`, type-checked |
| 2026-10-30 | minor | Entries 2 and 36: every Web canvas mirrors its tree as a parallel ARIA DOM in a host over the canvas, activated by a placeholder only a screen reader finds, its clicks reaching the action listener (headless Chrome, constructed events); the browser reports no screen reader's cursor |
| 2026-10-30 | minor | Entry 15: the iPad pointer scrolls, pinches, rotates, hovers and reads its secondary button, type-checked, never run on an iPad |
| 2026-10-30 | minor | Entry 30: a macOS live resize presents each frame in the Core Animation transaction of its window frame, type-checked |
| 2026-10-30 | minor | Entry 27: iOS attaches coalesced and predicted touches to a move; revised Pencil estimates are not reported, the planned `SampleRevised` event is named |
| 2026-10-30 | minor | Entry 34: the iOS display link and the Web frame loop stand still while no frame is asked for |
| 2026-10-30 | minor | Entry 34: iOS holds the high refresh rate only while a presentation is active; its display link still ticks on a still screen |
| 2026-10-30 | minor | Entries 5 and 6: the native X11 host owns and reads the `CLIPBOARD` selection on a connection of its own, run between two of its clipboards on Xvfb; its primary selection and the handoff to a clipboard manager are still owed |
| 2026-10-30 | minor | Entry 22 narrows: the backends refuse a release below the floor, and the GPU falls back to GLES 3.0, WebGL2 or WARP (run on llvmpipe GL only); packaging the floor everywhere remains |
| 2026-10-30 | minor | Entry 22 narrows: `flui build` packages the floors and an application may raise them in `flui.toml`; the Windows manifest and runs on each floor remain |
| 2026-10-30 | minor | Entry 40: on the Web, background compute runs on the UI thread |
| 2026-10-30 | minor | Entries 5 and 6: the native X11 host owns and reads the `PRIMARY` selection beside `CLIPBOARD`, the application's selection asked for its text at the loop's turn, run between two of its clipboards on Xvfb |

</details>
