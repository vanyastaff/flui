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
mod container;
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
/// `<data dir>/<app>` and `<local data dir>/<app>`, in the process's
/// container when it runs in one.
///
/// The base directories are the platform's own (`%APPDATA%` and
/// `%LOCALAPPDATA%` on Windows, including a redirected or synced folder;
/// `~/Library/Application Support` on macOS and inside the iOS app container;
/// `$XDG_DATA_HOME` on Linux, where both are the same directory). A
/// sandboxed process is answered its container's, never a folder its
/// sandbox keeps it from opening: a Flatpak's `~/.var/app/<id>/data`, the
/// macOS App Sandbox's `~/Library/Containers/<id>/Data/Library/Application
/// Support`, and a packaged Windows application's
/// `%LOCALAPPDATA%\Packages\<family name>\RoamingState` and `LocalState`.
/// `None` when the platform reports none, or when a Flatpak's description
/// could not be read. On wasm32 both roots are `app`, the prefix of the page
/// store's keys.
///
/// Reads the file system (a Flatpak's `/.flatpak-info`): off the owner
/// thread only. A host's store resolves it on its IO thread
/// ([`DataStore::of_process`]).
#[must_use]
pub fn data_dirs(app: &AppDataDir) -> Option<DataDirs> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        container::data_dirs_in(
            &container::Container::of_process().ok()?,
            app.as_str(),
            &base_dirs(),
        )
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
    #[cfg(not(target_arch = "wasm32"))]
    OfProcess(ProcessStore),
}

/// The platform's data folders for the user, the container's for a
/// sandboxed or packaged process.
#[cfg(not(target_arch = "wasm32"))]
fn base_dirs() -> container::BaseDirs {
    container::BaseDirs {
        data: dirs::data_dir(),
        local: dirs::data_local_dir(),
    }
}

/// A store under the process's own data folders, found at its first call,
/// on the IO thread that makes it: the container is read then
/// (`/.flatpak-info`, the package identity), and a packaged application's
/// store from before it was packaged is copied into the package's folders
/// once.
#[cfg(not(target_arch = "wasm32"))]
struct ProcessStore {
    app: AppDataDir,
    /// How the store is found: [`process_store`], or a test's stand-in.
    find: FindStore,
    store: std::sync::OnceLock<Option<FileStore>>,
}

/// What finds a [`ProcessStore`]'s store, on the IO thread.
#[cfg(not(target_arch = "wasm32"))]
type FindStore = Box<dyn Fn(&AppDataDir) -> std::io::Result<Option<FileStore>> + Send + Sync>;

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for ProcessStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessStore")
            .field("app", &self.app)
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ProcessStore {
    /// A store of `app` that `find` finds at its first call.
    fn new(app: AppDataDir, find: FindStore) -> Self {
        Self {
            app,
            find,
            store: std::sync::OnceLock::new(),
        }
    }

    /// The store, found at the first call that could read the container;
    /// a platform that names no data folder leaves every call refused, and
    /// a container that could not be read refuses this call and is read
    /// again at the next.
    fn store(&self) -> Result<&FileStore, flui_platform::StorageError> {
        let found = if let Some(found) = self.store.get() {
            found
        } else {
            let found = (self.find)(&self.app).map_err(|error| {
                flui_platform::StorageError::Inaccessible { kind: error.kind() }
            })?;
            self.store.get_or_init(|| found)
        };
        found
            .as_ref()
            .ok_or(flui_platform::StorageError::Inaccessible {
                kind: std::io::ErrorKind::NotFound,
            })
    }
}

/// The store of `app` in the process's container; `None` when the platform
/// names no data folder. Blocks on the file system.
///
/// # Errors
///
/// The container could not be read (a Flatpak's description out of reach).
#[cfg(not(target_arch = "wasm32"))]
fn process_store(app: &AppDataDir) -> std::io::Result<Option<FileStore>> {
    let found = container::Container::of_process()?;
    let base = base_dirs();
    let Some(dirs) = container::data_dirs_in(&found, app.as_str(), &base) else {
        return Ok(None);
    };
    #[cfg(windows)]
    if matches!(found, container::Container::Package { .. }) {
        let before = container::Container::Sandbox(flui_platform::sandbox::Confinement::Unconfined);
        if let Some(earlier) = container::data_dirs_in(&before, app.as_str(), &base) {
            let dirs = container::roots_after_migration(earlier, dirs, &mut container::migrate);
            return Ok(Some(FileStore::new(dirs)));
        }
    }
    Ok(Some(FileStore::new(dirs)))
}

impl DataStore {
    /// The values under `dirs`, one file each.
    #[must_use]
    pub fn on_disk(dirs: DataDirs) -> Self {
        Self {
            backing: Backing::Files(FileStore::new(dirs)),
        }
    }

    /// The values of `app` under the process's own data folders
    /// ([`data_dirs`]), found at the store's first call, on the thread that
    /// makes it: the owner thread never reads the file system for them. A
    /// packaged Windows application's store kept under `%APPDATA%` before
    /// it was packaged is copied into the package's folders then, once:
    /// only into a root that holds no store, within a bound, nothing
    /// overwritten, one instance at a time (another waits for it). A store
    /// that was not copied stays where it is and stays the store, the calls
    /// reading and writing it there: one whose copy failed is copied again
    /// at a later start, one past the bound never. A package folder that
    /// could not be read stays the store. A platform that names no data
    /// folder refuses every call, and a container that could not be read
    /// refuses the call and is read again at the next
    /// (`StorageError::Inaccessible`).
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn of_process(app: AppDataDir) -> Self {
        Self {
            backing: Backing::OfProcess(ProcessStore::new(app, Box::new(process_store))),
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
            Backing::OfProcess(store) => store.store()?.read(name, limit),
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
            Backing::OfProcess(store) => store.store()?.write(name, bytes, mode),
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
            Backing::OfProcess(store) => store.store()?.copy_aside(name),
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_platform::{AppDataDir, StorageError};

    use super::{DataDirs, FileStore, ProcessStore};

    #[test]
    fn a_container_that_could_not_be_read_is_read_again_at_the_next_call() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dirs = DataDirs {
            roaming: scratch.path().join("roaming"),
            local: scratch.path().join("local"),
        };
        let asked = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&asked);
        let store = ProcessStore::new(
            AppDataDir::try_new("app").expect("a name"),
            Box::new(move |_| {
                if counting.fetch_add(1, Ordering::Relaxed) == 0 {
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                } else {
                    Ok(Some(FileStore::new(dirs.clone())))
                }
            }),
        );
        assert!(
            matches!(
                store.store(),
                Err(StorageError::Inaccessible {
                    kind: std::io::ErrorKind::PermissionDenied,
                    ..
                })
            ),
            "the first call is refused with the read's error"
        );
        assert!(
            store.store().is_ok(),
            "the next call reads the container again"
        );
        assert!(store.store().is_ok());
        assert_eq!(
            asked.load(Ordering::Relaxed),
            2,
            "a container read is kept once it answered"
        );
    }
}
