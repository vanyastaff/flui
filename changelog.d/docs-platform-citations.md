### Changed

- **`flui-native`**: host log lines carry structured fields instead of formatted text, and the
  Android host logs a window resize at `debug`, not `info`.

### Fixed

- **`flui-native`**: the headless platform's `app_path` names the executable the process runs,
  as the desktop hosts do, instead of the stand-in path `/mock/app/path`.
