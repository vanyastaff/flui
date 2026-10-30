### Changed

- **The exit has one deadline.** At loop exit the flush registry is written first, then the
  services and the background pools share one six-second budget (ADR-0258 §7) instead of
  five seconds each, one after the other. `ExecutionServices::shutdown` takes the
  `Instant` to finish by instead of a grace per pool.

### Added

- **`TeardownStep::Flush`**: the step of the exit order that writes the flush registry.
