### Added

- `HeadlessWindow::with_close_request_handler` and `TestWindow::close_request_handler`: a headless
  test answers a window's close requests with the application's `CloseRequestHandler`, asked
  after the window's close guard for a close or a quit and first at the session's end, as a
  runner asks it.

### Changed

- A headless host answers closes, quits and session ends through the same steps a runner does:
  a session-end query reaches the subscribers as `HostNotice::SessionEnding` and a block they take
  holds the session, a cancel is heard as `HostNotice::SessionEndCancelled`, a desktop session end
  writes the documents before the windows are torn down, and on Android and iOS a window's close
  is never put to its guard or handler, since the platform never asks.
