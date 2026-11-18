### Added

- `flui::services::system` works on iOS and iPadOS: an open dialog picks documents with the
  system's document picker and hands each over as its name and contents, read within
  `OpenRequest::max_bytes` (never a path, since access to a picked document ends with the read); a
  save dialog exports the request's contents to the place the user chooses; the message box is
  the system's alert, the share sheet the system's (a popover on an iPad), a link opens in the
  application that claims its scheme, the user's folders are the application's own (its home and
  Documents), and the system reports its name (iOS or iPadOS) and version. A dialog swiped away
  answers a cancel, and a dialog, an alert or a share sheet goes with its window.
- On iOS a folder dialog and the launcher entry answer `NotYetOnThisHost`, a save without
  contents `NotOnThisPlatform`, and an open with filters on iOS 13 `NotOnThisDevice`. Opening,
  revealing and trashing a path, autostart, global shortcuts and the jump list answer
  `NotOnThisPlatform` there.
