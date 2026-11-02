### Added

- `Executor::poll_room(Lane, cx)` and the `Lane` enum in `flui-platform`: a caller refused with
  `SpawnError::Saturated` waits for room on that lane instead of running the work in place.
  `HostComputePool` and `HostIoPool` gain a required `poll_room` too.

### Fixed

- An image decode refused by a full compute lane waits for room there instead of running on the
  IO lane's worker that read its bytes.
