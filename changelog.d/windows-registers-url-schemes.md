### Added

- An unpackaged Windows application registers the URL schemes it declares under
  `HKCU\Software\Classes` after its main window's first frame, so a link of its scheme launches
  the executable that runs. Each key it writes carries the application id; a key the user or
  the machine already has for another handler is never touched. A release build registers by
  itself, a development build under `flui run --register-links`, and
  `Application::register_links(bool)` decides over both. Running the executable with
  `--flui-unregister-links` deletes the keys it wrote and exits, for an uninstaller. A packaged
  (MSIX) process registers nothing.

### Changed

- `flui build` and `flui doctor` refuse a one-letter URL scheme in `url-schemes`, which Windows
  reads as a drive in the arguments a link launches with.
