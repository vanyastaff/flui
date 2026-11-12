### Changed

- The agent protocol's parameter types take bare builder names (ADR-0325): `ActParams::snapshot`,
  `AnnouncementsParams::max_entries`, `MotionRequest::rate` and `step_ms`, `ReadParams::query`,
  `RecordParams::keep_text` and `WaitParams::timeout_ms` replace their `with_` forms.
