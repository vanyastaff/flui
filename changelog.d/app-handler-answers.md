### Fixed

- A window answers a frame request with what became of it: a request its
  presentation refused (the UI runtime or the presentation is gone or closing)
  is `FrameOutcome::Declined`, where the runner used to answer `Produced` for
  every request once the window was installed, which let a loop that waits on a
  wake deadline ask for the same due instant again.
- A platform input raised on the owner thread is answered with the handler's
  answer to that input, not with the answer to an input another thread queued
  ahead of it.
- A window that remembers its placement (`AppConfig::restore_placement`) reads
  the stored placement once, before it opens; the second, blocking read while
  the window was being installed is gone. The warning for a run without
  storage names `AppConfig::storage_dir`.
- Android: a window's surface target answers `HandleError::Unavailable` for its
  display handle wherever it does for its window handle (off the looper thread,
  with no native window, after the window's retirement).
