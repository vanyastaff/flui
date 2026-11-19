### Fixed

- On macOS a covered window that owes its kept surface a frame no longer makes the loop turn at the
  frame rate of a shown window beside it: the frame is asked of the covered window alone, once per
  instant, and the application's wake deadline no longer carries it. Only that window's own served
  request runs the frame: a frame of another window, or work due elsewhere, never does, and a frame
  held by its text preparation keeps no loop turning. A held instant is asked for again once the
  preparation's outcome moves on (its text sizes answered, or the segment withdrawn), never while
  it waits.

### Added

- `PlatformWindow::request_frame_at`, a required method: ask a window for a frame at an instant on
  the event timeline, or withdraw it. The host serves it for that window alone, once; a host that
  keeps no covered window's surface returns at once, as a standalone `MockWindow` does; one built
  over another window hands it to that window.
- `UiRuntime::owed_frame_requests` and `OwedFrameRequest`: what each covered window is to be asked
  for the frame it owes, which the owner delivery hands to the window at the end of each work item.
- `UiRuntime::owed_frame_served`: whether a frame of a covered window's own window serves the frame
  it owes, which a runner's wake gate and dirty gate count.
- `HeadlessWindow::timed_frame_requests` and `HeadlessHostWindow::timed_frame_requests`: the timed
  frame requests a test window took. `HeadlessHost` serves them: its loop waits no later than the
  earliest, and the first pump at or after it delivers that window a frame; a window that cannot be
  seen is asked for no other frame.
- `HeadlessHost::script_text_sizing`, `HeadlessHost::answer_text_sizing` and `ScriptedTextSizing`:
  a scripted text-sizing provider that answers the runtime's frontiers at once or holds them until
  the test answers, so a test reaches a text preparation that waits.

### Changed

- `UiRuntime::next_wake` and `UiRuntime::has_pending_due_work` no longer include the frame a
  covered window owes its surface.
- `HeadlessHost::time_to_next_frame` and `HeadlessHost::pump_until_settled` first ask each covered
  window for the frame it owes, which runs the windows' `request_frame_at` and records the request.
