### Added

- On the Web, `flui-system` opens a link in a new tab (or hands a `mailto:` or another allowed scheme to the system's handler), opens and saves files through the browser's file pickers (an `<input type=file>` or a download where the browser has none), and shows the launcher entry's count as an installed application's badge. A picker or a new tab asked for outside a user's gesture is refused with `Unavailable`; autostart, global shortcuts and the user's folders answer `NotOnThisPlatform`.
- `SaveRequest::contents`: a save writes the bytes it carries to the file the user chooses and answers once they are written, on every host (on the Web it is the only save).
- `OpenRequest::max_bytes`: the most bytes a browser's picks are read into memory, `TransferLimits`' default unless a request opts into more; more answers `DialogError::TooLarge`.
- `PickedFile::try_from_contents`, `PickedFile::contents` and `PickedFile::file_name`: a pick a browser handed over by its name and contents.
- `SystemFailure::Browser` and `BrowserException`: the exception a browser rejected a system service's call with.

### Changed

- `PickedFile::path` returns `Option<&Path>`: a browser's pick has no path.
