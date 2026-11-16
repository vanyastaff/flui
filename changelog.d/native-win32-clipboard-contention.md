### Added

- **`flui-platform`**: `ClipboardWritten`, the answer to a clipboard write
  ([ADR-0337](/docs/adr/ADR-0337-a-clipboard-write-answers-and-win32-serves-the-clipboard-off-the-owner-thread.md)):
  `Ok(())` once the system clipboard took the text; `ClipboardError::ApplicationOnly` when it
  cannot now and only this application can read the text back (a web page without the Clipboard
  API, a Wayland window with no input serial); any other error when nothing was kept.
  `ClipboardError::Gone` says a host's clipboard will serve nothing more this run.
- **`flui-platform`**: `Script::on_answer`, which hears every answer a scenario script gives,
  however it is given, before the caller's completion settles.
- **`flui-testing`**: `ScriptedClipboard::writes`, the script that answers writes (each a
  `WriteRequest`), with `fail_writes_with`, `stop_failing_writes` and `hold_writes`, whose
  `ClipboardWriteHold` holds writes while it lives; reads and writes keep one order, and whatever
  answers a write `Ok` or `ApplicationOnly` lands its text.
- **`flui-native`**: `testing::clipboard::SettlingOwner`, `write_unobserved` and `read_within`, for
  a test of a host's clipboard that answers from another thread.

### Changed

- **`flui-interaction`**: `ClipboardHandle::write` and `write_text` return
  `Completion<ClipboardWritten>`, and `ClipboardBackend::write` takes a
  `Resolver<ClipboardWritten>` it answers. A caller with nothing to undo when the write fails may
  drop the completion.
- **`flui-native`**: `NativeClipboard::write` takes a `Resolver<ClipboardWritten>`, and
  `NativeClipboard::write_text` is gone. Every host answers it: X11 once it owns the selection;
  Wayland `ApplicationOnly` with no input serial (none yet, or the keyboard left the window),
  keeping the text for the application's reads, and `Unavailable` for a concealed write it could
  only serve without the hint; the web when the browser's promise settles, a page with no
  Clipboard API keeping the text and answering `ApplicationOnly`.
- **`flui-widgets`**: a cut in an `EditableText` deletes its selection at the owner turn the
  clipboard answers that it took the text, or that only the application can read it back, and
  deletes nothing when the clipboard refused it or the field lost the focus or was disposed
  meanwhile; cut and paste are offered and performed only while the field takes an edit.

### Fixed

- **`flui-native`**: on Windows, a clipboard read or write made while another application (a
  clipboard manager, a remote-desktop client) holds the clipboard open for a moment lands once it
  lets go, instead of reading as unavailable or losing the write; a write still refused after half
  a second answers `ClipboardError::Unavailable` and leaves the clipboard as it was. A read of text
  another application renders only when asked no longer freezes the application's windows while
  that application answers; one that never answers ends after Windows' thirty seconds as
  `ClipboardError::Timeout`. A panicking `tracing` subscriber no longer stops the clipboard.
