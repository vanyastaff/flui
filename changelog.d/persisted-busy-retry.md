### Added

- A `Persisted` write the storage refuses as `Busy` is tried again on the UI runtime's clock after
  50, 100, 200, 400 and 800 ms, the status staying `Saving`; a refusal after the last retry is
  `Failed(Storage(Busy))`. A waiting retry runs at once when the application is hidden or paused
  or a close is asked for.
- `LaidOut::set_window_visible` in the testing kit reports the window shown or hidden.
