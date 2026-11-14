### Added

- On Windows and Linux (Wayland, X11) the URLs and existing files an application is launched with (a registered scheme's `"%1"`, a desktop entry's `%U`, a `file:` URI as a file) reach `AppHandler::opened` as a cold launch, so a link that started the application opens its first Router on it and files reach `Application::on_open_files`. Other arguments stay the application's.
