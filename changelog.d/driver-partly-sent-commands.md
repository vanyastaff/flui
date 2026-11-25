### Added

- **`flui-driver`**: `Lane::send_counted` and `lane::Delivered`, the one record
  `lane::send_guarded`'s partly sent answer is read from. A lane says each input of an event that
  reached the device (`Delivered::input`, `Delivered::command`; by default an event sent whole is
  one input where it acts), and the run counts every event taken whole, a pause and a move
  included, and the failing event where part of it went out. The Windows, X11 and macOS lanes
  say each keystroke, button and click of the wheel, so text cut part way is partly sent; the
  Android lane says each command, an `input` line or a pen report, and names how many the device
  took in the effect's detail.
- **`flui-driver`**: `lane::android::adb::RunFailure`: `Adb::run` says whether a command that
  failed reached the device, `NotStarted` when `adb` never started and `Unanswered` when its
  answer was lost; `testing::ScriptedAdb::not_started_once` scripts the first, and
  `testing::ScriptedLane::losing_focus_after_keystrokes` text cut part way.

### Changed

- **`flui-driver`**: `Adb::run` answers `RunFailure` in place of `Error`.
- **`flui-driver`**: a desktop lane's refusal for a window not in front says the input is not
  sent there, where it said no input was sent.

### Fixed

- **`flui-driver`**: the Android lane counts a command whose answer was lost as one that may have
  run, as its record of what is held keeps it, and forgets a contact recorded before its press
  only when the press never reached the device or the device refused it; a pen report that
  unwinds ends its device and the record of what it held; each dump writes a file of its own,
  and every dump that runs to its end and the lane's end remove the session's dump files; every
  read of the device (a capture included) that times out says nothing may have run; a reply
  naming no focused display is refused where the API level is unread; a focused display printed
  on two lines is refused, and one printed before the window list ends is not read.
