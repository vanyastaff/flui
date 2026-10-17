//! [`FileStore`] on wasm32: one `localStorage` entry per name.

use std::path::PathBuf;

use flui_platform_api::{StorageError, StorageName, Stored, StoredVersion, WriteMode};

use super::DataDirs;

/// The prefix machine-local values get under the local root, as on native
/// targets, so equal text in the two scopes names two entries.
const MACHINE_LOCAL_DIR: &str = ".machine-local";

/// A store of named byte values in the page's `localStorage`, one entry each,
/// keyed by the root and the name.
///
/// The browser writes an entry in one step, so a reader sees the old value or
/// the new one. Every byte is kept as one UTF-16 code unit (`U+0000` to
/// `U+00FF`), so a value costs two bytes of the origin's quota per byte.
/// Without a `localStorage` (blocked by the user, an opaque origin) every
/// call is [`StorageError::Unavailable`].
#[derive(Debug)]
pub struct FileStore {
    roaming: PathBuf,
    local: PathBuf,
}

impl FileStore {
    /// A store under `dirs`, whose roots prefix the entries' keys.
    #[must_use]
    pub fn new(dirs: DataDirs) -> Self {
        let DataDirs { roaming, local } = dirs;
        Self {
            roaming,
            local: local.join(MACHINE_LOCAL_DIR),
        }
    }

    /// Read the value stored under `name`; a missing value is `Ok` with no
    /// bytes and [`StoredVersion::ABSENT`].
    ///
    /// # Errors
    ///
    /// [`StorageError::Unavailable`] without a `localStorage`;
    /// [`StorageError::TooLarge`] for a value over `limit` bytes;
    /// [`StorageError::Inaccessible`] for an entry this store did not write.
    pub fn read(&self, name: &StorageName, limit: u64) -> Result<Stored, StorageError> {
        // `getItem` hands over the whole value: refuse an oversized one from
        // its recorded length before loading it.
        if let Some(len) = self.recorded_len(name)?
            && len > limit
        {
            return Err(StorageError::TooLarge { len, limit });
        }
        let Some(text) = self.entry(name)? else {
            return Ok(Stored {
                bytes: None,
                version: StoredVersion::ABSENT,
            });
        };
        let len = u64::try_from(text.chars().count()).expect("BUG: a length fits in u64");
        if len > limit {
            return Err(StorageError::TooLarge { len, limit });
        }
        let bytes = decode(&text)?;
        Ok(Stored {
            version: StoredVersion::of_bytes(&bytes),
            bytes: Some(bytes),
        })
    }

    /// Store `bytes` under `name`. Under [`WriteMode::IfUnchanged`] the
    /// stored version is compared first; the page runs on one thread, so
    /// nothing replaces the entry between the comparison and the write.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] when the stored version is not the base;
    /// [`StorageError::Full`] when the origin's quota refuses the value;
    /// [`StorageError::Unavailable`] without a `localStorage`.
    pub fn write(
        &self,
        name: &StorageName,
        bytes: &[u8],
        mode: WriteMode,
    ) -> Result<StoredVersion, StorageError> {
        match mode {
            WriteMode::Replace => {}
            WriteMode::IfUnchanged(base) => {
                // A different length is a conflict without loading the value.
                if let Some(len) = self.recorded_len(name)?
                    && Some(len) != base.byte_len()
                {
                    return Err(StorageError::Conflict);
                }
                let current = match self.entry(name)? {
                    Some(text) => StoredVersion::of_bytes(&decode(&text)?),
                    None => StoredVersion::ABSENT,
                };
                if current != base {
                    return Err(StorageError::Conflict);
                }
            }
            _ => return Err(StorageError::Unavailable),
        }
        let text: String = bytes.iter().copied().map(char::from).collect();
        let storage = page_storage()?;
        let len_key = self.len_key(name);
        storage
            .set_item(&self.key(name), &text)
            .map_err(|_| StorageError::Full)?;
        // The value goes first, then its length. A quota refusing the record
        // drops it, and a read then loads the value to measure it. Only while
        // another tab is between its two writes can a reader see the previous
        // record: a too-large refusal or a conflict that the next read or
        // based write no longer hits.
        if storage
            .set_item(&len_key, &bytes.len().to_string())
            .is_err()
        {
            let _ = storage.remove_item(&len_key);
        }
        Ok(StoredVersion::of_bytes(bytes))
    }

    /// The key of `name`'s length record. A `StorageName` holds no `.`, so it
    /// never names a value.
    fn len_key(&self, name: &StorageName) -> String {
        format!("{}.len", self.key(name))
    }

    /// The recorded length of `name`'s value; `None` without a record (an
    /// entry written before records, or by another script).
    fn recorded_len(&self, name: &StorageName) -> Result<Option<u64>, StorageError> {
        Ok(page_storage()?
            .get_item(&self.len_key(name))
            .map_err(|_| StorageError::Unavailable)?
            .and_then(|text| text.parse().ok()))
    }

    fn key(&self, name: &StorageName) -> String {
        let root = if name.is_machine_local() {
            &self.local
        } else {
            &self.roaming
        };
        format!("{}/{}", root.display(), name.as_str())
    }

    fn entry(&self, name: &StorageName) -> Result<Option<String>, StorageError> {
        page_storage()?
            .get_item(&self.key(name))
            .map_err(|_| StorageError::Unavailable)
    }
}

fn page_storage() -> Result<web_sys::Storage, StorageError> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or(StorageError::Unavailable)
}

/// The bytes an entry holds; a code unit past `U+00FF` means another script
/// wrote the entry.
fn decode(text: &str) -> Result<Vec<u8>, StorageError> {
    text.chars()
        .map(u8::try_from)
        .collect::<Result<_, _>>()
        .map_err(|_| StorageError::Inaccessible {
            kind: std::io::ErrorKind::InvalidData,
        })
}
