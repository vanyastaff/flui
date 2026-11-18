### Added

- On the Web, `FileDialogs::open(OpenRequest::folders())` picks a folder: through the browser's directory picker where it has one, through a folder file input otherwise. The answer is one folder, its name and the files under it with their paths and contents, read when the dialog answers; a browser that can show no folder chooser answers `NotOnThisDevice`.
- `PickedForm::Folder`, `PickedFile::files`, `PickedFile::try_from_folder` and `FolderFile`: a folder a browser handed over without a path. A `FolderFile`'s path is relative to the folder and only descends from it.
- `OpenRequest::max_entries` (by default `OpenRequest::DEFAULT_MAX_ENTRIES`): the most files and folders a handed-over folder may hold; more answers `DialogError::TooManyEntries` before anything is read. `OpenRequest::max_bytes` bounds the folder's files together.

### Changed

- An open or save dialog whose backend answers a pick of a form the request cannot have (a handed-over folder to a file dialog or a save, a handed-over file to a folder dialog, a download to an open) answers `SystemFailure::MalformedAnswer` on every host. A pick at a path is taken as before.
- On the Web, a picked file whose name a pick cannot carry refuses the answer before any file is read, where it was refused after that file was read.
