### Changed

- **`flui-view`**: `OpenDeclarations::admit` takes the page origin a web application runs in, so
  the application runner and `UiRuntime::open_url` admit an opened URL by one rule
  ([ADR-0339](/docs/adr/ADR-0339-an-opened-url-has-one-destination-decided-at-the-runner.md)).
- **`flui-native`**: on Linux and Windows a later launch of a single-instance application asks the
  running one to reopen only when it opens nothing; one that carries links or files reaches the
  application's opened handler alone, and the runner decides whether a window opens.

### Fixed

- **`flui-app`**: on Windows, macOS, Linux and the headless host, a link the application declares
  that arrives while it runs with no window (a resident run, an `ExitPolicy::ExplicitQuit`
  application after its last window closed) opens the main window on the link's page instead of
  waiting in silence until some window opened; so does a link that launched a resident
  application. On iOS a link opens in the scene it addresses, on Android in the activity's window
  and on the web in the page, as before. A link the application does not declare never opens a
  window.
- **`flui-app`**: links waiting for a window are bounded by their target: the application keeps
  only its newest link, which is the one its window opens on, and each window being installed keeps
  its own in order; a link for a window that closed or that no installation is opening is refused
  as it arrives; links opened into a window whose installation failed wait for the next window
  instead of being lost; and when the bootstrap fails, the files and links that launched the
  application are reported as refused instead of disappearing without a log line.
- **`flui-app`**: a reopen (a Dock click, or a link that finds no window) made while a quit is
  pending opens the main window if the quit is abandoned, and nothing if it goes ahead.
- **`flui-app`**: the window a link opens in is made visible on every host: shown if the
  application hid it (a tray application), restored if minimized, brought forward; a link it
  refuses shows nothing. A link for the application goes to its main window, reopened when
  closed, never to another window, whether that one has the focus, opened first or was opened for
  a file in the same batch.
- **`flui-app`**: a link causes at most one opening of the main window. If that window fails to
  open, before or after its UI runtime is built, the link is refused ("its window could not be
  opened"), and
  a window the user closes before it finished opening does not come back for the link.
