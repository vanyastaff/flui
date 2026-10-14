# Desktop platform-layer feature checklist (candidate list)

Snapshot taken 2026-10-14. This lists every capability that a production desktop UI framework's platform layer is
expected to cover. The list is deduplicated across Electron, Tauri v2, winit, GPUI, Flutter desktop and Compose
Multiplatform Desktop, and each entry gives the native API for Windows, macOS, Linux-Wayland and Linux-X11. It is
meant to be diffed against FLUI's platform contract and backends. Feature IDs are stable labels for that diff.

## Legend

**Reference codes** (the "Refs" line of each entry):

| Code | Framework | "core" means | Other notations |
|---|---|---|---|
| E | Electron (latest docs, `electron/electron` `docs/api`) | built-in module | all of Electron is core |
| T | Tauri v2 | `tauri` crate / `@tauri-apps/api` | `T:plugin-x` = official plugin in `tauri-apps/plugins-workspace` |
| W | winit `master` (0.31 beta) | `winit-core` trait | `W(plat)` = per-platform extension trait. Scope is windowing + input only, so app services are "out of scope" |
| G | GPUI (`zed-industries/zed` `main`) | method on `Platform`/`PlatformWindow` | `G(mac)` etc. = implemented on that OS only |
| F | Flutter desktop | engine embedder or framework | `F:pkg` = first-party `flutter/packages`; `F:community` = third-party pub.dev only; `F(exp)` = experimental multi-window API (`widgets/_window.dart`) |
| C | Compose Multiplatform Desktop | `compose.desktop` / `compose.ui` | `C(AWT)` = reachable only through JDK `java.awt.Desktop` / `java.awt.Taskbar` / Swing, not a Compose API |

A dash (`E -`) means the framework does not provide it.

**Tier** (the tag after each feature name):

- `U` (universal core): core in every reference whose scope covers the feature. winit counts only for windowing,
  input, DnD and theme.
- `C` (common core): core in 3 or more references.
- `P` (plugin-level): usually shipped as a Tauri plugin or Flutter package (Electron has it in core), and core in
  fewer than 3 references.
- `S` (specialized): at most 2 references, or a concept that exists on one OS only.

**OS columns:** "n/a" means the OS has no such concept or no API for apps. "Linux (both)" means the service does not
depend on the display server (D-Bus, xdg-desktop-portal or files), so it is the same on Wayland and X11. A Wayland
protocol marked *(staging)* or *(unstable)* is not in the stable set, and compositor support varies.

## The references at a glance

| Ref | Windowing substrate | Linux implementation | Relevance to FLUI |
|---|---|---|---|
| E | Chromium (Win32 / Cocoa / Ozone) | Ozone with native Wayland **and** X11 backends; GTK or portal for dialogs; SNI tray; D-Bus notifications | widest app-service surface |
| T | tao (winit fork) + wry webview | GTK3 + WebKitGTK; menus via `muda`, tray via `tray-icon` (appindicator), dialogs via `rfd` | shows the core vs plugin split |
| W | own backends per OS | own Wayland (sctk) and X11 backends | closest analog for windowing/input |
| G | own backends `gpui_windows`/`gpui_macos`/`gpui_linux` | own Wayland + X11; `ashpd` portals; `oo7` secrets; `notify-rust`; AccessKit | closest analog overall: a Rust framework with its own native backends |
| F | per-OS embedders | GTK3 embedder; reads settings through the portal | many services pushed to packages |
| C | AWT/Swing (JetBrains Runtime) | AWT on X11/XWayland | services via JDK `Desktop`/`Taskbar` |

## Cross-cutting findings

1. **App identity comes before many services.** Toasts, jump lists and taskbar grouping on Windows need an
   AppUserModelID. Notifications, URL schemes and the Keychain on macOS need a bundle ID, an Info.plist and signing.
   On Linux, the Wayland `app_id`, portals, URL schemes, SNI and LauncherEntry all need a `.desktop` file that matches
   the app ID. Electron's docs say `xdg-desktop-portal` 1.21+ refuses sessions for app IDs it cannot map to a
   `.desktop` file, and that the GlobalShortcuts portal requires a valid portal identity.
2. **Wayland removes several capabilities that X11, Windows and macOS have.** A client cannot read or set a window's
   global position, query the global cursor position, read the work area or the primary monitor, set always-on-top,
   pin a window to all workspaces, steal focus without an activation token, read the clipboard while unfocused, or
   grab global keys. Each has a replacement or a hard "n/a": `xdg_popup` + `xdg_positioner`, `xdg-activation`, the
   GlobalShortcuts portal, `xdg-session-management` *(staging)*. Electron's docs list several of these as
   unsupported on Wayland.
3. **GTK-backed frameworks inherit a lot that FLUI must build itself.** Tauri and Flutter get CSD, IME, clipboard,
   DnD, cursor themes, settings and AT-SPI from GTK. Own-backend frameworks (GPUI, winit, Chromium Ozone) had to
   implement each of those. For FLUI's native Linux backend the CSD, text-input/XIM, data-device/XDND, xcursor,
   XSETTINGS/portal-settings and AT-SPI rows below are the expensive ones.
4. **Universally core:** the event loop, window lifecycle and states, DPI and monitors, pointer, keyboard and IME
   input, cursors, theme detection, close/quit with veto, and accessibility (winit leaves it to the external
   `accesskit_winit` adapter; Compose has no Linux support). Clipboard text and file drop are core everywhere except
   winit (no clipboard), Flutter (text only, no DnD in core) and Tauri (clipboard is a plugin).
5. **Typically plugin-level:** single instance, deep links/URL schemes, autostart, global shortcuts, notifications
   (core in E, G, C), file and message dialogs (core in E, G), the OS opener and trash, secure storage, window-state
   persistence, the updater. Tauri's plugin list is the cleanest statement of that boundary.
6. **Specialized but expected by power users:** macOS window tabs, the Services/Dock menus and the document-edited
   marker; Windows jump lists, the thumbnail toolbar and overlay icons; Linux PRIMARY selection and SSD negotiation;
   screen capture; printing; color and font pickers. Printing and the color/font pickers are absent from almost
   every reference.

---

## 1. Event loop and threading

**EL-01 Main-thread event loop** `U` - Run the OS message pump on the UI thread and dispatch window and input events.
- Win: `GetMessageW`/`PeekMessageW`/`MsgWaitForMultipleObjectsEx` + `DispatchMessageW` → `WndProc`.
- macOS: `NSApplication run` / `CFRunLoop` on the main thread (AppKit is main-thread-only).
- Wayland: poll the `wl_display` fd with `wl_display_prepare_read` → `read_events` → `dispatch_pending`.
- X11: poll the xcb/Xlib connection fd.
- Refs: E · T · W · G · F · C

**EL-02 Cross-thread wake-up / user events** `U` - Wake the UI loop from another thread to deliver work.
- Win: `PostMessageW` to a message-only window (`HWND_MESSAGE`).
- macOS: `CFRunLoopSourceSignal` + `CFRunLoopWakeUp`, or `dispatch_async` to the main queue.
- Wayland / X11: an `eventfd`/pipe in the poll set (X11 can also send a `ClientMessage` to its own window).
- Refs: E · T · W (`EventLoopProxy`) · G (`PlatformDispatcher`) · F · C

**EL-03 Timers and vsync-aligned frame scheduling** `U` - Schedule redraws on the display's cadence.
- Win: `SetTimer`, waitable timers; `DwmFlush`, the DXGI frame-latency waitable object, `DCompositionWaitForCompositorClock` (Win11).
- macOS: `CADisplayLink` (`NSView`/`NSScreen.displayLink`, macOS 14+); `CVDisplayLink` (deprecated).
- Wayland: `wl_surface.frame` callbacks, `wp_presentation` (stable), `fifo-v1` / `commit-timing-v1` *(staging)*.
- X11: Present extension (`PresentCompleteNotify`); `_NET_WM_FRAME_DRAWN`/`_NET_WM_FRAME_TIMINGS` under compositing WMs.
- Refs: E · T · W (`request_redraw`, `pre_present_notify`) · G (`on_request_frame`) · F · C

**EL-04 Rendering during OS modal loops** `U` (implicit) - Keep animating while the OS runs a nested loop.
- Win: the modal move/size loop (`WM_ENTERSIZEMOVE`…`WM_EXITSIZEMOVE`), `TrackPopupMenu` and `DoDragDrop` block the caller, so the app must render from `WM_TIMER`/`WM_PAINT`.
- macOS: live resize and menu tracking run in `NSEventTrackingRunLoopMode`, so run-loop sources must be added in the common modes.
- Wayland / X11: n/a (there are no nested loops).
- Refs: implicit in all.

---

## 2. Windowing

**WN-01 Create and destroy top-level windows; multiple windows** `U`
- Win: `RegisterClassExW` + `CreateWindowExW`, `DestroyWindow`.
- macOS: `NSWindow` (+ delegate), `close`.
- Wayland: `xdg_wm_base` → `xdg_surface` → `xdg_toplevel`.
- X11: `xcb_create_window`; set `WM_PROTOCOLS` (`WM_DELETE_WINDOW`, `_NET_WM_PING`).
- Refs: E · T · W · G · F(exp: regular/dialog/popup/tooltip/satellite controllers) · C

**WN-02 Title** `U`
- Win: `SetWindowTextW`. macOS: `NSWindow.title`. Wayland: `xdg_toplevel.set_title`. X11: `_NET_WM_NAME` (UTF8_STRING) + `WM_NAME`.
- Refs: E · T · W · G · F(exp) · C

**WN-03 Size and position (outer and content bounds)** `U` (Wayland caveat)
- Win: `SetWindowPos`, `GetWindowRect`/`GetClientRect`, `AdjustWindowRectExForDpi`; the visible frame comes from `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`.
- macOS: `setFrame:display:`, `contentRectForFrameRect:` (origin is bottom-left, so flip Y).
- Wayland: size is negotiated through `xdg_toplevel.configure` → `ack_configure`. **Global position can be neither read nor set.**
- X11: `ConfigureWindow`; frame size from `_NET_FRAME_EXTENTS`; `TranslateCoordinates`.
- Refs: E (returns 0,0 on Wayland) · T · W · G · F(exp) · C

**WN-04 Size constraints: min/max (U); aspect ratio and resize increments (S)**
- Win: `WM_GETMINMAXINFO`; aspect and increments by adjusting the rect in `WM_SIZING`.
- macOS: `contentMinSize`/`contentMaxSize`, `contentAspectRatio`, `contentResizeIncrements`.
- Wayland: `xdg_toplevel.set_min_size`/`set_max_size` only (no aspect, no increments).
- X11: `WM_NORMAL_HINTS` (`PMinSize`, `PMaxSize`, `PAspect`, `PResizeInc`, `PBaseSize`).
- Refs: E (`setAspectRatio`) · T (`setSizeConstraints`) · W (`surface_resize_increments`) · G (`window_min_size`) · F(exp `setConstraints`) · C (`minSize`/`maxSize`)

**WN-05 Minimize, maximize, restore and state readback** `U` - Change state and observe state changes, including tiled/snapped.
- Win: `ShowWindow(SW_MINIMIZE|SW_MAXIMIZE|SW_RESTORE)`, `IsIconic`/`IsZoomed`, the `WM_SIZE` wParam; snapped state is inferred from the placement.
- macOS: `miniaturize:`, `zoom:`, `deminiaturize:`, delegate notifications.
- Wayland: `xdg_toplevel.set_minimized` (fire-and-forget; **minimized is never reported back**), `set_maximized`/`unset_maximized`; configure states `maximized`, `fullscreen`, `activated`, `tiled_*`, `resizing`, `suspended`.
- X11: `_NET_WM_STATE_MAXIMIZED_VERT/HORZ`, `_NET_WM_STATE_HIDDEN`, `WM_CHANGE_STATE` (`IconicState`).
- Refs: E (`isSnapped` Win; no minimize state on Wayland) · T · W · G · F(exp) · C (`WindowPlacement`, `isMinimized`)

**WN-06 Fullscreen: borderless on a chosen monitor (U); exclusive video mode (S); macOS "simple fullscreen" (S)**
- Win: borderless `WS_POPUP` sized to `GetMonitorInfoW().rcMonitor`. Exclusive mode via `ChangeDisplaySettingsExW` or DXGI `SetFullscreenState`.
- macOS: `toggleFullScreen:` (opens its own Space). Simple fullscreen via `presentationOptions` (no Space). Exclusive via `CGDisplayCapture` + `CGDisplaySetDisplayMode`.
- Wayland: `xdg_toplevel.set_fullscreen(wl_output)`; exclusive is n/a.
- X11: `_NET_WM_STATE_FULLSCREEN` (+ `_NET_WM_FULLSCREEN_MONITORS`); exclusive via RandR `SetCrtcConfig`.
- Refs: E (`simpleFullScreen`) · T (`setFullscreenOnMonitor`, `setSimpleFullscreen`) · W (borderless and exclusive) · G (`toggle_simple_fullscreen`) · F(exp `setFullscreen(display)`) · C

**WN-07 Show, hide and show-without-activating** `U`
- Win: `ShowWindow(SW_SHOW|SW_HIDE|SW_SHOWNOACTIVATE)`.
- macOS: `orderFront:`, `orderOut:`, `orderFrontRegardless`.
- Wayland: there is no hide state; unmap by attaching a null buffer or destroying the role.
- X11: `MapWindow`/`UnmapWindow`.
- Refs: E (`showInactive`) · T · W · G · F(exp) · C (`visible`)

**WN-08 Focus/activation and attention request** `U`
- Win: `SetForegroundWindow` (subject to foreground-lock rules; `AllowSetForegroundWindow`), `FlashWindowEx`.
- macOS: `NSApp activate`, `makeKeyAndOrderFront:`, `requestUserAttention:` (bounces the Dock icon).
- Wayland: `xdg-activation-v1` with a token (from the launcher via `XDG_ACTIVATION_TOKEN`, or from another surface). Without a token the compositor may only mark the window urgent.
- X11: `_NET_ACTIVE_WINDOW` client message (with source indication and timestamp); `_NET_WM_STATE_DEMANDS_ATTENTION` / `XUrgencyHint`; startup notification `_NET_STARTUP_ID`.
- Refs: E (`focus`, `flashFrame`) · T (`setFocus`, `requestUserAttention`) · W (`focus_window`, `request_user_attention`, `ActivationToken`) · G (`activate`, `request_attention`) · F(exp `activate`) · C (AWT `toFront`; `Taskbar.requestUserAttention`)

**WN-09 Close request with veto** `U`
- Win: `WM_CLOSE`. macOS: `windowShouldClose:`. Wayland: `xdg_toplevel.close`. X11: `WM_DELETE_WINDOW` `ClientMessage`.
- Refs: E (`close` event preventDefault) · T (`CloseRequested`) · W (`CloseRequested`) · G (`on_should_close`) · F (`onWindowCloseRequested`; app-level `requestAppExit`) · C (`onCloseRequest`)

**WN-10 Capability flags: resizable, movable, minimizable, maximizable, closable** `C`
- Win: `WS_THICKFRAME`, `WS_MINIMIZEBOX`, `WS_MAXIMIZEBOX`; disable close with `EnableMenuItem(GetSystemMenu(), SC_CLOSE)`.
- macOS: `styleMask` (resizable / miniaturizable / closable), `movable`.
- Wayland: only implicit (min size == max size means non-resizable). `xdg_toplevel.wm_capabilities` reports the compositor's capabilities, not the app's.
- X11: `_MOTIF_WM_HINTS` functions; `_NET_WM_ALLOWED_ACTIONS` is set by the WM.
- Refs: E · T · W (`set_enabled_buttons`) · G (`is_resizable`, `is_minimizable`, `is_movable`) · C (`resizable`)

**WN-11 Always-on-top / always-on-bottom / window level** `C`
- Win: `SetWindowPos(HWND_TOPMOST|HWND_BOTTOM)`.
- macOS: `NSWindow.level` (floating, statusBar, popUpMenu, screenSaver).
- Wayland: **n/a** for normal clients (layer-shell exists only for shell components).
- X11: `_NET_WM_STATE_ABOVE` / `_NET_WM_STATE_BELOW`.
- Refs: E (macOS level names) · T (`setAlwaysOnTop`, `setAlwaysOnBottom`) · W (`set_window_level`) · G (`WindowKind::Floating`) · C (`alwaysOnTop`)

**WN-12 Owned (parent/child) top-level windows** `C` - A child stays above its owner and minimizes with it.
- Win: owner `HWND` passed at creation (not `WS_CHILD`).
- macOS: `addChildWindow:ordered:`.
- Wayland: `xdg_toplevel.set_parent`.
- X11: `WM_TRANSIENT_FOR`.
- Refs: E (`parent`, `setParentWindow`) · T (`parent`) · W(plat: Win32 owner, X11 parent/embed) · G (dialog/popup kinds) · F(exp satellite/dialog) · C (`DialogWindow`)

**WN-13 Modal windows (window-modal and app-modal)** `C`
- Win: an owned window + `EnableWindow(owner, FALSE)`.
- macOS: `beginSheet:completionHandler:` (window-modal sheet) or `runModalForWindow:` (app-modal).
- Wayland: `set_parent` + `xdg-dialog-v1.set_modal` *(staging)*.
- X11: `WM_TRANSIENT_FOR` + `_NET_WM_STATE_MODAL`.
- Refs: E (`modal`, macOS sheets) · T - (parent only) · G (`WindowKind::Dialog`) · F(exp `DialogWindowController`) · C (`DialogWindow` modalityType)

**WN-14 Popup surfaces (menus, tooltips, dropdowns) placed relative to a parent and allowed outside its bounds** `C`
- Win: owned `WS_POPUP` with `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`, and `MA_NOACTIVATE`.
- macOS: borderless child `NSWindow`/`NSPanel` at the `popUpMenu` level.
- Wayland: `xdg_popup` + `xdg_positioner` (anchor, gravity, flip/slide/resize constraint adjustment, `reposition`); `xdg_popup.grab` for menus. **This is the only way to place a surface relative to another one.**
- X11: override-redirect window with `_NET_WM_WINDOW_TYPE_DROPDOWN_MENU`/`POPUP_MENU`/`TOOLTIP` + pointer/keyboard grab.
- Refs: W (master: `window_type` + `positioner`) · G (`PopUp`, `AnchoredPopup`) · F(exp popup/tooltip) · E/T (native menus; other popups render inside the webview) · C (`Popup` renders in-window by default)

**WN-15 Non-activating, panel and utility windows** `C` - Windows that do not take focus.
- Win: `WS_EX_NOACTIVATE` (+ `WS_EX_TOOLWINDOW`).
- macOS: `NSPanel` with `NSWindowStyleMaskNonactivatingPanel`, `becomesKeyOnlyIfNeeded`.
- Wayland: n/a (focus is compositor policy).
- X11: `_NET_WM_WINDOW_TYPE_UTILITY` / `_NOTIFICATION`; `WM_HINTS` input = False.
- Refs: E (`focusable:false`, `type: 'panel'`) · T (`setFocusable`) · W(plat macOS `with_panel`) · G (`Floating`/`PopUp`) · C (`focusable`)

**WN-16 Skip taskbar and window switcher** `C`
- Win: `WS_EX_TOOLWINDOW` or `ITaskbarList::DeleteTab`.
- macOS: no per-window taskbar; `collectionBehavior` (`.ignoresCycle`, `.transient`) and `excludedFromWindowsMenu`.
- Wayland: n/a.
- X11: `_NET_WM_STATE_SKIP_TASKBAR` / `_SKIP_PAGER`.
- Refs: E (`setSkipTaskbar`, `setHiddenInMissionControl`) · T (`setSkipTaskbar`, unsupported on macOS) · W(plat Win32 `set_skip_taskbar`)

**WN-17 Visible on all workspaces / virtual desktops** `S`
- Win: n/a publicly (`IVirtualDesktopManager` only queries and moves windows).
- macOS: `collectionBehavior .canJoinAllSpaces`.
- Wayland: n/a (`ext-workspace` is for shells).
- X11: `_NET_WM_DESKTOP = 0xFFFFFFFF` / `_NET_WM_STATE_STICKY`.
- Refs: E (macOS/Linux) · T (unsupported on Windows)

**WN-18 Window icon and app identity (taskbar grouping)** `U`
- Win: `WM_SETICON` (small and big icons); AppUserModelID through `SetCurrentProcessExplicitAppUserModelID` or per window through `SHGetPropertyStoreForWindow` (`PKEY_AppUserModel_ID`).
- macOS: no per-window icon. The app and Dock icon is `NSApp.applicationIconImage`; identity is `CFBundleIdentifier`.
- Wayland: `xdg_toplevel.set_app_id` (must match the `.desktop` basename); `xdg-toplevel-icon-v1` *(staging)*.
- X11: `_NET_WM_ICON`, `WM_CLASS`, `_NET_WM_PID`, `_GTK_APPLICATION_ID`.
- Refs: E (`setIcon`, `setAppUserModelId`, `setDesktopName`) · T (`setIcon`) · W (icon; Win32 taskbar icon/class; Wayland/X11 name) · G (`icon`, `app_id`, `set_app_identity`) · F (runner) · C (`icon`)

**WN-19 Transparent windows with per-pixel alpha** `C`
- Win: DirectComposition + a flip-model swapchain with `DXGI_ALPHA_MODE_PREMULTIPLIED` (`WS_EX_NOREDIRECTIONBITMAP`), or layered windows (`UpdateLayeredWindow`).
- macOS: `opaque = NO`, clear `backgroundColor`, `CAMetalLayer.opaque = NO`.
- Wayland: ARGB buffer + an accurate `wl_surface.set_opaque_region`.
- X11: 32-bit ARGB visual; needs a compositing manager (`_NET_WM_CM_Sn` owner).
- Refs: E · T · W (`set_transparent`) · G (`Transparent`) · C (`transparent` requires `undecorated`)

**WN-20 Whole-window opacity** `S`
- Win: `SetLayeredWindowAttributes(LWA_ALPHA)`. macOS: `alphaValue`. Wayland: bake alpha into the buffer, or `alpha-modifier-v1` *(staging)*. X11: `_NET_WM_WINDOW_OPACITY` (compositor).
- Refs: E (`setOpacity`, Win/macOS)

**WN-21 Click-through / input region** `C` - Let mouse events pass through all or part of a window.
- Win: `WS_EX_TRANSPARENT | WS_EX_LAYERED`, or `HTTRANSPARENT` from `WM_NCHITTEST`.
- macOS: `ignoresMouseEvents`.
- Wayland: `wl_surface.set_input_region`.
- X11: `XFixesSetWindowShapeRegion(ShapeInput)`.
- Refs: E (`setIgnoreMouseEvents{forward}`) · T (`setIgnoreCursorEvents`) · W (`set_cursor_hittest`) · G (`set_input_region`)

**WN-22 Non-rectangular window shape** `S`
- Win: `SetWindowRgn`. macOS: transparent window + shadow. Wayland: transparent buffer + input region. X11: XShape bounding region.
- Refs: E (`setShape`, Win/Linux, experimental)

**WN-23 Content protection (exclude the window from screen capture)** `C`
- Win: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (Win10 2004+; older versions fall back to `WDA_MONITOR`, which captures black).
- macOS: `NSWindow.sharingType = NSWindowSharingNone`.
- Wayland: n/a. X11: n/a.
- Refs: E (`setContentProtection`) · T (`setContentProtected`) · W (`set_content_protected`)

**WN-24 Occlusion/visibility state (for throttling)** `C`
- Win: no direct occlusion API (`DWMWA_CLOAKED`, `IsIconic`; Chromium computes occlusion itself).
- macOS: `occlusionState` + `NSWindowDidChangeOcclusionStateNotification`.
- Wayland: frame callbacks stop; `xdg_toplevel` `suspended` state (xdg-shell v6).
- X11: `VisibilityNotify` (unreliable under compositors), `_NET_WM_STATE_HIDDEN`.
- Refs: W (`Occluded`) · G (`on_visibility_change`) · E/T (`backgroundThrottling`)

**WN-25 Synchronized live resize (content matches the frame, no jitter)** `U` (implicit)
- Win: paint synchronously in `WM_SIZE`/`WM_PAINT` during modal sizing; DirectComposition or flip-model `ResizeBuffers`.
- macOS: `CAMetalLayer.presentsWithTransaction` during live resize; `viewWillStartLiveResize`.
- Wayland: `configure` → `ack_configure` → commit a buffer of exactly that size; `xdg_surface.set_window_geometry`.
- X11: `_NET_WM_SYNC_REQUEST` + an XSync counter.
- Refs: implicit in all (W `SurfaceResized`; G `on_resize`)

**WN-26 First-frame polish** `C` - Avoid a white flash; show the window only once content is ready.
- Win: `DwmSetWindowAttribute(DWMWA_CLOAK)` until the first present; class background brush / `WM_ERASEBKGND`.
- macOS: set `backgroundColor`; `orderFront` after the first draw.
- Wayland: the window maps on the first buffer commit (initial configure/ack first).
- X11: map after the first draw; background `None`.
- Refs: E (`show:false` + `ready-to-show`, `backgroundColor`) · T (`visible:false`, `setBackgroundColor`) · W (`with_visible(false)`) · G (`show` option)

**WN-27 Save and restore window placement across runs** `P`
- Win: `GetWindowPlacement`/`SetWindowPlacement` (normal rect + state).
- macOS: `setFrameAutosaveName:`; state restoration (`NSWindowRestoration`, `restorable`).
- Wayland: clients cannot restore position; `xdg-session-management-v1` *(staging)*.
- X11: the client sets position itself (XSMP for session restore).
- Refs: E (persisted window state: `persisted-state-restored`, `clearPersistedState`) · T:plugin-window-state · G (`native_window_state` / `restore_native_window_state`) · C (in-session `WindowState` only) · F:community (window_manager)

**WN-28 App-initiated move/resize drag (custom title bars)** `C`
- Win: `WM_NCHITTEST` returning `HTCAPTION`/`HTLEFT`/…, or `WM_NCLBUTTONDOWN(HTCAPTION)` / `WM_SYSCOMMAND SC_MOVE|SC_SIZE`.
- macOS: `performWindowDragWithEvent:` (resize relies on the frame).
- Wayland: `xdg_toplevel.move` / `resize(seat, serial, edges)`, which needs the input event serial.
- X11: `_NET_WM_MOVERESIZE` client message (after ungrabbing the pointer).
- Refs: E (CSS `app-region: drag`) · T (`startDragging`, `startResizeDragging`) · W (`drag_window`, `drag_resize_window`) · G (`start_window_move`, `start_window_resize`) · C (`WindowDraggableArea`)

**WN-29 System window menu (Alt+Space / title-bar right-click)** `C`
- Win: `GetSystemMenu` + `TrackPopupMenu`, `WM_SYSCOMMAND`.
- macOS: n/a.
- Wayland: `xdg_toplevel.show_window_menu(seat, serial, x, y)`.
- X11: `_GTK_SHOW_WINDOW_MENU` (a GTK/Mutter convention, not EWMH).
- Refs: E (`system-context-menu` event) · W (`show_window_menu`) · G (`show_window_menu`)

**WN-30 Native window tabs** `S` (macOS)
- macOS: `tabbingIdentifier`, `tabbingMode`, `addTabbedWindow:ordered:`, `mergeAllWindows:`, `moveTabToNewWindow:`. Win: n/a. Linux: n/a.
- Refs: E · T (`tabbingIdentifier`) · W(plat) · G (`tabbed_windows`, `merge_all_windows`, …)

**WN-31 Document-edited marker and represented file (proxy icon)** `S` (macOS)
- macOS: `documentEdited`, `representedURL`/`representedFilename`. Win / Linux: n/a (convention: `*` in the title).
- Refs: E · W(plat `set_document_edited`) · G (`set_edited`, `set_document_path`)

**WN-32 Kiosk / presentation lock-down** `S`
- Win: fullscreen + `WH_KEYBOARD_LL` (OS-level Assigned Access is configuration, not API).
- macOS: `NSApplicationPresentationOptions` (HideDock, HideMenuBar, DisableProcessSwitching, …).
- Wayland / X11: n/a (compositor/WM configuration).
- Refs: E (`kiosk`)

**WN-33 Embedding native child surfaces (video, webview, external renderers)** `C`
- Win: child `HWND` (`WS_CHILD`) or a DirectComposition visual.
- macOS: `NSView` subview / `CALayer`.
- Wayland: `wl_subsurface` + `wp_viewporter`.
- X11: child window / XEmbed.
- Refs: E (`WebContentsView`) · T (multiple webviews) · F (platform views) · C (`SwingPanel`) · W (raw handles only)

**WN-34 Raw native handles (`HWND`, `NSView`/`NSWindow`, `wl_surface`, X window)** `U`
- `raw-window-handle` 0.6 on every OS.
- Refs: E (`getNativeWindowHandle`) · T · W · G · F (embedder API) · C (AWT window)

**WN-35 Native message hook (escape hatch)** `S` - Let app code see raw OS messages.
- Win: subclassing or a `WndProc` hook. macOS: `NSEvent addLocalMonitorForEventsMatchingMask:`. Wayland / X11: n/a, or raw xcb event access.
- Refs: E (`hookWindowMessage`, Win) · W(plat Win32 `with_msg_hook`) · G (Win32 `get_raw_handle`)

**WN-36 Shell-surface windows (panels, docks, overlays, desktop widgets)** `S`
- Win: AppBar (`SHAppBarMessage`), or topmost tool windows.
- macOS: window levels + `collectionBehavior .stationary`.
- Wayland: `wlr-layer-shell-unstable-v1` (wlroots, KDE; **not GNOME**).
- X11: `_NET_WM_WINDOW_TYPE_DOCK`/`_DESKTOP` + `_NET_WM_STRUT_PARTIAL`.
- Refs: G (`WindowKind::LayerShell`, `set_exclusive_zone`) · E (`type: 'desktop' | 'dock'` on Linux)

---

## 3. Window chrome, decorations and backdrop

**CH-01 Decorated and undecorated windows; toggle after creation** `U`
- Win: `WS_OVERLAPPEDWINDOW` vs `WS_POPUP`, or keep the frame and hide it via `WM_NCCALCSIZE`.
- macOS: `styleMask` titled vs borderless.
- Wayland: **client-side decorations by default**. Server-side only through `xdg-decoration-unstable-v1`, which KDE and wlroots support and GNOME/Mutter does not, so on GNOME the app must draw its own frame.
- X11: the WM decorates; `_MOTIF_WM_HINTS` decorations = 0 removes them.
- Refs: E · T · W · G (`WindowDecorations`) · F (runner) · C (`undecorated`)

**CH-02 Custom title bar that keeps native behavior** `C` - Content under the title bar, native caption buttons, and Windows 11 Snap Layouts.
- Win: `DwmExtendFrameIntoClientArea` + `WM_NCCALCSIZE` + `WM_NCHITTEST` (`HTCAPTION`, `HTMINBUTTON`/`HTMAXBUTTON`/`HTCLOSE`). Returning `HTMAXBUTTON` is what shows the Snap Layouts flyout. Keep the resize borders and the DWM shadow.
- macOS: `titlebarAppearsTransparent`, `titleVisibility = hidden`, `NSWindowStyleMaskFullSizeContentView`; reposition the traffic lights through the `standardWindowButton` frames; `NSTitlebarAccessoryViewController`.
- Wayland: full CSD (CH-03).
- X11: undecorated + `_NET_WM_MOVERESIZE`.
- Refs: E (`titleBarStyle`, `titleBarOverlay` Win/Linux, `setWindowButtonPosition`, `setWindowButtonVisibility`) · T (`titleBarStyle` overlay, `trafficLightPosition`) · W(plat macOS titlebar flags; Win32 title colors) · G (`TitlebarOptions`, `set_traffic_light_position`, `on_hit_test_window_control`) · C (`undecorated` + `WindowDraggableArea`) · F:community

**CH-03 Client-side decoration obligations** `C` (Linux/Wayland; any undecorated window) - What a self-drawn frame must honor.
- Draw the title bar, buttons, shadow and resize edges. Use `xdg_surface.set_window_geometry` to exclude the shadow. Honor configure states: `tiled_*` (drop the shadow and rounded corners) and `activated` (focus styling). Hide actions that `xdg_toplevel.wm_capabilities` does not offer. Open `show_window_menu` on right-click.
- Take the button order from `org.gnome.desktop.wm.preferences button-layout` and the title-bar double-click/middle-click action from `action-double-click-titlebar` (both readable through portal Settings).
- The macOS analog is the `AppleActionOnDoubleClick` user default.
- Refs: G (`WindowDecorations::Client`, `window_controls`, `button_layout`, `titlebar_double_click`, `set_client_inset`) · W (Wayland fallback frame; `with_prefer_csd`) · E (Chromium draws its own CSD on Wayland) · T/F (GTK draws it)

**CH-04 Request server-side decorations** `S` (Linux)
- Wayland: `zxdg_toplevel_decoration_v1.set_mode(server_side)`; legacy KDE `org_kde_kwin_server_decoration`. X11: default.
- Refs: W · G (`request_decorations`)

**CH-05 Dark/light frame and caption colors** `C`
- Win: `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)`; Win11 `DWMWA_CAPTION_COLOR`, `DWMWA_TEXT_COLOR`, `DWMWA_BORDER_COLOR`. Dark Win32 *menus* need undocumented `uxtheme.dll` ordinals (e.g. `SetPreferredAppMode`).
- macOS: `NSWindow.appearance` / `NSApp.appearance` (e.g. `NSAppearanceNameDarkAqua`).
- Wayland: self-drawn CSD; KDE `org_kde_kwin_server_decoration_palette`.
- X11: `_GTK_THEME_VARIANT = dark` (GTK-aware WMs).
- Refs: E (`nativeTheme.themeSource`) · T (`setTheme`) · W (`set_theme`; Win32 title colors; X11 GTK theme variant) · G (`set_window_appearance`)

**CH-06 System backdrop materials (Mica, Acrylic, vibrancy, blur, Liquid Glass)** `C`
- Win: Win11 `DWMWA_SYSTEMBACKDROP_TYPE` (`DWMSBT_MAINWINDOW` = Mica, `DWMSBT_TRANSIENTWINDOW` = Acrylic, `DWMSBT_TABBEDWINDOW` = Mica Alt), which needs the frame extended into the client area. Win10 acrylic exists only through undocumented `SetWindowCompositionAttribute`.
- macOS: `NSVisualEffectView` (material, `blendingMode = behindWindow`, state); `NSGlassEffectView` (macOS 26).
- Wayland: `ext-background-effect-v1` *(staging)*; KDE `org_kde_kwin_blur`.
- X11: KDE `_KDE_NET_WM_BLUR_BEHIND_REGION` only.
- Refs: E (`setVibrancy` macOS, `setBackgroundMaterial` Win) · T (`setEffects`: Mica/Acrylic/Tabbed/Blur/macOS materials/Liquid Glass; unsupported on Linux) · W (`set_blur`; Win32 `set_system_backdrop`; macOS blur material; Wayland KWin blur and ext-background-effect) · G (`Blurred`, `MicaBackdrop`, `MicaAltBackdrop`) · F:community

**CH-07 Corner rounding and shadow control** `C`
- Win: `DWMWA_WINDOW_CORNER_PREFERENCE` (Win11); a shadow on undecorated windows via a 1px `DwmExtendFrameIntoClientArea` margin or `CS_DROPSHADOW`.
- macOS: `hasShadow`, `invalidateShadow`.
- Wayland: CSD draws its own shadow.
- X11: compositor-dependent.
- Refs: E (`setHasShadow`, `roundedCorners`) · T (`setShadow`, unsupported on Linux) · W(plat corner preference, undecorated shadow, `has_shadow`)

**CH-08 Window background color (before the content draws)** `C`
- Win: class brush / `WM_ERASEBKGND`. macOS: `backgroundColor`. Wayland: n/a (comes from buffer contents). X11: `background_pixel`.
- Refs: E · T · G (background appearance)

---

## 4. Displays and DPI

**DS-01 Monitor list: bounds, work area, scale, refresh rate, name, primary** `U`
- Win: `EnumDisplayMonitors` + `GetMonitorInfoW` (`rcMonitor`, `rcWork`, `MONITORINFOF_PRIMARY`), `GetDpiForMonitor`, `EnumDisplaySettingsW` (refresh rate), `QueryDisplayConfig`/`DisplayConfigGetDeviceInfo` (friendly name).
- macOS: `NSScreen.screens` (`frame`, `visibleFrame`, `backingScaleFactor`, `localizedName`, `maximumFramesPerSecond`), `CGDirectDisplayID`.
- Wayland: `wl_output` (mode, scale, name/description in v4) + `zxdg_output_v1` (logical position and size). **No work area and no primary-monitor concept.**
- X11: RandR 1.5 `RRGetMonitors` (primary flag); `_NET_WORKAREA` (one area for the whole screen).
- Refs: E (`screen.getAllDisplays`) · T (`availableMonitors`, `primaryMonitor`, `workArea`) · W (`available_monitors`, `primary_monitor`) · G (`displays`, `primary_display`, `visible_bounds`) · F (`PlatformDispatcher.displays`) · C (AWT `GraphicsEnvironment`)

**DS-02 Display configuration change events** `C`
- Win: `WM_DISPLAYCHANGE`; `WM_SETTINGCHANGE(SPI_SETWORKAREA)`.
- macOS: `NSApplicationDidChangeScreenParametersNotification` / `CGDisplayRegisterReconfigurationCallback`.
- Wayland: `wl_registry` global/global_remove for `wl_output`, then `wl_output.done`.
- X11: `RRScreenChangeNotify` / `RRNotify`.
- Refs: E (`display-added`/`display-removed`/`display-metrics-changed`) · G (`on_displays_changed`) · F

**DS-03 Per-monitor DPI and scale-factor change** `U`
- Win: Per-Monitor-V2 awareness (manifest or `SetProcessDpiAwarenessContext`), `GetDpiForWindow`, `WM_DPICHANGED` (apply the suggested rect), `WM_GETDPISCALEDSIZE`.
- macOS: `backingScaleFactor` + `NSWindowDidChangeBackingPropertiesNotification`.
- Wayland: `wl_surface.preferred_buffer_scale` (wl_compositor v6) or `wl_output.scale`; fractional scaling via `wp_fractional_scale_v1` *(staging)* + `wp_viewporter`.
- X11: **no per-monitor standard.** Use `Xft.dpi` (RESOURCE_MANAGER), XSETTINGS `Xft/DPI` and `Gdk/WindowScalingFactor`, or the `GDK_SCALE` env; per-monitor values need RandR heuristics.
- Refs: E · T (`onScaleChanged`) · W (`ScaleFactorChanged`) · G (`on_resize(size, scale)`) · F · C

**DS-04 Which monitor a window is on; moving between monitors** `U`
- Win: `MonitorFromWindow`. macOS: `NSWindow.screen` + `NSWindowDidChangeScreenNotification`. Wayland: `wl_surface.enter`/`leave(wl_output)`. X11: compute from geometry vs RandR.
- Refs: E (`getDisplayMatching`) · T (`currentMonitor`) · W (`current_monitor`) · G (`display`, `on_display_changed`) · F · C

**DS-05 Global cursor position and the display under a point** `C`
- Win: `GetCursorPos`, `MonitorFromPoint`. macOS: `NSEvent.mouseLocation`. Wayland: **n/a** (surface-local coordinates only). X11: `XQueryPointer`.
- Refs: E (`getCursorScreenPoint`, not on Wayland) · T (`cursorPosition`, `monitorFromPoint`) · G (window-local `mouse_position`)

**DS-06 Video modes** `S`
- Win: `EnumDisplaySettingsExW` / `IDXGIOutput::GetDisplayModeList`. macOS: `CGDisplayCopyAllDisplayModes`. Wayland: `wl_output.mode` (only the current mode is meaningful). X11: RandR modes.
- Refs: W

**DS-07 HDR and wide-gamut capability, color space** `S`
- Win: `IDXGIOutput6::GetDesc1` (ColorSpace, luminance), WinRT `AdvancedColorInfo`.
- macOS: `NSScreen.maximumExtendedDynamicRangeColorComponentValue`, `colorSpace`, `CAMetalLayer.wantsExtendedDynamicRangeContent`.
- Wayland: `color-management-v1`, `color-representation-v1` *(staging)*.
- X11: practically n/a.
- Refs: (internal to E/Chromium)

**DS-08 Stable monitor identity across reboots** `S`
- Win: device path / EDID via `DisplayConfigGetDeviceInfo`. macOS: `CGDisplayCreateUUIDFromDisplayID`. Wayland: `wl_output.name` (v4). X11: RandR output name + EDID property.
- Refs: G (`PlatformDisplay::uuid`) · E (`display.id`)

---

## 5. Pointer input

**IN-01 Mouse buttons, motion, enter/leave, wheel** `U`
- Win: `WM_MOUSEMOVE`, `WM_[LRM]BUTTON*`, `WM_XBUTTON*`, `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`, `TrackMouseEvent` → `WM_MOUSELEAVE`; or unified `WM_POINTER*`.
- macOS: `mouseDown:`…`otherMouseUp:`, `NSTrackingArea` for enter/exit, `scrollWheel:`.
- Wayland: `wl_pointer` (enter, leave, motion, button, axis, frame).
- X11: XInput2 `XI_Motion`/`XI_ButtonPress`/`XI_Enter`/`XI_Leave` (core events as fallback).
- Refs: E · T · W · G · F · C

**IN-02 Precise scrolling with phases and momentum** `C`
- Win: precision touchpads send high-resolution `WM_MOUSEWHEEL` deltas; phases and inertia come only through DirectManipulation (`IDirectManipulationViewport`).
- macOS: `hasPreciseScrollingDeltas`, `phase`, `momentumPhase`.
- Wayland: `wl_pointer.axis_source` (finger/continuous/wheel), `axis_stop`, `axis_value120` (v8), `axis_relative_direction` (v9).
- X11: XI2.1 smooth-scroll valuators (no phase, no momentum).
- Refs: W (`PixelDelta` + `TouchPhase`) · G (scroll phases; Win32 `direct_manipulation.rs`) · F (Win `direct_manipulation.cc`, Linux scrolling manager) · E

**IN-03 Click count and double-click** `U` (framework side)
- Win: `GetDoubleClickTime`, `SM_CXDOUBLECLK`. macOS: `NSEvent.clickCount` (computed by the OS). Wayland / X11: the app counts clicks, using XSETTINGS `Net/DoubleClickTime` or GNOME `org.gnome.desktop.peripherals.mouse`.
- Refs: G · F · C · E

**IN-04 Pointer capture during drags** `U`
- Win: `SetCapture`/`ReleaseCapture` (+ `WM_CAPTURECHANGED`). macOS: implicit. Wayland: implicit grab while a button is held. X11: implicit grab / `XIGrabDevice`.
- Refs: all (internal)

**IN-05 Cursors: system shapes, custom image, hide, hide-until-move** `U`
- Win: `SetCursor` in `WM_SETCURSOR` with `LoadCursorW(IDC_*)`; custom via `CreateIconIndirect`; `ShowCursor`.
- macOS: `NSCursor` (standard set; `initWithImage:hotSpot:`), `NSCursor.setHiddenUntilMouseMoves:`.
- Wayland: `wp_cursor_shape_v1` *(staging)*. Otherwise load the XCursor theme yourself (`XCURSOR_THEME`/`XCURSOR_SIZE`, or GNOME `cursor-theme`/`cursor-size` via portal Settings) and call `wl_pointer.set_cursor(surface)`.
- X11: Xcursor library by name (themed) + `XDefineCursor`; `XFixesHideCursor`.
- Refs: E (CSS cursors; `setAutoHideCursor` macOS) · T (`setCursorIcon`, `setCursorVisible`) · W (cursor icon and custom image) · G (`set_cursor_style`, `hide_cursor_until_mouse_moves`) · F (system cursors) · C (`pointerHoverIcon`)

**IN-06 Warp the pointer** `C`
- Win: `SetCursorPos`. macOS: `CGWarpMouseCursorPosition`. Wayland: `pointer-warp-v1` *(staging)*, or a hint while locked. X11: `XWarpPointer` / `XIWarpPointer`.
- Refs: W (`set_cursor_position`) · T (`setCursorPosition`) · E -

**IN-07 Pointer lock/confine and relative motion** `C`
- Win: `ClipCursor` + raw input (`RegisterRawInputDevices`, `WM_INPUT`).
- macOS: `CGAssociateMouseAndMouseCursorPosition(false)` + event `deltaX`/`deltaY`.
- Wayland: `pointer-constraints-unstable-v1` (lock/confine) + `relative-pointer-unstable-v1`.
- X11: `XGrabPointer(confine_to)` + XI2 raw events.
- Refs: W (`set_cursor_grab`, device events) · T (`setCursorGrab`) · E (web Pointer Lock)

**IN-08 Touchscreen multi-touch** `C`
- Win: `WM_POINTER` (`PT_TOUCH`, with `EnableMouseInPointer`) or `WM_TOUCH` (`RegisterTouchWindow`).
- macOS: n/a (no touchscreens; trackpad touches are indirect `NSTouch`).
- Wayland: `wl_touch` (down, up, motion, frame, cancel).
- X11: XI2.2 touch events (`XI_TouchBegin`/`Update`/`End`).
- Refs: W (touch pointer source) · G (`Touch`, `TouchDrag`, `LongPress`) · F · E · C

**IN-09 Touchpad gestures (pinch, rotate, swipe, smart zoom, hold)** `C`
- Win: precision-touchpad pinch arrives as Ctrl + `WM_MOUSEWHEEL` or through DirectManipulation zoom; touchscreen gestures via `WM_GESTURE` / `InteractionContext`.
- macOS: `magnifyWithEvent:`, `rotateWithEvent:`, `swipeWithEvent:`, `smartMagnifyWithEvent:`.
- Wayland: `pointer-gestures-unstable-v1` (swipe, pinch, hold).
- X11: XI 2.4 gesture events (`XI_GesturePinch*`, `XI_GestureSwipe*`; X.Org 21.1+).
- Refs: W (`PinchGesture`, `PanGesture`, `RotationGesture`, `DoubleTapGesture`, `HoldGesture`) · G (`Pinch`) · E (`swipe`/`rotate-gesture` events on macOS) · F (pan-zoom pointer events)

**IN-10 Force-touch pressure and trackpad haptics** `S` (macOS)
- macOS: `pressureChangeWithEvent:` (`pressure`, `stage`), `NSHapticFeedbackManager`.
- Win: n/a for touchpads (pen haptics via `SimpleHapticsController`). Linux: n/a.
- Refs: W (`TouchpadPressure`) · G (`MousePressure`)

**IN-11 Pen/stylus: pressure, tilt, twist, eraser, barrel button, hover** `C`
- Win: `WM_POINTER` with `PT_PEN`; `GetPointerPenInfo` (pressure, `tiltX`/`tiltY`, rotation, `PEN_FLAG_ERASER`/`PEN_FLAG_BARREL`/`PEN_FLAG_INVERTED`).
- macOS: tablet-point events (`pressure`, `tilt`, `rotation`, `tangentialPressure`) + `tabletProximity:` (`pointingDeviceType` eraser).
- Wayland: tablet protocol (`zwp_tablet_tool_v2`; now in stable).
- X11: XI2 valuators on tablet devices (pressure/tilt axes).
- Refs: W (`TabletTool`; Wayland `wp_tablet_input_v2`) · F (stylus device kind) · E (Pointer Events) · C (limited)

**IN-12 Raw device events (including while unfocused)** `S`
- Win: `RegisterRawInputDevices(RIDEV_INPUTSINK)`. macOS: `IOHIDManager` (needs the Input Monitoring TCC permission). Wayland: n/a. X11: XI2 raw events on the root window.
- Refs: W (`device_event`, `listen_device_events`). Gamepads are a separate concern (gilrs etc.).

---

## 6. Keyboard input

**KB-01 Key down/up with physical code, logical key and text** `U`
- Win: `WM_KEYDOWN`/`WM_SYSKEYDOWN` (scan code + extended bit in lParam), `WM_CHAR`/`WM_UNICHAR`; layout-aware mapping via `ToUnicodeEx`/`MapVirtualKeyExW`.
- macOS: `keyDown:`/`keyUp:`/`flagsChanged:`; `keyCode` (physical), `characters`/`charactersIgnoringModifiers`; `UCKeyTranslate` with `TISCopyCurrentKeyboardLayoutInputSource`.
- Wayland: `wl_keyboard.keymap` (XKB keymap fd) + key/modifiers events → `libxkbcommon` state.
- X11: XKB via `xkbcommon-x11` (`KeyPress` + `XkbStateNotify`).
- Refs: E · T · W · G · F · C

**KB-02 Modifier and lock state (Caps/Num, left vs right)** `U`
- Win: `GetKeyState`. macOS: `modifierFlags` (device-dependent bits for left/right). Wayland / X11: xkb state and LEDs.
- Refs: W (`ModifiersChanged`) · G (`modifiers`, `capslock`) · all

**KB-03 Key repeat** `U`
- Win / macOS / X11: the OS repeats (`WM_KEYDOWN` repeat count; `NSEvent.isARepeat`; X11 detectable auto-repeat via `XkbSetDetectableAutoRepeat`).
- Wayland: **the client implements repeat** from `wl_keyboard.repeat_info` (rate, delay).
- Refs: all

**KB-04 Keyboard-layout info and change notification** `C` - Map shortcuts correctly across layouts.
- Win: `GetKeyboardLayout`, `WM_INPUTLANGCHANGE`.
- macOS: TIS input source + the `kTISNotifySelectedKeyboardInputSourceChanged` distributed notification.
- Wayland: a new `wl_keyboard.keymap` or a group change.
- X11: `XkbNewKeyboardNotify` / `XkbStateNotify` group change.
- Refs: G (`keyboard_layout`, `keyboard_mapper`, `on_keyboard_layout_change`) · F (layout tracking on all desktops) · W (dead-key reset only)

**KB-05 Dead keys and compose sequences** `U`
- Win: `ToUnicodeEx` dead-key state (`WM_DEADCHAR`). macOS: `interpretKeyEvents:` (marked text) or `UCKeyTranslate` `deadKeyState`. Wayland / X11: `xkb_compose` tables, or the IM.
- Refs: W (`reset_dead_keys`) · all

**KB-06 OS standard key bindings and text commands** `C` (essential on macOS)
- macOS: `interpretKeyEvents:` → `doCommandBySelector:` (`moveWordLeft:`, `deleteBackward:`, …; honors `DefaultKeyBinding.dict`).
- Win: n/a (app-defined). Linux: n/a (toolkit key themes).
- Refs: W(plat macOS `standard_key_binding`) · G · F (`DefaultTextEditingShortcuts` + macOS selectors)

**KB-07 Option-as-Alt, AltGr** `S`
- macOS: Option produces characters by default. Win: AltGr arrives as a synthesized Ctrl+Alt (fake left-Ctrl down).
- Refs: W(plat macOS `option_as_alt`)

**KB-08 Capture OS shortcuts (remote desktop, games)** `S`
- Win: `WH_KEYBOARD_LL` (Win+L and Ctrl+Alt+Del cannot be blocked). macOS: `CGEventTap` (Accessibility/Input Monitoring permission). Wayland: `keyboard-shortcuts-inhibit-unstable-v1`. X11: `XGrabKeyboard`.
- Refs: none of the six in core (Chromium has the web Keyboard Lock API).

**KB-09 Mnemonics/access keys and Alt-activated menu bar** `C` (Win/Linux)
- Win: `&` in menu text, `WM_SYSKEYDOWN`/`WM_SYSCHAR`, `SC_KEYMENU`. macOS: n/a. Linux: `_` mnemonics (toolkit convention).
- Refs: E · T (muda `&`) · C

**KB-10 Secure input for password fields** `S`
- macOS: `EnableSecureEventInput`. Win: a TSF password InputScope (disables IME suggestions). Wayland: text-input-v3 content purpose `password`. X11: n/a.
- Refs: E (`setSecureKeyboardEntryEnabled`) · W (`ImePurpose::Password`)

---

## 7. Text input services and IME

**TX-01 IME composition (preedit/commit) with candidate-window placement** `U`
- Win: TSF (`ITfThreadMgr` + `ITextStoreACP2`) is the full path. IMM32 is the minimal one (`WM_IME_STARTCOMPOSITION`/`COMPOSITION`/`ENDCOMPOSITION`, `ImmGetCompositionStringW`, `ImmSetCandidateWindow`).
- macOS: `NSTextInputClient` (`setMarkedText:selectedRange:replacementRange:`, `insertText:replacementRange:`, `firstRectForCharacterRange:actualRange:`, `unmarkText`) via `NSTextInputContext`.
- Wayland: `text-input-unstable-v3` (`enable`, `set_cursor_rectangle`, `preedit_string`, `commit_string`, `done` serials).
- X11: XIM (`XOpenIM`/`XCreateIC`, preedit callbacks, `XNSpotLocation`), or IBus/Fcitx over D-Bus.
- Refs: E · T · W (`Ime` events, `set_ime_cursor_area`, `set_ime_allowed`) · G (`InputHandler`, `update_ime_position`) · F (Win IMM32, Linux `GtkIMContext`) · C

**TX-02 Surrounding text, reconversion, delete-surrounding** `C`
- Win: the TSF text store gives document access and reconversion; IMM32 `IMR_RECONVERTSTRING`.
- macOS: `attributedSubstringForProposedRange:`, `selectedRange`, `characterIndexForPoint:`.
- Wayland: `set_surrounding_text` + the `delete_surrounding_text` event.
- X11: XIM string-conversion callbacks (rarely supported).
- Refs: W (`Ime::DeleteSurrounding`, `request_ime_update`) · G (`text_for_range`, `selected_text_range`) · F (`TextEditingValue`)

**TX-03 Input purpose and hints (password, number, email, URL, multiline, autocorrect off)** `C`
- Win: TSF `InputScope` (`SetInputScopes`); disable the IME via `ImmAssociateContextEx`.
- macOS: `NSTextContentType`, secure input.
- Wayland: text-input-v3 `set_content_type(hint, purpose)`.
- X11: n/a (IBus content type over D-Bus).
- Refs: W (`set_ime_purpose`) · G (`TextInputConfiguration`) · F (`TextInputType`) · E (HTML input types)

**TX-04 Emoji / character palette** `C`
- Win: `CoreInputView.TryShow(CoreInputViewKind.Emoji)` (Win10 1803+), or the Win+. hotkey.
- macOS: `NSApp orderFrontCharacterPalette:`.
- Linux: no system API (IBus emoji, toolkit choosers).
- Refs: E (`showEmojiPanel`, macOS/Win) · G (`show_character_palette`) · (F/T -)

**TX-05 Touch / on-screen keyboard** `S`
- Win: invoked automatically when a TSF/UIA editable control gets focus; `InputPane` for show/hide/occlusion.
- macOS: n/a.
- Wayland: text-input-v3 `enable` triggers the OSK on touch shells.
- X11: n/a.
- Refs: G (`show_soft_keyboard`, aimed at mobile)

**TX-06 System spell checking** `S`
- Win: `ISpellCheckerFactory`/`ISpellChecker` (Win8+). macOS: `NSSpellChecker` (also grammar, autocorrect, text replacement). Linux: no system service (Hunspell/Enchant libraries).
- Refs: E (native on Win/macOS, Hunspell on Linux) · F (spellcheck channel, mobile only)

**TX-07 macOS text services: Services menu, Look Up, substitutions, dictation, Writing Tools** `S` (macOS)
- `NSServicesMenuRequestor` (`validRequestorForSendType:returnType:`), `showDefinitionForAttributedString:atPoint:`, `NSTextCheckingTypes`, Writing Tools (macOS 15+).
- Refs: E (menu roles `services`, `showSubstitutions`, `startSpeaking`, …)

---

## 8. Accessibility

**AX-01 Platform accessibility tree for screen readers** `U` (gaps noted)
- Win: UI Automation provider (`WM_GETOBJECT` → `UiaReturnRawElementProvider`; `IRawElementProviderFragmentRoot`/`Fragment` + control patterns); MSAA is legacy.
- macOS: the `NSAccessibility` protocol on views and `NSAccessibilityElement`.
- Linux (both): AT-SPI2 over D-Bus (`org.a11y.Bus.GetAddress` → the a11y bus; `org.a11y.atspi.Accessible`/`Component`/`Text`/`Action`, …).
- Refs: E (Chromium) · F (UIA / NSAccessibility / ATK→AT-SPI) · G (AccessKit: `a11y_init`, `a11y_tree_update`) · T (via the webview) · C (macOS; Windows via Java Access Bridge, off by default; **no Linux**) · W - (external `accesskit_winit`)

**AX-02 Announcements / live regions** `C`
- Win: `UiaRaiseNotificationEvent`. macOS: `NSAccessibilityAnnouncementRequestedNotification`. Linux: AT-SPI live-region object attributes.
- Refs: E · F · G (AccessKit live regions)

**AX-03 Detect an active assistive technology; build the tree lazily** `C`
- Win: `SPI_GETSCREENREADER`, `UiaClientsAreListening`. macOS: `NSWorkspace.isVoiceOverEnabled` (attributes are requested lazily). Linux: `org.a11y.Status` `IsEnabled`/`ScreenReaderEnabled`.
- Refs: E (`accessibilitySupportEnabled`, `accessibility-support-changed`) · F · G (lazy `a11y_init`)

**AX-04 Accessibility trust (observing or controlling other apps)** `S` (macOS)
- macOS: `AXIsProcessTrustedWithOptions`. Win: n/a (`uiAccess` for elevated targets). Linux: n/a.
- Refs: E (`isTrustedAccessibilityClient`)

Display-preference settings (reduce motion, contrast, transparency) are under SP-03..SP-05.

---

## 9. Clipboard and drag-and-drop

**CB-01 Plain-text clipboard** `U` (except W)
- Win: `OpenClipboard`/`EmptyClipboard`/`SetClipboardData(CF_UNICODETEXT)`/`GetClipboardData`.
- macOS: `NSPasteboard.generalPasteboard` (`NSPasteboardTypeString`).
- Wayland: `wl_data_device_manager` (`wl_data_source`/`wl_data_offer`, data over a pipe fd). **Reading needs keyboard focus.**
- X11: the CLIPBOARD selection per ICCCM (`SetSelectionOwner`, `SelectionRequest`/`SelectionNotify`, `UTF8_STRING`, INCR for large data). The owner must stay alive to serve requests.
- Refs: E · T:plugin-clipboard-manager · G · F (text only) · C · W - (no clipboard; its data-transfer API is DnD-only)

**CB-02 Rich formats: HTML, RTF, images, file lists, custom types** `C`
- Win: `RegisterClipboardFormatW` ("HTML Format", "PNG"), `CF_DIBV5`, `CF_HDROP`, or `OleSetClipboard(IDataObject)`.
- macOS: `NSPasteboardItem` with UTIs (`public.html`, `public.rtf`, `public.png`, `public.file-url`).
- Wayland / X11: MIME types (`text/html`, `image/png`, `text/uri-list`, custom).
- Refs: E (html, rtf, image, bookmark, custom buffers) · T:plugin-clipboard-manager (image, html) · G (`ClipboardItem` entries incl. images) · C (AWT `DataFlavor`) · F:community (super_clipboard)

**CB-03 Lazy/delayed rendering; persistence after exit** `S`
- Win: delayed rendering (`SetClipboardData(fmt, NULL)` + `WM_RENDERFORMAT`/`WM_RENDERALLFORMATS`) or `IDataObject`.
- macOS: `NSPasteboardItemDataProvider` (macOS keeps the data after exit).
- Wayland / X11: lazy by design. Data is lost on exit unless a clipboard manager takes it (Wayland `ext-data-control-v1`; X11 `CLIPBOARD_MANAGER` `SAVE_TARGETS`).
- Refs: implicit in E/G

**CB-04 Clipboard change monitoring** `S`
- Win: `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE`. macOS: poll `changeCount`. Wayland: only while focused (`wl_data_device.selection`); `ext-data-control-v1` for privileged managers. X11: `XFixesSelectSelectionInput`.
- Refs: none core

**CB-05 PRIMARY selection (middle-click paste)** `S` (Linux)
- Wayland: `primary-selection-unstable-v1`. X11: the PRIMARY selection.
- Refs: E (clipboard type `selection`) · G (`read_from_primary`/`write_to_primary`) · C (AWT system selection)

**CB-06 macOS find pasteboard** `S`
- macOS: `NSPasteboardNameFind`.
- Refs: E (`readFindText`/`writeFindText`) · G (`read_from_find_pasteboard`)

**CB-07 Concealed clipboard entries (kept out of history and sync)** `S`
- Win: formats `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0`, `CanUploadToCloudClipboard = 0`.
- macOS: `org.nspasteboard.ConcealedType` (nspasteboard.org convention).
- Linux: MIME `x-kde-passwordManagerHint = secret`.
- Refs: T:plugin-clipboard-manager (`writeSecret`)

**DD-01 Drop target: files, text and custom types from other apps, with effect negotiation** `U` (except F core)
- Win: OLE `RegisterDragDrop` + `IDropTarget` (needs `OleInitialize` on an STA thread), `DROPEFFECT`, `IDropTargetHelper` for drag images; legacy `DragAcceptFiles`/`WM_DROPFILES` (files only).
- macOS: `registerForDraggedTypes:` + `NSDraggingDestination` (`draggingEntered:`/`Updated:`/`Exited:`, `performDragOperation:`).
- Wayland: `wl_data_device` enter/motion/leave/drop + `wl_data_offer.accept`/`set_actions`/`finish`.
- X11: XDND (`XdndAware`, `XdndEnter`/`Position`/`Status`/`Drop`/`Finished`).
- Refs: E (HTML5 DnD) · T (`onDragDropEvent`, file paths) · W (`DragEntered`/`DragPosition`/`DragDropped`/`DragLeft` + typed data) · G (`FileDrop`) · C (`dragAndDropTarget`) · F:community (desktop_drop, super_drag_and_drop)

**DD-02 Drag source: drag data out to other apps, with a drag image** `C`
- Win: `DoDragDrop` + `IDropSource` + `IDataObject` (+ `IDragSourceHelper`).
- macOS: `beginDraggingSessionWithItems:event:source:` (`NSDraggingSource`).
- Wayland: `wl_data_device.start_drag(source, origin, icon, serial)`, which needs the implicit-grab serial.
- X11: XDND source (owns `XdndSelection`, grabs the pointer).
- Refs: E (`webContents.startDrag`) · G (`start_external_drag`) · W (master `start_drag`) · C (`dragAndDropSource`) · T - · F -

**DD-03 Promised / virtual files** `S`
- Win: `CFSTR_FILEDESCRIPTORW` + `CFSTR_FILECONTENTS`. macOS: `NSFilePromiseProvider`/`NSFilePromiseReceiver`. Wayland: n/a (write a temp file and offer `text/uri-list`). X11: XDS (Direct Save).
- Refs: none core

**DD-04 Tab tear-off (a new window follows the pointer)** `S`
- Win: `WM_NCLBUTTONDOWN(HTCAPTION)` on the new window. macOS: `performWindowDragWithEvent:`. Wayland: `xdg-toplevel-drag-v1` *(staging)*. X11: `_NET_WM_MOVERESIZE`.
- Refs: G (macOS native tabs only)

---

## 10. App lifecycle and session

**LC-01 Launch-finished / ready, and explicit exit** `U`
- Win: message loop + `PostQuitMessage`. macOS: `applicationDidFinishLaunching:`, `terminate:`. Linux: process-level.
- Refs: E (`ready`, `quit`, `exit`) · T (`Ready`/`Exit`) · W (`exit`) · G (`run`/`quit`) · F (`System.exitApplication`) · C (`exitApplication`)

**LC-02 Quit request with veto; last-window-closed policy** `U`
- macOS: `applicationShouldTerminate:` (`NSTerminateCancel`/`Later`), `applicationShouldTerminateAfterLastWindowClosed:`.
- Win / Linux: app-defined (plus the session-end path, LC-09).
- Refs: E (`before-quit`, `will-quit`, `window-all-closed`) · T (`ExitRequested` + `prevent_exit`) · G (`on_quit`) · F (`System.requestAppExit` → `onExitRequested`) · C (`onCloseRequest`; AWT `QuitHandler`)

**LC-03 Reopen with no windows (Dock click)** `S` (macOS)
- macOS: `applicationShouldHandleReopen:hasVisibleWindows:`.
- Refs: E (`activate`) · T (`RunEvent::Reopen`) · G (`on_reopen`) · C(AWT `AppReopenedListener`)

**LC-04 App active/inactive; hide/unhide the app** `C`
- macOS: `applicationDidBecomeActive:`/`DidResignActive:`, `hide:`, `hideOtherApplications:`, `unhideAllApplications:`.
- Win: `WM_ACTIVATEAPP` (no app-level hide).
- Linux: n/a (per-window focus only).
- Refs: E (`did-become-active`, `app.hide`) · G (`hide`, `hide_other_apps`, `on_app_lifecycle`) · F (lifecycle states) · T (`app.show`/`hide` macOS) · C(AWT)

**LC-05 Activation policy / agent app (no Dock icon)** `S` (macOS)
- macOS: `setActivationPolicy:` (Regular/Accessory/Prohibited), `LSUIElement`. Win: n/a. Linux: n/a.
- Refs: E (`setActivationPolicy`, `dock.hide`) · T (`setDockVisibility`) · W(plat) · G (`set_activation_policy`)

**LC-06 Single instance, forwarding argv/URLs to the running instance** `P`
- Win: a named mutex (`CreateMutexW`, `Local\`) + IPC (named pipe or `WM_COPYDATA`).
- macOS: LaunchServices enforces one instance per bundle by default; activation and open events go to the running instance.
- Linux (both): a D-Bus well-known name (`RequestName`) + `org.freedesktop.Application` `Activate`/`Open` (the GApplication model), or a Unix socket.
- Refs: E (`requestSingleInstanceLock`, `second-instance`) · T:plugin-single-instance · G - · F:community · C - · W -

**LC-07 Files/URLs delivered to the app (cold start and while running)** `C`
- macOS: `application:openURLs:` / Apple Events `kAEOpenDocuments` and `kAEGetURL`; the handler must be installed before launch finishes.
- Win: argv (+ single-instance forwarding).
- Linux (both): argv, or D-Bus activation `org.freedesktop.Application.Open`.
- Refs: E (`open-file`/`open-url` macOS) · T (`RunEvent::Opened`) · G (`on_open_urls`) · C(AWT `OpenFilesHandler`/`OpenURIHandler`)

**LC-08 Relaunch / restart** `P`
- Win: `CreateProcessW` + exit. macOS: `NSWorkspace openApplicationAtURL:` (new instance) + exit. Linux: spawn/exec.
- Refs: E (`relaunch`) · T:plugin-process · G (`restart`)

**LC-09 Session end (log off / shutdown) with veto or delay** `C`
- Win: `WM_QUERYENDSESSION`/`WM_ENDSESSION`, `ShutdownBlockReasonCreate`, `RegisterApplicationRestart`.
- macOS: `applicationShouldTerminate:` during logout, `NSWorkspaceWillPowerOffNotification`, `disableSuddenTermination`.
- Linux (both): portal `Inhibit` (logout flag) + `CreateMonitor`/`QueryEndResponse`; logind `Inhibit("shutdown", "delay")` + `PrepareForShutdown`; X11 XSMP (libSM).
- Refs: E (`query-session-end`/`session-end` Win; `powerMonitor` `shutdown` Linux/macOS) · C(AWT `QuitStrategy`, `UserSessionListener`) · F (exit request)

**LC-10 Sleep/resume notifications** `C`
- Win: `WM_POWERBROADCAST` (`PBT_APMSUSPEND`, `PBT_APMRESUMEAUTOMATIC`) / `PowerRegisterSuspendResumeNotification`.
- macOS: `NSWorkspaceWillSleepNotification`/`DidWakeNotification` (+ `ScreensDidSleep`/`Wake`).
- Linux (both): logind `org.freedesktop.login1.Manager.PrepareForSleep(bool)`.
- Refs: E (`suspend`/`resume`) · G (`on_system_sleep`/`on_system_wake`) · C(AWT `SystemSleepListener`)

**LC-11 Screen lock/unlock and fast user switching** `S`
- Win: `WTSRegisterSessionNotification` → `WM_WTSSESSION_CHANGE` (`WTS_SESSION_LOCK`/`UNLOCK`).
- macOS: `NSWorkspaceSessionDidBecomeActive`/`ResignActive`; `com.apple.screenIsLocked`/`Unlocked` distributed notifications (undocumented).
- Linux (both): logind `Session` `Lock`/`Unlock` + `LockedHint`; `org.freedesktop.ScreenSaver` `ActiveChanged`.
- Refs: E (`lock-screen`/`unlock-screen`, Win/macOS) · C(AWT `UserSessionListener`)

**LC-12 Memory pressure** `S`
- Win: `CreateMemoryResourceNotification`. macOS: `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`. Linux (both): portal `MemoryMonitor` (`LowMemoryWarning`), PSI.
- Refs: W/G (`memory_warning`, mobile-oriented)

**LC-13 Responsiveness pings and hang detection** `U` (mandatory on Linux)
- Win: the OS "ghosts" windows after about 5 s without message pumping; `IsHungAppWindow`.
- macOS: the spinning cursor (no API).
- Wayland: answer `xdg_wm_base.ping` with `pong`.
- X11: answer `_NET_WM_PING`.
- Refs: implicit (E `unresponsive` event)

**LC-14 macOS Handoff / NSUserActivity** `S`
- Refs: E (`setUserActivity`, `continue-activity`)

---

## 11. System preferences and theme

**SP-01 Dark/light appearance, change event, per-app override** `U`
- Win: `UISettings.GetColorValue(Background)` or `AppsUseLightTheme` (HKCU `Themes\Personalize`); change via `WM_SETTINGCHANGE("ImmersiveColorSet")` / `UISettings.ColorValuesChanged`.
- macOS: `NSApp.effectiveAppearance` (KVO); override with `NSApp.appearance`.
- Linux (both): portal `Settings` `org.freedesktop.appearance` `color-scheme` + `SettingChanged`. Fallbacks: GNOME `org.gnome.desktop.interface color-scheme` / gtk-theme "-dark"; XSETTINGS `Net/ThemeName` on X11.
- Refs: E (`nativeTheme`) · T (`theme`, `onThemeChanged`, `setTheme`) · W (`theme`, `ThemeChanged`, `system_theme`) · G (`window_appearance`, `on_appearance_changed`) · F (`platformBrightness`; Linux via the portal) · C (`isSystemInDarkTheme`)

**SP-02 Accent color** `S`
- Win: `UISettings.GetColorValue(UIColorType.Accent…)` / `DwmGetColorizationColor`. macOS: `NSColor.controlAccentColor` + `NSSystemColorsDidChangeNotification`. Linux (both): portal `accent-color` (r, g, b as doubles).
- Refs: E (`getAccentColor`, `accent-color-changed` Win/Linux)

**SP-03 High contrast / increased contrast / forced colors** `C`
- Win: `SPI_GETHIGHCONTRAST` + the `GetSysColor` palette (forced colors).
- macOS: `NSWorkspace.accessibilityDisplayShouldIncreaseContrast`.
- Linux (both): portal `contrast`; GNOME `org.gnome.desktop.a11y.interface high-contrast`.
- Refs: E (`shouldUseHighContrastColors`, `inForcedColorsMode`) · F (high-contrast accessibility feature)

**SP-04 Reduce motion / animations enabled** `C`
- Win: `SPI_GETCLIENTAREAANIMATION` / `UISettings.AnimationsEnabled`. macOS: `accessibilityDisplayShouldReduceMotion`. Linux (both): portal `reduced-motion`; GNOME `enable-animations`.
- Refs: E (`getAnimationSettings`) · F (`disableAnimations`; Linux `enable-animations`) · C -

**SP-05 Reduce transparency / transparency effects off** `S`
- Win: `UISettings.AdvancedEffectsEnabled`. macOS: `accessibilityDisplayShouldReduceTransparency`. Linux: n/a.
- Refs: E (`prefersReducedTransparency`)

**SP-06 System text-scale factor** `S`
- Win: `UISettings.TextScaleFactor` (+ change event). macOS: n/a for third-party AppKit apps. Linux (both): GNOME `text-scaling-factor` (portal Settings / gsettings); `Xft.dpi` as a proxy on X11.
- Refs: F (`textScaleFactor`)

**SP-07 Locale, preferred languages, regional formats (24-hour clock, first weekday)** `C`
- Win: `GetUserDefaultLocaleName`, `GetUserPreferredUILanguages`, `GetLocaleInfoEx`; `WM_SETTINGCHANGE("intl")`.
- macOS: `NSLocale.currentLocale` / `preferredLanguages`, `NSCurrentLocaleDidChangeNotification`.
- Linux (both): `LANG`/`LC_*`/`LANGUAGE`; GNOME `clock-format`; `org.freedesktop.locale1` (system locale).
- Refs: E (`getLocale`, `getSystemLocale`, `getPreferredSystemLanguages`) · T:plugin-os (`locale`) · F (localization + `alwaysUse24HourFormat`) · C (JVM)

**SP-08 System fonts and font-rendering settings** `C` (feeds the text system)
- Win: `SPI_GETNONCLIENTMETRICS` (message font), `SPI_GETFONTSMOOTHING`/`SPI_GETFONTSMOOTHINGTYPE` (ClearType), DirectWrite rendering params.
- macOS: `NSFont.systemFontOfSize:` / text styles.
- Linux (both): fontconfig; GNOME `font-name`, `font-antialiasing`, `font-hinting`, `font-rgba-order` (via the portal); XSETTINGS `Xft/Antialias`, `Xft/Hinting`, `Xft/RGBA`.
- Refs: G (`PlatformTextSystem`, `is_subpixel_rendering_supported`) · F/E (engine)

**SP-09 Scrollbar visibility preference (overlay vs always visible)** `S`
- Win: `UISettings.AutoHideScrollBars`. macOS: `NSScroller.preferredScrollerStyle` + change notification. Linux: GNOME `overlay-scrolling`.
- Refs: G (`should_auto_hide_scrollbars`)

**SP-10 Input metrics** `C` (implicit) - Double-click time/distance, drag threshold, caret blink, key-repeat delay/rate, wheel lines, hover time, natural scrolling.
- Win: `GetDoubleClickTime`, `SM_CXDOUBLECLK`/`SM_CXDRAG`, `GetCaretBlinkTime`, `SPI_GETKEYBOARDDELAY`/`SPEED`, `SPI_GETWHEELSCROLLLINES`/`CHARS`, `SPI_GETMOUSEHOVERTIME`.
- macOS: `NSEvent.doubleClickInterval`, `keyRepeatDelay`/`keyRepeatInterval`, the insertion-point blink defaults, `com.apple.swipescrolldirection`.
- Linux (both): XSETTINGS `Net/DoubleClickTime`, `Net/DndDragThreshold`, `Net/CursorBlinkTime`; GNOME `org.gnome.desktop.peripherals.*`, `cursor-blink-time`. Wayland key repeat comes from `wl_keyboard.repeat_info`.
- Refs: W(plat Win32 `use_system_scroll_speed`) · G · F (Linux cursor blink) · E

**SP-11 System semantic colors** `S`
- Win: `GetSysColor`, `UISettings.UIElementColor`. macOS: semantic `NSColor`s (`controlBackgroundColor`, `selectedContentBackgroundColor`, …). Linux: n/a (theme-specific).
- Refs: E (`getColor`, `getSystemColor`)

**SP-12 Power source, power mode, thermal state** `C`
- Win: `GetSystemPowerStatus`, `RegisterPowerSettingNotification(GUID_ACDC_POWER_SOURCE, GUID_POWER_SAVING_STATUS)`, `PowerRegisterForEffectivePowerModeNotifications`.
- macOS: `IOPSNotificationCreateRunLoopSource`, `NSProcessInfo.isLowPowerModeEnabled`, `thermalState` + `NSProcessInfoThermalStateDidChangeNotification`.
- Linux (both): UPower `OnBattery`; portal `PowerProfileMonitor` (`power-saver-enabled`); thermal is n/a.
- Refs: E (`on-ac`/`on-battery`, `isOnBatteryPower`, `thermal-state-change` macOS, `speed-limit-change`) · G (`thermal_state`, `on_thermal_state_change`)

**SP-13 User idle time** `S`
- Win: `GetLastInputInfo`. macOS: `CGEventSourceSecondsSinceLastEventType`. Wayland: `ext-idle-notify-v1` *(staging)*, which notifies after a timeout and cannot be queried. X11: `XScreenSaverQueryInfo`.
- Refs: E (`getSystemIdleState`/`getSystemIdleTime`)

---

## 12. Menus

**MN-01 Application menu bar (macOS global menu)** `C`
- macOS: `NSApp.mainMenu` (`NSMenu`/`NSMenuItem`: key equivalents, submenus, separators, check state, images); validation through `NSMenuItemValidation`/`menuNeedsUpdate:`; special menus `servicesMenu`, `windowsMenu` (auto-populated), `helpMenu` (Help search); standard first-responder actions (`copy:`, `paste:`, `undo:`, …).
- Win / Linux: see MN-02.
- Refs: E (`Menu.setApplicationMenu` + roles) · T (core menu: `Menu`, `Submenu`, `MenuItem`, `CheckMenuItem`, `IconMenuItem`, `PredefinedMenuItem`; `setAsAppMenu`, `setAsWindowsMenuForNSApp`, `setAsHelpMenuForNSApp`) · G (`set_menus`, native on macOS only; `on_validate_app_menu_command`, `on_will_open_app_menu`) · F (`PlatformMenuBar`, macOS only) · C (`MenuBar` → Swing; macOS screen menu bar) · W(plat macOS `with_default_menu` only)

**MN-02 In-window menu bar (Windows/Linux)** `C`
- Win: `HMENU` via `CreateMenu`/`SetMenu`; `WM_COMMAND`/`WM_MENUCOMMAND`, `WM_INITMENUPOPUP` (update state), accelerator tables (`CreateAcceleratorTable`, `TranslateAccelerator`).
- Linux (both): no protocol, so the toolkit draws it. An optional global-menu export goes over `com.canonical.dbusmenu` + `com.canonical.AppMenu.Registrar` (X11) / `org_kde_kwin_appmenu` (KDE Wayland).
- Refs: E (`setMenu`, `autoHideMenuBar`) · T (`setAsWindowMenu`) · W(plat Win32 `with_menu`) · C (Swing) · G (app-drawn) · F -

**MN-03 Context menus (native popup)** `C`
- Win: `CreatePopupMenu` + `TrackPopupMenuEx`. macOS: `popUpMenuPositioningItem:atLocation:inView:` / `popUpContextMenu:withEvent:forView:`.
- Wayland: **no native menu**; self-drawn in an `xdg_popup` with a grab. X11: self-drawn override-redirect window + grabs.
- Refs: E (`menu.popup`) · T (`Menu.popup`) · G (app-drawn) · F (app-drawn) · C (app-drawn `ContextMenuArea`)

**MN-04 Menu item model** `C` - Accelerators shown, mnemonics, check/radio, icons, enabled, submenus, separators, predefined roles.
- Native APIs as in MN-01..03.
- Refs: E (`role` list) · T (`PredefinedMenuItem`) · G (`OsAction`) · F (`PlatformProvidedMenuItem`) · C

**MN-05 Dock menu** `S` (macOS)
- macOS: `applicationDockMenu:`.
- Refs: E (`dock.setMenu`) · G (`set_dock_menu`) · C(AWT `Taskbar.setMenu`)

**MN-06 Windows jump list; Linux desktop actions** `S`
- Win: `ICustomDestinationList` (`BeginList`, `AppendCategory`, `AddUserTasks` with `IShellLinkW`, `CommitList`).
- Linux: static `.desktop` `[Desktop Action]` entries (the dynamic Unity quicklist is effectively obsolete).
- macOS: the Dock menu (MN-05).
- Refs: E (`setJumpList`, `setUserTasks`) · G (`update_jump_list`, Windows)

**MN-07 OS recent-documents list** `S`
- Win: `SHAddToRecentDocs` (feeds the jump list). macOS: `NSDocumentController noteNewRecentDocumentURL:`. Linux: `recently-used.xbel` (Desktop Bookmark spec; `GtkRecentManager`).
- Refs: E (`addRecentDocument`, macOS/Win) · G (`add_recent_document`, macOS)

**MN-08 Windows thumbnail toolbar, clip and tooltip** `S` (Windows)
- Win: `ITaskbarList3::ThumbBarAddButtons`/`ThumbBarUpdateButtons`, `SetThumbnailClip`, `SetThumbnailTooltip`.
- Refs: E

**MN-09 Touch Bar** `S` (macOS, discontinued hardware)
- macOS: `NSTouchBar`.
- Refs: E

**MN-10 Share sheet** `S`
- macOS: `NSSharingServicePicker`. Win: `IDataTransferManagerInterop::ShowShareUIForWindow`. Linux: n/a.
- Refs: E (`ShareMenu`, macOS)

---

## 13. Tray / status item

**TR-01 Tray/status icon with tooltip, click events and a menu** `C`
- Win: `Shell_NotifyIconW` (`NOTIFYICONDATAW` with `guidItem`, `NOTIFYICON_VERSION_4` → `NIN_SELECT`/`WM_CONTEXTMENU`). After an Explorer restart, re-add on `RegisterWindowMessage("TaskbarCreated")`.
- macOS: `NSStatusBar.systemStatusBar statusItemWithLength:` (`button` image (template)/title, `menu`).
- Linux (both): StatusNotifierItem over D-Bus (`org.kde.StatusNotifierItem` registered with `org.kde.StatusNotifierWatcher`; the menu goes over `com.canonical.dbusmenu`). GNOME needs the AppIndicator extension. X11 also has the legacy XEmbed System Tray protocol (`_NET_SYSTEM_TRAY_S{n}`); Wayland is SNI only.
- Refs: E (`Tray`; SNI on Linux) · T (core `TrayIcon`; appindicator on Linux) · C (`Tray` composable; `isTraySupported`) · G - · F:community (tray_manager) · W -

**TR-02 Tray icon geometry for anchoring a popover** `S`
- Win: `Shell_NotifyIconGetRect`. macOS: `statusItem.button.window.frame`. Linux: n/a (SNI gives only the click point).
- Refs: E (`getBounds`, macOS/Win) · T:plugin-positioner (tray positions)

**TR-03 Status-item title text and template/pressed images** `S` (macOS)
- Refs: E (`setTitle`, `setPressedImage`) · T

**TR-04 Tray balloon notification (legacy)** `S` (Windows)
- Win: `NIF_INFO`.
- Refs: E (`displayBalloon`)

---

## 14. Notifications

**NT-01 System notification (title, body, icon)** `C`
- Win: WinRT `ToastNotificationManager`, or the Windows App SDK `AppNotificationManager`. Both need an AppUserModelID; an unpackaged app registers one through a Start-menu shortcut that carries the AUMID (+ `ToastActivatorCLSID` for COM activation) or through `AppNotificationManager.Register()`.
- macOS: `UNUserNotificationCenter` (`requestAuthorizationWithOptions:`, `UNNotificationRequest`); needs a bundled app (Compose's docs: macOS shows notifications only when the app is packaged).
- Linux (both): `org.freedesktop.Notifications` (`Notify`, `ActionInvoked`, `NotificationClosed`), or portal `Notification` (required inside Flatpak).
- Refs: E (`Notification`) · T:plugin-notification (notify-rust) · G (`show_system_notification`, Win/macOS/Linux) · C (`TrayState.sendNotification`, needs a `Tray`) · F:community · W -

**NT-02 Actions, inline reply, activation back into the app (including when it is not running)** `C`
- Win: toast XML actions; COM activator (background/not running) or protocol activation.
- macOS: `UNNotificationCategory`/`UNNotificationAction`/`UNTextInputNotificationAction` + delegate `didReceiveNotificationResponse:`.
- Linux (both): `Notify` actions + `ActionInvoked` (the app must be running unless D-Bus-activated); KDE `x-kde-reply` hint for inline reply.
- Refs: E (`action`, `reply` macOS/Win, `click`) · T:plugin-notification (`registerActionTypes`, `onAction`) · G (actions + `on_system_notification_response`)

**NT-03 Replace, remove and group notifications; permission state** `C`
- Win: `ToastNotificationHistory` (Tag/Group). macOS: `removeDeliveredNotificationsWithIdentifiers:`, `threadIdentifier`, `getNotificationSettingsWithCompletionHandler:`. Linux (both): `replaces_id`, `CloseNotification`; portal `RemoveNotification`.
- Refs: E (`getHistory`/`remove`/`removeGroup` macOS) · T:plugin-notification (`cancel`, `removeActive`, permission) · G (`dismiss_system_notification`)

**NT-04 Urgency, sound, silent, timeout** `S`
- Win: toast scenario, audio, duration. macOS: `UNNotificationSound`, `interruptionLevel`. Linux (both): `urgency` hint, `expire_timeout`.
- Refs: E (`urgency`, `timeoutType`, `sound`, `silent`)

**NT-05 App badge count** `C`
- macOS: `NSApp.dockTile.badgeLabel`.
- Win: no count badge for unpackaged desktop apps, so use `ITaskbarList3::SetOverlayIcon`; packaged apps use `BadgeUpdateManager`.
- Linux (both): `com.canonical.Unity.LauncherEntry` `Update` (`count`, `count-visible`), shown by KDE Plasma and dock extensions.
- Refs: E (`app.setBadgeCount` macOS/Linux, `dock.setBadge`) · T (`setBadgeCount`/`setBadgeLabel`; Windows → `setOverlayIcon`) · C(AWT `Taskbar.setIconBadge`)

---

## 15. Taskbar and Dock integration

**TB-01 Taskbar/Dock progress** `C`
- Win: `ITaskbarList3::SetProgressValue`/`SetProgressState` (normal, paused, error, indeterminate).
- macOS: no native API; draw it into a custom `NSDockTile.contentView`.
- Linux (both): LauncherEntry `progress`/`progress-visible`.
- Refs: E (`setProgressBar`) · T (`setProgressBar`; Linux via libunity) · C(AWT `Taskbar.setProgressValue`, `setWindowProgressState`)

**TB-02 Overlay icon** `S` (Windows)
- Win: `ITaskbarList3::SetOverlayIcon`.
- Refs: E · T

**TB-03 Dock/taskbar attention, Dock visibility, Dock icon image** `C`
- macOS: `requestUserAttention:` (critical/informational), `NSApp.applicationIconImage`, `NSDockTile`.
- Win: `FlashWindowEx`.
- Linux (both): urgency hint / LauncherEntry `urgent`.
- Refs: E (`dock.bounce`, `flashFrame`, `dock.setIcon`, `dock.hide`) · T (`requestUserAttention`, `setDockVisibility`) · W · G · C(AWT)

**TB-04 Windows per-window taskbar properties (AUMID, relaunch command)** `S` (Windows)
- Win: `SHGetPropertyStoreForWindow` (`PKEY_AppUserModel_ID`, `PKEY_AppUserModel_RelaunchCommand`, …).
- Refs: E (`setAppDetails`)

---

## 16. Dialogs

**DL-01 Open-file dialog (filters, multi-select, default path)** `C` (plugin in T, package in F)
- Win: `IFileOpenDialog` (Common Item Dialog: `SetFileTypes`, `FOS_ALLOWMULTISELECT`, `SetFolder`).
- macOS: `NSOpenPanel` (`allowedContentTypes`, `beginSheetModalForWindow:`).
- Linux (both): portal `FileChooser.OpenFile` (filters, `multiple`, `current_folder` from portal v4). The `parent_window` is `wayland:<xdg-foreign handle>` or `x11:<XID>`. Fallbacks: GTK, zenity, kdialog.
- Refs: E (`dialog.showOpenDialog`) · T:plugin-dialog (rfd: GTK3 by default, `xdg-portal` feature) · G (`prompt_for_paths`; Linux via the ashpd portal) · F:pkg (file_selector) · C (AWT `FileDialog`) · W -

**DL-02 Save-file dialog (name, filters, overwrite confirmation)** `C`
- Win: `IFileSaveDialog`. macOS: `NSSavePanel`. Linux (both): portal `FileChooser.SaveFile` (`SaveFiles` for several files).
- Refs: E · T:plugin-dialog (`save`) · G (`prompt_for_new_path`) · F:pkg (file_selector) · C (AWT)

**DL-03 Folder picker (and mixed files + folders)** `C`
- Win: `IFileOpenDialog` + `FOS_PICKFOLDERS`. macOS: `NSOpenPanel.canChooseDirectories` (mixed allowed). Linux (both): portal `OpenFile` with `directory=true` (mixed not supported).
- Refs: E (`openDirectory`) · T:plugin-dialog (`directory`) · G (`can_select_mixed_files_and_dirs`) · F:pkg (`getDirectoryPath`)

**DL-04 Message box / alert / confirm (buttons, severity, checkbox)** `C`
- Win: `TaskDialogIndirect` (comctl32 v6 manifest; custom buttons, verification checkbox) / `MessageBoxW`.
- macOS: `NSAlert` (`alertStyle`, `addButtonWithTitle:`, `suppressionButton`, `beginSheetModalForWindow:`).
- Linux (both): **no portal or standard**. Options are GTK/libadwaita dialogs, zenity/kdialog, or a self-drawn window with the `xdg-dialog-v1` modal hint.
- Refs: E (`showMessageBox`, `showErrorBox`) · T:plugin-dialog (`message`, `ask`, `confirm`) · G (`PlatformWindow::prompt`: TaskDialog on Windows, NSAlert on macOS; returns `None` on Linux, so the app draws it) · C (`DialogWindow` / Swing) · F - (app-drawn)

**DL-05 Color picker and eyedropper** `S`
- Win: `ChooseColorW`. macOS: `NSColorPanel`, `NSColorSampler` (eyedropper). Linux (both): GTK color chooser; portal `Screenshot.PickColor` (eyedropper).
- Refs: none core (E via web `<input type=color>`/EyeDropper; C via Swing)

**DL-06 Font picker** `S`
- Win: `ChooseFontW`. macOS: `NSFontPanel`/`NSFontManager`. Linux: GTK font chooser.
- Refs: none core

**DL-07 About panel** `S`
- macOS: `orderFrontStandardAboutPanelWithOptions:`. Win / Linux: app-drawn.
- Refs: E (`showAboutPanel`, `setAboutPanelOptions`) · T (`PredefinedMenuItem` about) · C(AWT `AboutHandler`)

Dialog parenting: every dialog takes a parent (a sheet on macOS, an owner `HWND` on Windows, a portal parent handle via `xdg-foreign` on Wayland). Without a parent, Wayland compositors cannot keep the dialog above the window.

---

## 17. Shell integration

**SH-01 Open a URL in the default handler** `C` (plugin in T, package in F)
- Win: `ShellExecuteExW("open")` / `Launcher.LaunchUriAsync`.
- macOS: `NSWorkspace openURL:` (configuration variant).
- Linux (both): portal `OpenURI.OpenURI`, else `xdg-open` / GIO `g_app_info_launch_default_for_uri`. On Wayland, pass an xdg-activation token so the target can take focus.
- Refs: E (`shell.openExternal`) · T:plugin-opener (`openUrl`) · G (`open_url`) · F:pkg (url_launcher) · C (`LocalUriHandler`; AWT `Desktop.browse`)

**SH-02 Open a file with its default app** `C`
- Win: `ShellExecuteExW`. macOS: `NSWorkspace openURL:` (file URL). Linux (both): portal `OpenURI.OpenFile` (fd) / `xdg-open`.
- Refs: E (`shell.openPath`) · T:plugin-opener (`openPath`) · G (`open_with_system`) · F:pkg (url_launcher) · C(AWT `Desktop.open`)

**SH-03 Reveal (select) an item in the file manager** `C`
- Win: `SHOpenFolderAndSelectItems`. macOS: `NSWorkspace activateFileViewerSelectingURLs:`. Linux (both): `org.freedesktop.FileManager1.ShowItems`; portal `OpenURI.OpenDirectory`.
- Refs: E (`shell.showItemInFolder`) · T:plugin-opener (`revealItemInDir`) · G (`reveal_path`) · C(AWT `Desktop.browseFileDirectory`)

**SH-04 Move to Trash / Recycle Bin** `P`
- Win: `IFileOperation::DeleteItem` with `FOFX_RECYCLEONDELETE` (or `SHFileOperationW` + `FOF_ALLOWUNDO`).
- macOS: `NSFileManager trashItemAtURL:resultingItemURL:error:`.
- Linux (both): portal `Trash.TrashFile` (fd), or the freedesktop Trash spec (`$XDG_DATA_HOME/Trash` + `.trashinfo`) / GIO `g_file_trash`.
- Refs: E (`shell.trashItem`) · C(AWT `Desktop.moveToTrash`) · T - · G - · F -

**SH-05 System beep / alert sound** `C`
- Win: `MessageBeep` / `PlaySoundW(SND_ALIAS)`. macOS: `NSBeep` / `NSSound`. Wayland: `xdg-system-bell-v1` *(staging)* or a sound-theme library. X11: `XkbBell`.
- Refs: E (`shell.beep`) · G (`play_system_bell`) · F (`SystemSound.play`) · C (AWT `Toolkit.beep`)

**SH-06 Query the default handler for a URL scheme or file type** `S`
- Win: `AssocQueryStringW`. macOS: `NSWorkspace URLForApplicationToOpenURL:`. Linux (both): `xdg-mime query` / GIO `g_app_info_get_default_for_type`.
- Refs: E (`getApplicationNameForProtocol`, `getApplicationInfoForProtocol`)

**SH-07 File/system icons for a path** `S`
- Win: `SHGetFileInfoW` / `IShellItemImageFactory`. macOS: `NSWorkspace iconForFile:`. Linux (both): GIO file info + an icon-theme lookup.
- Refs: E (`app.getFileIcon`)

**SH-08 Windows shortcut (.lnk) creation** `S`
- Win: `IShellLinkW`.
- Refs: E (`writeShortcutLink`/`readShortcutLink`)

---

## 18. Deep links, URL schemes and file associations

**DLK-01 Register as the handler for a custom URL scheme** `P`
- Win: `HKCU\Software\Classes\<scheme>` (`URL Protocol` value + `shell\open\command`), or an MSIX `uap:Protocol`.
- macOS: Info.plist `CFBundleURLTypes` (static); `NSWorkspace setDefaultApplicationAtURL:toOpenURLsWithScheme:completionHandler:` (12+).
- Linux (both): `.desktop` `MimeType=x-scheme-handler/<scheme>` + `update-desktop-database` / `xdg-mime default`.
- Refs: E (`setAsDefaultProtocolClient`; macOS requires the Info.plist entry) · T:plugin-deep-link (`register`, `isRegistered`) · G (`register_url_scheme`, macOS only; Win/Linux unimplemented) · C (packaging) · F:community

**DLK-02 Receive a deep link at cold start and while running** `P`
- macOS: `kAEGetURL` / `application:openURLs:`. Win: argv of a new process → single-instance forwarding. Linux (both): argv or D-Bus `Open`.
- Refs: E (`open-url` macOS; `second-instance` argv on Win/Linux) · T:plugin-deep-link (`onOpenUrl`, `getCurrent`) with plugin-single-instance · G (`on_open_urls`) · C(AWT `OpenURIHandler`)

**DLK-03 File-type associations** `P` (mostly packaging)
- Win: a ProgID under `HKCU\Software\Classes` + `.ext\OpenWithProgids` + `SHChangeNotify(SHCNE_ASSOCCHANGED)`. The default handler cannot be set programmatically (the UserChoice hash).
- macOS: Info.plist `CFBundleDocumentTypes` / `UTExportedTypeDeclarations`.
- Linux (both): `.desktop` `MimeType=` + a shared-mime-info package XML + `update-mime-database`.
- Refs: E (packager config + `open-file`) · T (bundle `fileAssociations` + `RunEvent::Opened`) · C (packaging)

---

## 19. Global shortcuts

**GS-01 System-wide hotkeys while unfocused** `P`
- Win: `RegisterHotKey` → `WM_HOTKEY` (fails if another app holds the combination).
- macOS: Carbon `RegisterEventHotKey` (no permission needed). `NSEvent` global monitors need the Accessibility/Input Monitoring permission.
- Wayland: **no direct grab**. Use portal `GlobalShortcuts` (`CreateSession` → `BindShortcuts`; the user confirms; needs a valid app identity; KDE and recent GNOME).
- X11: `XGrabKey` on the root window, for each lock-modifier combination.
- Refs: E (`globalShortcut`; Wayland via the portal, async `registration-resolved`) · T:plugin-global-shortcut · G - · F:community (hotkey_manager) · C - · W -

---

## 20. Autostart

**AS-01 Launch at login** `P`
- Win: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (respect `Explorer\StartupApproved\Run`), a Startup-folder `.lnk`, or MSIX `windows.startupTask`.
- macOS: `SMAppService.mainApp` `register`/`unregister`/`status` (13+); legacy LaunchAgent plist.
- Linux (both): XDG Autostart `~/.config/autostart/<app>.desktop`; in a sandbox, portal `Background.RequestBackground(autostart=true)`.
- Refs: E (`setLoginItemSettings`, macOS/Win) · T:plugin-autostart · F:community (launch_at_startup) · G - · C -

---

## 21. Secure storage

**SS-01 OS credential store (save/read/delete secrets)** `P`
- Win: Credential Manager `CredWriteW`/`CredReadW`/`CredDeleteW` (`CRED_TYPE_GENERIC`; blob limit `CRED_MAX_CREDENTIAL_BLOB_SIZE` = 2560 bytes) or WinRT `PasswordVault`.
- macOS: Keychain `SecItemAdd`/`SecItemCopyMatching`/`SecItemUpdate`/`SecItemDelete` (generic/internet password classes).
- Linux (both): the Secret Service D-Bus API `org.freedesktop.secrets` (gnome-keyring, KWallet ≥ 5.97, KeePassXC) via libsecret/oo7. In a sandbox, portal `Secret.RetrieveSecret` gives a per-app master secret, and the app encrypts a local file with it.
- Refs: G (`write_credentials`/`read_credentials`/`delete_credentials`; Linux via oo7) · E (`safeStorage`: encrypt/decrypt with an OS-protected key (Keychain, DPAPI, libsecret/KWallet/portal), not a credential store) · T - (stronghold is its own encrypted vault, not the OS store) · F:community (flutter_secure_storage) · C -

**SS-02 User-bound data encryption (DPAPI-like)** `S`
- Win: `CryptProtectData`/`CryptUnprotectData`. macOS / Linux: a key kept in the Keychain or Secret Service.
- Refs: E (`safeStorage`)

**SS-03 Biometric / user-presence check** `S`
- Win: `UserConsentVerifier` (Windows Hello). macOS: `LAContext evaluatePolicy:` (Touch ID). Linux: n/a (fprintd/polkit; no portal).
- Refs: E (`promptTouchID`, macOS) · F:pkg (local_auth, Win/macOS) · T:plugin-biometric (mobile only)

---

## 22. App paths and storage

**PT-01 Standard per-user directories** `C` - Config, data, cache, state, logs, temp, plus Documents, Downloads, Desktop, Pictures, Music, Videos.
- Win: `SHGetKnownFolderPath` (`FOLDERID_RoamingAppData`, `LocalAppData`, `Documents`, `Downloads`, …).
- macOS: `NSFileManager URLsForDirectory:inDomains:` (Application Support, Caches, Documents, Downloads; `~/Library/Logs`); sandboxed apps get container paths.
- Linux (both): XDG Base Directory (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR`) + xdg-user-dirs (`user-dirs.dirs`).
- Refs: E (`app.getPath`) · T (core `path` API: `appConfigDir`, `appDataDir`, `appCacheDir`, `appLogDir`, `documentDir`, …) · F:pkg (path_provider, xdg_directories) · G (`app_path` only) · C -

**PT-02 Executable and resource location** `U`
- Win: `GetModuleFileNameW`. macOS: `NSBundle.mainBundle` (`resourceURL`, `bundleURL`). Linux: `/proc/self/exe`, `$APPDIR` (AppImage), `/app` (Flatpak).
- Refs: E (`getAppPath`) · T (`resourceDir`, `executableDir`) · G (`app_path`, `path_for_auxiliary_executable`) · F · C

**PT-03 Persisted key-value preferences** `P`
- Win: registry or a file. macOS: `NSUserDefaults`. Linux: a file under `XDG_CONFIG_HOME` (GSettings for GNOME apps).
- Refs: T:plugin-store · F:pkg (shared_preferences) · E (`systemPreferences` user defaults, macOS)

**PT-04 Sandbox file-access persistence** `S`
- macOS: security-scoped bookmarks (`startAccessingSecurityScopedResource`). Linux: portal `Documents` (document store). Win: MSIX capabilities.
- Refs: E (`startAccessingSecurityScopedResource`, MAS)

---

## 23. Screen capture

**SC-01 Enumerate and capture screens/windows as a video stream** `S`
- Win: `Windows.Graphics.Capture` (`IGraphicsCaptureItemInterop::CreateForMonitor`/`CreateForWindow`, `GraphicsCapturePicker`) or DXGI Desktop Duplication.
- macOS: ScreenCaptureKit (`SCShareableContent`, `SCStream`, `SCContentSharingPicker` 14+); needs the Screen Recording TCC permission (`CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess`).
- Wayland: portal `ScreenCast` + a PipeWire stream (the user picks the source; `restore_token` keeps the choice); `ext-image-copy-capture-v1` *(staging, privileged clients)*.
- X11: `XShmGetImage`/`XGetImage`; XComposite for individual windows.
- Refs: E (`desktopCapturer`; PipeWire on Linux) · G (`screen_capture_sources`) · T - · F - · C - · W -

**SC-02 One-shot screenshot** `S`
- Win: Graphics.Capture / `BitBlt` from the screen DC. macOS: `SCScreenshotManager`. Linux (both): portal `Screenshot` (interactive option).
- Refs: E (capturer thumbnails)

The reverse case, excluding the app's own window from capture, is WN-23.

---

## 24. Power management

**PW-01 Prevent idle sleep / display sleep** `P`
- Win: `PowerCreateRequest` + `PowerSetRequest(PowerRequestDisplayRequired | SystemRequired)`, or `SetThreadExecutionState`.
- macOS: `IOPMAssertionCreateWithName` (`kIOPMAssertPreventUserIdleDisplaySleep`/`…SystemSleep`) or `NSProcessInfo beginActivityWithOptions:reason:`.
- Linux (both): portal `Inhibit` (idle=8, suspend=4), `org.freedesktop.ScreenSaver.Inhibit`, logind `Inhibit("idle:sleep")`. Wayland `idle-inhibit-unstable-v1` works only while the surface is visible.
- Refs: E (`powerSaveBlocker`: `prevent-app-suspension` / `prevent-display-sleep`) · G (`prevent_idle_sleep`, Win/macOS) · T - · F:community · C -

Power and session events are covered in LC-09..LC-11 and SP-12.

---

## 25. Printing

**PR-01 Print dialog, page setup, job submission** `S`
- Win: `PrintDlgExW` + GDI `StartDoc`, the XPS Print API, or `PrintManager` via `IPrintManagerInterop::ShowPrintUIForWindowAsync`.
- macOS: `NSPrintOperation` + `NSPrintPanel` + `NSPageLayout`.
- Linux (both): portal `Print` (`PreparePrint` → dialog/settings, `Print` → an fd with PDF/PS); `GtkPrintOperation`; CUPS/IPP.
- Refs: E (`webContents.print`, web content) · C (AWT `PrinterJob`, `Desktop.print`) · T (webview `window.print` only) · F:community (printing) · G - · W -

**PR-02 Render UI to PDF** `S`
- Refs: E (`printToPDF`)

---

## 26. Permissions and other platform extras

**PX-01 Privacy permissions (camera, microphone, screen recording, accessibility, input monitoring)** `S`
- macOS: TCC (`AVCaptureDevice requestAccessForMediaType:`, `CGRequestScreenCaptureAccess`, `AXIsProcessTrustedWithOptions`). Win: privacy settings (`AppCapability`). Linux (both): portals `Camera`, `ScreenCast`, `RemoteDesktop`.
- Refs: E (`getMediaAccessStatus`, `askForMediaAccess`)

**PX-02 OS info (platform, version, arch, hostname, session type, compositor)** `C`
- Linux: `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`/`DISPLAY`, `XDG_CURRENT_DESKTOP`.
- Refs: T:plugin-os · G (`compositor_name`, `graphical_environment`) · E (`process`) · F (`Platform`)

**PX-03 macOS app polish** `S`
- Move to /Applications (`isInApplicationsFolder`/`moveToApplicationsFolder`), Quick Look previews (`QLPreviewPanel`).
- Refs: E

Adjacent and out of scope for a platform layer, listed so they are not mistaken for gaps: auto-update (E `autoUpdater`; T:plugin-updater), crash reporting (E `crashReporter`/Crashpad), in-app purchase (E; F:pkg in_app_purchase), CLI parsing (T:plugin-cli), gamepads, network status (portal `NetworkMonitor`), location, camera, wallpaper.

---

## Appendix A - Linux backend dependency map

These are what a native Wayland + X11 backend has to speak to cover the rows above. *Status* follows
wayland-protocols' `stable`/`staging`/`unstable` directories; *vendor* means KDE/wlroots-specific.

| Need | Wayland | X11 | Both (D-Bus / files) |
|---|---|---|---|
| Windows, popups, states | `xdg-shell` (stable; v5 `wm_capabilities`, v6 `suspended`), `xdg-decoration` (unstable), `xdg-dialog-v1` (staging), `xdg-toplevel-icon-v1` (staging), `xdg-toplevel-drag-v1` (staging), `xdg-session-management` (staging), `wlr-layer-shell` (vendor) | ICCCM + EWMH (`_NET_WM_*`), `_MOTIF_WM_HINTS`, XSync, RandR, XFixes, XShape, Present | - |
| Focus/activation | `xdg-activation-v1` (staging) | `_NET_ACTIVE_WINDOW`, startup-notification | - |
| Scale, viewport, frames | `fractional-scale-v1` (staging), `viewporter` (stable), `presentation-time` (stable), `fifo-v1`, `commit-timing-v1` (staging) | XSETTINGS, `Xft.dpi`, Present | portal Settings |
| Pointer | `cursor-shape-v1` (staging), `pointer-constraints`, `relative-pointer`, `pointer-gestures` (unstable), `pointer-warp-v1` (staging), `tablet` (stable) | XInput2 (2.2 touch, 2.4 gestures), Xcursor | cursor theme env/settings |
| Keyboard / IME | `wl_keyboard` + xkbcommon, `text-input-unstable-v3`, `keyboard-shortcuts-inhibit` (unstable) | XKB (`xkbcommon-x11`), XIM | IBus/Fcitx D-Bus (optional) |
| Clipboard / DnD | `wl_data_device`, `primary-selection` (unstable), `ext-data-control-v1` (staging; managers only) | ICCCM selections, XDND, XDS | - |
| Idle / inhibit | `idle-inhibit` (unstable), `ext-idle-notify-v1` (staging) | MIT-SCREEN-SAVER | portal Inhibit, `org.freedesktop.ScreenSaver`, logind |
| Backdrop, color | `ext-background-effect-v1` (staging), `org_kde_kwin_blur` (vendor), `color-management-v1` (staging), `alpha-modifier-v1` (staging) | `_KDE_NET_WM_BLUR_BEHIND_REGION` | - |
| Bell | `xdg-system-bell-v1` (staging) | XKB bell | sound theme |
| Parent handle for portals | `xdg-foreign` (unstable) | XID | - |
| Services | - | XEmbed tray (legacy) | Portals: FileChooser, OpenURI, Settings, Notification, Inhibit, Background, GlobalShortcuts, ScreenCast, Screenshot, Print, Trash, Secret, MemoryMonitor, PowerProfileMonitor, Documents. Non-portal: `org.freedesktop.Notifications`, StatusNotifierItem + dbusmenu, Unity LauncherEntry, `org.freedesktop.FileManager1`, Secret Service, logind, UPower, AT-SPI2 (`org.a11y.Bus`). Files: `.desktop`, XDG basedir/autostart/mime |

---

## Sources

Framework primary sources (fetched 2026-10-14):
- winit `FEATURES.md` on master, which now holds only the scope and tiers (the feature matrix was removed in #3479): https://github.com/rust-windowing/winit/blob/master/FEATURES.md
- winit `FEATURES.md` at the last revision with the per-platform matrix (commit 82d9bbe559, 2024-06-24): https://github.com/rust-windowing/winit/blob/82d9bbe559/FEATURES.md
- winit master API: `winit-core/src/{window.rs,event.rs,event_loop/mod.rs,data_transfer.rs,application/*}` and `winit-{win32,appkit,wayland,x11}/src/lib.rs`: https://github.com/rust-windowing/winit/tree/master
- Electron API docs (index and modules app, BaseWindow, dialog, Menu, MenuItem, Tray, Notification, clipboard, shell, globalShortcut, powerMonitor, powerSaveBlocker, screen, systemPreferences, nativeTheme, safeStorage, desktopCapturer, dock, ShareMenu, autoUpdater, crashReporter): https://www.electronjs.org/docs/latest/api/app and source https://github.com/electron/electron/tree/main/docs/api
- Tauri v2 core JS API (`window.ts`, `app.ts`, `menu/*`, `tray.ts`, `path.ts`): https://github.com/tauri-apps/tauri/tree/dev/packages/api/src ; `RunEvent`: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/app.rs ; `WindowEvent`: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime/src/window.rs
- Tauri official plugins (README table and `plugins/*/guest-js/index.ts`): https://github.com/tauri-apps/plugins-workspace
- GPUI `Platform` / `PlatformWindow` / `PlatformDisplay`: https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs ; input enum: https://github.com/zed-industries/zed/blob/main/crates/gpui/src/interactive.rs ; backends: https://github.com/zed-industries/zed/tree/main/crates/gpui_windows/src , https://github.com/zed-industries/zed/tree/main/crates/gpui_macos/src , https://github.com/zed-industries/zed/tree/main/crates/gpui_linux
- Flutter SystemChannels: https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/services/system_channels.dart ; Windows embedder (`platform_handler.cc`, `settings_plugin.cc`, `host_window_*`): https://github.com/flutter/flutter/tree/master/engine/src/flutter/shell/platform/windows ; Linux embedder (`fl_settings_portal.cc`): https://github.com/flutter/flutter/tree/master/engine/src/flutter/shell/platform/linux ; macOS `FlutterMenuPlugin`: https://github.com/flutter/flutter/tree/master/engine/src/flutter/shell/platform/darwin/macos/framework/Source ; multi-window `_window.dart` and `platform_menu_bar.dart`: https://github.com/flutter/flutter/tree/master/packages/flutter/lib/src/widgets ; first-party packages: https://github.com/flutter/packages/tree/main/packages
- Compose Multiplatform Desktop: https://kotlinlang.org/docs/multiplatform/compose-desktop-components.html , https://kotlinlang.org/docs/multiplatform/compose-desktop-top-level-windows-management.html , https://kotlinlang.org/docs/multiplatform/compose-desktop-tray.html , https://kotlinlang.org/docs/multiplatform/compose-desktop-accessibility.html , https://kotlinlang.org/docs/multiplatform/compose-drag-drop.html
- JDK `Desktop.Action` and `Taskbar.Feature` (services Compose apps reach via AWT): https://docs.oracle.com/en/java/javase/21/docs/api/java.desktop/java/awt/Desktop.Action.html , https://docs.oracle.com/en/java/javase/21/docs/api/java.desktop/java/awt/Taskbar.Feature.html

Linux platform specifications:
- xdg-desktop-portal interfaces (`data/org.freedesktop.portal.*.xml`, including Settings keys `color-scheme`, `accent-color`, `contrast`, `reduced-motion`; Inhibit flags; Background autostart): https://github.com/flatpak/xdg-desktop-portal/tree/main/data and https://flatpak.github.io/xdg-desktop-portal/docs/api-reference.html
- wayland-protocols tree (stable / staging / unstable): https://gitlab.freedesktop.org/wayland/wayland-protocols ; browsable: https://wayland.app/protocols/
- freedesktop specs: notifications https://specifications.freedesktop.org/notification-spec/latest/ ; Secret Service https://specifications.freedesktop.org/secret-service/latest/ ; Base Directory https://specifications.freedesktop.org/basedir-spec/latest/ ; Desktop Entry https://specifications.freedesktop.org/desktop-entry-spec/latest/ ; Autostart https://specifications.freedesktop.org/autostart-spec/latest/ ; EWMH https://specifications.freedesktop.org/wm-spec/latest/ ; XEmbed system tray https://specifications.freedesktop.org/systemtray-spec/latest/ ; StatusNotifierItem https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/ ; AT-SPI2 https://www.freedesktop.org/wiki/Accessibility/AT-SPI2/

Windows and macOS API references (for the less common entries):
- DWM window attributes (dark mode, backdrop, corners, cloak): https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
- Snap Layouts with a custom title bar: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/apply-snap-layout-menu
- High DPI: https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows
- TSF: https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework ; UIA providers: https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview
- ITaskbarList3: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-itaskbarlist3 ; ICustomDestinationList: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-icustomdestinationlist ; notification area: https://learn.microsoft.com/en-us/windows/win32/shell/notification-area
- App notifications: https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/app-notifications-quickstart
- SetWindowDisplayAffinity: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity ; PowerSetRequest: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-powersetrequest ; WM_QUERYENDSESSION: https://learn.microsoft.com/en-us/windows/win32/shutdown/wm-queryendsession
- WM_POINTERUPDATE: https://learn.microsoft.com/en-us/windows/win32/inputmsg/wm-pointerupdate ; DirectManipulation: https://learn.microsoft.com/en-us/windows/win32/directmanipulation/direct-manipulation-portal
- Common Item Dialog: https://learn.microsoft.com/en-us/windows/win32/shell/common-file-dialog ; Task dialogs: https://learn.microsoft.com/en-us/windows/win32/controls/task-dialogs-overview ; UISettings: https://learn.microsoft.com/en-us/uwp/api/windows.ui.viewmanagement.uisettings ; screen capture: https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture ; CredWriteW: https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credwritew ; clipboard formats: https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats ; OLE drag and drop: https://learn.microsoft.com/en-us/windows/win32/com/drag-and-drop ; file associations: https://learn.microsoft.com/en-us/windows/win32/shell/fa-intro ; URL protocol registration: https://learn.microsoft.com/en-us/previous-versions/windows/internet-explorer/ie-developer/platform-apis/aa767914(v=vs.85)
- Apple: NSTextInputClient https://developer.apple.com/documentation/appkit/nstextinputclient ; UNUserNotificationCenter https://developer.apple.com/documentation/usernotifications/unusernotificationcenter ; SMAppService https://developer.apple.com/documentation/servicemanagement/smappservice ; ScreenCaptureKit https://developer.apple.com/documentation/screencapturekit ; NSStatusItem https://developer.apple.com/documentation/appkit/nsstatusitem ; NSAccessibility https://developer.apple.com/documentation/appkit/nsaccessibility ; Keychain Services https://developer.apple.com/documentation/security/keychain-services ; NSPasteboard https://developer.apple.com/documentation/appkit/nspasteboard ; NSVisualEffectView https://developer.apple.com/documentation/appkit/nsvisualeffectview ; IOPMLib https://developer.apple.com/documentation/iokit/iopmlib_h

Provenance: framework attributions in the Refs lines come from the sources above. Native API names for Windows,
macOS and the freedesktop/D-Bus services come from the vendor references above and from established practice in the
frameworks' backend code (e.g. GPUI's Win32 notification backend uses `windows::UI::Notifications`, its prompt uses
`TaskDialogIndirect`, and its credential store uses `CredWriteW`). Items marked "undocumented" depend on private OS
interfaces.
