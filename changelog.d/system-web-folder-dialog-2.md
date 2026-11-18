### Added

- On the Web, `FileDialogs::open(OpenRequest::folders())` picks a folder: through the browser's directory picker where it has one, through a folder file input otherwise. The answer is one folder, its name and the files under it with their paths and contents, read when the dialog answers; a browser that can show no folder chooser answers `NotOnThisDevice`.
- `PickedForm::Folder`, `PickedFile::files`, `PickedFile::try_from_folder`, `FolderFile` and `FolderPath`: a folder a system handed over without a path. A `FolderPath` is the names from the folder down to a file, held to one rule on every host (no empty name, no `.` or `..`, no `/`, `\` or NUL); it is not a path of the host, so an application that writes the files to disk maps the names for its host.
- `OpenRequest::max_entries` (by default `OpenRequest::DEFAULT_MAX_ENTRIES`): the most files a handed-over folder may hold; more answers `DialogError::TooManyEntries`. `OpenRequest::max_bytes` bounds a folder's files together.

### Changed

- An open or save dialog whose backend answers a pick of a form the request cannot have (a handed-over folder to a file dialog or a save, a handed-over file to a folder dialog, a download to an open) answers `SystemFailure::MalformedAnswer` on every host. A pick at a path is taken as before.
- `FileDialogs::open` holds what any backend hands over to the request's `max_entries` and `max_bytes`, where only the Web provider checked `max_bytes`.

### Fixed

- On the Web, a browser's file chooser is one for the page: a second UI runtime of the page asking for a picker while one is up answers `Busy`, where it showed a second input the browser dropped.
- On the Web, an open dialog answered through a file input is no longer answered as a cancel a moment after the window regains focus on a browser that reports a dismissed chooser itself (Chromium 113, Firefox 112, Safari 17 and later): a pick the browser reports later than that is the answer.
