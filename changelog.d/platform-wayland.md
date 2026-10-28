### Added

- `flui_native::WaylandPlatform`, the native Wayland host (the `wayland` feature, now part of
  `desktop`): its loop blocks on the compositor's socket and an `eventfd` with no timer of its own;
  `xdg-shell` windows publish their snapshot from the compositor's configures, take the commands
  their capabilities admit and hand out a surface target over libwayland; displays, the Settings
  portal, logind and AT-SPI come from the live Linux layer. No input, text input, clipboard or
  drag and drop yet.
- `current_platform` chooses the Linux host at run time: winit while `winit-backend` is built,
  otherwise the Wayland host for a Wayland session.

### Changed

- On Linux `zbus`, `accesskit_unix` and `futures-util` are unconditional dependencies of
  `flui-native`; `winit-backend` no longer brings them.
