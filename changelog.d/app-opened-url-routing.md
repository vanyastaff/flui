### Fixed

- **`flui-app`**: a link the application declares that arrives while it runs with no window (a
  resident run, an `ExitPolicy::ExplicitQuit` application after its last window closed, macOS
  after the last window closed) opens the main window on its page, on every host, instead of
  waiting in silence until some window opened. A link that launched a resident application opens
  its main window the same way. A link the application does not declare never opens a window
  ([ADR-0330](/docs/adr/ADR-0330-navigation-is-a-typed-router-reached-from-the-context.md)).
- **`flui-app`**: links waiting for different windows wait side by side, in order and bounded,
  instead of a link for one window replacing those held for another; a link addressed to a window
  that has closed is refused as it arrives instead of waiting until the run ends; and when the
  bootstrap fails, the files and links that launched the application are reported as refused
  instead of disappearing without a log line.
- **`flui-app`**: a reopen (a Dock click, or a link that finds no window) made after the
  application asked to quit shows no window.
