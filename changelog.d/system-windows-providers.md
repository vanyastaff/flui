### Added

- `flui::services::system` provides every service on Windows: the launcher (a URI through its
  scheme's registered handler, a document judged by the file it opened, reveal in Explorer, move to
  the Recycle Bin, asking first where the bin cannot take an item), the open, save and folder
  dialogs, modal to the widget's window while the window keeps drawing, autostart (the user's
  `Run` key, keeping the user's own choice), global shortcuts (`RegisterHotKey`, each bound to the
  trigger it prefers), the launcher entry's progress and count on the taskbar buttons of the
  windows it was acquired in, and the user's folders.
- A message box (`dialog::MessageBoxes`), sharing through the system's share sheet
  (`share::Share`), the Windows jump list (`jump_list::JumpList`) and what the system is
  (`os::Os`), each provided on Windows. Linux refuses the message box, the share sheet and the jump
  list with `NotOnThisPlatform`, since no portal has them; the other hosts refuse what they do not
  provide yet with `NotYetOnThisHost`, never a made-up success.
- `ShortcutError::Exhausted`: a binding the UI runtime has no hot key number left for ends with it.
- `testing::ScriptedSystem` scripts every service, and `flui_platform::launch::is_local_path` names
  the locality rule the launcher and the share sheet keep.

### Changed

- On Windows, opening a file whose link leads to a network share, or a file on a mapped network
  drive, is refused with `LaunchError::NotLocal`; revealing or trashing it is not.
- A file dialog's title or accept label that is longer than `dialog::MAX_TEXT_LEN` or holds a
  control character is refused with `MalformedRequest::Text` on every host, before anything is
  shown: Windows would have cut it at a NUL and the desktop portal refused it.
