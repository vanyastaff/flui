### Fixed

- **`flui-app` (Android, web)**: an app that exits through its window's close no longer logs
  `close_this_window: dispatch refused`. On Android, Back finishes the Activity, and its
  `Destroy` closes the window before the event loop exits and runs the platform's quit hook; the
  hook sent a second close for the presentation the first had already retired. It now closes the
  window only when nothing closed it first, and still does on a `quit()` without `Destroy`.
