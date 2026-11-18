### Changed

- `flui_testing::backend_conformance::RowOutcome::Failed` carries a `RowFailure`: the failure's
  `FailureKind` beside its message, so a backend's suite that expects a row to fail names the kind
  it expects instead of matching words of the message. The kinds are `NoReading` (the backend has
  no reading for a step the row performs, or for a contact of any kind), `Interval` (two events
  at another distance than their readings), `OutOfOrder` (an event's time before an earlier
  event's), `Output` and `Panicked`.
