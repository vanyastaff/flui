### Fixed

- A window covered by other windows, where the host tells a covering (macOS, X11 without a
  compositing manager, Wayland), draws one frame of what changed once it has drawn nothing for a
  second, so uncovering it shows its current content rather than the frame of before the covering.
- A window that cannot be seen no longer wakes the loop for its tree's work: a covered window with
  a dirty tree waits until it is seen again, or for its owed frame.
- On X11 a compositing manager that quits, crashes or gives its selection up is heard at once, and
  windows report a covering again without waiting for the next read of the window manager's support.
- On Windows a session that loses its display (its remote client disconnects, or another session
  takes the console) hides its windows, as a locked session does, until it connects again.

### Added

- `HeadlessHost::set_window_occluded` and `HeadlessWindow::with_occlusion_reports` let a test cover
  a window the way a host that tells a covering does.
