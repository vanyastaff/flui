//! [`MemoryStore`]: the values of a [`DataStore`](super::DataStore) kept in
//! memory, for a host with no disk of its own to write to.

use std::collections::HashMap;
use std::sync::Arc;

use flui_platform::{
    CopiedAside, StorageError, StorageName, Stored, StoredVersion, Unsupported, UnsupportedReason,
    WriteMode,
};
use parking_lot::Mutex;

/// Named byte values in memory, with the comparisons and copies of a
/// [`FileStore`](super::FileStore): a [`WriteMode::IfUnchanged`] write is
/// refused with [`StorageError::Conflict`] over another version, and a copy
/// aside is kept as `<name>.corrupt-<n>` in the scope of its name.
///
/// Clones share the values, so the host that owns it keeps them across the
/// runs on it. Each call answers at once.
#[derive(Clone, Default)]
pub(crate) struct MemoryStore {
    values: Arc<Mutex<Values>>,
}

#[derive(Default)]
struct Values {
    stored: HashMap<StorageName, Vec<u8>>,
    /// The copies kept aside of each name, oldest first: copy `n` is kept
    /// as `<name>.corrupt-<n>`.
    kept: HashMap<StorageName, Vec<Vec<u8>>>,
}

impl MemoryStore {
    /// Read the value stored under `name`, as
    /// [`FileStore::read`](super::FileStore::read) does.
    ///
    /// # Errors
    ///
    /// [`StorageError::TooLarge`] for a value over `limit` bytes.
    pub(crate) fn read(&self, name: &StorageName, limit: u64) -> Result<Stored, StorageError> {
        let values = self.values.lock();
        let Some(bytes) = values.stored.get(name) else {
            return Ok(Stored {
                bytes: None,
                version: StoredVersion::ABSENT,
            });
        };
        let len = u64::try_from(bytes.len()).expect("BUG: a byte length fits in u64");
        if len > limit {
            return Err(StorageError::TooLarge { len, limit });
        }
        Ok(Stored {
            version: StoredVersion::of_bytes(bytes),
            bytes: Some(bytes.clone()),
        })
    }

    /// Store `bytes` under `name`, as
    /// [`FileStore::write`](super::FileStore::write) does.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] when the stored version is not the base of
    /// a [`WriteMode::IfUnchanged`] write.
    pub(crate) fn write(
        &self,
        name: &StorageName,
        bytes: &[u8],
        mode: WriteMode,
    ) -> Result<StoredVersion, StorageError> {
        let mut values = self.values.lock();
        match mode {
            WriteMode::Replace => {}
            WriteMode::IfUnchanged(base) => {
                let stored = values
                    .stored
                    .get(name)
                    .map_or(StoredVersion::ABSENT, |bytes| {
                        StoredVersion::of_bytes(bytes)
                    });
                if stored != base {
                    return Err(StorageError::Conflict);
                }
            }
            // A mode this store does not know how to honour is refused
            // rather than guessed at, as the file store refuses it.
            _ => return Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into()),
        }
        values.stored.insert(*name, bytes.to_vec());
        Ok(StoredVersion::of_bytes(bytes))
    }

    /// Copy the value stored under `name` to the first free
    /// `<name>.corrupt-<n>`, counting `n` from 1, as
    /// [`FileStore::copy_aside`](super::FileStore::copy_aside) does.
    ///
    /// # Errors
    ///
    /// [`StorageError::NotFound`] when `name` holds no value.
    pub(crate) fn copy_aside(&self, name: &StorageName) -> Result<CopiedAside, StorageError> {
        let mut values = self.values.lock();
        let bytes = values
            .stored
            .get(name)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        // Copies are only ever added, so the next free number is one past
        // the count of this name's copies.
        let copies = values.kept.entry(*name).or_default();
        let kept_as = format!("{}.corrupt-{}", name.as_str(), copies.len() + 1);
        let version = StoredVersion::of_bytes(&bytes);
        copies.push(bytes);
        Ok(CopiedAside { kept_as, version })
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Counted under the lock, written after it: the formatter's writer is
        // the caller's code and may use this store.
        let (values, kept) = {
            let values = self.values.lock();
            (values.stored.len(), values.kept.len())
        };
        f.debug_struct("MemoryStore")
            .field("values", &values)
            .field("kept", &kept)
            .finish()
    }
}
