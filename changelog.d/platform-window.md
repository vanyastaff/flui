### Changed

- **`flui-native`**: `MacOSWindow::new` is no longer public. A macOS window opens through
  `Platform::open_window`, as every other host's window does; the constructor took the host's
  internal window map, which only the platform builds.
- **`flui-native`**: the window installation acknowledgement is named at the crate root:
  `flui_native::{WindowInstallation, PendingWindowInstallation, InstallationCancelled}`.
  `flui_native::shared::window_installation` is gone.
