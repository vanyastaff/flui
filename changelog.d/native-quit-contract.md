### Added

- `flui_native::AppHandler::quit_asked` and `flui_native::QuitAnswer`: a quit the user asks for through the platform (AppKit's Quit in the Dock, a quit Apple Event) asks the application, which keeps running or quits; a handler that panics keeps it running. `HeadlessHostEvents::quit` reports one in a test.
- `flui_platform::SessionEndSupport::Background { window }`: iOS and Android report the end of the session each time the application leaves the screen, and an answer of `Block` to `Ending` keeps the process running for the writes in flight, at most `window`, until the application releases it.

### Fixed

- On macOS, Quit from the Dock no longer closes every window past its close handler: each window is asked as for `AppHandle::request_quit`, and a window with unsaved work keeps the application running.
- On iOS, a write still running when the scene goes to the background is held by a UIKit background task until it ends, and a value the window could not store is named in the log, on Android too.
