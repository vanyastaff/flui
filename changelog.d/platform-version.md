### Changed

- **`flui-native`**: each native host's window extensions and scene events are named at the
  crate root: `flui_native::{WindowsWindowExt, Win32Window, WindowsBackdrop, WindowsTheme,
  WindowCornerPreference}` on Windows; `flui_native::{MacOSWindowExt, AppKitWindow,
  TabJoinError, MacOSWindowLevel, MacOSCollectionBehavior}` on macOS;
  `flui_native::{IOSSceneEvent, IOSSceneSessionId, IOSSceneAttachmentId}` on iOS.

### Removed

- **`flui-native`**: the public `flui_native::platforms` module and its per-host modules
  (`platforms::{windows, macos, ios, winit, headless}`). The names that stay public are at the
  crate root (see Changed), where the headless test controls already were; the hosts' windows,
  `win32`, the Liquid Glass types, the tiling types, `TaskbarProgressState` and `IOSExecutor` are
  no longer public (each has its own entry under Removed).
- **`flui-native`**: the modules `flui_native::text::{android, ime, web}`, hidden from the
  documentation and not API: the rules the crate's own tests decoded with left them for
  `flui_native::input`, and nothing outside the crate named what stayed.

### Fixed

- **`flui-cli`**: `flui run --device <serial>` finds an Android device among the attached
  ones before probing the other platforms, so it no longer starts every installed browser
  to read its version, which on Linux could take ten seconds a browser.
