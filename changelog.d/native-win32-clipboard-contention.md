### Added

- **`flui-platform`**: `ClipboardWritten`, the answer to a clipboard write
  ([ADR-0337](/docs/adr/ADR-0337-a-clipboard-write-answers-and-win32-serves-the-clipboard-off-the-owner-thread.md)):
  `Ok(())` once the system clipboard took the text; `ClipboardError::ApplicationOnly` when it
  cannot now and the host keeps the text for this application, owing it to the system clipboard
  at its first chance (a web page without the Clipboard API; a Wayland window with no input in
  focus or no data device, which offers it at the next input); any other error when nothing was
  kept. `ClipboardError::Gone` is every host's answer for a clipboard no retry can reach.
- **`flui-platform`**: `Script::on_answer`, which adds an observer that hears every answer a
  scenario script gives, however it is given, before the caller's completion settles.
- **`flui-testing`**: `ScriptedClipboard::writes`, the script that answers writes (each a
  `WriteRequest`), with `fail_writes_with`, `stop_failing_writes` and `hold_writes`, whose
  `ClipboardWriteHold` holds writes while any hold lives; reads and writes keep one order,
  whatever answers a write `Ok` or `ApplicationOnly` lands its text, and a write's completion
  settles only once its text has landed.
- **`flui-native`**: `testing::clipboard::SettlingOwner`, `write_unobserved` and `read_within`, for
  a test of a host's clipboard that answers from another thread.

### Changed

- **`flui-interaction`**: `ClipboardHandle::write` and `write_text` return
  `Completion<ClipboardWritten>`, and `ClipboardBackend::write` takes a
  `Resolver<ClipboardWritten>` it answers. A caller with nothing to undo when the write fails may
  drop the completion.
- **`flui-native`**: `NativeClipboard::write` takes a `Resolver<ClipboardWritten>`, and
  `NativeClipboard::write_text` is gone. Every host answers it: X11 once it owns the selection,
  `Gone` once its thread has ended; Wayland `ApplicationOnly` with no input in focus (none yet,
  or the keyboard left the window) or no data device, keeping the text for the application's
  reads through any selection that arrives before it is offered at the next input, and `Gone`
  once the loop released it; UIKit `Gone` off the main thread; the web when the browser's
  promise settles, a page with no Clipboard API keeping the text and answering
  `ApplicationOnly`.
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
  `ClipboardError::Timeout`. A panicking `tracing` subscriber no longer stops the clipboard. The
  requests waiting hold at most 64 MiB of text besides one write of any size.
