### Fixed

- A `Persisted` document writes to the storage it acquired: an application's own
  `StorageCapability` provider receives the writes as well as the reads.
- After a reload that found another writer's change, `Persisted::set` is written over that change
  instead of failing with `Conflict`.
- An edit made from a replaced value's `Drop` is the one stored; a load in flight when `set` is
  called is not applied over the edit; a reload that fails after a save reports the load.

### Changed

- The testing kit's `MemoryStorage` honours `WriteMode::IfUnchanged`: a write based on a version
  the name no longer holds fails with `StorageError::Conflict`, as on disk.
