### Added

- **Widgets get working storage** (`flui-app`, `flui-platform`): with
  `AppConfig::with_storage_dir(name)`, `LifecycleContext::storage` reads and writes named values
  that outlive the process.
  - On desktop and iOS they live under the user's data directories. On Android they live in the
    app's private `files` directory, with session values in `no_backup`. On the Web they live in
    the page's `localStorage`.
  - Reads and writes run on the host's IO pool. A newer write of a name replaces an older one not
    yet started, which then resolves to `StorageError::Superseded`.
  - A write the pool refuses or drops on shutdown resolves to `StorageError::Cancelled` instead of
    waiting forever.
- **`Platform::data_dirs` and `SharedPlatform::data_dirs`** (`flui-platform`): where an
  application keeps its data on the running platform.

### Changed

- **Storage needs no Cargo feature.** `AppConfig::with_storage_dir` alone turns it on, and the
  `two_screens` and `notes_faults` examples need only `material`. `dirs` and `tempfile` are
  always linked on native targets.

### Removed

- **The `persist` feature** of `flui` and `flui-app`, and the `storage` feature of
  `flui-platform`.
- **`AppWindowError::UnsupportedConfiguration`** (`flui-app`): a configured storage directory
  was its only cause, and it is no longer refused.
