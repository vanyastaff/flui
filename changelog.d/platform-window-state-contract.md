### Added

- **`flui-platform-api`**: a window publishes an immutable `WindowState` snapshot with a
  `WindowRevision` (`PlatformWindow::state`, `on_state_changed`) and takes `WindowCommand`s
  through `PlatformWindow::apply`. `WindowCapabilities` says which commands a window honours,
  `apply` answers `WindowCommandError::Unsupported` exactly when the capability is `false`, and
  `WindowCommand::validate` refuses non-finite or non-positive geometry before the OS is called
  ([ADR-0237](/docs/adr/ADR-0237-window-state-and-host-notices.md)).
- **`WindowOptions`** gains `position`, `mode` and `normal_geometry`; **`DisplayInfo`** gains
  `persistent_key` (`with_persistent_key`), a string that names a display across restarts.
- **`flui-platform`**: the headless platform takes `with_window_capabilities` and
  `with_host_limits` (`HostLimits`: a largest window size, a host that declines to maximize), and
  its windows gain `simulate_move`.

### Changed

- **`WindowMode`** is now `Normal`, `Minimized`, `Maximized` or `Fullscreen`, without the saved
  bounds the old enum carried.
- **`flui-platform`**: `OpenWindowError::LaneFull` and `OwnerGone` carry the rejected
  `WindowOptions` boxed.

### Removed

- **`PlatformWindow`** loses `set_title`, `get_title`, `minimize`, `maximize`, `restore`,
  `toggle_fullscreen`, `resize`, `bounds`, `content_size`, `window_bounds`, `is_maximized`,
  `is_fullscreen` and `on_moved`; `WindowEvent` loses `Moved`, `Minimized`, `Maximized`,
  `Restored`, `Fullscreen` and `ExitFullscreen`. Read the snapshot and apply a `WindowCommand`
  instead.
- **`flui-platform`**: the second window trait `flui_platform::window::Window` with
  `WindowBuilder` and `WindowManager`, the macOS `WindowManager`, and `WindowBounds`.
