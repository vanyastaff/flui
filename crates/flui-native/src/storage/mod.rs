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
//! wasm32 has no file system: there [`FileStore`] keeps one record per name
//! in the page's IndexedDB, under the roots as key prefixes, and its calls
//! return futures the host's IO task awaits. A based write compares and
//! replaces inside one readwrite transaction, which the browser does not
//! interleave with another tab's.
//!
//! [`StorageName`]: flui_platform::StorageName
//! [`StorageName::from_static`]: flui_platform::StorageName::from_static
//! [`StorageName::machine_local`]: flui_platform::StorageName::machine_local
//! [`WriteMode::IfUnchanged`]: flui_platform::WriteMode::IfUnchanged

use std::path::PathBuf;

use flui_platform::AppDataDir;

#[cfg(not(target_arch = "wasm32"))]
mod file_store;
#[cfg(not(target_arch = "wasm32"))]
mod os_error;
#[cfg(target_arch = "wasm32")]
mod page_store;

mod headless;
mod memory_store;
// Every host but Android and headless keeps its data there.
#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
    all(feature = "winit-backend", target_os = "linux")
))]
mod user_dirs;

#[cfg(not(target_arch = "wasm32"))]
pub use file_store::FileStore;
pub(crate) use headless::HeadlessData;
pub(crate) use memory_store::MemoryStore;
#[cfg(target_arch = "wasm32")]
pub use page_store::FileStore;
#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
    all(feature = "winit-backend", target_os = "linux")
))]
pub(crate) use user_dirs::UserDataDirs;

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
/// `~/Library/Application Support` on macOS and inside the iOS app container;
/// `$XDG_DATA_HOME` on Linux, where both are the same directory). `None` when
/// the platform reports none. On wasm32 both roots are `app`, the prefix of
/// the page store's keys.
#[must_use]
pub fn data_dirs(app: &AppDataDir) -> Option<DataDirs> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(DataDirs {
            roaming: dirs::data_dir()?.join(app.as_str()),
            local: dirs::data_local_dir()?.join(app.as_str()),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        Some(DataDirs {
            roaming: PathBuf::from(app.as_str()),
            local: PathBuf::from(app.as_str()),
        })
    }
}

/// Where a platform keeps an application's values: a [`FileStore`] under
/// its data roots, or values in memory on a host with no disk of its own to
/// write to (headless, unless a test gives it a root).
///
/// Its calls are the [`FileStore`]'s: blocking on native targets, so the
/// host runs them on an IO thread, and futures on wasm32.
#[derive(Debug)]
pub struct DataStore {
    backing: Backing,
}

#[derive(Debug)]
enum Backing {
    Files(FileStore),
    Memory(MemoryStore),
}

impl DataStore {
    /// The values under `dirs`, one file each.
    #[must_use]
    pub fn on_disk(dirs: DataDirs) -> Self {
        Self {
            backing: Backing::Files(FileStore::new(dirs)),
        }
    }

    /// The values `store` keeps; clones of it see the same values.
    #[must_use]
    pub(crate) fn in_memory(store: MemoryStore) -> Self {
        Self {
            backing: Backing::Memory(store),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DataStore {
    /// [`FileStore::read`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::read`].
    pub fn read(
        &self,
        name: &flui_platform::StorageName,
        limit: u64,
    ) -> Result<flui_platform::Stored, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.read(name, limit),
            Backing::Memory(store) => store.read(name, limit),
        }
    }

    /// [`FileStore::write`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::write`].
    pub fn write(
        &self,
        name: &flui_platform::StorageName,
        bytes: &[u8],
        mode: flui_platform::WriteMode,
    ) -> Result<flui_platform::StoredVersion, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.write(name, bytes, mode),
            Backing::Memory(store) => store.write(name, bytes, mode),
        }
    }

    /// [`FileStore::copy_aside`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::copy_aside`].
    pub fn copy_aside(
        &self,
        name: &flui_platform::StorageName,
    ) -> Result<flui_platform::CopiedAside, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.copy_aside(name),
            Backing::Memory(store) => store.copy_aside(name),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl DataStore {
    /// [`FileStore::read`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::read`].
    pub async fn read(
        &self,
        name: &flui_platform::StorageName,
        limit: u64,
    ) -> Result<flui_platform::Stored, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.read(name, limit).await,
            Backing::Memory(store) => store.read(name, limit),
        }
    }

    /// [`FileStore::write`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::write`].
    pub async fn write(
        &self,
        name: &flui_platform::StorageName,
        bytes: &[u8],
        mode: flui_platform::WriteMode,
    ) -> Result<flui_platform::StoredVersion, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.write(name, bytes, mode).await,
            Backing::Memory(store) => store.write(name, bytes, mode),
        }
    }

    /// [`FileStore::copy_aside`].
    ///
    /// # Errors
    ///
    /// As [`FileStore::copy_aside`].
    pub async fn copy_aside(
        &self,
        name: &flui_platform::StorageName,
    ) -> Result<flui_platform::CopiedAside, flui_platform::StorageError> {
        match &self.backing {
            Backing::Files(store) => store.copy_aside(name).await,
            Backing::Memory(store) => store.copy_aside(name),
        }
    }
}
