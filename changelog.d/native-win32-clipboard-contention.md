### Added

- **`flui-platform`**: `ClipboardWritten`, the answer to a clipboard write: `Ok(())` once the
  clipboard holds the text, or the `ClipboardError` the host gave up with
  ([ADR-0337](/docs/adr/ADR-0337-a-clipboard-write-answers-and-win32-serves-the-clipboard-off-the-owner-thread.md)).
  `ClipboardError::ApplicationOnly` names a write no system clipboard could take but the
  application kept for its own reads (a web page without the Clipboard API).
- **`flui-testing`**: `ScriptedClipboard::writes`, the script that answers writes, and
  `fail_writes_with`, `stop_failing_writes`, `hold_writes`
  and `answer_held_writes`, which refuse or hold writes as a host whose clipboard another
  application holds does.
- **`flui-native`**: `testing::clipboard::SettlingOwner`, `write_unobserved` and `read_within`, for
  a test of a host's clipboard that answers from another thread.

### Changed

- **`flui-interaction`**: `ClipboardHandle::write` and `write_text` return
  `Completion<ClipboardWritten>`, and `ClipboardBackend::write` takes a
  `Resolver<ClipboardWritten>` it answers. A caller with nothing to undo when the write fails may
  drop the completion.
- **`flui-native`**: `NativeClipboard::write` takes a `Resolver<ClipboardWritten>`, and
  `NativeClipboard::write_text` is gone. Every host answers it, and a refused write is never what a
  later read in the process answers: X11 answers once it owns the selection; Wayland refuses a write
  with no user input to set the selection under (`Denied`) instead of holding its text in the
  process, and refuses a concealed write it could only serve without the hint; the web answers when
  the browser's promise settles and keeps a write's text for the page only if the clipboard took
  it, and a page with no Clipboard API answers `ApplicationOnly` instead of `Ok`; Wayland names
  no input once the keyboard left the window, so a later write is refused rather than sent under
  a stale serial.
- **`flui-widgets`**: a cut in an `EditableText` deletes its selection at the owner turn the
  clipboard answers that it took the text, and deletes nothing when the clipboard refused it or
  the field lost the focus or was disposed meanwhile; like a paste, a cut is refused at a field
  that does not hold the focus.

### Fixed

- **`flui-native`**: on Windows, a clipboard read or write made while another application (a
  clipboard manager, a remote-desktop client) holds the clipboard open for a moment lands once it
  lets go, instead of reading as unavailable or losing the write; a write still refused after half
  a second answers `ClipboardError::Unavailable` and leaves the clipboard as it was. A read of text
  another application renders only when asked no longer freezes the application's windows while
  that application answers; one that never answers ends after Windows' thirty seconds as
  `ClipboardError::Timeout`. A panicking `tracing` subscriber no longer stops the clipboard.
