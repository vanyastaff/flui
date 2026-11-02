//! The byte storage the host hands every UI runtime it builds: the
//! platform's [`FileStore`] under the configured data roots, its calls run
//! on the host's IO pool: blocking on native targets, awaited on wasm32,
//! where the page's IndexedDB answers in a later task.
//!
//! A read and a copy aside are one IO task each. Writes go through one lane
//! per name: [`publish`] accepts the bytes at once, and the lane's single IO
//! task writes the latest accepted value of that name, then the next, until
//! none is left. A value accepted while an older one of the same name has
//! not started replaces it, and the older request resolves to
//! [`StorageError::Superseded`]. Accepted work is never dropped silently: if
//! the IO pool refuses the task, or shuts down before running it, every
//! value it held resolves to the refusal.
//!
//! [`publish`]: Storage::publish

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Weak};

use flui_native::storage::{DataDirs, FileStore};
use flui_platform::{
    CopiedAside, Storage, StorageError, StorageFuture, StorageName, Stored, StoredVersion,
    Unsupported, UnsupportedReason, WriteMode,
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

/// What the store answers to `$call`: at once on native targets, where the
/// call blocks this IO task's thread, and awaited on wasm32.
macro_rules! answer {
    ($call:expr) => {{
        #[cfg(not(target_arch = "wasm32"))]
        let answer = $call;
        #[cfg(target_arch = "wasm32")]
        let answer = $call.await;
        answer
    }};
}

/// Whether the host's IO pool still admits work: alive and not shutting down.
fn admitting(io: &Weak<ExecutionServices>) -> bool {
    io.upgrade().is_some_and(|services| services.is_accepting())
}

impl HostStorage {
    fn spawn(&self, task: IoFuture) -> Result<(), StorageError> {
        let io = self.io.upgrade().ok_or(StorageError::Cancelled)?;
        io.spawn_io(task).map_err(|error| match error {
            SpawnError::ShuttingDown => StorageError::Cancelled,
            SpawnError::Saturated => StorageError::Busy,
            // The pool could not start; a later request may.
            SpawnError::Unavailable | _ => Unsupported::new(UnsupportedReason::Unavailable).into(),
        })
    }
}

impl Storage for HostStorage {
    fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored> {
        let (reply, answer) = oneshot::channel();
        let store = Arc::clone(&self.store);
        let name = *name;
        let task: IoFuture = Box::pin(async move {
            let _ = reply.send(answer!(store.read(&name, limit)));
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
        // A lane already draining would otherwise take this value after the
        // pool stopped admitting work.
        if !admitting(&self.io) {
            return Box::pin(std::future::ready(Err(StorageError::Cancelled)));
        }
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
            let handoff = Arc::new(Mutex::new(Handoff::Pending));
            let writer = LaneWriter {
                store: Arc::clone(&self.store),
                lanes: Arc::clone(&self.lanes),
                io: Weak::clone(&self.io),
                name: *name,
                handoff: Arc::clone(&handoff),
                finished: false,
            };
            let spawned = self.spawn(Box::pin(writer.run()));
            // A refused task was dropped unpolled while the handoff was
            // pending, so its writer left the lane to us: fail what the lane
            // holds now (this value, or a newer one that superseded it) with
            // the refusal itself.
            let release = {
                let mut state = handoff.lock();
                match (spawned, *state) {
                    (Ok(()), Handoff::Pending) => {
                        *state = Handoff::Taken;
                        None
                    }
                    // The pool dropped the task before we could hand it over.
                    (Ok(()), _) => Some(StorageError::Cancelled),
                    (Err(error), _) => Some(error),
                }
            };
            if let Some(error) = release {
                release_lane(&self.lanes, name, error);
            }
        }
        // This request's own answer: its write's result, `Superseded` if a
        // newer value replaced it, or the refusal that released its lane.
        Box::pin(async move { answer.await.unwrap_or(Err(StorageError::Cancelled)) })
    }

    fn copy_aside(&self, name: &StorageName) -> StorageFuture<CopiedAside> {
        let (reply, answer) = oneshot::channel();
        let store = Arc::clone(&self.store);
        let name = *name;
        let task: IoFuture = Box::pin(async move {
            let _ = reply.send(answer!(store.copy_aside(&name)));
        });
        match self.spawn(task) {
            Ok(()) => Box::pin(async move { answer.await.unwrap_or(Err(StorageError::Cancelled)) }),
            Err(error) => Box::pin(std::future::ready(Err(error))),
        }
    }
}

/// Who releases a lane whose writer never ran: the publisher, while it has
/// not handed the writer to the pool, and the writer's `Drop` after.
#[derive(Clone, Copy)]
enum Handoff {
    /// `spawn` has not returned yet.
    Pending,
    /// The pool took the task; a later drop is a cancellation.
    Taken,
    /// The task was dropped before the publisher learned the pool took it.
    DroppedUnclaimed,
}

/// Remove `name`'s lane and fail the value it still holds with `error`.
fn release_lane(
    lanes: &Mutex<HashMap<StorageName, Lane>>,
    name: &StorageName,
    error: StorageError,
) {
    let orphan = lanes.lock().remove(name).and_then(|lane| lane.next);
    if let Some(write) = orphan {
        let _ = write.reply.send(Err(error));
    }
}

/// The task that writes one name's lane. Built before it is spawned; a task
/// dropped before it empties its lane releases it through its `Drop`, or
/// leaves that to the publisher while the handoff is pending (see
/// [`Handoff`]).
struct LaneWriter {
    store: Arc<FileStore>,
    lanes: Arc<Mutex<HashMap<StorageName, Lane>>>,
    io: Weak<ExecutionServices>,
    name: StorageName,
    handoff: Arc<Mutex<Handoff>>,
    finished: bool,
}

impl LaneWriter {
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            clippy::unused_async,
            reason = "the wasm32 store's writes are awaited; native ones block"
        )
    )]
    async fn run(mut self) {
        while let Some(write) = self.take_next() {
            // Shutdown can begin while this lane drains: what it accepted
            // before then resolves `Cancelled` rather than writing after it.
            let result = if admitting(&self.io) {
                answer!(self.store.write(&self.name, &write.bytes, write.mode))
            } else {
                Err(StorageError::Cancelled)
            };
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
        {
            let mut handoff = self.handoff.lock();
            if matches!(*handoff, Handoff::Pending) {
                // The publisher has not seen `spawn` return: it releases the
                // lane with the pool's actual answer.
                *handoff = Handoff::DroppedUnclaimed;
                return;
            }
        }
        // Cancelled after the handoff, before the lane emptied: release it,
        // and fail the value it still held rather than leave it waiting.
        release_lane(&self.lanes, &self.name, StorageError::Cancelled);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::future::Future;
    use std::sync::Arc;

    use flui_native::storage::DataDirs;
    use flui_platform::{Storage, StorageError, StorageName, Stored, StoredVersion, WriteMode};
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
        fixture.services.shutdown(std::time::Instant::now());
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

    fn a_publish_after_shutdown_neither_writes_nor_supersedes() {
        let fixture = Fixture::new();
        let accepted = fixture
            .storage
            .publish(&NOTES, b"accepted".to_vec(), WriteMode::Replace);
        fixture.services.shutdown(std::time::Instant::now());
        let late = fixture
            .storage
            .publish(&NOTES, b"late".to_vec(), WriteMode::Replace);
        fixture.io.run_until_idle();
        assert_eq!(
            pollster::block_on(late),
            Err(StorageError::Cancelled),
            "a lane still holding a value does not take one after shutdown"
        );
        assert_eq!(
            pollster::block_on(accepted),
            Err(StorageError::Cancelled),
            "the accepted value is cancelled, not superseded by a refused one"
        );
        assert!(!fixture.roots.path().join("roaming").join("notes").exists());
    }

    /// A pool that, while refusing the lane's task, lets a second publish of
    /// the same name in: the race of two threads publishing before the first
    /// reaches `spawn`.
    fn a_refused_lane_gives_each_request_its_own_answer() {
        use flui_platform::StorageFuture;
        use flui_runtime::execution::{
            ComputeJob, HostComputePool, HostExecutors, HostIoPool, IoFuture, SpawnError,
        };
        use parking_lot::Mutex;

        type During = Box<dyn FnOnce() + Send>;
        struct Inline;
        impl HostComputePool for Inline {
            fn spawn_job(&self, job: ComputeJob) -> Result<(), SpawnError> {
                job();
                Ok(())
            }
        }
        struct Refusing(Mutex<Option<During>>);
        impl HostIoPool for Refusing {
            fn spawn_future(&self, future: IoFuture) -> Result<(), SpawnError> {
                let during = self.0.lock().take();
                if let Some(during) = during {
                    during();
                }
                drop(future);
                Err(SpawnError::Saturated)
            }
        }

        let roots = tempfile::tempdir().expect("temporary data root");
        let pool = Arc::new(Refusing(Mutex::new(None)));
        let services = Arc::new(ExecutionServices::with_host(HostExecutors::new(
            Arc::new(Inline),
            Arc::clone(&pool) as Arc<dyn HostIoPool>,
        )));
        let storage = super::host_storage(
            DataDirs {
                roaming: roots.path().join("roaming"),
                local: roots.path().join("local"),
            },
            Arc::downgrade(&services),
        );
        let second: Arc<Mutex<Option<StorageFuture<StoredVersion>>>> = Arc::default();
        {
            let storage = Arc::clone(&storage);
            let second = Arc::clone(&second);
            *pool.0.lock() = Some(Box::new(move || {
                *second.lock() =
                    Some(storage.publish(&NOTES, b"newer".to_vec(), WriteMode::Replace));
            }));
        }
        let first = storage.publish(&NOTES, b"older".to_vec(), WriteMode::Replace);
        let second = second
            .lock()
            .take()
            .expect("the pool published a second value");
        assert_eq!(
            pollster::block_on(first),
            Err(StorageError::Superseded),
            "the first value was replaced before the pool refused"
        );
        assert_eq!(
            pollster::block_on(second),
            Err(StorageError::Busy),
            "the value the lane held gets the pool's refusal, not a cancellation"
        );
        drop(services);
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
                    "a_publish_after_shutdown_neither_writes_nor_supersedes",
                    a_publish_after_shutdown_neither_writes_nor_supersedes,
                ),
                (
                    "a_refused_lane_gives_each_request_its_own_answer",
                    a_refused_lane_gives_each_request_its_own_answer,
                ),
                (
                    "a_storage_outliving_its_host_refuses_requests",
                    a_storage_outliving_its_host_refuses_requests,
                ),
            ],
        );
    }
}
