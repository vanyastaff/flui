### Added

- **`flui-platform`, `flui-interaction`**: `ClipboardWrite`, a clipboard write that `concealed()`
  keeps out of the clipboard's history and cloud sync, and `Concealment` (`Honored`, `HintOnly`,
  `Unavailable`), what the host does with one
  ([ADR-0152 §6](/docs/adr/ADR-0152-capability-seam-revised.md)). `ClipboardHandle::write` and
  `ClipboardHandle::concealment` take and answer them; `write_text` is
  `write(ClipboardWrite::text(text))`.
- **`flui-native`**: Windows, macOS and iOS keep a concealed write out of the system's history and
  sync; X11, Wayland and Android mark it for the clipboard managers and keyboards that read the
  mark; the Web writes it as an ordinary entry and warns once in a debug build.
  `flui_native::testing::clipboard` is the scenario every host's clipboard runs.
- **`flui-testing`**: `ScriptedClipboard::is_concealed` and
  `ScriptedClipboard::answer_concealment`.

### Changed

- **`flui-interaction`, `flui-native`**: `ClipboardBackend` and `NativeClipboard` take
  `write(ClipboardWrite)` and answer `concealment()` in place of `write_text(String)`;
  `NativeClipboard::write_text` remains as the shorthand.
