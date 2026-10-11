### Fixed

- Keep runtime redraw requests and debt independent across windows, and fold desktop presentation deadlines without replacing the platform hook on window installation.
- Invoke service factories and host executors outside app storage borrows, with owner validation before registration.
- Deliver lifecycle notifications in transition order after reentry, and finish surviving listeners before propagating a callback panic.
- Collect frame timings only for subscribers, bound retained batches, and deliver them automatically at frame completion.
- Deliver contained owner callback failures to the configured typed failure handler and initialize managed diagnostics through `Application::run`.

### Changed

- Rename `MainWindowRequest` to `WindowRequest`; `open_window` returns this receipt so callers can observe completed installation or deferred failure.
- Remove redundant scheduler aliases; use `phase` and `is_frame_scheduled`.
- Remove unimplemented fullscreen and debug-paint configuration fields and builders. A configured storage directory now produces a typed unsupported-configuration error until the file-backed host integration is implemented.
