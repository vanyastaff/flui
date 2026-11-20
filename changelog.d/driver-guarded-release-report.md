### Added

- `ScriptedLane::failing_releases_once_sent` and `ScriptedLane::panicking_sends`, and
  `LaneLog::disarms`, in `flui_driver::testing`.

### Fixed

- A guarded OS-input sequence tries a failed release once more on every exit. After a send that
  went through, the second failure is the answer; after a failed send (interrupted by the user,
  the window gone), whose error stays the caller's, what stays held is logged at error level
  instead of being dropped.
- The next guarded sequence starts by releasing what an earlier one left held, and is refused,
  with nothing sent, while it stays held, instead of sending its input with a key still down.
- A send that panics disarms the user-activity guard and releases what it sent before the panic
  goes on.
