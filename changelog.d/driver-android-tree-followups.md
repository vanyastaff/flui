### Added

- **`flui-driver`**: `testing::ScriptedAdb::reply_once_after` answers a command after it has run
  for a while, as a slow device does; a command whose deadline comes first is ended there and
  answers `Error::Timeout`, as the host's `adb` does. `ScriptedAdb::unwind_once` lets a command
  unwind, as an `Adb` the driver does not own may.
