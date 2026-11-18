### Added

- `flui::services::system` provides every service on Windows: the launcher (a URI through its
  scheme's registered handler, a document judged by the file it opened, reveal in Explorer, move to
  the Recycle Bin, asking first where the bin cannot take an item) and the open, save and folder
  dialogs, modal to the widget's window while the window keeps drawing.
- A message box (`dialog::MessageBoxes`), sharing through the system's share sheet
  (`share::Share`), launch at login (`login::LaunchAtLogin`, which reports and keeps the user's own
  choice), global shortcuts (`shortcut::GlobalShortcuts`), a window's taskbar progress and badge
  (`taskbar::Taskbar`), the Windows jump list (`jump_list::JumpList`) and the user's folders and
  what the system is (`os::Os`). Each is provided on Windows; Linux and the other hosts refuse
  what they do not provide yet with `NotYetOnThisHost`, never a made-up success.
- `testing::ScriptedSystem` scripts every service, and `flui_platform::launch::is_local_path` names
  the locality rule the launcher and the share sheet keep.

### Changed

- On Windows, opening a file whose link leads to a network share, or a file on a mapped network
  drive, is refused with `LaunchError::NotLocal`; revealing or trashing it is not.
- A global shortcut registration a runtime cannot number any more answers
  `ShortcutError::Exhausted`.

- A file dialog's title or accept label that is longer than `dialog::MAX_TEXT_LEN` or holds a
  control character is refused with `MalformedRequest::Text` on every host, before anything is
  shown: Windows would have cut it at a NUL and the desktop portal refused it.
