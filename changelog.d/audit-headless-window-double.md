### Changed

- **`flui-native`**: the headless host's window `MockWindow` is renamed `HeadlessHostWindow`, the
  native window every `HeadlessPlatform` opens. It stays distinct from
  `flui_testing::HeadlessWindow`, the window a `HeadlessHost` presents a UI runtime into without
  a host. No alias is kept: replace `flui_native::MockWindow` with
  `flui_native::HeadlessHostWindow`.
