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
///
/// `localStorage` is shared by every tab of the origin and has no lock
/// (HTML, "The `localStorage` getter"), so a write based on a version cannot
/// compare and replace atomically: it is refused with
/// [`StorageError::LockUnsupported`], as on a file system without locks.
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
    /// `getItem` hands over the whole entry, so the limit is checked after it
    /// is loaded; the origin's quota (about 5 MiB of UTF-16 text in current
    /// browsers) bounds what that can be.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unavailable`] without a `localStorage`;
    /// [`StorageError::TooLarge`] for a value over `limit` bytes;
    /// [`StorageError::Inaccessible`] for an entry this store did not write.
    pub fn read(&self, name: &StorageName, limit: u64) -> Result<Stored, StorageError> {
        let Some(text) = page_storage()?
            .get_item(&self.key(name))
            .map_err(|_| StorageError::Unavailable)?
        else {
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

    /// Store `bytes` under `name`, replacing whatever is there.
    ///
    /// # Errors
    ///
    /// [`StorageError::LockUnsupported`] for [`WriteMode::IfUnchanged`];
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
            WriteMode::IfUnchanged(_) => return Err(StorageError::LockUnsupported),
            _ => return Err(StorageError::Unavailable),
        }
        let text: String = bytes.iter().copied().map(char::from).collect();
        page_storage()?
            .set_item(&self.key(name), &text)
            .map_err(|_| StorageError::Full)?;
        Ok(StoredVersion::of_bytes(bytes))
    }

    fn key(&self, name: &StorageName) -> String {
        let root = if name.is_machine_local() {
            &self.local
        } else {
            &self.roaming
        };
        format!("{}/{}", root.display(), name.as_str())
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
