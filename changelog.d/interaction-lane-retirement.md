### Fixed

- Retire interaction-lane handlers and payloads without destroying later user-owned values after a teardown failure or during unwind. Preserve the first failure, ordinary release of shared owner-local captures, and stale-lane rejection.
- Revoke cached raw mouse callbacks when their lane closes, including callbacks already snapshotted before reentrant closure. Final framework snapshots preserve exceptional retirement without retaining external non-last callback owners.
