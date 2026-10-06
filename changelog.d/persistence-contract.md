### Added

- `flui::platform::Storage`, a byte-storage capability with `StorageName` (checked at compile time in a `const`), `StoredVersion`, `Stored`, `WriteMode` and `StorageError`; widgets reach a realm's storage through `LifecycleContext::storage`.
- `flui::view::persist`: `Document`, `Persisted`, `SaveStatus`, `Revision`, `PersistError` (with `CodecStep`), `ReadOnlyReason` and `DecodeError`, the versioned-document API applications keep data through. Loading and writing are not wired yet: `load` reports `StorageError::Unavailable` and `set` refuses.
- `Router::from_stack`, `RouterHandle::stack` and `RouterError::EmptyStack` for reopening a saved navigation stack; for now the router opens on, and reports, the top route only.
- The `persist` facade feature and `AppConfig::with_storage_dir`; no storage is given to widgets yet.
- `flui::testing::storage::MemoryStorage`, an in-memory storage with held writes, counted commit barriers and injected failures, and `flui::testing::widgets::lay_out_with_storage`.

### Changed

- The `two_screens` example now requires the `persist` feature as well as `material` (`cargo run --example two_screens --features material,persist`).
