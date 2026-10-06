### Added

- **`flui_platform::storage::FileStore`** (feature `storage`): the file store behind application
  persistence. One file per `StorageName` under the user's roaming or machine-local data directory
  (`storage::data_dirs`), each write staged in a uniquely named file and moved over the target with
  one rename, so a crash leaves the old or the new value whole. A write based on a version holds a
  short cross-process lock and is refused with `Conflict` when the value changed, `LockUnsupported`
  where the file system has no locks. A Windows sharing violation is `Busy`; an access refusal is
  probed, so a read-only or ACL-protected target is `Inaccessible` at once instead of retried. On
  wasm32 every call is `Unavailable`.
- **`StorageName::is_machine_local`**: tells a storage implementation which root a name belongs to.
