### Added

- **`flui-platform`**: `ClipboardWritten`, the answer to a clipboard write: `Ok(())` once the
  clipboard holds the text, or the `ClipboardError` the host gave up with
  ([ADR-0336](/docs/adr/ADR-0336-a-clipboard-write-answers-and-win32-serves-the-clipboard-off-the-owner-thread.md)).
- **`flui-testing`**: `ScriptedClipboard::fail_writes_with` and `stop_failing_writes`, which refuse
  writes as a host whose clipboard another application holds does.
- **`flui-native`**: `testing::clipboard::SettlingOwner`, `write_unobserved` and `read_within`, for
  a test of a host's clipboard that answers from another thread.

### Changed

- **`flui-interaction`**: `ClipboardHandle::write` and `write_text` return
  `Completion<ClipboardWritten>`, and `ClipboardBackend::write` takes a
  `Resolver<ClipboardWritten>` it answers. A caller with nothing to undo when the write fails may
  drop the completion.
- **`flui-native`**: `NativeClipboard::write` takes a `Resolver<ClipboardWritten>`, and
  `NativeClipboard::write_text` is gone. Every host answers it: X11 once it owns the selection,
  Wayland `Denied` with no user input to set the selection under, the web when the browser's
  promise settles.
- **`flui-widgets`**: a cut in an `EditableText` deletes its selection at the owner turn the
  clipboard answers that it took the text, and deletes nothing when the clipboard refused it.

### Fixed

- **`flui-native`**: on Windows, a clipboard read or write made while another application (a
  clipboard manager, a remote-desktop client) holds the clipboard open for a moment lands once it
  lets go, instead of reading as unavailable or losing the write; a write still refused after half
  a second answers `ClipboardError::Unavailable`. A read of text another application renders only
  when asked no longer freezes the application's windows while that application answers; one that
  never answers ends after Windows' thirty seconds as `ClipboardError::Timeout`.
