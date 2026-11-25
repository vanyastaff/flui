### Added

- **`flui-driver`**: `Error::PartlySent { effect, retry, first }`: a call that went out in part
  before `first` ended it. `lane::send_guarded` answers it, on every lane, for whatever ends an OS
  sequence once the lane took an event that acts (`OsEvent::acts`): a refusal, an interruption of
  the guard, a failed command, a release that fails after the last event; its effect counts the
  events taken (`Effect::partial`, in events) and it invites no retry. A failure that names an
  effect of its own and invites no retry (a command that timed out) stays as it is.
- **`flui-driver`**: `testing::ScriptedLane::reading_tree` reads the OS tree through a named API,
  answering each read from the app's tree.

### Changed

- **`flui-driver`**: `Error::Busy`, `Error::NotFound`, `Error::InputHeld` and
  `Error::NotActionable` carry the effect an app's answer names (part of an input it played).
  `Error::from_wire` reads an effect on a code whose variant holds none as `Error::PartlySent`,
  the code and retry as the app said, where it dropped the effect before.
- **`flui-driver`**: `Window::compare_projection` reads again on a `NotFound` or `Ambiguous` only
  through a lane that matches against the app's tree; any other lane's answers at once. A tree
  that never held still answers `Error::Busy` with the busy code's `Retry::Soon`, the lane's last
  answer in its message, where it answered `Retry::Never`.
- **`flui-driver`**: the Android lane refuses a press, key, text and scroll whose window moved to
  another display as its locate does (`android_default_display`), and, from Android 10, a reply
  of `dumpsys window` that names no display holding the device's focus; it reads that display
  only from its own line, and refuses a reply whose part headers a window's title forged. A dump
  that wrote no window because the window never went idle, or because another `UiAutomation`
  client holds accessibility, answers `Error::Busy`; a quarter turn or a resize during the dump
  answers `Error::Busy` where it answered a lane error.

### Fixed

- **`flui-driver`**: a timed-out read of the Android device (its windows, its settings, its dump)
  says nothing may have run; a pen report written and never confirmed says it may have; a press
  whose device binding unwinds is cancelled by the release after it. The OS lanes' refusals say
  only what they refuse, and the effect what went out.
