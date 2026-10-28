### Added

- `flui_native::WaylandPlatform`, the native Wayland host (the `wayland` feature, now part of
  `desktop`): its loop blocks on the compositor's socket and an `eventfd` with no timer of its own;
  `xdg-shell` windows publish their snapshot from the compositor's configures, take the commands
  their capabilities admit and hand out a surface target over libwayland; displays, the Settings
  portal, logind and AT-SPI come from the live Linux layer. No input, text input, clipboard or
  drag and drop yet.
- `current_platform()` chooses the Linux host at run time. It still picks the winit host whenever
  `winit-backend` is built, which `flui-app` builds on Linux; until the native host carries input,
  it is reached only through `WaylandPlatform::new()`. Without `winit-backend`, a Wayland session
  (`WAYLAND_DISPLAY` naming a socket) gets the Wayland host.

### Changed

- On Linux `zbus`, `accesskit_unix` and `futures-util` are unconditional dependencies of
  `flui-native`; `winit-backend` no longer brings them.
- On Linux the display source chooses Wayland only when `WAYLAND_DISPLAY` names a socket it can
  connect to; `WAYLAND_SOCKET` alone no longer makes a session Wayland.
- The winit host on Linux reaps the `xdg-open` it starts for `reveal_path` and `open_path`.
