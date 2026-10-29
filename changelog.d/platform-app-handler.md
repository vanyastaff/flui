### Added
- `flui_native::AppHandler`, passed to `Platform::run(Rc<dyn AppHandler>)`: one method per obligation and none with a default — `ready(OwnerPlatform)` (the bootstrap), `quit_requested`, `reopen`, `session_end` (answering a `SessionEndAnswer`), `host_notice`, `exit_policy` (answering an `ExitAnswer`) and `wake_deadline`, each borrowing the owner capability. `flui-app` implements it for its runners; an example or a test that drives `flui-native` directly implements its own.
- `flui_native::ExitAnswer`: `End`, `KeepRunning`, or `HostDecides`, which leaves the exit to the host as with no handler bound (the Win32, AppKit and winit hosts end the loop, the headless host keeps it running).
- `flui_native::testing::app` (the `testing` feature): `AppLog`, a handler that records every call and answers what it is told, and `on_ready`, `with_log` and `run_keeping_owner` to run a platform with one.
- `HeadlessHostEvents::reopen` reports a reopen to the headless platform's handler, as AppKit reports a click on the Dock icon.

### Changed
- `Platform::run` takes the application's handler instead of a one-shot ready callback: the bootstrap is `AppHandler::ready(OwnerPlatform)`, which receives the owner capability by value as the callback did.
- The exit question is `AppHandler::exit_policy`, answered with an `ExitAnswer` instead of the `bool` the exit-policy hook returned. A `flui-app` run that routes no exit policy answers `HostDecides`, so the host decides as it did with no hook; direct mode still ends the loop.
- What a platform reports to the application (a quit, a reopen, a host notice) is no longer lost when nothing can hear it yet: one accepted before `ready` returns, while another method of the handler runs, or on another thread waits and is delivered in order on the owner thread. The quit reaches the handler once per run, also when the loop ends from inside a method (the web page and the Android Activity let go of the handler only after the running delivery heard it). A handler's panic is contained, what was accepted behind it is still delivered, and the first panic is raised afterwards.
- A session end, the exit policy and the wake deadline are asked of the handler at once; a report the handler raises while it answers waits for the answer.
- `SharedPlatform::request_exit_policy_reevaluation` asks the handler's `exit_policy` again.
- A platform run from inside a `flui-app` handler (as a test does) answers with its own routes and leaves the outer run's quit, exit policy and wake deadline as they were.

### Removed
- `PlatformReadyCallback`, and the registrations `Platform::on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`, `on_open_urls`, `set_exit_policy_hook` and `set_wake_deadline_hook`, with their `OwnerPlatform` and `SharedPlatform` forwards. The opened URLs and files join `AppHandler` with the activation feature.
