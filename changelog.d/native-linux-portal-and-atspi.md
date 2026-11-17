### Fixed

- On Linux the Settings portal is no longer polled. A portal that has neither motion key, a
  session with no portal, and a session with no bus used to be read every second for the life of
  the process; now a missing key, a missing portal and a missing Settings interface wait for the
  portal's next change notice or a new portal on the bus, a read that failed for a moment is
  retried on a doubling schedule that runs out, and a session bus that never answers is tried for
  about a minute and then left alone.
- On Linux an assistive technology places a window's nodes where the window is: on X11 a node's
  screen coordinates follow the window as it moves and resizes, and on Wayland they are relative to
  the window with the content below the title bar the window draws itself. Before, every window
  was reported at the screen's origin, so a screen reader's mouse review, a magnifier following
  the focus and a test tool clicking a node landed in the wrong place.

### Added

- `flui_native::host::ScreenBounds` and the required `WindowAccessibility::bounds_changed`: a
  window tells its accessibility bridge where it and its content lie on the screen, in device
  pixels.
