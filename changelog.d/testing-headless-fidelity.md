### Added

- **`TestWindow::plugin` and `TestWindow::capability`**: a `Tester` registers the
  application's plugins and overrides as `Application::plugin` and
  `Application::capability` do, so an application using `flui::services::system`
  or its own capability gets its providers under test instead of `NotRegistered`.
  A restart registers them again.
- **`Tester::background` and `Tester::foreground`**: a phone's trip to the
  background and back, in the order Android and UIKit report it: focus lost,
  execution suspended, hidden and the session-ended notice with the documents
  written, the window kept open; then shown, running and focused.
- **`HeadlessHost::set_window_execution`, `HeadlessHost::notify`,
  `HeadlessHost::leave_unshown` and `UnshownFrame`**: a test suspends and resumes
  a window, delivers sleep, wake, session lock and memory pressure, and leaves
  frames unshown for the reasons a real surface gives (not ready, withdrawn,
  lost, device lost, failed, refused). `leave_unshown(n, UnshownFrame::NotReady)`
  replaces `defer_presents(n)`.
- **`HeadlessHost::dispatch_to` and `HeadlessHost::is_retired`.**
- **`ServiceContext::lifecycle`** and **`LifecycleWatch`**: an application
  service reads the application's lifecycle from its own thread (the most
  active state of its UI runtimes, `Detached` when none is left) and waits for
  its changes, so it can pause polling and timers while the application is
  paused or hidden.

### Changed

- The headless host closes a window as a real host does: a window another one
  outlives is removed and the OS focus moves to a survivor, and the last
  window's close retires the UI runtime, so whatever it still held to answer
  ends as `OwnerRetired` and its waiting closures never run.
  `LaidOut::request_close` takes `&mut self` and closes through it.
- The headless host registers the built-ins a runner registers: the primary
  selection only when the primary window's platform is Linux, and the scripted
  HTTP client only with `flui-testing`'s `http` feature, which the facade's
  `http` turns on. A test of a middle-press paste names
  `TargetPlatform::Linux`.
- `HeadlessHost::end_session(SessionEndPhase::Ending)` delivers the
  session-ended notice before it writes what the documents published, as a
  runner does.

### Removed

- `HeadlessHost::defer_presents` (use `leave_unshown`) and
  `UiRuntime::stop_presentation_for_test`.
