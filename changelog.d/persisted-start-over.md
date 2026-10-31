### Added

- `Storage::copy_aside` copies a stored value to the first free `<name>.corrupt-<n>` in its scope,
  claimed atomically, and returns the version of the bytes copied; `CopiedAside` and
  `StorageError::NotFound` go with it. `FileStore`, its IndexedDB counterpart and the testing kit's
  `MemoryStorage` implement it; `MemoryStorage::kept_aside` and `fail_copies` read and fail it.
- `Persisted::start_over` keeps a stored document the application cannot read with `copy_aside`,
  then writes `Document::initial` over exactly those bytes; `retry` resumes a failed fresh start
  without a second copy.

### Security

- On Unix, `FileStore` creates every stored value and every copy aside readable and writable by
  its owner only (`0600`), whatever the umask.
