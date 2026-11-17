### Added

- **`TestWindow::plugin` and `TestWindow::capability`**: a `Tester` registers the
  application's plugins and overrides as `Application::plugin` and
  `Application::capability` do, so an application using `flui::services::system`
  or its own capability gets its providers under test instead of `NotRegistered`.
  A restart registers them again. `TestWindow` now holds them shared and is no
  longer `Send` or `Sync`.
- **`Tester::request_quit` and `HeadlessHost::request_quit`**: a quit the user or
  the application asks for, as a runner carries it out (ADR-0350 §1): every window
  is asked, a recorded refusal defers the quit until its hold is released, an
  unrecorded one or a window kept open cancels it.
- **`Tester::background` and `Tester::foreground`** (and their `try_` forms): a
  phone's trip to the background and back, in the order Android and UIKit report
  it, the session's end announced once a trip with the window kept open; the
  session end's answer is returned (`Block` while writes are in flight). A window
  that names no phone, a second trip and a return without a trip are refused
  (`TestError::NotAPhone`, `InBackground`, `InForeground`).
- **`HeadlessHost::set_window_execution`, `notify`, `leave_unshown` with
  `UnshownFrame`, `session_end_support`, `session_end_releases`,
  `dispatch_to` and `is_retired`.** `leave_unshown(n, UnshownFrame::NotReady)`
  replaces `defer_presents(n)`.
- **`ServiceContext::lifecycle`** and **`LifecycleWatch`**: an application
  service reads the application's lifecycle from its own thread (the most active
  state of its UI runtimes, `Detached` when none is left) and waits for its
  changes; the watch ends when the loop does, and a next run starts a fresh one.
- **`AppLifecycleState::most_active`.**

### Changed

- The headless host closes a window as a real host does: a window another one
  outlives is removed and the OS focus moves to a survivor, and the last
  window's close retires the UI runtime, so whatever it still held to answer
  ends as `OwnerRetired`. A close a widget asks for through its window's handle
  is carried out at the next turn after the window's guard is asked; every close
  a turn finds due is carried out though one panics; a program's close a guard
  held is consent to a quit and closes nothing alone. A closed window hears no
  input or window fact, and its handle's commands answer `Closed`.
  `LaidOut::request_close` takes `&mut self` and closes through the host.
- The headless host's session end follows the platform the window names: a
  desktop closes every window and retires, a phone keeps them open and answers
  `Block` while writes are in flight; `HeadlessHost::end_session` takes
  `&mut self`, and `Tester::end_session` returns the answer.
- The headless host registers the built-ins a runner registers: the primary
  selection only when the primary window's platform is Linux, and the scripted
  HTTP client only with `flui-testing`'s `http` feature, which the facade's
  `http` turns on. A test of a middle-press paste names `TargetPlatform::Linux`.

### Removed

- `HeadlessHost::defer_presents` (use `leave_unshown`) and
  `UiRuntime::stop_presentation_for_test`.
