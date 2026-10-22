### Removed

- **`flui-native`**: the OS types of the core host's public paths (ADR-0151 §8): the
  `flui_native::win32` module (`HWND`, `DWMWINDOWATTRIBUTE`, `DwmExtendFrameIntoClientArea`,
  `DwmSetWindowAttribute`, `MARGINS`); the hosts' windows `WindowsWindow` (with its `hwnd()` and
  `toggle_fullscreen_for_hwnd`), `MacOSWindow`, `IOSWindow` and `WinitWindow`;
  `MacOSPlatform::app` (the `NSApplication`); `AndroidPlatform`, with its `new(AndroidApp)` and
  `app()`; and `BackgroundExecutor::handle` (the tokio runtime). A window is the
  `Arc<dyn HostWindow>` the platform opens: its raw window and display handles come from
  `raw-window-handle`'s `HasWindowHandle` and `HasDisplayHandle`, and what one OS offers beyond
  the window contract from the extensions on it (see Changed). The facade's `flui::native::<os>`
  modules will publish those extensions.
- **`flui-native`**: the Liquid Glass backdrop (`LiquidGlassConfig`, `LiquidGlassMaterial`,
  `BlendingMode` and `MacOSWindowExt::{set_liquid_glass, set_liquid_glass_config,
  clear_liquid_glass}`): a window's backdrop material becomes one window command for every host,
  with the macOS materials among its values.
- **`flui-native`**: `WindowConfiguration`, `FullscreenMonitor`, the `flui_native::config` module,
  `WindowsPlatform::with_config` and `MacOSPlatform::with_config`. No host read the resize
  debounce or the fullscreen monitor; `WindowsPlatform::new()` and `MacOSPlatform::new()` open
  windows with F11 as the fullscreen key, as the default configuration did.
- **`flui-native`**: `FakeAccessibility::{published, request_action}`, which named AccessKit types,
  and the root names nothing outside the crate used: `FakeSystemChrome`, `IOSExecutor` and
  `FakeHaptics`, which is `flui_platform::haptics::FakeHaptics`.

### Changed

- **`flui-platform`**: `PlatformWindow::as_any` is gone; the downcast is
  `flui_native::HostWindow::as_any`. A test double that implements `PlatformWindow` drops its
  `as_any`, and a test that downcast a headless window keeps it as the `Arc<dyn HostWindow>` the
  platform opened.
- **`flui-native`**: `WindowsWindowExt` and `MacOSWindowExt` are implemented for
  `dyn HostWindow`, take `&self` and answer `Option`: `None` for a window of another host
  (headless, winit), which they leave alone. `MacOSWindowExt::add_tab_to_window` answers
  `Some(false)` when the other window is not an AppKit window.
- **`flui-native`**: the Android host starts through `flui_native::android_platform(app)`, which
  takes the `AndroidApp` the application's `android_main` was handed and returns the
  `Box<dyn Platform>` `AndroidPlatform::new(app)` built.
- **`flui-native`**: `FakeAccessibility` counts the trees published while an assistive technology is
  attached (`published_count`) instead of keeping them.
