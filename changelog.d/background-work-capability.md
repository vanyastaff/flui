### Added
- `flui_view::background`: `BackgroundCapability` hands a widget a `Background` that runs a CPU job (`compute`) or an IO future (`io`) on the host's bounded lanes and answers with a `BackgroundTask` on the owner thread. Dropping the task cancels the work; a panic answers `BackgroundError::Panicked`. `flui-app` provides it over the run's execution services (ADR-0272).
- `flui_platform::executor::Executor`, the lanes as a capability handle holds them; `ExecutionServices` implements it.
- `HeadlessHost` and `LaidOut` run background work in `pump`, with `hold_background`, `release_background`, `run_background` and `pending_background`; `pump_until_settled` waits for it.
- `DeterministicExecutors::runnable` counts work that a drive would progress now.

### Changed
- `ComputeJob`, `IoFuture` and `SpawnError` moved to `flui_platform::executor`; `flui_runtime::execution` re-exports them.
