### Added

- **`flui-driver`**: `Error::PartlySent { effect, retry, first }`: a call that went out in part
  before `first` ended it, which `lane::send_guarded` answers for whatever ends a run once an
  input of it reached the device; a failure that names an effect of its own and invites no retry
  (a command that timed out) stays as it is. `Error::Busy`, `Error::NotFound`, `Error::InputHeld`
  and `Error::NotActionable` carry the effect an app's answer names (part of an input it played),
  and `Error::from_wire` reads an effect on a code whose variant holds none as `Error::PartlySent`,
  with the code and retry the app said.
- **`flui-driver`**: `testing::ScriptedLane::reading_tree` reads the OS tree through a named API,
  answering each read from the app's tree.
- **`flui-driver`**: `Window::compare_projection` reads again on a `NotFound` or `Ambiguous` only
  through a lane that matches against the app's tree; any other lane's answers at once. A tree
  that never held still answers `Error::Busy` with the busy code's `Retry::Soon`, the lane's last
  answer in its message.
- **`flui-driver`**: the Android lane refuses a press, key, text and scroll whose window moved to
  another display as its locate does (`android_default_display`), and, from Android 10, a reply
  of `dumpsys window` that names no display holding the device's focus; it reads that display
  only from its own line, and refuses a reply whose part headers a window's title forged. A dump
  that wrote no window because the window never went idle, or because another `UiAutomation`
  client holds accessibility, and a quarter turn or a resize during the dump, answer
  `Error::Busy`. A pen report written and never confirmed says it may have run, and a press
  whose device binding unwinds is cancelled by the release after it.
