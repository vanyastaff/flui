### Changed

- **Web storage keeps values in IndexedDB** (`flui-platform`, `flui-app`), and a write based on a
  version (`WriteMode::IfUnchanged`) works there instead of failing with
  `StorageError::LockUnsupported`, so persisted documents are writable on the Web.
  - The comparison and the replacement run in one readwrite transaction, which the browser does
    not interleave with another tab's: of two writes from the same base, one lands and the other
    gets `StorageError::Conflict`.
  - On wasm32 `FileStore::read` and `FileStore::write` return futures.
