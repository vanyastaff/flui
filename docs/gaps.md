# Where FLUI falls short

> **Last reviewed:** 2026-10-17 · **Last updated:** 2026-10-17 · **Change:** minor (storage wired on every backend; status-bar icon style)

The work list. One entry per shortfall, ranked by how much it stops someone from choosing FLUI for a
production app on that environment. Each entry says what is missing, the evidence, who it hurts, an
agent wall-clock estimate, and the parity document that tracks it. Every entry blocks a ready
platform ([`platform-parity.md`](platform-parity.md)). Pick from the top, check open pull requests
first (`gh pr list`), and when a gap closes, update this file and the parity document in the same
pull request.

## Ranked

| # | Gap | Environments | Evidence | Who it hurts | Hours | Doc |
|---|---|---|---|---|---|---|
| 1 | **No text input on mobile and the Web**: no IME connection, soft keyboard, keyboard insets or input hints | iOS, Android, Web | No `UITextInput`; NativeActivity has no IME, so Android needs GameActivity or a Java `InputConnection`; no hidden input element on the Web (`EditContext` is Chromium-only) | Every app with a text field | 45–75 | [Text input](platform-parity.md#text-input) |
| 2 | **No accessibility on mobile and the Web; desktop off by default and untested with a screen reader** | iOS, Android, Web; desktop | AccessKit has iOS and Android adapters but none for the Web (it needs a parallel ARIA DOM); the desktop adapters sit behind the `a11y` feature; the runs read one control, not a Narrator, VoiceOver or Orca session | Blind and low-vision users; anyone who must meet accessibility law | 30–50 | [Accessibility](platform-parity.md#accessibility) |
| 3 | **No system back** | Android, iOS, Web | From targetSdk 36 Android delivers neither `onBackPressed` nor `KEYCODE_BACK`, so only predictive back works; iOS needs its own edge swipe; the Web needs History or Navigation API routing | Every multi-screen mobile or web app | 10–16 | [Input](platform-parity.md#input) |
| 4 | **Most implemented rows never ran on their environment** | all | 🟡 dominates every column of the parity summary; device checks exist for a handful of rows, and the macOS and iOS drivers are Python and Swift | Every claim of support | 40–80 | [Summary](platform-parity.md#summary) |
| 5 | **Native Linux backend (Wayland and X11)** replaces the winit fallback | Wayland, X11 | winit leaves the clipboard inert on pure Wayland, reports no occlusion on Wayland, has no pen, no pinch, no window bounds | Linux users | 150–250 | [Summary](platform-parity.md#summary) |
| 6 | **Clipboard is broken on three environments; no rich formats** | Android, Wayland, Web; all for rich | Android is an in-process mock; pure Wayland without data-control is inert; Web reads only its own writes | Every app with copy and paste | 14–22 | [Clipboard](platform-parity.md#clipboard-and-drag-and-drop) |
| 7 | **File dialogs, open URL, reveal, trash, message box and share exist on Windows only, or nowhere** | all | Every non-Windows backend answers a file dialog with "cancelled"; `open_url` is real on Windows and Web only, and nothing calls it | Every document app; any app with a link | 25–42 | [System services](platform-parity.md#system-services) |
| 8 | **No drag and drop** | all | No `IDropTarget`, no `NSDraggingDestination`; winit's dropped files are discarded by `flui-runtime`; no drag source anywhere | Document, media and file-manager apps | 22–36 | [Clipboard and drag and drop](platform-parity.md#clipboard-and-drag-and-drop) |
| 9 | **Apps can't control their windows** | desktop | No API for title, position, state or constraints; Windows ignores min/max size and resizable; no state readback on macOS and Linux; Windows and X11 flash a blank first frame; no placement restore | Every desktop app | 18–32 | [Windows](platform-parity.md#windows) |
| 10 | **No custom title bar, popups or owned windows** | desktop | Undecorated Windows windows can't be dragged or resized; no caption hit-testing or snap layouts; menus, tooltips and dropdowns can't leave the window | Apps with modern chrome; every menu and dropdown | 22–36 | [Windows](platform-parity.md#windows) |
| 11 | **No native menus** | macOS, iPadOS; Windows | No macOS menu bar (no Quit or Edit menu), no native context menus, no Dock menu or jump list | Every macOS app | 20–32 | [Menus](platform-parity.md#menus) |
| 12 | **Lifecycle holes** | all | Session end is never delivered and `flui-app` can't register it; Android never dispatches visibility or execution state; Web ignores page freeze and context loss; no sleep, wake or memory-pressure events | Apps that save state or hold resources | 20–35 | [App lifecycle](platform-parity.md#app-lifecycle) |
| 13 | **No deep links, URL schemes, file associations or single instance** | all | `on_open_urls` is never called; packaging (app identity) not audited | Apps opened from links, files or a second launch | 15–25 | [App lifecycle](platform-parity.md#app-lifecycle) |
| 14 | **Most system preferences are read on Windows only** | all but Windows | Contrast, languages and regional formats, Linux and Web text scale, accent colour, gesture timings; headless can't inject any | Users who rely on accessibility settings or another locale | 20–35 | [System preferences](platform-parity.md#system-preferences) |
| 15 | **Pointer and keyboard gaps** | various | No pen on macOS or Linux; no trackpad pinch on Windows or the Web; no precise-scroll phases on Windows or the Web; iPad pointer and hardware keyboard missing; no cursors on mobile; keyboard layout read on Windows only | Pen, trackpad and iPad users | 30–50 | [Input](platform-parity.md#input) |
| 16 | **Mobile system UI** | iOS, Android, Web | Status-bar icon style ran on Android, iOS is type-checked only; no navigation-bar or immersive control (Android enforces edge-to-edge from targetSdk 35); no orientation lock; no haptics; no safe area on the Web or for the macOS notch | Every mobile app | 14–24 | [System UI](platform-parity.md#system-ui) |
| 17 | **Displays and frame pacing** | Windows, Android; all for changes | Windows reports no refresh period and always the primary display; Android hard-codes its display; no display-change events anywhere | High-refresh and multi-monitor users | 10–16 | [Displays and metrics](platform-parity.md#displays-and-metrics) |
| 18 | **Desktop shell services** | desktop | No tray, notifications, taskbar progress or badge, global shortcuts, launch at login, keep-awake or secure storage | Background, messaging and utility apps | 37–60 | [System services](platform-parity.md#system-services) |
| 19 | **Text services** | all | No spell check, system edit menu, autofill or handwriting | Writing apps; sign-in forms; tablet users | 22–36 | [Text input](platform-parity.md#text-input) |
| 20 | **No native views inside the UI** (web view, map, video) | all | Not in the contract; the design shapes compositing, hit-testing and accessibility | Apps that embed a map, browser or player | 30–60 + owner decision | [Windows](platform-parity.md#windows) |
| 21 | **Window looks**: backdrop materials, transparency, icon, capture exclusion | desktop; mobile for capture | Windows forces Mica; macOS Liquid Glass unreachable; no window icon on Windows or Linux | Apps with a native look; apps showing sensitive data | 9–16 | [Windows](platform-parity.md#windows) |
| 22 | **Unaudited rows** | desktop, Web | App identity and packaging, live resize | Unknown | 2–4 | [Windows](platform-parity.md#windows) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | major | First version, from the first parity table |
| 2026-10-17 | minor | Dark mode leaves the list: every backend reads the user's theme and reports its changes; its live runs belong to "never ran" |
| 2026-10-17 | minor | Storage leaves the list: `with_storage_dir` gives widgets storage on every backend, run on Windows and Android |
