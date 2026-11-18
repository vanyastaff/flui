### Added

- `flui::services::system` provides every service on macOS but the jump list: open and save
  dialogs and the message box as sheets on the widget's window, the share menu, opening a link or a
  document through Launch Services (an application, a script or an executable refused by what was
  opened), revealing in the Finder and moving to the Trash, the application's login item (macOS 13
  and later; the user's switch-off in System Settings is kept), global shortcuts that also report
  their release, the Dock tile's progress bar and badge, the user's folders and the system's
  version and processor. A runtime whose owner thread is not the main thread, such as the headless
  test host, is refused the AppKit services with `NotYetOnThisHost`.
- `flui::platform::SystemFailure::Framework` carries the code a macOS framework failed with
  outside the POSIX domain: an `NSError`'s code or an `OSStatus`.
