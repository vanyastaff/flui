### Changed

- **`flui-platform`**: `Script::hold` returns a `ScriptHold`, and the script holds requests while
  any of its holds lives. Dropping or `release`-ing the last one sends the requests the holds
  kept, oldest first, on to the steps and the standing answer; a request a step holds still waits
  for the test. `ScriptedLauncher::hold` returns that hold, and `ScriptedFileDialogs::hold`
  (`flui-system`) returns the holds of its open and save scripts.

### Removed

- **`flui-platform`**: `Script::resume`; drop or `release` the `ScriptHold` instead.

### Fixed

- **`flui-interaction`**: a `ClipboardWriteHold` dropped while a test's own hold of the write
  script lives no longer answers the writes that hold kept, and a test's hold no longer lifts a
  live `ClipboardWriteHold`. Likewise `answer_held_reads` on the scripted clipboard and primary
  selection answers held reads only once no other hold of the read script lives, and leaves
  reads a step holds to the test.
