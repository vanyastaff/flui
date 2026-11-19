//! A store under the process's own data folders, found at its first call:
//! the container is read then (`/.flatpak-info`, the package identity), and
//! a packaged application's store from before it was packaged is copied
//! into the package's folders once.
//!
//! Finding blocks the thread that finds (the file system, and a migration
//! that waits for another instance's lock), so a host runs the store's calls
//! off the threads it shares with other work. One find runs at a time, and
//! every call that arrives while it runs waits for it and shares its
//! outcome: the store it found, or its refusal. The next call after a
//! refusal finds again.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use flui_platform::{AppDataDir, StorageError};

use super::FileStore;

/// What finds a [`ProcessStore`]'s store: `None` when the platform names no
/// data folder.
pub(super) type FindStore =
    Box<dyn Fn(&AppDataDir) -> std::io::Result<Option<FileStore>> + Send + Sync>;

/// The store of `app`, which `find` finds at the first call.
pub(super) struct ProcessStore {
    app: AppDataDir,
    /// [`super::process_store`], or a test's stand-in.
    find: FindStore,
    /// The one owner of the find's state.
    state: Mutex<FindState>,
    /// Signalled when a find ends, for the calls that wait for it.
    ended: Condvar,
}

/// What a find that answered keeps, for good.
#[derive(Debug, Default)]
enum Found {
    /// No find answered yet.
    #[default]
    Nothing,
    /// The store.
    Store(Arc<FileStore>),
    /// The platform names no data folder: every call is refused.
    NoDataFolder,
}

/// Where finding the store stands.
#[derive(Debug, Default)]
struct FindState {
    found: Found,
    /// The find under way, by its number.
    running: Option<u64>,
    /// The number the next find gets.
    next: u64,
    /// The last find that failed, by its number, and its error's kind: what
    /// the calls that waited for it answer.
    failed: Option<(u64, std::io::ErrorKind)>,
    /// How many calls wait for the find under way.
    waiting: usize,
}

impl std::fmt::Debug for ProcessStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessStore")
            .field("app", &self.app)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// Clears a find that panicked, so the calls waiting for it look again.
struct Running<'a> {
    store: &'a ProcessStore,
    find: u64,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut state = self.store.lock();
        if state.running == Some(self.find) {
            state.running = None;
            drop(state);
            self.store.ended.notify_all();
        }
    }
}

impl ProcessStore {
    /// A store of `app` that `find` finds at its first call.
    pub(super) fn new(app: AppDataDir, find: FindStore) -> Self {
        Self {
            app,
            find,
            state: Mutex::new(FindState::default()),
            ended: Condvar::new(),
        }
    }

    /// The state, whatever a panic elsewhere left: every write to it keeps
    /// it whole.
    fn lock(&self) -> MutexGuard<'_, FindState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store, found at the first call that could read the container.
    /// A call that arrives while a find runs waits for it and shares its
    /// outcome; a refusal (`StorageError::Inaccessible` with the read's
    /// error) answers every call that waited for that find, and the next
    /// call finds again. A platform that names no data folder refuses every
    /// call.
    pub(super) fn store(&self) -> Result<Arc<FileStore>, StorageError> {
        /// The refusal of a platform that names no data folder.
        const NO_DATA_FOLDER: StorageError = StorageError::Inaccessible {
            kind: std::io::ErrorKind::NotFound,
        };
        let mut state = self.lock();
        loop {
            match &state.found {
                Found::Store(store) => return Ok(Arc::clone(store)),
                Found::NoDataFolder => return Err(NO_DATA_FOLDER),
                Found::Nothing => {}
            }
            if let Some(find) = state.running {
                state.waiting += 1;
                state = self
                    .ended
                    .wait_while(state, |state| state.running == Some(find))
                    .unwrap_or_else(PoisonError::into_inner);
                state.waiting -= 1;
                if let Some((failed, kind)) = state.failed
                    && failed == find
                {
                    return Err(StorageError::Inaccessible { kind });
                }
                // Found, or the find panicked: look again.
                continue;
            }
            let find = state.next;
            state.next = find
                .checked_add(1)
                .expect("BUG: a store is found fewer than u64::MAX times");
            state.running = Some(find);
            drop(state);
            let running = Running { store: self, find };
            let result = (self.find)(&self.app);
            let mut state = self.lock();
            state.running = None;
            let answer = match result {
                Ok(Some(store)) => {
                    let store = Arc::new(store);
                    state.found = Found::Store(Arc::clone(&store));
                    Ok(store)
                }
                Ok(None) => {
                    state.found = Found::NoDataFolder;
                    Err(NO_DATA_FOLDER)
                }
                Err(error) => {
                    state.failed = Some((find, error.kind()));
                    Err(StorageError::Inaccessible { kind: error.kind() })
                }
            };
            drop(state);
            self.ended.notify_all();
            drop(running);
            return answer;
        }
    }

    /// How many calls wait for the find under way.
    #[cfg(test)]
    fn waiting(&self) -> usize {
        self.lock().waiting
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_platform::{AppDataDir, StorageError};

    use super::ProcessStore;
    use crate::storage::{DataDirs, FileStore};

    fn dirs(scratch: &tempfile::TempDir) -> DataDirs {
        DataDirs {
            roaming: scratch.path().join("roaming"),
            local: scratch.path().join("local"),
        }
    }

    #[test]
    fn a_container_that_could_not_be_read_is_read_again_at_the_next_call() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dirs = dirs(&scratch);
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

    /// Three calls on one find that fails: one find, three refusals; the
    /// call after them finds again.
    #[test]
    fn the_calls_that_waited_for_a_failed_find_share_its_refusal() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dirs = dirs(&scratch);
        let finds = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&finds);
        let (starting, started) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = std::sync::Mutex::new(released);
        let store = Arc::new(ProcessStore::new(
            AppDataDir::try_new("app").expect("a name"),
            Box::new(move |_| {
                if counting.fetch_add(1, Ordering::Relaxed) == 0 {
                    let _ = starting.send(());
                    let _ = released
                        .lock()
                        .expect("one find at a time")
                        .recv_timeout(std::time::Duration::from_secs(30));
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                } else {
                    Ok(Some(FileStore::new(dirs.clone())))
                }
            }),
        ));
        let call = |store: &Arc<ProcessStore>| {
            let store = Arc::clone(store);
            std::thread::spawn(move || store.store().map(drop))
        };
        let finder = call(&store);
        started.recv().expect("the first call finds");
        let waiters = [call(&store), call(&store)];
        let since = std::time::Instant::now();
        while store.waiting() < 2 {
            assert!(
                since.elapsed() < std::time::Duration::from_secs(10),
                "both calls wait for the find"
            );
            std::thread::yield_now();
        }
        release.send(()).expect("the find waits");
        for call in std::iter::once(finder).chain(waiters) {
            assert_eq!(
                call.join().expect("the call returned"),
                Err(StorageError::Inaccessible {
                    kind: std::io::ErrorKind::PermissionDenied
                }),
                "every call that waited answers the find's refusal"
            );
        }
        assert_eq!(finds.load(Ordering::Relaxed), 1, "one find for the three");
        assert!(store.store().is_ok(), "the next call finds again");
        assert_eq!(finds.load(Ordering::Relaxed), 2);
    }
}
