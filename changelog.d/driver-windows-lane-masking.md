### Added

- `flui_driver::Error::ReleaseFailed`: a lane could not release input it pressed itself, naming
  what is still held. `input_held` keeps meaning someone else's input in the way.

### Changed

- `flui-driver`'s Windows lane sends Control+Shift, Alt+Shift and Shift taps, a Shift-click
  included, and refuses only the presses the shell takes (the Windows key, Alt+Tab,
  Ctrl+Shift+Esc, a lock key): a key event that would switch the input language or turn on Sticky
  Keys (the fifth lone Shift press or its release) is preceded by a press and release of the
  unassigned virtual key `0xE8`, which the app sees too.
- `flui_driver::Error::Gone` carries `effect`, read from the wire's error object; an element that
  went away while the Windows lane invoked it reports `may_have_run`.

### Fixed

- A lane's worker that ends, or a lane job that panics, after a failed release releases once
  more, and logs at error level what still stays held.
- A panic in the release after a lane job's panic (the lane's code, a log subscriber) no longer
  ends the lane's worker and loses the job's panic: the job's panic reaches the caller.
