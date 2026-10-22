### Changed

- `flui-native`: the clipboard, drag and drop, accessibility and haptics of every host are
  feature directories (`clipboard/`, `drag/`, `accessibility/`, `haptics/`), each its model
  beside one file per platform. Behaviour is unchanged, and so are the crate-root names
  (`FakeAccessibility`, `FakeHaptics`, `HeadlessDragAndDrop`).
- `flui-native`: the hidden haptics degradation tables the conformance suite reads moved from
  `flui_native::shared::haptic_degradation` to `flui_native::haptics::degradation`.

### Removed

- `flui-native`: the paths `flui_native::platforms::windows::{WindowsClipboard,
  WindowsAccessibility}`, `flui_native::platforms::macos::{MacOSClipboard,
  MacosAccessibility}`, `flui_native::platforms::ios::IOSClipboard`,
  `flui_native::platforms::linux::UnixAccessibility`,
  `flui_native::platforms::winit::{ArboardClipboard, WinitDataTransfer}`,
  `flui_native::platforms::{FakeAccessibility, FakeHaptics, HeadlessDragAndDrop}` and
  `flui_native::platforms::headless::{FakeAccessibility, FakeHaptics, HeadlessDragAndDrop}`;
  each host's table names these types, and the crate root keeps the headless ones.
- `flui-native`: the hidden rule modules `flui_native::shared::{ole_drag_drop, android_drag,
  html_drag_drop}`; their tests run beside them in the crate.
