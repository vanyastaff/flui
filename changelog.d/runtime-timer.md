### Added

- `LifecycleContext::timer` and `AsyncDriver::timer` hand out a `TimerHandle`: `sleep` and
  `sleep_until` return a `Sleep` future that resolves on the UI runtime's clock (the testing kit's
  virtual clock in a test). `UiRuntime::next_wake` includes the earliest armed deadline, so the
  host wakes for it; `OwnerFrame::next_deadline` reports it.
