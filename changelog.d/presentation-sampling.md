### Added

- Typed sampling failure provenance distinguishes the first cause from a later
  publication blocker in addressed `SamplingPanic` reports.
- Gesture deadline registrations can publish timing or cancellation through
  identity-bound update authority that commits scheduling state before waking.

### Fixed

- Armed deadline-query retries now participate in the native pending-work gate,
  allowing recovered overdue input to run in its due owner opportunity.
- Explicit deadline queries keep successful observations across unrelated owner
  turns, including after recovery. Due polls still resample changed timing.
- Animation and input deadline failures preserve independent healthy sampling,
  build and completion. A committed delivery failure keeps valid state; a
  computation or unknown failure rejects only its admitted presentation attempt.
- Rejected retained text withdraws old receipt and geometry authority, preserves
  completion debt and permits a fresh replacement attempt.
- Host scheduling reads use cached observations. Deadline query faults receive
  one bounded re-probe; unknown poll faults do not replay the same revision.
  Background owner work advances due input deadlines while frames are disabled.
