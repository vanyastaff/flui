### Added

- **Every window holds its own close.** A presentation's `LifecycleContext::close_guard()`
  now returns its guard: a hold keeps a user or program close (and, on Windows and macOS, a
  session end) until it is released, and the window then closes without asking the
  application again. Writes queued in the host's flush registry run on the I/O pool.
- **`HeadlessHost::request_close` and `HeadlessHost::end_session`** carry out a close or a
  session end through the presentations' guards and the flush registry, within
  `flui_testing::SESSION_END_BUDGET` (3 s).
