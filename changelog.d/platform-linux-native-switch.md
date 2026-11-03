### Changed

- **`flui-app`, `flui-native`**: Linux applications run on the native Wayland and X11 hosts,
  which `current_platform` picks by the session, instead of winit
  ([ADR-0151](/docs/adr/ADR-0151-platform-layer-boundary-and-names.md)). `flui-app` no longer
  builds `flui-native`'s `winit-backend` on Linux, and `current_platform` never picks the winit
  host there even when it is built. On Wayland this brings the clipboard and the primary
  selection, drag and drop, pens, pinches, presentation feedback and `xdg-activation`; on X11,
  IBus, Fcitx5 and XIM input, `PRIMARY`, and drag and drop.
