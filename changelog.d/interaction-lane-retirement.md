### Fixed

- Retire interaction-lane handlers and payloads without destroying later user-owned values after a teardown failure or during unwind. Preserve the first failure, ordinary release of shared owner-local captures, and stale-lane rejection.
