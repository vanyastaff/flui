### Added

- **`flui-platform`, macOS**: a window reads its state from `NSWindow` (outer top-left
  position, mode from the style mask, `isMiniaturized` and `isZoomed`, title, focus,
  `contentMinSize`/`contentMaxSize`, the rectangle it returns to from a zoomed, minimized or
  full-screen mode) and publishes it again after `windowDidMove`, `windowDidResize`, and the
  miniaturize and full-screen notifications. `apply` carries out every `WindowCommand`; a window
  honours all of them (`WindowCapabilities::ALL`). `WindowOptions::position`, `mode`, `normal_geometry`
  and `display` place the window and put it in its mode when it opens; a hidden window enters its
  mode when it is first shown. `Platform::window_frame_inset` measures the title bar before a
  window exists.
- **`flui-platform`, macOS**: system notices reach the host: `NSWorkspace` will-sleep and did-wake
  (`Sleep`, `Wake`), the distributed `com.apple.screenIsLocked` and `screenIsUnlocked`
  (`SessionLocked`, `SessionUnlocked`), and a memory-pressure dispatch source (warn is `Moderate`,
  critical is `Critical`). A wake marks the displays and every window's state stale first.
- **`flui-platform`, macOS**: logging out, restarting and shutting down are announced to the
  application (`NSWorkspaceWillPowerOffNotification` asks the session-end hook). A session end the
  application holds is answered `NSTerminateLater` and replied to when the application releases its
  blocks (`Platform::release_session_end`), or after four seconds; one it does not hold terminates at
  once after the hook hears `Ending`. A quit nobody announced (Cmd-Q, the Dock menu) still goes
  through the run loop.
- **`flui-platform`, macOS**: `DisplayInfo::persistent_key` is the display's UUID.
- **`flui-platform`, iOS**: `applicationDidReceiveMemoryWarning:` is a critical memory-pressure
  notice, and `applicationWillTerminate:` tells the session-end hook the session is ending before the
  quit handler runs.

### Changed

- **`flui-platform`, macOS**: display `bounds` and `usable_bounds` are measured from the primary
  screen's top-left corner, as on every other platform, so a window position and a display origin are
  in the same space. They were measured from the bottom-left corner.
- **`flui-platform`, macOS**: a window's size limits (`WindowOptions::min_size`, `max_size`) are
  limits on its content size, and are read back from the window.
- **`flui-platform`, macOS**: a session end the application does not hold no longer cancels the
  logout: `applicationShouldTerminate:` answers it `NSTerminateNow`.
