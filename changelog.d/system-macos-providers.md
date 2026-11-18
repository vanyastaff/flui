### Added

- `flui::services::system` provides every service on macOS but the jump list: open and save
  dialogs and the message box as sheets on the widget's window, the share menu, opening a link or a
  document through Launch Services (an application, a script or an executable refused by what was
  opened, and anything but a regular file or a folder refused without being opened), revealing in
  the Finder and moving to the Trash, the application's login item (macOS 13 and later; the user's
  switch-off in System Settings is kept), global shortcuts that also report their release and stay
  on the key that types their letter when the keyboard layout changes, the Dock tile's progress bar
  and badge, the user's folders and the system's version and processor. A runtime whose owner
  thread is not the main thread, such as the headless test host, is refused the AppKit services
  with `NotYetOnThisHost`.
- `flui::platform::SystemFailure::Framework` carries the code a macOS framework failed with
  outside the POSIX domain: an `NSError`'s code or an `OSStatus`.

### Changed

- An open dialog's picks are held to the request's filters on every system, not only in a
  browser: when the filters name extensions only and a pick's name ends in none of them (a name
  typed into the dialog, a link to a file of another name), the answer is
  `DialogError::NotInFilter`. A request with a MIME type among its filters is judged by the
  system that knows the file's type.
- `ShortcutEvent::Changed` also arrives on macOS when a change of keyboard layout moves a
  shortcut onto a chord another application holds, or frees one for a shortcut left out before.
