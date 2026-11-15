### Added

- An unpackaged Windows application registers the URL schemes and the file types it declares
  under `HKCU\Software\Classes` after its main window's first frame, so a link of its scheme or a
  file of its type launches the executable that runs. A file type is a ProgID offered in its
  extension's "Open with" list; the extension's default stays the user's choice. Each key it
  writes carries the application id; a key the user or the machine already has for another
  handler is never touched. A release build registers by itself, a development build under
  `flui run --register-associations`, and `Application::register_associations(bool)` decides over
  both. Running the executable with `--flui-unregister-associations` deletes the keys and offers
  it wrote and exits, for an uninstaller. A packaged (MSIX) process registers nothing.

### Changed

- `flui build` and `flui doctor` refuse a one-letter URL scheme in `url-schemes`, which Windows
  reads as a drive in the arguments a link launches with.
