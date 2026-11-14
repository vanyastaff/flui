### Added

- On macOS the links and documents AppKit opens the application with (`application:openURLs:`)
  reach `AppHandler::opened`: what launched the application as a cold launch, so its first Router
  opens on the link and documents reach `Application::on_open_files`, and what reaches it later
  while it runs.

### Fixed

- A `file:` URI on a desktop launch's command line with an escape that is not two hex digits
  (`%+1`) is refused instead of read as a byte.
