### Removed

- `LinuxPlatform` (`flui-native`): a stub whose constructor and most methods panicked with
  `unimplemented!()`; Linux runs on the winit fallback host until the native Wayland and X11 host
  replaces it ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md)). The Linux
  display source, notices, X11 window probe and AT-SPI adapter stay.
- `LinuxWindowExt` (`flui-native`): an extension trait nothing implemented.
