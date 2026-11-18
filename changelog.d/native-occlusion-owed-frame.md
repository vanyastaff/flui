### Fixed

- On macOS a window covered by other windows draws one frame of what changed once it has drawn
  nothing for a second, so uncovering it shows its current content rather than the frame of before
  the covering. It draws none while the session is locked or the machine sleeps, and none on hosts
  that keep no image of a covered window (X11 without a compositing manager) or hold its frames
  (a suspended Wayland toplevel).
- A window that cannot be seen no longer wakes the loop for its tree's work or its dropped frame's
  retry: a covered window with a dirty tree waits until it is seen again, or for its owed frame.
- On X11 a compositing manager that quits, crashes or gives its selection up is heard at once, and
  windows report a covering again without waiting for the next read of the window manager's support.
- On Windows a session that loses its display (its remote client disconnects, or another session
  takes the console) hides its windows, as a locked session does, until it connects again.

### Added

- `OccludedSurface` and `PresentationWindow::with_occluded_surface`: the host names whether the
  window system keeps a covered window's last frame.
- `UiRuntime::runs_frames`: whether a wake runs a frame or a background turn, which every runner's
  wake gate asks.
- `HeadlessHost::set_window_occluded` and `HeadlessWindow::with_occlusion_reports` let a test cover
  a window the way AppKit does; `HeadlessHost::pump` takes a background turn while no window is
  seen and none owes a frame, as a runner does.
