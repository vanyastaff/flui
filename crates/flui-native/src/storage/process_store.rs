//! A store under the process's own data folders, found at its first call:
//! the container is read then (`/.flatpak-info`, the package identity), and
//! a packaged application's store from before it was packaged is copied
//! into the package's folders once.
//!
//! Finding blocks the thread that finds (the file system, and a migration
//! that waits for another instance's lock). One find runs at a time, and
//! every call that arrives while it runs waits for it and takes its outcome,
//! whatever a later find answers: the store it found, or its refusal. The
//! next call after a refusal finds again. Once a store is found, a call
//! takes no lock.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

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
    /// What a find answered, kept for good: read with no lock.
    found: OnceLock<Found>,
    /// Whether a find runs, and which.
    state: Mutex<FindState>,
    /// Signalled when a find ends, for the calls that wait for it.
    ended: Condvar,
}

/// What a find that answered keeps.
#[derive(Debug)]
enum Found {
    /// The store.
    Store(Arc<FileStore>),
    /// The platform names no data folder: every call is refused.
    NoDataFolder,
}

impl Found {
    fn answer(&self) -> Result<Arc<FileStore>, StorageError> {
        match self {
            Self::Store(store) => Ok(Arc::clone(store)),
            Self::NoDataFolder => Err(StorageError::Inaccessible {
                kind: std::io::ErrorKind::NotFound,
            }),
        }
    }
}

/// Whether a find runs.
#[derive(Debug, Default)]
enum FindState {
    /// None runs: the next call that finds no store finds.
    #[default]
    Idle,
    /// This find runs; the calls that wait for it hold it too.
    Running(Arc<Find>),
}

/// One find, and how it ended once it has.
#[derive(Debug, Default)]
struct Find {
    ended: OnceLock<Ended>,
}

/// How a find ended.
#[derive(Clone, Copy, Debug)]
enum Ended {
    /// It answered, and [`ProcessStore::found`] holds the answer.
    Answered,
    /// It failed with this kind of error: the refusal of every call that
    /// waited for it.
    Failed(std::io::ErrorKind),
    /// It panicked: the calls that waited for it look again.
    Panicked,
}

impl std::fmt::Debug for ProcessStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessStore")
            .field("app", &self.app)
            .field("found", &self.found)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// Ends a find on every exit: a find that returned has its end set first,
/// and one that panicked ends `Panicked`; the state goes back to idle and
/// the waiting calls wake.
struct Finding<'a> {
    store: &'a ProcessStore,
    find: Arc<Find>,
}

impl Drop for Finding<'_> {
    fn drop(&mut self) {
        let _ = self.find.ended.set(Ended::Panicked);
        let mut state = self.store.lock();
        if matches!(&*state, FindState::Running(running) if Arc::ptr_eq(running, &self.find)) {
            *state = FindState::Idle;
        }
        drop(state);
        self.store.ended.notify_all();
    }
}

impl ProcessStore {
    /// A store of `app` that `find` finds at its first call.
    pub(super) fn new(app: AppDataDir, find: FindStore) -> Self {
        Self {
            app,
            find,
            found: OnceLock::new(),
            state: Mutex::new(FindState::Idle),
            ended: Condvar::new(),
        }
    }

    /// The state, whatever a panic elsewhere left: every write to it keeps
    /// it whole.
    fn lock(&self) -> MutexGuard<'_, FindState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store, found at the first call that could read the container.
    /// A call that arrives while a find runs waits for it and takes its
    /// outcome; a refusal (`StorageError::Inaccessible` with the read's
    /// error) answers every call that waited for that find, and the next
    /// call finds again. A platform that names no data folder refuses every
    /// call.
    pub(super) fn store(&self) -> Result<Arc<FileStore>, StorageError> {
        if let Some(found) = self.found.get() {
            return found.answer();
        }
        let mut state = self.lock();
        loop {
            if let Some(found) = self.found.get() {
                return found.answer();
            }
            if let FindState::Running(running) = &*state {
                let find = Arc::clone(running);
                state = self
                    .ended
                    .wait_while(state, |_| find.ended.get().is_none())
                    .unwrap_or_else(PoisonError::into_inner);
                // The outcome of the find this call waited for, whatever a
                // later find has answered since.
                match find.ended.get() {
                    Some(Ended::Failed(kind)) => {
                        return Err(StorageError::Inaccessible { kind: *kind });
                    }
                    Some(Ended::Answered) => {
                        if let Some(found) = self.found.get() {
                            return found.answer();
                        }
                    }
                    // It panicked: look again.
                    Some(Ended::Panicked) | None => {}
                }
                continue;
            }
            let find = Arc::new(Find::default());
            *state = FindState::Running(Arc::clone(&find));
            drop(state);
            let finding = Finding {
                store: self,
                find: Arc::clone(&find),
            };
            let answer = match (self.find)(&self.app) {
                Ok(found) => {
                    let found =
                        found.map_or(Found::NoDataFolder, |store| Found::Store(Arc::new(store)));
                    let answer = self.found.get_or_init(|| found).answer();
                    let _ = find.ended.set(Ended::Answered);
                    answer
                }
                Err(error) => {
                    let _ = find.ended.set(Ended::Failed(error.kind()));
                    Err(StorageError::Inaccessible { kind: error.kind() })
                }
            };
            drop(finding);
            return answer;
        }
    }

    /// How many calls wait for the find under way: the holders of its
    /// handle besides the state and the call that runs it.
    #[cfg(test)]
    fn waiting(&self) -> usize {
        match &*self.lock() {
            FindState::Running(find) => Arc::strong_count(find).saturating_sub(3),
            FindState::Idle => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_platform::{AppDataDir, StorageError};

    use super::{Ended, Find, FindState, Found, ProcessStore};
    use crate::storage::{DataDirs, FileStore};

    fn dirs(scratch: &tempfile::TempDir) -> DataDirs {
        DataDirs {
            roaming: scratch.path().join("roaming"),
            local: scratch.path().join("local"),
        }
    }

    /// Wait until `count` calls wait for the find under way.
    fn until_waiting(store: &ProcessStore, count: usize) {
        let since = std::time::Instant::now();
        while store.waiting() < count {
            assert!(
                since.elapsed() < std::time::Duration::from_secs(10),
                "{count} calls wait for the find"
            );
            std::thread::yield_now();
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

    /// A first find that stalls until `release` is sent, then answers
    /// `first`; every later find finds the store under `scratch`.
    fn stalling_store(
        scratch: &tempfile::TempDir,
        first: fn() -> std::io::Result<()>,
    ) -> (
        Arc<ProcessStore>,
        std::sync::mpsc::Sender<()>,
        std::sync::mpsc::Receiver<()>,
        Arc<AtomicUsize>,
    ) {
        let dirs = dirs(scratch);
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
                    first()?;
                }
                Ok(Some(FileStore::new(dirs.clone())))
            }),
        ));
        (store, release, started, finds)
    }

    fn call(store: &Arc<ProcessStore>) -> std::thread::JoinHandle<Result<(), StorageError>> {
        let store = Arc::clone(store);
        std::thread::spawn(move || store.store().map(drop))
    }

    /// Three calls on one find that fails: one find, three refusals; the
    /// call after them finds again.
    #[test]
    fn the_calls_that_waited_for_a_failed_find_share_its_refusal() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let (store, release, started, finds) = stalling_store(&scratch, || {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        let finder = call(&store);
        started.recv().expect("the first call finds");
        let waiters = [call(&store), call(&store)];
        until_waiting(&store, 2);
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

    /// A find panics while two calls wait for it: one of them runs a new
    /// find, and both return with the store it found.
    #[test]
    fn the_calls_that_waited_for_a_find_that_panicked_find_again() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let (store, release, started, finds) =
            stalling_store(&scratch, || panic!("the find panics"));
        let finder = call(&store);
        started.recv().expect("the first call finds");
        let (answered, answers) = std::sync::mpsc::channel();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let answered = answered.clone();
            std::thread::spawn(move || {
                let _ = answered.send(store.store().map(drop));
            });
        }
        until_waiting(&store, 2);
        release.send(()).expect("the find waits");
        assert!(
            finder.join().is_err(),
            "the panic reaches the call that found"
        );
        for _ in 0..2 {
            assert_eq!(
                answers
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .expect("a waiter returns, not left waiting on the find that panicked"),
                Ok(()),
                "a waiter finds again, and both answer from its store"
            );
        }
        assert_eq!(finds.load(Ordering::Relaxed), 2, "one new find for the two");
    }

    /// A call waits for a find that fails; before it wakes, a later find
    /// starts and answers a store. The call takes the refusal of the find
    /// it waited for, not the later find's store.
    #[test]
    fn a_call_takes_the_outcome_of_the_find_it_waited_for() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let dirs = dirs(&scratch);
        let store = Arc::new(ProcessStore::new(
            AppDataDir::try_new("app").expect("a name"),
            Box::new(|_| unreachable!("the test runs the finds")),
        ));
        let first = Arc::new(Find::default());
        *store.lock() = FindState::Running(Arc::clone(&first));
        let waiter = call(&store);
        // The state's handle, this test's and the waiter's.
        let since = std::time::Instant::now();
        while Arc::strong_count(&first) < 3 {
            assert!(since.elapsed() < std::time::Duration::from_secs(10));
            std::thread::yield_now();
        }
        {
            // The first find fails and a later one answers a store, all
            // before the waiter takes the lock again.
            let mut state = store.lock();
            let _ = first
                .ended
                .set(Ended::Failed(std::io::ErrorKind::PermissionDenied));
            let later = Arc::new(Find::default());
            *state = FindState::Running(Arc::clone(&later));
            let _ = store
                .found
                .set(Found::Store(Arc::new(FileStore::new(dirs))));
            let _ = later.ended.set(Ended::Answered);
            *state = FindState::Idle;
        }
        store.ended.notify_all();
        assert_eq!(
            waiter.join().expect("the call returned"),
            Err(StorageError::Inaccessible {
                kind: std::io::ErrorKind::PermissionDenied
            }),
            "the refusal of the find it waited for"
        );
    }
}
