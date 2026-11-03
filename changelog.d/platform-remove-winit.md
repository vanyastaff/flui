### Removed

- **`flui-native`**: the winit host is deleted: `WinitPlatform`, the `winit-backend` feature,
  `input::availability::winit` and `WinitSession`, and the `winit`, `ui-events-winit` and
  `arboard` dependencies. Windows and macOS run on the native Win32 and AppKit hosts, Linux on the
  native Wayland and X11 hosts
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md) §11), and `deny.toml` bans
  the `winit` crate. `PlatformProxy::open_window` was served only by the winit host; every host
  now refuses it with `ProxySendError::Unsupported`.
- **`flui-protocol`**: `native_recording::Backend::Winit` is removed; no backend records
  `"winit"` any more.
