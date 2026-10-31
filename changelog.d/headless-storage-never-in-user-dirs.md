### Added

- `HeadlessPlatform::with_data_root` keeps each application's stored values on disk under a root
  the test names, so a test can read back the bytes written.

### Changed

- A headless platform no longer writes to the user's data directories: without a root, its
  applications keep their stored values in memory for as long as the platform lives.
- `Platform::data_dirs` is now `Platform::data_store` and returns a `storage::DataStore`: a file
  store under the data roots, or values in memory on headless.
