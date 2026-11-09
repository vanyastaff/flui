# Platform parity

> [!NOTE]
> **Last reviewed:** —

Rendered from `platform-parity.toml`; do not edit by hand.

## Legend

| Mark | Meaning |
|:-:|:--|
| ❌ | Missing: no code path, a stub, a mock, or a default that pretends to succeed |
| — | Not applicable: the environment has no such concept |
| 🟡 | Implemented but never run there, or partial; the note says which |
| ❔ | Not audited yet |
| ✅ | Implemented and passed an executed run on that environment; the note names the run |

## Summary

| Environment | Host | ✅ | 🟡 | ❌ | — | ❔ |
|:--|:--|--:|--:|--:|--:|--:|
| Windows | Win32 (native) | 11 | 53 | 33 | 19 | 10 |
| macOS | AppKit (native) | 10 | 42 | 44 | 20 | 10 |
| Wayland | Wayland (native) | 11 | 50 | 30 | 28 | 7 |
| X11 | X11 (native) | 12 | 45 | 33 | 29 | 7 |
| iOS | UIKit (native) | 7 | 37 | 41 | 33 | 8 |
| Android | android-activity (native) | 18 | 40 | 33 | 29 | 6 |
| Web | wasm canvas (native) | 7 | 40 | 30 | 42 | 7 |
| Headless | test double | 53 | 2 | 50 | 19 | 2 |

## Parity

| Capability | Windows | macOS | Wayland | X11 | iOS | Android | Web | Headless |
|:--|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| **App lifecycle** | | | | | | | | |
| Event loop, startup and wake-ups | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Version floor: declared, packaged, refused below it | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| Quit, exit request, last-window policy | 🟡 | ✅ | ✅ | ✅ | — | 🟡 | 🟡 | ✅ |
| Close request with veto | 🟡 | ✅ | 🟡 | 🟡 | — | — | ❌ | ✅ |
| Reopen with no windows (Dock click) | — | ✅ | — | — | — | — | — | ❌ |
| Focus and active state | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Visibility, minimize, occlusion | 🟡 | ✅ | 🟡 | ✅ | ✅ | 🟡 | 🟡 | ✅ |
| Suspend, resume, GPU-surface loss | — | — | — | — | ✅ | 🟡 | 🟡 | ✅ |
| Configuration changes reach the running app (no Activity recreation) | — | — | — | — | — | 🟡 | — | — |
| Sleep, wake, screen lock | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | ❌ |
| Session end (log off, shutdown) with veto | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ |
| Memory pressure | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | — | ❌ |
| Bring the app forward, request attention | 🟡 | ❌ | 🟡 | ❌ | — | — | — | ❌ |
| Single instance, forward arguments | ❌ | — | ❌ | ❌ | — | — | — | ❌ |
| Deep links, URL schemes, file associations | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ |
| State restoration | — | ❌ | — | — | ❌ | ❌ | ❌ | ❌ |
| Activation obeys the OS focus rules (Wayland `xdg-activation` token, Windows foreground rights handed to the activated process) | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | — |
| App identity (AUMID, bundle ID, `.desktop` file) | ❔ | ❔ | ❔ | ❔ | — | — | — | — |
| **Window** | | | | | | | | |
| Open and close a window | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multiple windows | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | — | ✅ |
| Show without a blank first frame | 🟡 | ✅ | 🟡 | 🟡 | — | — | — | ✅ |
| Launch-screen handoff: the first frame matches the OS splash | — | — | — | — | ❌ | ❌ | ❌ | — |
| Title | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ |
| Minimize, maximize, restore, state readback | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | ✅ |
| Fullscreen | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ |
| Position, size, move and resize events | 🟡 | 🟡 | 🟡 | 🟡 | — | — | 🟡 | ✅ |
| Size constraints, resizable and other flags | 🟡 | 🟡 | 🟡 | 🟡 | — | — | — | ✅ |
| Live resize without jitter | ❔ | ❔ | ❔ | ❔ | — | — | ❔ | — |
| Save and restore window placement | ✅ | 🟡 | 🟡 | 🟡 | — | — | — | ✅ |
| Frameless windows, custom title bar | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| Backdrop material, transparency, corners | 🟡 | 🟡 | ❌ | ❌ | — | — | — | ❌ |
| Owned, modal and popup windows; always on top | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| Window icon | ❌ | — | ❌ | ❌ | — | — | — | ❌ |
| Exclude a window from screen capture | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ |
| Native views inside the UI (web view, map, video) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — |
| **Display** | | | | | | | | |
| Displays: list, work area, current display, changes | ✅ | 🟡 | ✅ | ✅ | 🟡 | ✅ | ✅ | ✅ |
| Scale factor and DPI changes | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | ✅ | ✅ |
| Refresh rate and frame pacing | ✅ | 🟡 | ✅ | ✅ | 🟡 | ✅ | ✅ | ✅ |
| Present mode, queue depth, VRR and tearing control | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | — |
| HDR, wide gamut and colour management (surface colour space, SDR/HDR headroom, display profile changes) | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | 🟡 | ✅ |
| HDR and SDR-white change events | 🟡 | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | ✅ |
| Presentation feedback: when each frame reached the screen | ✅ | ✅ | ✅ | — | ❌ | ❌ | — | ✅ |
| GPU surface | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| Safe area | — | ❌ | — | — | ✅ | ✅ | ❌ | ✅ |
| Soft-keyboard insets | — | — | — | — | 🟡 | 🟡 | 🟡 | ✅ |
| System-gesture insets | — | — | — | — | — | 🟡 | — | ✅ |
| System-gesture exclusion | — | — | — | — | 🟡 | ✅ | — | ✅ |
| Display cutout, fold posture | — | — | — | — | — | 🟡 | ❌ | ❌ |
| **Input** | | | | | | | | |
| Mouse: buttons, hover, capture | ✅ | ✅ | ✅ | ✅ | 🟡 | 🟡 | ✅ | ✅ |
| Touch | 🟡 | — | 🟡 | 🟡 | ✅ | ✅ | 🟡 | ✅ |
| Pen and stylus | 🟡 | ❌ | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Precise scrolling with phases and momentum | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Touchpad pan as a drag or a scroll | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ✅ |
| Trackpad pinch and rotate | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Pointer lock and warp | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Keyboard: keys, modifiers, repeat, dead keys | ✅ | 🟡 | 🟡 | 🟡 | ❌ | 🟡 | 🟡 | ✅ |
| Keyboard layout and its changes | 🟡 | ❌ | 🟡 | 🟡 | ❌ | ❌ | ❌ | ❌ |
| Gamepad and D-pad: a controller's buttons and sticks, a remote's keys, moving focus by direction | ❌ | ❌ | ❌ | ❌ | ❌ | 🟡 | ❌ | 🟡 |
| Cursor shapes | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ❌ | 🟡 | ✅ |
| System back and history | — | — | — | — | ❌ | ❌ | ❌ | ❌ |
| Browser gesture suppression | — | — | — | — | — | — | 🟡 | — |
| Input device availability | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Input latency mode and high-rate sampling (raw updates, coalesced and predicted samples, one monotonic clock) | 🟡 | ❔ | ❌ | ❌ | ❔ | 🟡 | 🟡 | ✅ |
| Event timestamps converted without wrap or base skew (Win32 `GetMessageTime` 32-bit ms, Wayland undefined base, Android uptime ms) | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❔ | ❔ | — |
| **Text input** | | | | | | | | |
| IME composition and candidate placement | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Text-input client queries (selected range, surrounding text, first rect) for dictation, reconversion and handwriting | ❔ | ❔ | ❔ | ❔ | ❔ | ❔ | ❔ | — |
| Text-input session survives window move, resize, DPI change and focus loss | 🟡 | ❔ | ❔ | ❔ | ❔ | ❔ | ❔ | — |
| Soft keyboard show and hide | — | — | — | — | ❔ | ✅ | ❌ | ❌ |
| Input purpose and hints (password, number, autocorrect) | 🟡 | ❌ | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ |
| System edit menu | — | — | — | — | ❌ | ❌ | — | ❌ |
| Spell check | ❌ | ❌ | — | — | ❌ | ❌ | — | ❌ |
| Autofill (passwords, one-time codes) | — | — | — | — | ❌ | ❌ | ❌ | ❌ |
| Handwriting into text fields | ❌ | — | — | — | ❌ | 🟡 | — | ❌ |
| Secure text entry (password keystrokes and text kept from other apps, input-method learning and the clipboard history) | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| **Accessibility** | | | | | | | | |
| Tree and actions | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Properties (value, hint, description, state) | ❔ | ❔ | 🟡 | ✅ | ❔ | ❔ | ❔ | ❔ |
| Announcements and live regions | ❔ | ❔ | 🟡 | 🟡 | ❔ | ❔ | ❔ | ❔ |
| Gesture alternatives (custom actions, context menu, keys, controls) | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Assistive-technology focus (screen reader cursor) | ❌ | ❌ | ❌ | ❌ | 🟡 | 🟡 | ❌ | ✅ |
| **Transfer** | | | | | | | | |
| Plain-text clipboard | 🟡 | 🟡 | ✅ | ✅ | 🟡 | 🟡 | 🟡 | ✅ |
| Rich clipboard (images, HTML, files) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Drop into the app | 🟡 | ❌ | ✅ | ✅ | ❌ | 🟡 | 🟡 | ✅ |
| Drag out of the app | 🟡 | ❌ | ✅ | ✅ | ❌ | 🟡 | 🟡 | ✅ |
| Concealed clipboard entries (kept out of clipboard history and cloud sync) | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ | ✅ |
| **Preferences** | | | | | | | | |
| Dark mode and its changes | ✅ | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ✅ |
| Accent colour | ❌ | ❌ | ❌ | ❌ | — | ❌ | — | ❌ |
| Text scale | 🟡 | — | 🟡 | 🟡 | 🟡 | ✅ | ❌ | ✅ |
| Reduce motion | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 |
| Contrast | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ❌ |
| Bold text | — | — | — | — | 🟡 | ✅ | — | ❌ |
| Languages and regional formats | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ❌ |
| Layout direction and per-app language | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| System fonts and fallback | ❔ | ❔ | ❔ | ❔ | ❔ | ❔ | ❔ | — |
| Gesture and wheel settings | 🟡 | 🟡 | ❌ | ❌ | ❌ | 🟡 | ❌ | ✅ |
| Haptics setting | ❔ | ❔ | — | — | — | ✅ | — | ✅ |
| Power state (battery, low-power mode, thermal) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Power-aware pacing (frame cap and background pause under low-power or thermal state) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Preference changes arrive as OS notifications, never by polling | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | — |
| **Mobile system UI** | | | | | | | | |
| Status and navigation bars, immersive mode | — | — | — | — | 🟡 | ✅ | ❌ | ❌ |
| Orientation lock | — | — | — | — | ❌ | ❌ | ❌ | ❌ |
| Haptics | 🟡 | 🟡 | — | — | 🟡 | 🟡 | 🟡 | ✅ |
| System sounds, alert beep | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ |
| **Menus** | | | | | | | | |
| App menu bar | — | ❌ | — | — | ❌ | — | — | ❌ |
| Native context menus | ❌ | ❌ | — | — | — | — | — | ❌ |
| Dock menu, jump list, recent documents | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| **Services** | | | | | | | | |
| Open a URL | 🟡 | ❌ | 🟡 | 🟡 | ❌ | ❌ | 🟡 | ✅ |
| Open a file, reveal it in the file manager | 🟡 | ❌ | 🟡 | 🟡 | — | — | — | ❌ |
| Move to trash | ❌ | ❌ | 🟡 | 🟡 | — | — | — | ❌ |
| Open, save and folder dialogs | 🟡 | ❌ | 🟡 | 🟡 | ❌ | ❌ | ❌ | ❌ |
| Message box | ❌ | ❌ | — | — | ❌ | ❌ | ❌ | ❌ |
| Share | ❌ | ❌ | — | — | ❌ | ❌ | ❌ | ❌ |
| App folders and storage | ✅ | 🟡 | 🟡 | 🟡 | 🟡 | ✅ | 🟡 | ✅ |
| Persisted documents survive a kill, a read-only file, a redirected profile, a second instance and a session end | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| HTTP requests | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | 🟡 | ✅ |
| Tray and status icon | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| Local notifications | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Taskbar and Dock progress, badge | 🟡 | ❌ | ❌ | ❌ | ❌ | — | ❌ | ❌ |
| Global shortcuts | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| Launch at login | ❌ | ❌ | ❌ | ❌ | — | — | — | ❌ |
| Keep the screen awake | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Secure storage | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ |
| Sandbox-aware file and resource access (the layer never hands an app a path it cannot use) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | ❌ |
| **Packaging** | | | | | | | | |
| Package the OS installs (MSIX or installer, notarised `.app`, Flatpak and `.deb`, `.ipa`, `.aab`, web bundle) | ❔ | 🟡 | ❔ | ❔ | 🟡 | 🟡 | 🟡 | — |
| Code signing and notarisation | ❔ | ❌ | — | — | ❌ | 🟡 | — | — |
| App identity reaches the package (AUMID, bundle ID, `.desktop` file and `app_id`, manifest) | ❔ | ❔ | ❔ | ❔ | ❔ | 🟡 | 🟡 | — |
| Sandbox and entitlement manifests generated from declared capabilities | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | — | — |

### App lifecycle — Event loop, startup and wake-ups

Runs: a manual Windows run, the macOS live check, an application of `flui-app` on the native Wayland host under Weston and the native X11 host under Xvfb (`native_linux`, in CI), the iOS Simulator, the Android emulator, the Web counter. Owed: idle checks on hidden and occluded windows, which may get no frame callbacks on Wayland.


### App lifecycle — Version floor: declared, packaged, refused below it

[ADR-0244](adr/ADR-0244-os-version-floors-and-gates.md) sets a floor per environment, and `flui build` packages it (raised by `[platforms.<environment>] min` in `flui.toml`) everywhere but the Windows manifest. Windows, macOS, iOS and Android refuse a release below it at start, and Linux a compositor without `xdg_wm_base` or an X server without RandR 1.5, as `LaunchError::UnsupportedSystem`; the GPU falls back to GLES 3.0, WebGL2 or WARP and reports `NoSupportedGpu` with a reason per backend when none answers (run on llvmpipe GL only), and none has run on its floor.


### App lifecycle — Quit, exit request, last-window policy

macOS Cmd+Q and close ran; a manual Linux run checks the exit code; iOS has no quit API


### App lifecycle — Close request with veto

Web has no `beforeunload`


### App lifecycle — Reopen with no windows (Dock click)

a manual macOS run; headless can't drive it


### App lifecycle — Focus and active state

Android follows `onWindowFocusChanged` alone, and focus no longer stands for the Activity being resumed.


### App lifecycle — Visibility, minimize, occlusion

Shown, minimized and occluded are three facts of the snapshot on every host (`visible`, `mode`, `occluded`), and a minimized or covered window stays shown. Wayland reports a window occluded while the compositor suspends it or keeps it on no output once shown, never minimized; Win+L and a desktop switch have not run on Windows.


### App lifecycle — Suspend, resume, GPU-surface loss

Android dispatches `onResume` and `onPause` as the execution state, apart from focus; the Web's page events ran against a fake page, not yet in Chrome.


### App lifecycle — Configuration changes reach the running app (no Activity recreation)

Android's generated manifest declares the changes the host follows; dark mode, font scale, bold text, density and rotation ran on a Galaxy S21 without a second `android_main`. A language change still recreates the Activity.


### App lifecycle — Sleep, wake, screen lock

Windows, macOS and Linux deliver them; none ran through a real sleep or lock, and [#1736](https://github.com/vanyastaff/flui/issues/1736) lists the macOS run.


### App lifecycle — Session end (log off, shutdown) with veto

Delivered on every environment but headless, with a veto only on Windows and macOS; none ran through a real log-off or shutdown.


### App lifecycle — Memory pressure

Delivered on Windows, macOS, iOS, Android and Linux; only Android's levels were driven on a device.


### App lifecycle — Bring the app forward, request attention

Windows: `WindowCommand::Activate` brings the window forward where the foreground rules allow and flashes its taskbar button where they refuse, run against a second process; attention has no request of its own. Wayland asks with an `xdg-activation` token (sway marks the window urgent), run by hand on sway only.


### App lifecycle — Single instance, forward arguments

macOS enforces one instance itself


### App lifecycle — Deep links, URL schemes, file associations

No host reports an opened URL or file, at cold start or while running (`opened(urls, files)` joins the application handler with the activation feature); the Web entry URL is under history routing


### App lifecycle — State restoration

macOS window restoration, iOS scene restoration, Android state after process death, Web `history.state`


### App lifecycle — Activation obeys the OS focus rules (Wayland `xdg-activation` token, Windows foreground rights handed to the activated process)

Windows: an activation never takes the foreground from another application and comes forward once that application hands the right over, run with two processes; opening a link or file hands the right to the handler process the shell started; no second launch hands it yet (single instance, in the activation spec). The Wayland token's whole path runs only on sway, by hand: CI's Weston has no `xdg_activation_v1`. A Wayland token names the seat and serial of the user input behind the activation. macOS cooperative activation (`NSApplication.yieldActivation(to:)`, macOS 14) is not audited.


### App lifecycle — App identity (AUMID, bundle ID, `.desktop` file)

Notifications, URL schemes, the tray, jump lists and portals need it; `flui-cli` packaging not audited


### Window — Open and close a window

iOS: the scene supplies the window; Android: one native window, which the application cannot close (`closes` is `Unavailable`)


### Window — Multiple windows

No iPad scenes, no Android multi-window


### Window — Show without a blank first frame

Windows and X11 create the window hidden and show it after the first presented frame; Wayland maps with the first buffer, so nothing is deferred, and `show` is refused.


### Window — Launch-screen handoff: the first frame matches the OS splash

Nothing aligns the first frame with the OS splash (the iOS launch screen, Android's `SplashScreen`, the Web page before the wasm loads). Desktop is the row above.


### Window — Title

Linux: Wayland sets `xdg_toplevel`'s title and X11 `_NET_WM_NAME` and `WM_NAME`; neither lets a client read the title shown back, so the snapshot holds the one last set. Other hosts: not yet


### Window — Minimize, maximize, restore, state readback

Linux reads the mode back, on X11 from `WM_STATE` and `_NET_WM_STATE`, on Wayland from the compositor's configures, which never report minimized; sway, which tiles, carries out no maximize. macOS: the AppKit window conformance kit passes on an M1, macOS 27.0, 2026-10-27 (nextest flui-native 233/233); the green button, Cmd-M and the Dock have not run by hand ([#1736](https://github.com/vanyastaff/flui/issues/1736)).


### Window — Fullscreen

Linux asks the compositor or window manager for fullscreen (`xdg_toplevel.set_fullscreen`, `_NET_WM_STATE_FULLSCREEN`) and reads it back.


### Window — Position, size, move and resize events

Wayland can't read or set a global position by design; X11 ran on an Xvfb server. macOS: the AppKit window conformance kit passes on an M1, macOS 27.0, 2026-10-27 (nextest flui-native 233/233); a drag, the y-axis by hand and a second display have not run ([#1736](https://github.com/vanyastaff/flui/issues/1736)).


### Window — Size constraints, resizable and other flags

Windows holds limits and flags at the operating system (checked against the real system, not by dragging an edge); Linux hands them to the compositor or window manager (`xdg_toplevel`'s minimum and maximum size, `WM_NORMAL_HINTS`), run in the window conformance on Weston and under openbox on Xvfb with no edge dragged. macOS: the AppKit window conformance kit passes on an M1, macOS 27.0, 2026-10-27 (nextest flui-native 233/233); no edge was dragged against the limits by hand ([#1736](https://github.com/vanyastaff/flui/issues/1736)).


### Window — Live resize without jitter

Not audited; Windows has no `WM_ENTERSIZEMOVE` handling. It passes when every frame presented during a scripted border drag has the content size of its window.


### Window — Save and restore window placement

`AppConfig::restore_placement`. Windows opens at the remembered position, size and mode (content size, `SetWindowPlacement`) and ran in a real window.


### Window — Frameless windows, custom title bar

`WindowFrame::AppTitleBar` is in the contract, taken by the headless host, Win32, macOS and the native X11 and Wayland hosts, as are the chrome input regions (move, resize, caption buttons, ADR-0286) that answer the system's hit test and the `flui-widgets` parts that mark them (`WindowTitleBar`, `WindowCaptionButton`, `WindowResizeArea`). Win32 drops the caption and answers `WM_NCHITTEST` from the regions, `HTMAXBUTTON` included for the snap layouts, type-checked and not run; macOS keeps a transparent system title bar under full-size content, moves the window from a move region and answers a double-click as System Settings says, its traffic lights hidden or moved, type-checked and not run; the native X11 host does the same through `_NET_WM_MOVERESIZE` where the window manager lists it, run under openbox on Xvfb; the native Wayland host asks for no frame, keeps the margins of its own and moves and maximizes the window from them, run on Weston 13.


### Window — Backdrop material, transparency, corners

The contract's material (`WindowCommand::SetMaterial`, `WindowMaterial::{Opaque, System}`, `WindowCapabilities::material`): a Win32 window opens on Mica (`System`) and changes it from build 22621 on, and below that build the capability is unavailable on this device and admission answers `NotOnThisDevice`; ran in a Win32 window on Windows 11 (build 26300), the desktop window manager's attribute read back after each change. macOS draws `System` as `NSVisualEffectView`'s window-background material, type-checked only. Linux and Web report the material `Unavailable`; no window draws transparent content. On Windows the corners and title-bar theme are creation options (`Win32WindowOptions`), and `flui::native::windows` sets the backdrop, corners and theme or answers why not; ran in a Win32 window. macOS draws an `NSVisualEffectView` material or Liquid Glass (`NSGlassEffectView`, macOS 26, found by name) behind the content through `flui::native::macos` (`AppKitWindow::set_backdrop`, ADR-0254 §3), which ran on macOS 27 (M1, 2026-10-27); the contract's material on macOS is type-checked only, and the cell stays 🟡 for it and for transparency


### Window — Owned, modal and popup windows; always on top

Not in the contract, so menus, tooltips and dropdowns can't leave the window; Wayland needs `xdg_popup`


### Window — Window icon

`WM_SETICON`, `xdg-toplevel-icon`, `_NET_WM_ICON`


### Window — Exclude a window from screen capture

`SetWindowDisplayAffinity`, `NSWindow.sharingType`, iOS capture detection, Android `FLAG_SECURE`


### Window — Native views inside the UI (web view, map, video)

Not in the contract; it shapes compositing, hit-testing and accessibility


### Display — Displays: list, work area, current display, changes

Windows, Wayland, X11, Android and the Web ran live; macOS and iOS are type-checked only. A move between two monitors ran only on Windows, and a Windows disconnect did not run.


### Display — Scale factor and DPI changes

Web watches `(resolution: Xdppx)` and re-arms it after each change for the display snapshot.


### Display — Refresh rate and frame pacing

Every host reads the mode's rate or estimates the period; macOS and iOS are type-checked only. A display that reports none uses 60 Hz, with a diagnostic.


### Display — Present mode, queue depth, VRR and tearing control

The engine always presents with `Fifo` and a frame latency of 2; nothing lets the platform layer or the app choose a present mode, a queue depth, variable-refresh behaviour or tearing.


### Display — HDR, wide gamut and colour management (surface colour space, SDR/HDR headroom, display profile changes)

Every display carries its colour profile (`DisplayInfo::color`: colour space and headroom, or why there is none), and the engine sets a surface up for the colour of its window's display (ADR-0256). Windows read an SDR monitor correctly; no HDR or P3 display has run anywhere, and sources stay sRGB. X11 reads no colour (`NotYetOnThisHost`); the Web reports capability and no headroom.


### Display — HDR and SDR-white change events

A profile change is a new display snapshot revision. Windows re-reads on `WM_DISPLAYCHANGE` (the SDR white level moving raises none), macOS on the screen-parameters notification, Wayland on `image_description_changed`, the Web on the colour queries' `change`; none ran with a real change. iOS and Android re-read only on their other display events. The window receives the change as a new `WindowDisplay`, and its renderer configures the surface again before the next frame (ADR-0256 §6, amended).


### Display — Presentation feedback: when each frame reached the screen

A frame request carries when the window's newest earlier frame was shown (`FrameRequest::presented`, on the event timeline input is stamped on); the frame clock keeps it and bounds idle work by the next refresh. Windows reads the DXGI frame statistics of the renderer's swap chain; headless reports what a test injects. Wayland reads `wp_presentation_time`, run on Weston 13. The Web (a `requestAnimationFrame` stamp is a frame's start) and X11 (a present completes on the GPU driver's connection) have none, and their windows say so; macOS reads a presented handler on each drawable of the view's own `CAMetalLayer`; iOS and Android do not report yet.


### Display — GPU surface

Rendered in every run above


### Display — Safe area

macOS ignores the notch in fullscreen; Web doesn't read `env(safe-area-inset-*)`.


### Display — Soft-keyboard insets

In the contract (ADR-0277): a window reports the keyboard's target and its motion, and each frame lays out with the occlusion at its vsync; headless scripts a motion. Android reports the keyboard (`Type.ime()` from API 30, the system window bottom below) and, from API 30, its motion from `WindowInsetsAnimation.Callback`, type-checked, no device run; below API 30 it jumps to the target. iOS reports the keyboard and its motion from `UIKeyboardWillChangeFrameNotification` (end frame, duration, curve), type-checked, no device run. The Web reports the keyboard at rest, from `navigator.virtualKeyboard` (which the window has overlay the page) or else the visual viewport, run against a fake page on Node, not in a mobile browser.


### Display — System-gesture insets

`WindowInsets::system_gestures` reaches `MediaQueryData::system_gesture_insets` (ADR-0277). Android reads `getSystemGestureInsets()` from API 29 (`NotOnThisDevice` below), type-checked, no device run.


### Display — System-gesture exclusion

Android's exclusion rects ran on a Galaxy S21; the iOS path never ran and was not type-checked on the authoring host.


### Display — Display cutout, fold posture

Android folds the cutout into the safe area; no fold posture


### Input — Mouse: buttons, hover, capture

The iPad pointer hovers, scrolls, pinches, rotates and reads its secondary button, type-checked only, never run on an iPad; Android ran no hardware mouse, hover or capture.


### Input — Touch

Linux translates `wl_touch` and XInput 2 touches, held by their translation tests only; no touch device ran on either.


### Input — Pen and stylus

macOS doesn't translate tablet events; Wayland (`tablet-v2`) and X11 (XInput 2 pens) are held by their decoders only; no hardware pen has run on Windows, Android or the Web.


### Input — Precise scrolling with phases and momentum

Windows' precision-touchpad pan is implemented but no touchpad has run; Android's two-finger scroll ran only from a `uinput` touchpad. the iPad trackpad's scroll is type-checked only, never run on an iPad; the Web has no phases.


### Input — Touchpad pan as a drag or a scroll

Runs on every host that sends phased precise scroll (see the row above); headless runs it with synthetic packets, no touchpad has. the iPad trackpad is type-checked only; the Web has no phases.


### Input — Trackpad pinch and rotate

Windows, Android and the Web decode a pinch, none from a hardware trackpad; the iPad decodes a pinch and a rotation, type-checked only; Wayland (`pointer-gestures`) and X11 (XInput 2.4 gestures) decode a pinch, held by their decoders only.


### Input — Pointer lock and warp

Not in the contract


### Input — Keyboard: keys, modifiers, repeat, dead keys

iOS handles no hardware keyboard; Android ran hardware key events through a manual Android run, with no repeat, dead key or physical keyboard.


### Input — Keyboard layout and its changes

Web hard-codes `en-US`; Wayland and X11 read the layout through XKB and report its changes, not run with a layout switched


### Input — Gamepad and D-pad: a controller's buttons and sticks, a remote's keys, moving focus by direction

No gamepad source on any host (XInput or GameInput, `GCController`, Android `SOURCE_GAMEPAD` axes, the Web Gamepad API, evdev); Android's D-pad arrives as the arrow keys and Enter through the key table, and the arrows drive directional focus, never run on a TV.


### Input — Cursor shapes

No iPad pointer styles or Android `PointerIcon`; no custom image or hide-until-move


### Input — System back and history

Android predictive back (from targetSdk 36, `KEYCODE_BACK` isn't delivered), the iOS edge swipe, the Web History and Navigation APIs


### Input — Browser gesture suppression

The canvas sets `touch-action: none` inline; `overscroll-behavior` is not set, and it never ran in Safari or on a phone.


### Input — Input device availability

Headless, Wayland and X11 report presence changes, the other hosts only their initial table so far; none ran on its environment.


### Input — Input latency mode and high-rate sampling (raw updates, coalesced and predicted samples, one monotonic clock)

`AppConfig::with_input_latency` sets `WindowOptions::input_latency`; Android takes unbuffered pointer dispatch for a `Low` window from API 30 (type-checked).


### Input — Event timestamps converted without wrap or base skew (Win32 `GetMessageTime` 32-bit ms, Wayland undefined base, Android uptime ms)

Windows pointer ticks, the Apple uptime seconds and the Wayland and X11 millisecond stamps are rebased onto `EventTime` without a wrap, a Linux stamp only within a trusted skew of its arrival; Android and the Web are not audited, and nothing ran across a wrap or a clock-base change.


### Text input — IME composition and candidate placement

Windows TSF and macOS `NSTextInputClient` haven't run with a real input method. iOS composes through an invisible `UITextInput` responder, which now edits the field through the shared text-store host as macOS does (type-checked; the run below predates it): on the iOS 27.1 Simulator (2026-10-28, an M1 host, on-screen keyboard driven by XCUITest) the Japanese Romaji keyboard composed `にほんご` and committed `日本語`, and the nine-key Pinyin keyboard composed `ni hao` and committed `你好`, each reaching an `EditableText`; candidate placement was not observed (the Simulator device gives no screenshot) and no device ran. Android ran with Samsung Keyboard on a Galaxy S21 only, the Web through Chrome's own input-method path only.


### Text input — Text-input client queries (selected range, surrounding text, first rect) for dictation, reconversion and handwriting

The text store has the shape of TSF's `ITextStoreACP`, and iOS's responder answers `UITextInput`'s range and rect queries from the field's store and layout (type-checked), but dictation, reconversion and first-rect queries are not audited on any host.


### Text input — Text-input session survives window move, resize, DPI change and focus loss

Windows tells TSF the focused field's layout changed on a move, a size-and-move loop and a DPI change, checked with no input method composing; the other hosts are not audited.


### Text input — Soft keyboard show and hide

Android shows Samsung Keyboard when a field takes focus and hides it when none does. iOS shows the system keyboard when a field takes focus (the Simulator run above); hiding it when focus leaves did not run.


### Text input — Input purpose and hints (password, number, autocorrect)

Windows, Android, iOS and the Web map the purpose, and only Android's plain single-line field ran on a device; Wayland (`text-input-v3`) and X11 (IBus and Fcitx5, not XIM) send the purpose, unit-tested only; otherwise it is not in the contract.


### Text input — System edit menu

`UIEditMenuInteraction` (its Paste avoids the iOS paste prompt), Android floating `ActionMode`


### Text input — Spell check

`ISpellChecker`, `NSSpellChecker`, `UITextChecker`, `SpellCheckerSession`


### Text input — Autofill (passwords, one-time codes)




### Text input — Handwriting into text fields

Windows Ink and iPad Scribble are missing; Android asks `startStylusHandwriting` (API 33) and never ran (the S21 takes no S Pen).


### Text input — Secure text entry (password keystrokes and text kept from other apps, input-method learning and the clipboard history)

macOS holds `EnableSecureEventInput` while a protected field has focus in the key window and iOS sets `isSecureTextEntry` (both type-checked, not run); Wayland sends a protected field as the `text-input-v3` password purpose with the hidden-text and sensitive-data hints, and X11 sends IBus and Fcitx5 the password purpose (XIM has none), unit-tested only.


### Accessibility — Tree and actions

The desktop adapters are built into every window, with no feature, iOS's into every UIKit window, Android's into every Activity window, and every Web canvas mirrors its tree as a parallel ARIA DOM; none is checked with a screen reader.


### Accessibility — Properties (value, hint, description, state)

X11: an AT-SPI client reads role, name, hint, a field's text, a slider's value and range, states and actions (`linux_atspi`); Wayland has the same adapter, not run on a Wayland window. The Web writes states, ranges and descriptions as ARIA attributes.


### Accessibility — Announcements and live regions

X11: an AT-SPI client hears announcements, live regions and a status that appears (`linux_atspi`), and a route change by name (`a_route_change_is_heard_by_its_name`).


### Accessibility — Gesture alternatives (custom actions, context menu, keys, controls)

Checked headless by `assert_gesture_alternatives` (ADR-0216); no locked AccessKit adapter routes custom actions or `ShowContextMenu`, so assistive technology reaches them only through the controls.


### Accessibility — Assistive-technology focus (screen reader cursor)

`Semantics::on_assistive_focus_within` hears the cursor where an adapter reports it; iOS and Android do through vendored `accesskit_ios` and `accesskit_android` changes, not yet run with VoiceOver or TalkBack, and no other adapter does ([gap 36](gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)); checked headless by `the_screen_reader_entering_the_carousel_pauses_autoplay`.


### Transfer — Plain-text clipboard

Wayland reads and writes its seats' selection through `wl_data_device`, run between two clients of Weston 13; X11 owns and reads `CLIPBOARD` on a connection of its own, run between two clipboards on Xvfb; Android reads and writes `ClipboardManager` through JNI, type-checked, no device run; the Web reads `navigator.clipboard` asynchronously, unrun in a browser, and falls back to its own writes


### Transfer — Rich clipboard (images, HTML, files)




### Transfer — Drop into the app

Wayland and X11 take drops from a source of another client, run under sway 1.9 and on Xvfb; Windows, the Web and Android take drops, run within one process, in headless Chrome or on a Galaxy S21 only; none ran from Explorer or another app or page.


### Transfer — Drag out of the app

Wayland and X11 start OS drags, taken by a target of another client under sway 1.9 and on Xvfb; Windows, the Web and Android start them too, none run into another app or page; macOS and iOS report outgoing drags `Unavailable`.


### Transfer — Concealed clipboard entries (kept out of clipboard history and cloud sync)

`ClipboardWrite::concealed` (ADR-0152 §6); each host answers its `Concealment`. X11 and Wayland offer KDE's hint and Android its sensitive extra, which only what reads them honours, so those cells stay 🟡 by design; the Web has no mark and fails the scenario as gap 26.


### Preferences — Dark mode and its changes

Every window seeds `MediaQueryData::platform_brightness` and follows changes; only Windows and Android ran a live flip.


### Preferences — Accent colour

Not in the contract


### Preferences — Text scale

iOS and Android answer each size natively, and Android ran a live change on a Galaxy S21; Windows applies its one `TextScaleFactor` linearly, while WinUI scales larger text less; Linux reads GNOME's `text-scaling-factor` through the Settings portal, linear as in GTK (ran in WSL against a stand-in portal, not in a GNOME session). Headless: `the_system_text_scale_grows_each_span_once`, `a_field_measures_and_hit_tests_the_current_text_scale`


### Preferences — Reduce motion

Headless injects preferences through `HeadlessHost::set_system_preferences`; no reduce-motion row drives it yet


### Preferences — Contrast

Android reads the system contrast level (API 34+) and the high-contrast-text setting and follows both live: on a Galaxy S21 (Android 15) `high_text_contrast_enabled` and `contrast_level` changes reached `MediaQuery` without a restart. macOS reads `accessibilityDisplayShouldIncreaseContrast` on the workspace's display-options notification, iOS `UIAccessibilityDarkerSystemColorsEnabled` on its status notification (both type-checked only). Linux reads the Settings portal's `contrast`, or GNOME's `high-contrast`, and follows their changes (ran in WSL against a stand-in portal). The Web reads `prefers-contrast` and `forced-colors` and follows their changes: `browser_contrast_and_languages_reach_their_owner` runs on Node against a controlled browser boundary; not run in a browser


### Preferences — Bold text

Android ran a live change on a Galaxy S21


### Preferences — Languages and regional formats

Every host reads the languages (Android from the configuration's `LocaleList`, `[en-US]` on a Galaxy S21, a change not run; macOS and iOS from `NSLocale.preferredLanguages` on the current-locale notification, type-checked only; Linux from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` as gettext reads them, once at start; the Web from `navigator.languages`, following `languagechange`; run on Node against a controlled browser boundary, not in a browser); regional formats ([ADR-0278](adr/ADR-0278-regional-formats-are-the-unicode-locale-extension-keys.md)) on Windows from Region settings (first weekday, clock, units, native digits, formats region; type-checked only) and on Linux from `LC_TIME`, `LC_MEASUREMENT` and GNOME's `clock-format` (not run on a desktop), on macOS and iOS from Foundation's autoupdating locale and calendar (first weekday, the `j` skeleton's clock, units, the `numbers` keyword, the country; type-checked only), on Android from the default locale's `fw` and `nu` keywords, the calendar and `is24HourFormat`, following `TIME_12_24` (type-checked only), and on the Web from `Intl`'s default hour cycle and week info (not run in a browser)


### Preferences — Layout direction and per-app language

The platform layer exposes no layout direction and no per-app language override (Android 13 and iOS let the user pick a language for one app).


### Preferences — System fonts and fallback

Text layout resolves fonts through `fontdb` in `flui-painting`. `flui_native::fonts::system_fallback` asks each host which faces fall back (DirectWrite, CoreText, fontconfig, `AFontMatcher` on Android from API 29 and `fonts.xml` below it) and the collection tries them ahead of FLUI's tables (ADR-0271); fontconfig's answer is checked on the Linux CI host, the others are type-checked only, and no OS has rendered the CJK and emoji check yet.


### Preferences — Gesture and wheel settings

The Windows wheel smoke passed; the timings never ran.


### Preferences — Haptics setting

Android reads `HAPTIC_FEEDBACK_ENABLED` and ran on a Galaxy S21; iOS has no public read API, and the Windows pen and macOS Force Touch settings are not audited.


### Preferences — Power state (battery, low-power mode, thermal)

Not in the contract


### Preferences — Power-aware pacing (frame cap and background pause under low-power or thermal state)

Power state is not read, so nothing lowers the frame rate or pauses background work under a low-power, battery-saver or thermal state.


### Preferences — Preference changes arrive as OS notifications, never by polling

Android's `FluiView` reports setting, contrast and inset changes; on a Galaxy S21 the loop made no turn in 10 s idle, resumed or behind Home. macOS wakes on AppKit notifications and app activation (type-checked only). No host polls a healthy, installed source: Android keeps a 500 ms cadence only while `FluiView` is not in the window, and Linux retries a failed portal read every second.


### Mobile system UI — Status and navigation bars, immersive mode

Status-bar icon style ran on Android; iOS is type-checked only, and no host has navigation-bar or immersive control.


### Mobile system UI — Orientation lock

Android 16 ignores it on large screens


### Mobile system UI — Haptics

Both tiers and the frame queue ship (ADR-0210); macOS, iOS and the Android envelopes never ran on a device, and no haptic pen has vibrated.


### Mobile system UI — System sounds, alert beep




### Menus — App menu bar

A macOS app without one has no Quit or Edit menu; iPadOS 26 has a menu bar; elsewhere the app draws its own


### Menus — Native context menus

The mobile system edit menu is under Text input; the in-app, theme-free `ContextMenuArea` draws its own menu on every platform, run headless only


### Menus — Dock menu, jump list, recent documents

Linux: desktop actions in the `.desktop` file


### Services — Open a URL

Linux: `flui-system`'s launcher through the `OpenURI` portal, run against a stand-in portal on a private bus, not a desktop. Windows and Web: the core `open_url`, which nothing calls


### Services — Open a file, reveal it in the file manager

Linux: `OpenFile` and `OpenDirectory` portals with a descriptor, run against a stand-in portal; the file manager decides whether the file is selected


### Services — Move to trash

Linux: the `Trash` portal, run against a stand-in portal


### Services — Open, save and folder dialogs

Linux: `flui-system`'s dialogs through the `FileChooser` portal, filters, title, folder and accept label, modal to an X11 window; on Wayland not modal until the host exports an `xdg-foreign` handle; run against a stand-in portal. Windows has no filters, title or owner window; every other host answers "cancelled"


### Services — Message box

Linux has no native one: the app draws it


### Services — Share




### Services — App folders and storage

An application identifier or `AppConfig::with_storage_dir` gives widgets storage on every host; it ran on Windows and Android, and no Web app has run the probe. Headless keeps each application's values in memory for the platform's life, or on disk under `HeadlessPlatform::with_data_root`, never in the user's directories. Android `app_path` still reports a fixed path.


### Services — Persisted documents survive a kill, a read-only file, a redirected profile, a second instance and a session end

`Persisted<D>` over the host's storage and flush registry (ADR-0262); the acceptance app's persistence rows run headless, and no native process has run them


### Services — HTTP requests

A capability of the host (ADR-0280): `ureq` on Linux, WinHTTP on Windows, `NSURLSession` on macOS and iOS, the page's `fetch` on the Web, `HttpURLConnection` on Android, each exchange held on the IO lane, so an embedder's executors send on every host


### Services — Tray and status icon

Linux uses StatusNotifierItem


### Services — Local notifications

Needs app identity


### Services — Taskbar and Dock progress, badge

Windows `ITaskbarList3` sits in an extension nothing can reach; Linux `LauncherEntry`; Web Badging API


### Services — Global shortcuts

Wayland only through the GlobalShortcuts portal


### Services — Launch at login




### Services — Keep the screen awake




### Services — Secure storage

Credential Manager or DPAPI, Keychain, Secret Service, Keystore


### Services — Sandbox-aware file and resource access (the layer never hands an app a path it cannot use)

`data_dirs` and the dialogs assume an unsandboxed process: no security-scoped bookmarks, portal file access, AppContainer-aware paths, Storage Access Framework URIs or iOS document-scoped URLs.


### Packaging — Package the OS installs (MSIX or installer, notarised `.app`, Flatpak and `.deb`, `.ipa`, `.aab`, web bundle)

`flui build` stages an unsigned `.app`, a debug or release APK (no AAB) and a `wasm-bindgen` bundle; Windows and Linux packaging not audited.


### Packaging — Code signing and notarisation

Android signs with the debug or a given key; no macOS or iOS signing, no Windows Authenticode, no notarisation


### Packaging — App identity reaches the package (AUMID, bundle ID, `.desktop` file and `app_id`, manifest)

See the App identity row above; Android's manifest is generated by the CLI, the Web manifest is a template


### Packaging — Sandbox and entitlement manifests generated from declared capabilities

No manifest is derived from the capabilities an app declares: the CLI generates no entitlements, Flatpak or Snap permissions, MSIX capabilities or Android permissions from them (its Android template declares `VIBRATE` itself).


