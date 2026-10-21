### Added

- **Windows reports each display's refresh period** (`flui-platform`): the native Win32 and the
  winit backends on Windows read the period from `QueryDisplayConfig`'s rational refresh rate
  (59.94 Hz stays 60000/1001), falling back to the whole-hertz rate of the current mode, where 0 and
  1 Hz mean unknown.
- **A Windows window names the display it is on** (`flui-platform`): `PlatformWindow::display_id`
  resolves the window's nearest monitor to its display by native key instead of reporting none, and
  `on_display_changed` fires when the window moves to another monitor or its DPI changes.

### Changed

- **Windows re-samples its displays when they change** (`flui-platform`): `WM_DISPLAYCHANGE`, a
  work-area change and a DPI change invalidate the cached snapshot and wake the owner, instead of
  every owner turn querying the OS.
- **winit on Windows takes its snapshot from the native source** (`flui-platform`): monitors are keyed
  by display target rather than the reusable GDI device name.
