### Added

- **`flui-driver`**: `testing::ScriptedAdb::reply_once_after` answers a command after it has run
  for a while, as a slow device does; a command whose deadline comes first is ended there and
  answers `Error::Timeout`, as the host's `adb` does.

### Removed

- **`flui-driver`**: `lane::android::device::FOCUS` and `parse_focus`: the lane reads the focus
  with the windows, from one `dumpsys window` (`device::WINDOWS`), as `Windows::focus`, the
  window's `WindowName` (its `WindowHash` and its title).
