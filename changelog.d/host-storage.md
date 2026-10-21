### Added

- **Widgets get working storage** (`flui-app`, `flui-platform`): with
  `AppConfig::with_storage_dir(name)`, `LifecycleContext::storage` reads and writes named values
  that outlive the process.
  - On desktop and iOS they live under the user's data directories. On Android they live in the
    app's private `files` directory, with session values in `no_backup`. On the Web they live in
    the page's IndexedDB.
  - On the Web, a write based on a version (`WriteMode::IfUnchanged`) compares and replaces in
    one readwrite transaction, which the browser does not interleave with another tab's: of two
    writes from the same base, one lands and the other gets `StorageError::Conflict`.
  - Each Web value's length is a record of its own, written in the same transaction. An
    oversized read and a based write of another length are therefore decided without loading
    the value.
  - On wasm32, `FileStore::read` and `FileStore::write` return futures.
  - Reads and writes run on the host's IO pool. A newer write of a name replaces an older one not
    yet started, which then resolves to `StorageError::Superseded`.
  - A write the pool refuses or drops on shutdown resolves to `StorageError::Cancelled` instead of
    waiting forever, and nothing is written once the pool has begun shutting down.
- **`ExecutionServices::is_accepting`** (`flui-runtime`): whether the pool still admits work.
- **Android and Web runners install the host executors** set with `AppConfig::with_executors`,
  as the desktop and iOS runners already did.
- **`Platform::data_dirs` and `SharedPlatform::data_dirs`** (`flui-platform`): where an
  application keeps its data on the running platform.

### Changed

- **Storage needs no Cargo feature.** `AppConfig::with_storage_dir` alone turns it on, and the
  `dirs` and `tempfile` are
  always linked on native targets.

### Removed

- **The `persist` feature** of `flui` and `flui-app`, and the `storage` feature of
  `flui-platform`.
- **`AppWindowError::UnsupportedConfiguration`** (`flui-app`): a configured storage directory
  was its only cause, and it is no longer refused.
