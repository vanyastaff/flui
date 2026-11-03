### Added

- `Executor::room(Lane)` and the `Lane` enum in `flui-platform`: a caller refused with
  `SpawnError::Saturated` awaits a `RoomFuture` for room on that lane instead of running the work
  in place. Dropping the wait lets go of its waker. `HostComputePool` and `HostIoPool` gain a
  required `room` too.

### Fixed

- An image decode refused by a full compute lane waits for room there instead of running on the
  IO lane's worker that read its bytes.
