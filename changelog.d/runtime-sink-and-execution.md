### Changed

- **`flui-runtime`** ([ADR-0083](/docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md)):
  it also holds the performance-overlay frame-time window, the frame sink seam (`FrameSink`,
  which a host implements, and the `SubmitVerdict` the realm classifies) and the execution
  services of ADR-0047. They moved out of `flui-app`, where they were crate-private or
  re-exported: `flui_app::HostExecutors`, `ComputeJob`, `DeterministicExecutors`,
  `HostComputePool`, `HostIoPool`, `IoFuture` and `SpawnError` keep their paths, so no public
  path changed. `tokio` leaves `flui-app`'s direct dependencies with the default pools;
  `flui-runtime` lists `flui-app` as its only allowed normal dependent, so no library crate
  reaches the pools.
