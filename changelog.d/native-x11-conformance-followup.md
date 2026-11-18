### Changed

- `flui_testing::backend_conformance::RowOutcome::Failed` carries a `RowFailure`: the failure's
  `FailureKind` (`NoReading`, `Interval`, `Output`, `Panicked`) beside its message, so a backend's
  suite that expects a row to fail names the kind it expects instead of matching words of the
  message.
