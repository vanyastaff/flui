### Added

- **`flui`**: the `native` feature, off by default, and `flui::native`, the escape hatch
  (ADR-0254 §8): `HostWindow`, `with_window` and, on Windows, `flui::native::windows`
  (`WindowsWindowExt`, `Win32Window`, `Win32WindowError`, `WindowsBackdrop`), on macOS
  `flui::native::macos` (`MacOSWindowExt`, `AppKitWindow`, `MacOSBackdrop`, `MacOSMaterial`,
  `LiquidGlass`, `GlassStyle`, `GlassTint`, `MacOSBackdropError`, `MacOSWindowLevel`,
  `MacOSCollectionBehavior`, `TabJoinError`). It is Evolving, not Stable (ADR-0089 §5). Each
  extension narrows a window to its OS's view and answers `None` for a window of another host.
  The facade's feature turns on `flui-app`'s new `native` feature.
- **`flui-app`**: `native::with_window(address, f)` runs `f` on the host window the presentation
  at `address` is drawn in, on the thread that runs the application; `None` elsewhere, for an
  address nothing is presented at and once the presentation is gone.
- **`flui-platform`**: `WindowOptions::os_options` (`with_os_options`), an
  `os_options::OsWindowOptions` with one optional value per operating system:
  `windows: Option<Win32WindowOptions>` (`corner_preference: WindowsCornerPreference`,
  `theme: WindowsTheme`). Only that system's host reads its value. `AppConfig` carries it as
  `os_options` (`AppConfig::with_os_options`).
- **`flui-native`**: Liquid Glass returns: `AppKitWindow::set_backdrop(MacOSBackdrop::Glass(..))`
  puts an `NSGlassEffectView` (macOS 26, found by name at run time) with its style, tint and
  corner radius behind FLUI's view; `MacOSBackdrop::Material(..)` puts an `NSVisualEffectView`
  material there; `clear_backdrop` takes either away. Each
  answers `MacOSBackdropError`: a closed window, a window off the AppKit main lane, a system
  without Liquid Glass (`NotOnThisSystem`), a content view something else put there, a view
  AppKit did not create, a corner radius that is not finite or is negative.

### Changed

- **`flui-native`**: the OS extensions moved from the crate root to `flui_native::os::windows` and
  `flui_native::os::macos`, the modules the facade publishes.
- **`flui-native`**: every `Win32Window` command and query returns `Result<_, Win32WindowError>`:
  `Closed`, `NotOnTheOwnerThread`, `Refused` (the system refused, with its `HRESULT`; a corner
  preference on Windows 10), `Unrecognized` (a value the system reported that has no FLUI value).
  None reaches a handle the window no longer owns. Renamed with it: `enable_snap_layouts` and
  `disable_snap_layouts` are `set_snap_layouts(bool)`, `is_snap_layouts_enabled` is
  `snap_layouts`, `enable_blur_behind` is `set_blur_behind`, `is_dark_mode` is `dark_mode`.
- **`flui-native`**: `WindowsBackdrop` is `#[non_exhaustive]` and gains `Auto`; `Tabbed`, the
  same material as `MicaAlt`, is gone, with `to_dwm_value` and `requires_windows_11`.
- **`flui-native`**: a Win32 window opens with the corners and title-bar theme of its
  `Win32WindowOptions`; without them it opens as before, with rounded corners and the user's app
  theme.
- **`flui-platform`**: `WindowCornerPreference` and `WindowsTheme` moved from `flui-native` to the
  contract's `os_options` module as `WindowsCornerPreference` and `WindowsTheme`,
  `#[non_exhaustive]`, without their DWM conversions.

### Removed

- **`flui-native`**: `Win32Window::set_caption_color`, which did what `set_title_bar_color` does.
