### Fixed

- Evaluate developer timeline event names outside the timeline mutex, allowing consumer conversions to inspect or clear the same timeline without deadlocking.
- Preserve large timeline durations when converting microseconds to `Duration`, saturating values beyond its representable range.
