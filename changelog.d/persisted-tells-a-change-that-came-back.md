### Fixed

- `Persisted` tells its listeners of a change even when the state is back where they were last told by the time the delivery runs, such as a retried write that fails the same way. A widget that read the state in between no longer stays on the stale value.
