### Added

- **File dialogs** (`flui-system`): a filter of every file (`FileFilter::all_files`, offered beside
  specific filters; a request that offers one admits every pick) and exact file names in a filter
  (`FileFilter::with_file_name("Makefile")`, `".env"`), matched ignoring ASCII case and mapped on
  Windows, the portal, macOS, iOS and the web (ADR-0372). `FileFilter::is_all_files` and
  `file_names` read them; a malformed name is `MalformedRequest::FileName`, and a pattern given to
  a filter of every file is `MalformedRequest::PatternOnAllFiles`.
- **File dialogs** (`flui-system`): `OpenRequest::show_hidden` and `SaveRequest::show_hidden` show
  hidden files whatever the user's setting, on Windows and macOS; the portal's chooser, iOS's
  picker and a browser's picker have no such option.
- **Message box** (`flui-system`): `MessageBoxRequest::default_button` (so a destructive
  confirmation defaults to its safe answer), `detail`, and `button_label` for custom button labels,
  with `label`, `default_answer`, `is_labelled` and `MessageButtons::answers` for a backend to read
  them. A default or a label for a button the box does not offer is
  `MalformedRequest::DefaultButton` or `ButtonLabel`. macOS, iOS and the web show all three;
  Windows shows the detail and the default button and refuses a labelled box with
  `Unsupported(NotYetOnThisHost)`, since `MessageBoxW` cannot label a button.

### Changed

- **Message box** (`flui-system`): a box without a title shows no title on any host. On Windows it
  showed the system's "Error" caption whatever its level.
- **File dialogs on Windows** (`flui-system`): a dialog or a message box asked for in a window that
  is no Win32 window answers `Unsupported(NoWindow)` instead of showing unowned. A folder to open
  in that is gone (deleted since, a drive unplugged, a share that does not answer) is left out and
  the dialog opens where the system would, as on every other host; one the shell cannot name for
  another reason answers `DialogError::Failed` with the shell's code.
- **File dialogs on Linux** (`flui-system`): a save whose typed name has no extension is given the
  first extension of the filter the portal reports as selected, as Windows and macOS do, unless a
  file of the completed name is already there or the file is in the document portal's folder,
  where the portal granted the name it answered.
- **File dialogs on the web** (`flui-system`): the picker keeps its own entry of every file when a
  filter admits every file or names a file exactly, and the file input accepts every file then.
- **Saving contents** (`flui-system`): a save to a symbolic link writes the file the link leads to
  and leaves the link a link; the new file is made with the replaced file's mode on Unix, so its
  contents are never readable by more users than the old file's. Where the file system refuses a
  new file beside the chosen one by design (a sandbox's document folder, a read-only folder,
  another device, a sticky folder) the contents are written into the chosen file itself.
- **Saved names** (`flui-system`): a typed or suggested hidden name (`.env`, `.profile`) is not
  given the filter's extension on any host, as Windows' dialog leaves it.

### Fixed

- **Saving contents** (`flui-system`): a save over a file another process holds open without
  letting it be replaced (on Windows, open without `FILE_SHARE_DELETE`) answers the system's
  failure and leaves the file whole; it used to fall back to emptying the file and writing it
  again in place.
- **File dialogs on Windows** (`flui-system`): a folder given as a verbatim path (`\\?\C:\…`,
  `\\?\UNC\server\share\…`, what `std::fs::canonicalize` answers) is opened in; the dialog used to
  fall back to where the system opens it.
- **File dialogs on macOS** (`flui-system`): a process that may not show an open or a save panel
  (an App Sandbox without the user-selected-file entitlement) answers `Unsupported(Unavailable)`
  instead of panicking on the missing panel.
- **File dialogs** (`flui-system`): a file named by a dot and a word (`.env`) can be picked under a
  filter that names it, where no extension matched it and the pick was refused.
