### Added
- `flui_native::AppHandler`, passed to `Platform::run(Rc<dyn AppHandler>)`: one method per obligation and none with a default — `ready(OwnerPlatform)` (the bootstrap), `quit_requested`, `reopen`, `session_end` (answering a `SessionEndAnswer`), `host_notice`, `exit_policy` and `wake_deadline`, each borrowing the owner capability. `flui-app` is its one implementor.
- `HeadlessHostEvents::reopen` reports a reopen to the headless platform's handler, as AppKit reports a click on the Dock icon.

### Changed
- What a platform reports to the application (a quit, a reopen, a host notice) is no longer lost when nothing can hear it yet: one accepted before `ready` returns, while another method of the handler runs, or on another thread waits and is delivered in order on the owner thread. The quit reaches the handler once per run. A handler's panic is contained, what was accepted behind it is still delivered, and the first panic is raised afterwards.
- A session end, the exit policy and the wake deadline are asked of the handler at once; a report the handler raises while it answers waits for the answer.
- `SharedPlatform::request_exit_policy_reevaluation` asks the handler's `exit_policy` again.

### Removed
- `PlatformReadyCallback`, and the registrations `Platform::on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`, `on_open_urls`, `set_exit_policy_hook` and `set_wake_deadline_hook`, with their `OwnerPlatform` and `SharedPlatform` forwards. The opened URLs and files join `AppHandler` with the activation feature.
