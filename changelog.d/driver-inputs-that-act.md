### Added

- **`flui-driver`**: `lane::send_guarded` answers a run that went out in part as
  `Error::PartlySent`, read from one record, `lane::Delivered`: the run counts every event taken
  whole, a pause and a move included, and the failing event where part of it reached the device
  (`Effect::partial`, in events), and invites no retry. A lane says each input of an event through
  `Lane::send_counted` and `Delivered::send`, and whether an input acts on the app is its type's
  to say (`lane::OsInput`, `lane::ActingInput`): a move, a pen's hover and a lane's housekeeping
  (the X11 keymap change and its wait for the server to settle) never make a run partly sent. A
  send that put part of its input on the device says so (`lane::SendFailure`: a Windows
  `SendInput` that inserted some of its events, a macOS text run posted without its release), and
  what that part left down is held, so the release lets it go. The held parts of the Windows, X11
  and macOS lanes (`HeldKeys::deliver`, `HeldButtons::deliver`, `HeldButtons::scroll`,
  `Pointer::deliver`) take the run's `Delivered` and say each keystroke, button and click of the
  wheel there; the Android lane says each command, an `input` line or a pen report, as taken or
  unanswered (`Delivered::took_command`, `Delivered::unanswered_command`), and the effect's
  detail names both counts ("the device took N command(s) of the run, and M more may have run").
  `Delivered::effect` is what the run answers with. A desktop lane's refusal for a window not in
  front says the input is not sent there.
- **`flui-driver`**: `lane::android::adb::RunFailure`: `Adb::run` says whether a command that
  failed reached the device, `NotStarted` when `adb` never started and `Unanswered` when its
  answer was lost (a deadline, a bound, an output left open, a pipe). The Android lane holds a
  press whose answer was lost for the release to cancel and names a key it may have left down,
  counts it as may have run, and forgets a press that never reached the device or that the device
  refused; a forward whose answer was lost is removed with the lane. A pen report that unwinds
  ends its device and the record of what it held; every read of the device (a capture included)
  that times out says nothing may have run.
- **`flui-driver`**: `lane::android::uiautomator::DumpFiles`: each window dump of a session at a
  path of its own, removed with the earlier dumps' by every dump that runs to its end (never a
  later dump's) and with all of the session's by the lane's end; every shell line holds the
  directory and the session as one quoted word. The steps of the Android lane's end, and the
  destructors of its device and streams, each run contained, so a panic in one leaves the others
  to run and is logged, not raised.
- **`flui-driver`**: the Android lane refuses a reply naming no focused display where the API
  level is unread, refuses a focused display printed on two lines, and reads none printed before
  the window list ends.
- **`flui-driver`**: `testing::ScriptedAdb::not_started_once` and
  `ScriptedAdb::device_unwinding_on_drop`, and `testing::ScriptedLane::losing_focus_after_keystrokes`,
  which types text one keystroke per character and cuts it part way.
