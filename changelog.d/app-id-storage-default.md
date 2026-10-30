### Added

- `AppId`, a validated reverse-DNS application identifier (`com.example.notes`) every platform
  accepts as the application's name, set with `AppIdentity::with_app_id`.
- `flui_platform::AppDataDir`, the validated name of the directory an application's stored values
  live in.

### Changed

- An application with an identifier has storage without further setup: its directory under the
  platform's data roots is named after the identifier. On Android, iOS and the Web, whose roots
  belong to the application alone, a fixed name stands in when there is no identifier.
  `AppConfig::with_storage_dir` now takes an `AppDataDir` and only overrides the location. A
  debug build warns once a run when a widget asks for storage the application cannot name.
- `flui_native::storage::data_dirs` and `Platform::data_dirs` take an `AppDataDir`.

### Removed

- `AppleBundleId` and `AppIdentity::with_apple_bundle_id`: use `AppId` and
  `AppIdentity::with_app_id`, which also names the Apple unified-logging subsystem.
