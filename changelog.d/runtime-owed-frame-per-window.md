### Fixed

- On macOS a covered window that owes its kept surface a frame no longer makes the loop turn at the
  frame rate of a shown window beside it: the frame is asked of the covered window alone, once per
  instant, and the application's wake deadline no longer carries it.

### Added

- `PlatformWindow::request_frame_at`, a required method: ask a window for a frame at an instant on
  the event timeline, or withdraw it. The host serves it for that window alone, once; a host that
  keeps no covered window's surface returns at once, and `window_takes_no_frame_verbs!` gives a
  double that body.
- `UiRuntime::owed_frame_requests` and `OwedFrameRequest`: what each covered window is to be asked
  for the frame it owes, which the owner delivery hands to the window at the end of each work item.
- `HeadlessWindow::timed_frame_requests` and `HeadlessHostWindow::timed_frame_requests`: the timed
  frame requests a test window took. `HeadlessHost` serves them: its loop waits no later than the
  earliest, and the first pump at or after it serves it.

### Changed

- `UiRuntime::next_wake` no longer includes the frame a covered window owes its surface.
