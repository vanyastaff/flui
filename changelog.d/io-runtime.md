### Added

- `ExecutionServices::io_runtime` lends the native host the application's IO runtime, or none when the host brought its own pools.

### Changed

- On Wayland and X11 an application runs the host's IO sources (XIM, IBus, logind, the settings portal) on its own `flui-io` runtime instead of a second runtime the host started (ADR-0327 §3). The configured executors are installed before the platform starts.
