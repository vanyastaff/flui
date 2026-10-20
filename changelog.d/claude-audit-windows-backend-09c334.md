### Changed

- Windows: the fullscreen hotkey (F11 by default) is a default the app can prevent. It toggles
  after the key reaches the app, on a message of its own, instead of inside the keystroke.
- Windows: a resize that is not a live drag (a window command, a mode change, a DPI change)
  asks for one frame after the change instead of drawing inside `WM_SIZE`, and window commands
  apply on the owner's wake, not inside a frame. The window class no longer sets
  `CS_HREDRAW`/`CS_VREDRAW`.

### Fixed

- Windows: a task woken while the window is minimized is polled without waiting for the
  restore.
- Windows: a DPI change reaches the app's resize callback, and so its scale, also when the
  window keeps its client size in device pixels or the system's rectangle could not be applied.
- Windows: a fullscreen window keeps covering its monitor through a DPI change, and returns to
  a rectangle rescaled for the new DPI.
- Windows: `SetSize` and `SetPosition` beyond what Win32 can represent are refused with
  `WindowCommandError::InvalidGeometry` instead of being saturated.
- Windows: wake deadlines (caret blink, timers, the reveal fallback) keep firing while the
  window is moved or sized with the pointer at rest and while its menus are open.
- Windows: a window on another virtual desktop (cloaked) or in a locked session is reported not
  visible, so its frames stop.

### Removed

- `WindowsWindow::should_skip_render`: a minimized window's paint now requests its frame, and
  the runtime, told the window is not visible, decides what that frame does.
