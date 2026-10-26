### Added

- **`flui`**: the `native` feature, off by default, and `flui::native`, the escape hatch
  (ADR-0254 §8): `HostWindow` and, on Windows, `flui::native::windows` (`WindowsWindowExt`,
  `Win32Window`, `WindowsBackdrop`), on macOS `flui::native::macos` (`MacOSWindowExt`,
  `AppKitWindow`, `MacOSBackdrop`, `MacOSBackdropError`, `MacOSWindowLevel`,
  `MacOSCollectionBehavior`, `TabJoinError`). It is Evolving, not Stable (ADR-0089 §5). Each
  extension narrows a window to its OS's view and answers `None` for a window of another host.
- **`flui-platform`**: `WindowOptions::os`, an `OsWindowOptions`: what a window is created with on
  one operating system, as a value of that system's type. `OsWindowOptions::Windows` takes a
  `Win32WindowOptions` (`corner_preference: WindowsCornerPreference`, `theme: WindowsTheme`);
  only the Win32 host reads it, and another host opens the window without it. `AppConfig` carries
  it as `os_window_options` (`AppConfig::with_os_window_options`).
- **`flui-native`**: the macOS backdrop material, which was the Liquid Glass backdrop:
  `AppKitWindow::set_backdrop(MacOSBackdrop)` puts an `NSVisualEffectView` with that material
  behind FLUI's view, and `clear_backdrop` takes it away; both answer `MacOSBackdropError` for a
  closed window, a window off the AppKit main lane and an effect view AppKit did not create.

### Changed

- **`flui-native`**: the OS extensions moved from the crate root to `flui_native::os::windows` and
  `flui_native::os::macos`, the modules the facade publishes.
- **`flui-native`**: a Win32 window opens with the corners and title-bar theme of its
  `Win32WindowOptions`; without them it opens as before, with rounded corners and the user's app
  theme.
- **`flui-platform`**: `WindowCornerPreference` and `WindowsTheme` moved from `flui-native` to the
  contract, as `WindowsCornerPreference` and `WindowsTheme`, beside the options that name them,
  without their DWM conversions.
