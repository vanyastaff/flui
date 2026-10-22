### Changed

- **`flui-native`**: each native host's window and its extensions are named at the crate
  root: `flui_native::{WindowsWindow, WindowsWindowExt, WindowsBackdrop, WindowsTheme,
  WindowCornerPreference, TaskbarProgressState}` and the `flui_native::win32` module on
  Windows; `flui_native::{MacOSWindow, MacOSWindowExt, MacOSWindowLevel,
  MacOSCollectionBehavior, TilePosition, TilingConfiguration, TilingError, TilingLayout,
  TilingState, LiquidGlassConfig, LiquidGlassMaterial, BlendingMode}` on macOS;
  `flui_native::{IOSWindow, IOSSceneEvent, IOSSceneSessionId, IOSSceneAttachmentId,
  IOSExecutor}` on iOS; `flui_native::WinitWindow` with `winit-backend`.

### Removed

- **`flui-native`**: the public `flui_native::platforms` module and its per-host modules
  (`platforms::{windows, macos, ios, winit, headless}`). Their names are at the crate root
  (see Changed); the headless test controls already were.
- **`flui-native`**: the modules `flui_native::text::{android, ime, web}`, hidden from the
  documentation and not API: the rules the crate's own tests decoded with left them for
  `flui_native::input`, and nothing outside the crate named what stayed.
