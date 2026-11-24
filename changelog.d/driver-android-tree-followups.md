### Added

- **`flui-driver`**: `testing::ScriptedAdb::reply_once_after` answers a command after it has run
  for a while, as a slow device does; a command whose deadline comes first is ended there and
  answers `Error::Timeout`, as the host's `adb` does.

### Changed

- **`flui-driver`**: `lane::android::device::parse_focus` answers the focused window's
  `WindowName`, its `WindowHash` and its title, where it answered the title alone.
