### Added

- **`flui_platform::storage::FileStore`** (feature `storage`): the file store behind application
  persistence. One file per `StorageName` under the user's roaming data directory, or the
  `.machine-local` directory of the local one (`storage::data_dirs`), each write staged in a
  uniquely named file and moved over the target with one rename, so a crash leaves the old or the
  new value whole. A write based on a version holds a short cross-process lock, compares lengths
  before reading, sweeps staged files a killed writer left, and is refused with `Conflict` when the
  value changed, `LockUnsupported` where the file system has no locks. A Windows sharing violation
  or a target pending delete is `Busy`; a read-only or ACL-protected target is `Inaccessible` at
  once instead of retried. On wasm32 every call is `Unavailable`.
- **`StorageName::is_machine_local`** and **`StoredVersion::byte_len`**: tell a storage
  implementation which root a name belongs to, and let it compare a version's length before
  reading a value.
