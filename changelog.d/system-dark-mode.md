### Added

- **Headless appearance injection** (`flui-platform`): `MockWindow::simulate_appearance` reports
  a system light/dark change to the window, firing `on_appearance_changed` only when the value
  changes, so a test can drive `MediaQueryData::platform_brightness`.
- **`Brightness: From<WindowAppearance>`** (`flui-platform-api`): the light/dark half of an
  appearance; the vibrant variants map to their base.

### Removed

- **`Platform::window_appearance` and `OwnerPlatform::window_appearance`** (`flui-platform`):
  brightness belongs to a presentation (ADR-0172), so `PlatformWindow::appearance` is the one
  source. Most backends answered light here whatever the theme.

### Fixed

- **Dark mode follows the system on every backend** (`flui-platform`, `flui-app`):
  `MediaQueryData::platform_brightness` starts at the user's theme and follows its changes in
  every window, including secondary windows and the iOS, Android and Web runners, which never
  seeded or observed it.
  - Windows reports `AppsUseLightTheme` instead of reading back the DWM dark flag the backend
    had forced on, so a light desktop no longer reports dark. The title bar follows the theme,
    and an unrelated `WM_SETTINGCHANGE` no longer reports a change.
  - macOS reports the window's `effectiveAppearance` and its changes; it always reported light.
  - Linux (winit) reads the Settings portal's `color-scheme` where winit has no theme, which is
    X11 always and Wayland unless the app overrode it; it always reported light.
  - iOS reports the view's `userInterfaceStyle`, Android its configuration's night mode, and
    the Web listens for `prefers-color-scheme` changes.
