### Changed

- `flui_native::BackgroundExecutor` is replaced by `NativeExecutor`, the tokio runtime a host runs
  its IO on: `NativeExecutor::from_tokio` lends it a runtime its creator runs, `NativeExecutor::own`
  gives it a small one of its own (ADR-0327 §3). `CurrentHost::start_with` starts a host on one;
  `CurrentHost::start` uses `own`.

### Removed

- `BackgroundExecutor::block`, `spawn` and `timer`: the executor is the host's, not a public pool.
