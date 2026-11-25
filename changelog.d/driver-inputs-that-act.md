### Added

- **`flui-driver`**: `lane::OsInput`, `lane::ActingInput`, `lane::SendFailure`, and
  `Delivered::send`, `Delivered::took_command`, `Delivered::unanswered_command` and
  `Delivered::effect`: whether an input acts on the app is its type's to say, so a lane's
  housekeeping (the X11 lane's keymap change and its wait for the server to settle) is never said
  to the run, and a send that put part of its input on the device (a Windows `SendInput` that
  inserted some of its events, a macOS text run posted without its release) says that part went
  out. The held parts of the Windows, X11 and macOS lanes (`HeldKeys::deliver`,
  `HeldButtons::deliver`, `HeldButtons::scroll`, `Pointer::deliver`) take the run's `Delivered`
  and say each input there.
- **`flui-driver`**: `lane::android::uiautomator::DumpFiles`: where a session's dumps live on the
  device, and the shell lines that remove them.

### Changed

- **`flui-driver`**: the effect's detail of an Android run names the commands the device took and
  the commands whose answer was lost apart: "the device took N command(s) of the run, and M more
  may have run".

### Removed

- **`flui-driver`**: `Delivered::input`, `Delivered::command` and `Delivered::sent`, which
  `Delivered::send` and `Delivered::took_command` replace; `uiautomator::dump_path`,
  `uiautomator::session_dumps` and `uiautomator::dump_line`, which `DumpFiles` replaces.

### Fixed

- **`flui-driver`**: text the X11 lane types for a character no key carries, refused before its
  first key press, answers the plain refusal, where it answered as partly sent; a Windows
  `SendInput` that inserted only part of a character's or a wheel turn's events says that part
  went out, where the run invited a retry that sent it again.
- **`flui-driver`**: a dump's line removes its own files and the earlier dumps', never a later
  dump's, so a dump the host ended at its deadline no longer removes the file of the dump after
  it; a panic of the `Adb` at one step of the Android lane's end leaves the others (the forward's
  removal among them) to run.
