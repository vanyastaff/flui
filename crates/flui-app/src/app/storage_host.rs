//! The byte storage the host hands every UI runtime it builds: the
//! platform's [`FileStore`] under the configured data roots, its blocking
//! calls run on the host's IO pool.
//!
//! A read is one IO task. Writes go through one lane per name: [`publish`]
//! accepts the bytes at once, and the lane's single IO task writes the
//! latest accepted value of that name, then the next, until none is left.
//! A value accepted while an older one of the same name has not started
//! replaces it, and the older request resolves to
//! [`StorageError::Superseded`]. Accepted work is never dropped silently: if
//! the IO pool refuses the task, or shuts down before running it, every
//! value it held resolves to the refusal.
//!
//! [`publish`]: Storage::publish

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Weak};

use flui_platform::storage::{DataDirs, FileStore};
use flui_platform_api::{
    Storage, StorageError, StorageFuture, StorageName, Stored, StoredVersion, WriteMode,
};
use flui_runtime::execution::{ExecutionServices, IoFuture, SpawnError};
use parking_lot::Mutex;
use tokio::sync::oneshot;

/// The host's storage: `FileStore` under `dirs`, run on `io`.
///
/// Holds the IO pool weakly: once the host's execution services are gone,
/// every new request resolves to [`StorageError::Cancelled`].
pub(crate) fn host_storage(dirs: DataDirs, io: Weak<ExecutionServices>) -> Arc<dyn Storage> {
    Arc::new(HostStorage {
        store: Arc::new(FileStore::new(dirs)),
        io,
        lanes: Arc::default(),
    })
}

struct HostStorage {
    store: Arc<FileStore>,
    io: Weak<ExecutionServices>,
    lanes: Arc<Mutex<HashMap<StorageName, Lane>>>,
}

/// A name whose writer task is queued or running. Present exactly while that
/// task owns the name; `next` is the latest value accepted and not started.
struct Lane {
    next: Option<Write>,
}

struct Write {
    bytes: Vec<u8>,
    mode: WriteMode,
    reply: oneshot::Sender<Result<StoredVersion, StorageError>>,
}

impl HostStorage {
    fn spawn(&self, task: IoFuture) -> Result<(), StorageError> {
        let io = self.io.upgrade().ok_or(StorageError::Cancelled)?;
        io.spawn_io(task).map_err(|error| match error {
            SpawnError::ShuttingDown => StorageError::Cancelled,
            SpawnError::Saturated => StorageError::Busy,
            _ => StorageError::Unavailable,
        })
    }
}

impl Storage for HostStorage {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored> {
        let (reply, answer) = oneshot::channel();
        let store = Arc::clone(&self.store);
        let name = *name;
        let task: IoFuture = Box::pin(async move {
            let _ = reply.send(store.read(&name, limit));
        });
        match self.spawn(task) {
            Ok(()) => Box::pin(async move { answer.await.unwrap_or(Err(StorageError::Cancelled)) }),
            Err(error) => Box::pin(std::future::ready(Err(error))),
        }
    }

    fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: WriteMode,
    ) -> StorageFuture<StoredVersion> {
        let (reply, answer) = oneshot::channel();
        let write = Write { bytes, mode, reply };
        let (superseded, start) = {
            let mut lanes = self.lanes.lock();
            match lanes.entry(*name) {
                Entry::Occupied(mut lane) => (lane.get_mut().next.replace(write), false),
                Entry::Vacant(slot) => {
                    slot.insert(Lane { next: Some(write) });
                    (None, true)
                }
            }
        };
        // Replies wake their callers; send them with the lanes unlocked.
        if let Some(older) = superseded {
            let _ = older.reply.send(Err(StorageError::Superseded));
        }
        if start {
            let writer = LaneWriter {
                store: Arc::clone(&self.store),
                lanes: Arc::clone(&self.lanes),
                name: *name,
                finished: false,
            };
            // A refused task is dropped unpolled, and its writer fails the
            // value it would have written.
            if let Err(error) = self.spawn(Box::pin(async move { writer.run() })) {
                return Box::pin(std::future::ready(Err(error)));
            }
        }
        Box::pin(async move { answer.await.unwrap_or(Err(StorageError::Cancelled)) })
    }
}

/// The task that writes one name's lane. Built before it is spawned, so a
/// task the pool refuses or cancels unpolled still releases the lane through
/// its `Drop`.
struct LaneWriter {
    store: Arc<FileStore>,
    lanes: Arc<Mutex<HashMap<StorageName, Lane>>>,
    name: StorageName,
    finished: bool,
}

impl LaneWriter {
    fn run(mut self) {
        while let Some(write) = self.take_next() {
            let result = self.store.write(&self.name, &write.bytes, write.mode);
            let _ = write.reply.send(result);
        }
    }

    /// The next value to write, or `None` after releasing the lane: a later
    /// publish then starts a new writer.
    fn take_next(&mut self) -> Option<Write> {
        let mut lanes = self.lanes.lock();
        let next = lanes.get_mut(&self.name).and_then(|lane| lane.next.take());
        if next.is_none() {
            lanes.remove(&self.name);
            self.finished = true;
        }
        next
    }
}

impl Drop for LaneWriter {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Refused or cancelled before the lane emptied: release it, and fail
        // the value it still held rather than leave its request waiting.
        let orphan = self
            .lanes
            .lock()
            .remove(&self.name)
            .and_then(|lane| lane.next);
        if let Some(write) = orphan {
            let _ = write.reply.send(Err(StorageError::Cancelled));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::future::Future;
    use std::sync::Arc;

    use flui_platform::storage::DataDirs;
    use flui_platform_api::{Storage, StorageError, StorageName, Stored, StoredVersion, WriteMode};
    use flui_runtime::execution::{DeterministicExecutors, ExecutionServices};

    const NOTES: StorageName = StorageName::from_static("notes");
    const SESSION: StorageName = StorageName::machine_local("notes");

    /// A host storage under fresh roots, its IO run only when the test says.
    struct Fixture {
        roots: tempfile::TempDir,
        io: DeterministicExecutors,
        services: Arc<ExecutionServices>,
        storage: Arc<dyn Storage>,
    }

    impl Fixture {
        fn new() -> Self {
            let roots = tempfile::tempdir().expect("temporary data root");
            let io = DeterministicExecutors::new();
            let services = Arc::new(ExecutionServices::with_host(io.host_executors()));
            let storage = super::host_storage(
                DataDirs {
                    roaming: roots.path().join("roaming"),
                    local: roots.path().join("local"),
                },
                Arc::downgrade(&services),
            );
            Self {
                roots,
                io,
                services,
                storage,
            }
        }

        /// Run the queued IO, then take the request's answer.
        fn settle<T>(&self, request: impl Future<Output = T>) -> T {
            self.io.run_until_idle();
            pollster::block_on(request)
        }
    }

    fn a_write_lands_under_the_roaming_root_and_reads_back() {
        let fixture = Fixture::new();
        let written = fixture
            .storage
            .publish(&NOTES, b"first".to_vec(), WriteMode::Replace);
        let version = fixture.settle(written).expect("write");
        assert_eq!(version, StoredVersion::of_bytes(b"first"));
        assert_eq!(
            std::fs::read(fixture.roots.path().join("roaming").join("notes")).expect("file"),
            b"first"
        );
        let read = fixture.storage.read(&NOTES, 1024);
        assert_eq!(
            fixture.settle(read).expect("read"),
            Stored {
                bytes: Some(b"first".to_vec()),
                version,
            }
        );
    }

    fn a_machine_local_value_lands_under_the_local_root() {
        let fixture = Fixture::new();
        let written = fixture
            .storage
            .publish(&SESSION, b"open".to_vec(), WriteMode::Replace);
        fixture.settle(written).expect("write");
        assert!(
            fixture
                .roots
                .path()
                .join("local")
                .join(".machine-local")
                .join("notes")
                .is_file(),
            "the session value is under the local root"
        );
        assert!(
            !fixture.roots.path().join("roaming").join("notes").exists(),
            "and not under the roaming one"
        );
    }

    fn a_newer_value_supersedes_one_not_yet_started() {
        let fixture = Fixture::new();
        let older = fixture
            .storage
            .publish(&NOTES, b"older".to_vec(), WriteMode::Replace);
        let newer = fixture
            .storage
            .publish(&NOTES, b"newer".to_vec(), WriteMode::Replace);
        fixture.io.run_until_idle();
        assert_eq!(
            pollster::block_on(older),
            Err(StorageError::Superseded),
            "the value never started is superseded"
        );
        assert_eq!(
            pollster::block_on(newer),
            Ok(StoredVersion::of_bytes(b"newer"))
        );
        assert_eq!(
            std::fs::read(fixture.roots.path().join("roaming").join("notes")).expect("file"),
            b"newer"
        );
    }

    fn a_drained_lane_takes_the_next_write() {
        let fixture = Fixture::new();
        let first = fixture
            .storage
            .publish(&NOTES, b"one".to_vec(), WriteMode::Replace);
        fixture.settle(first).expect("first write");
        let second = fixture
            .storage
            .publish(&NOTES, b"two".to_vec(), WriteMode::Replace);
        assert_eq!(
            fixture.settle(second),
            Ok(StoredVersion::of_bytes(b"two")),
            "a write after the lane drained starts a new writer"
        );
    }

    fn a_shut_down_pool_fails_accepted_writes_instead_of_losing_them() {
        let fixture = Fixture::new();
        let accepted = fixture
            .storage
            .publish(&NOTES, b"late".to_vec(), WriteMode::Replace);
        fixture.services.shutdown(std::time::Duration::ZERO);
        fixture.io.run_until_idle();
        assert_eq!(
            pollster::block_on(accepted),
            Err(StorageError::Cancelled),
            "a write the pool dropped resolves instead of hanging"
        );
        let refused = fixture
            .storage
            .publish(&NOTES, b"later".to_vec(), WriteMode::Replace);
        assert_eq!(pollster::block_on(refused), Err(StorageError::Cancelled));
        assert!(!fixture.roots.path().join("roaming").join("notes").exists());
    }

    fn a_storage_outliving_its_host_refuses_requests() {
        let fixture = Fixture::new();
        let Fixture {
            roots: _roots,
            storage,
            services,
            ..
        } = fixture;
        drop(services);
        assert_eq!(
            pollster::block_on(storage.read(&NOTES, 1024)),
            Err(StorageError::Cancelled)
        );
    }

    #[test]
    fn host_storage_matrix() {
        crate::table_test::run_table(
            "host_storage_matrix",
            &[
                (
                    "a_write_lands_under_the_roaming_root_and_reads_back",
                    a_write_lands_under_the_roaming_root_and_reads_back,
                ),
                (
                    "a_machine_local_value_lands_under_the_local_root",
                    a_machine_local_value_lands_under_the_local_root,
                ),
                (
                    "a_newer_value_supersedes_one_not_yet_started",
                    a_newer_value_supersedes_one_not_yet_started,
                ),
                (
                    "a_drained_lane_takes_the_next_write",
                    a_drained_lane_takes_the_next_write,
                ),
                (
                    "a_shut_down_pool_fails_accepted_writes_instead_of_losing_them",
                    a_shut_down_pool_fails_accepted_writes_instead_of_losing_them,
                ),
                (
                    "a_storage_outliving_its_host_refuses_requests",
                    a_storage_outliving_its_host_refuses_requests,
                ),
            ],
        );
    }
}
