### Fixed

- On Linux the Settings portal is no longer polled. A portal that has neither motion key, a
  session with no portal, and a session with no bus used to be read every second for the life of
  the process. Now a missing key beside others that answer, a missing portal and a missing
  Settings interface wait for the portal's next change notice or a new portal on the bus. A read
  that failed for a moment, or a portal that answers no setting at all (its backend not started
  yet), is read again on a doubling schedule that runs out. A session bus that never answers, or
  that drops the connection before the portal answers, is tried for about a minute and then left
  alone.
- On Linux an assistive technology places a window's nodes where the window is. On X11 a node's
  screen coordinates follow each move and resize the X server reports to the window, a window
  manager's frame included. On Wayland they are relative to the window, with the content below
  the title bar the window draws itself, in device pixels at the window's scale. Before, every
  window was reported at the screen's origin, so a screen reader's mouse review, a magnifier
  following the focus and a test tool clicking a node landed in the wrong place.

### Added

- `flui_native::host::ScreenBounds` (built with `try_new`, refused with `InvalidScreenBounds`)
  says where a window and its content lie on the screen, in device pixels. A headless window
  reports what it last told its bridge in `HeadlessWindowSystem::accessibility_bounds`.

### Changed

- `flui_native::host::WindowAccessibility` has a new required method, `bounds_changed`. An
  implementation outside this crate must add it; a bridge whose adapter reads the window's
  position itself implements it as a no-op.
