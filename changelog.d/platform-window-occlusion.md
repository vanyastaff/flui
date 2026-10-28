### Added

- **`flui-platform`**: `WindowState::occluded` (and `with_occluded`): the window system keeps a
  shown, unminimized window off the screen (covered where it says so, cloaked, on another
  workspace, behind a locked session, a suspended Wayland toplevel, a background page or app).
  `WindowState::successor` drops an occlusion reported for a hidden or minimized window.
- **`flui-testing`**: `window_conformance::Readback::occluded`, compared with the snapshot where
  the window system answers a query for it; the conformance checks that a minimized window stays
  shown.

### Changed

- **`flui-platform`**: `WindowState::visible` is the window's show state on every host: minimizing
  or covering a window leaves it `true`. X11 and winit read a minimized window as hidden, and
  every host folded occlusion into `visible`; occlusion is now `occluded`. `WindowState::is_seen`
  reads all three (shown, not minimized, not occluded), so frames and the lifecycle stop as before.
- **`flui-native`**: on Wayland, Android, iOS and the Web, where an application cannot hide its
  window, `visible` stays `true` while the window is open, and a suspended toplevel, a stopped
  Activity, a background scene or a hidden document is `occluded`.
- **`flui-native`**: `MockWindow::simulate_visibility` is now `simulate_occlusion`, and
  `HeadlessWindowSystem` gains `occluded`; hiding a headless window is `WindowCommand::Hide`.
