//! The file store behind application persistence.
//!
//! [`FileStore`] keeps one file per [`StorageName`] under two roots
//! ([`DataDirs`]): values made with [`StorageName::from_static`] under the
//! roaming root, values made with [`StorageName::machine_local`] under the
//! local one. Its calls block on the disk, so the host runs them on an IO
//! thread and hands their results to a frame; nothing here is polled inside
//! one.
//!
//! Every write is staged in a uniquely named file next to its target and
//! moved over it with one rename, so a reader, or the next run after a crash,
//! sees the old value or the new one and never a mix. A write based on a
//! version ([`WriteMode::IfUnchanged`]) holds a short lock on the root while
//! it compares and replaces, so two processes sharing the directory cannot
//! both succeed from the same base.
//!
//! wasm32 has no file system: there [`data_dirs`] is `None` and every call is
//! [`StorageError::Unavailable`].
//!
//! [`StorageName`]: flui_platform_api::StorageName
//! [`StorageName::from_static`]: flui_platform_api::StorageName::from_static
//! [`StorageName::machine_local`]: flui_platform_api::StorageName::machine_local
//! [`WriteMode::IfUnchanged`]: flui_platform_api::WriteMode::IfUnchanged
//! [`StorageError::Unavailable`]: flui_platform_api::StorageError::Unavailable

use std::path::PathBuf;

use flui_platform_api::StorageName;

#[cfg(not(target_arch = "wasm32"))]
mod file_store;
#[cfg(not(target_arch = "wasm32"))]
mod os_error;

#[cfg(not(target_arch = "wasm32"))]
pub use file_store::FileStore;
#[cfg(target_arch = "wasm32")]
pub use unavailable::FileStore;

/// The two directories a [`FileStore`] writes under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDirs {
    /// Where values that follow the user between machines live.
    pub roaming: PathBuf,
    /// Where values that belong to this machine live.
    pub local: PathBuf,
}

/// The current user's data directories for the application `app`:
/// `<data dir>/<app>` and `<local data dir>/<app>`.
///
/// The base directories are the platform's own (`%APPDATA%` and
/// `%LOCALAPPDATA%` on Windows, including a redirected or synced folder;
/// `~/Library/Application Support` on macOS; `$XDG_DATA_HOME` on Linux, where
/// both are the same directory). `None` when the platform reports none, and
/// always on wasm32.
#[must_use]
pub fn data_dirs(app: &StorageName) -> Option<DataDirs> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(DataDirs {
            roaming: dirs::data_dir()?.join(app.as_str()),
            local: dirs::data_local_dir()?.join(app.as_str()),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod unavailable {
    use flui_platform_api::{StorageError, StorageName, Stored, StoredVersion, WriteMode};

    use super::DataDirs;

    /// A file store on a platform without files: every call is
    /// [`StorageError::Unavailable`].
    #[derive(Debug)]
    pub struct FileStore {
        _dirs: DataDirs,
    }

    impl FileStore {
        /// A store that will never hold anything.
        #[must_use]
        pub fn new(dirs: DataDirs) -> Self {
            Self { _dirs: dirs }
        }

        /// Always [`StorageError::Unavailable`].
        ///
        /// # Errors
        ///
        /// Always.
        pub fn read(&self, _name: &StorageName, _limit: u64) -> Result<Stored, StorageError> {
            Err(StorageError::Unavailable)
        }

        /// Always [`StorageError::Unavailable`].
        ///
        /// # Errors
        ///
        /// Always.
        pub fn write(
            &self,
            _name: &StorageName,
            _bytes: &[u8],
            _mode: WriteMode,
        ) -> Result<StoredVersion, StorageError> {
            Err(StorageError::Unavailable)
        }
    }
}
