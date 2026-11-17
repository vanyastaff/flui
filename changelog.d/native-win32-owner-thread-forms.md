### Changed

- **`flui-native`**: on Windows, a window's minimize, maximize and restore are logged at `debug`
  and each resize step at `trace`, with the window and its size as fields, instead of at `info`
  on every step of a drag.
