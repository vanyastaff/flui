### Fixed

- **`flui-app` (Android)**: pressing Back to finish the Activity no longer logs
  `close_this_window: dispatch refused`. `Destroy` closes the window before the event loop exits
  and runs the platform's quit hook. The hook used to send a second close for the presentation
  the first close had already retired. It now closes the window only when nothing closed it
  first, and still does on a `quit()` that no `Destroy` precedes.
