### Fixed

- **`flui-native`**: on macOS a system preference change wakes the application once instead of
  twice, and on Android the loop no longer reads the preferences every 500 ms while `FluiView` is
  not in the window; a view that never gets there is logged once. A preference read that fails
  is retried 0.5 to 16 s apart six times (on macOS only while the application is active) and then
  waits for the next change notice, instead of every 500 ms for as long as it fails.
