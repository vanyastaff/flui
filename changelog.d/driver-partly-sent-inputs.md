### Added

- **`flui-driver`**: `lane::Delivered::input`, `Delivered::command` and `Delivered::sent`: a lane
  says each input of an event that reached the device (by default an event sent whole is one
  input where it acts), and `lane::send_guarded` counts every event taken whole, a pause and a
  move included, and the failing event where part of it went out. The Windows, X11 and macOS
  lanes say each keystroke, button and click of the wheel, so text cut part way is partly sent;
  the Android lane says each command, an `input` line or a pen report, and names how many the
  device took in the effect's detail.
- **`flui-driver`**: `lane::android::adb::RunFailure`: `Adb::run` says whether a command that
  failed reached the device, `NotStarted` when `adb` never started and `Unanswered` when its
  answer was lost. `testing::ScriptedAdb::not_started_once` scripts the first, and
  `testing::ScriptedLane::losing_focus_after_keystrokes` types text one keystroke per character
  and cuts it part way.

### Changed

- **`flui-driver`**: `Adb::run` answers `RunFailure` in place of `Error`.
- **`flui-driver`**: the Windows, X11 and macOS lanes' refusal for a window not in front says the
  input is not sent there, where it said no input was sent.

### Fixed

- **`flui-driver`**: a run whose events all went out, its moves and pauses included, says all of
  them went out when the release after it fails, on every lane, where it counted only the events
  that act.
- **`flui-driver`**: the Android lane counts a command whose answer was lost as one that may have
  run, as its record of what is held keeps it; a press whose `adb` never started is forgotten
  and cancels nothing; a forward whose answer was lost is removed with the lane, as one that
  timed out was; every dump that runs to its end, and the lane's end, removes the files of the
  session's dumps, so dumps the host ended at their deadline leave none behind for long.
