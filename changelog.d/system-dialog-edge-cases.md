### Added

- **File dialogs** (`flui-system`): a filter of every file (`FileFilter::all_files`, offered beside
  specific filters; a request that offers one admits every pick) and exact file names in a filter
  (`FileFilter::with_file_name("Makefile")`, `".env"`), matched ignoring ASCII case and mapped on
  Windows, the portal and macOS (ADR-0371). `FileFilter::is_all_files` and `file_names` read them;
  a malformed name is `MalformedRequest::FileName`.
- **File dialogs** (`flui-system`): `OpenRequest::show_hidden` and `SaveRequest::show_hidden` show
  hidden files whatever the user's setting, on Windows and macOS; the portal's chooser and a
  browser's picker have no such option.
- **Message box** (`flui-system`): `MessageBoxRequest::default_button` (so a destructive
  confirmation defaults to its safe answer), `detail`, and `button_label` for custom button labels,
  with `label`, `default_answer` and `MessageButtons::answers` for a backend to read them. A
  default or a label for a button the box does not offer is `MalformedRequest::DefaultButton` or
  `ButtonLabel`. Windows shows the detail and the default button and keeps its own button labels;
  macOS shows all three; the web's box shows none of the three yet.

### Changed

- **Message box** (`flui-system`): a box without a title shows no title on any host. On Windows it
  showed the system's "Error" caption whatever its level.
- **File dialogs on Windows** (`flui-system`): a dialog or a message box asked for in a window that
  is no Win32 window answers `Unsupported(NoWindow)` instead of showing unowned. A folder to open
  in that the shell cannot name (gone, unreachable) answers `DialogError::Failed` with the shell's
  code, where the dialog used to open somewhere else without saying.
- **File dialogs on Linux** (`flui-system`): a save whose typed name has no extension is given the
  first extension of the filter the portal reports as selected, as Windows and macOS do, unless a
  file of the completed name is already there.
- **Saving contents** (`flui-system`): a save to a symbolic link writes the file the link leads to
  and leaves the link a link; the replaced file's mode is kept on Unix; where no file can be made
  beside the chosen one (a sandbox's document folder, a read-only folder) the contents are written
  into the chosen file itself instead of failing.

### Fixed

- **File dialogs on Windows** (`flui-system`): a folder given as a verbatim path (`\\?\C:\…`,
  `\\?\UNC\server\share\…`, what `std::fs::canonicalize` answers) is opened in; the dialog used to
  fall back to where the system opens it.
- **File dialogs on macOS** (`flui-system`): a process that may not show an open or a save panel
  (an App Sandbox without the user-selected-file entitlement) answers `Unsupported(Unavailable)`
  instead of panicking on the missing panel.
- **File dialogs** (`flui-system`): a file named by a dot and a word (`.env`) can be picked under a
  filter that names it, where no extension matched it and the pick was refused.
