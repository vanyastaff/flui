//! The byte storage the host hands every UI runtime it builds: the
//! platform's [`DataStore`] for the configured data directory, its calls
//! driven by tasks on the host's IO pool. On native targets a call blocks,
//! so the task runs it on the IO runtime's blocking threads and awaits it,
//! keeping the pool's workers free; on wasm32 the page's IndexedDB answers
//! in a later task.
//!
//! A read and a copy aside are one IO task each. Writes go through one lane
//! per name: [`publish`] accepts the bytes at once, and the lane's single IO
//! task writes the latest accepted value of that name, then the next, until
//! none is left. A value accepted while an older one of the same name has
//! not started replaces it, and the older request resolves to
//! [`StorageError::Superseded`]. Accepted work is never dropped silently: if
//! the IO pool refuses the task, or shuts down before running it, every
//! value it held that had not started resolves to the refusal. A call that
//! started answers its own outcome, its reply carried with it, so a write
//! the pool's shutdown lets finish on its blocking thread answers that it
//! landed, never `Cancelled`.
//!
//! [`publish`]: Storage::publish

#![expect(
    clippy::disallowed_types,
    reason = "the lanes are keyed by the storage names an application chooses"
)]

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Weak};

use flui_native::storage::DataStore;
use flui_platform::{
    CopiedAside, Storage, StorageError, StorageFuture, StorageName, Stored, StoredVersion,
    Unsupported, UnsupportedReason, WriteMode,
};
use flui_runtime::execution::{ExecutionServices, IoFuture, SpawnError};
use parking_lot::Mutex;
use tokio::sync::oneshot;

/// The host's storage: `store`, its calls run on `io`.
///
/// Holds the IO pool weakly: once the host's execution services are gone,
/// every new request resolves to [`StorageError::Cancelled`].
pub(crate) fn host_storage(store: DataStore, io: Weak<ExecutionServices>) -> Arc<dyn Storage> {
    Arc::new(HostStorage {
        store: Arc::new(store),
        io,
        lanes: Arc::default(),
    })
}

struct HostStorage {
    store: Arc<DataStore>,
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

/// Send on `$reply` what `$store` (an `Arc<DataStore>`) answers to `$call`:
/// on native targets the blocking call runs off the IO pool's workers, its
/// reply with it ([`answer_off_io_workers`]), and this IO task awaits it; on
/// wasm32 the call is a future, awaited here.
macro_rules! reply_with {
    ($io:expr, $reply:expr, $store:ident => $call:expr) => {{
        #[cfg(not(target_arch = "wasm32"))]
        {
            let $store = Arc::clone(&$store);
            answer_off_io_workers($io, $reply, move || $call).await;
        }
        #[cfg(target_arch = "wasm32")]
        {
            // The page's store answers in a later task: no thread to leave.
            let _ = $io;
            let _ = $reply.send($call.await);
        }
    }};
}

/// Run the store's blocking `call` off the IO pool's workers and send its
/// answer on `reply` from where it ran: on the IO runtime's blocking
/// threads, so a call that waits (a store whose first call waits for
/// another instance's migration, a slow disk) holds none of the workers
/// HTTP, image loads and background work share.
///
/// The reply travels with the call: a call that started answers what it
/// did, even when this IO task is cancelled meanwhile (the pool shutting
/// down), and only a call that never started answers
/// [`StorageError::Cancelled`]: the pool already shutting down or gone, or
/// its runtime dropping the call before it ran. A host's own IO pool (an
/// embedder's, a test's deterministic one) has no blocking threads, and the
/// call runs in this task there. A panic in `call` drops its reply
/// (`Cancelled`) and resumes in this task.
#[cfg(not(target_arch = "wasm32"))]
async fn answer_off_io_workers<T: Send + 'static>(
    io: &Weak<ExecutionServices>,
    reply: oneshot::Sender<Result<T, StorageError>>,
    call: impl FnOnce() -> Result<T, StorageError> + Send + 'static,
) {
    let runtime = match io.upgrade().map(|services| services.io_runtime()) {
        Some(Ok(Some(runtime))) => runtime,
        Some(Ok(None)) => {
            let _ = reply.send(call());
            return;
        }
        // Shutting down, unavailable or gone: the call never starts.
        Some(Err(_)) | None => {
            let _ = reply.send(Err(StorageError::Cancelled));
            return;
        }
    };
    let running = runtime.spawn_blocking(move || {
        let _ = reply.send(call());
    });
    if let Err(failed) = running.await
        && let Ok(payload) = failed.try_into_panic()
    {
        std::panic::resume_unwind(payload);
    }
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
        let io = Weak::clone(&self.io);
        let name = *name;
        let task: IoFuture = Box::pin(async move {
            reply_with!(&io, reply, store => store.read(&name, limit));
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
        let io = Weak::clone(&self.io);
        let name = *name;
        let task: IoFuture = Box::pin(async move {
            reply_with!(&io, reply, store => store.copy_aside(&name));
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
    store: Arc<DataStore>,
    lanes: Arc<Mutex<HashMap<StorageName, Lane>>>,
    io: Weak<ExecutionServices>,
    name: StorageName,
    handoff: Arc<Mutex<Handoff>>,
    finished: bool,
}

impl LaneWriter {
    async fn run(mut self) {
        while let Some(Write { bytes, mode, reply }) = self.take_next() {
            // Shutdown can begin while this lane drains: what it accepted
            // before then resolves `Cancelled` rather than writing after it.
            // A write that started answers its own outcome, this task
            // cancelled or not.
            if admitting(&self.io) {
                let store = &self.store;
                let name = self.name;
                reply_with!(&self.io, reply, store => store.write(&name, &bytes, mode));
            } else {
                let _ = reply.send(Err(StorageError::Cancelled));
            }
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
        // and fail the value it still held, which never started, rather than
        // leave it waiting. A write under way when the task was cancelled
        // carries its own reply, and answers its outcome.
        release_lane(&self.lanes, &self.name, StorageError::Cancelled);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::future::Future;
    use std::sync::Arc;

    use flui_native::storage::{DataDirs, DataStore};
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
                DataStore::on_disk(DataDirs {
                    roaming: roots.path().join("roaming"),
                    local: roots.path().join("local"),
                }),
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

            fn room(&self) -> flui_runtime::execution::RoomFuture {
                flui_runtime::execution::RoomFuture::ready(Ok(()))
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

            fn room(&self) -> flui_runtime::execution::RoomFuture {
                flui_runtime::execution::RoomFuture::new(std::future::pending())
            }
        }

        let roots = tempfile::tempdir().expect("temporary data root");
        let pool = Arc::new(Refusing(Mutex::new(None)));
        let services = Arc::new(ExecutionServices::with_host(HostExecutors::new(
            Arc::new(Inline),
            Arc::clone(&pool) as Arc<dyn HostIoPool>,
        )));
        let storage = super::host_storage(
            DataStore::on_disk(DataDirs {
                roaming: roots.path().join("roaming"),
                local: roots.path().join("local"),
            }),
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
                (
                    "a_store_call_that_waits_holds_no_io_worker",
                    a_store_call_that_waits_holds_no_io_worker,
                ),
                (
                    "a_write_the_shutdown_lets_finish_answers_what_landed",
                    a_write_the_shutdown_lets_finish_answers_what_landed,
                ),
                (
                    "a_call_on_a_pool_shutting_down_never_starts",
                    a_call_on_a_pool_shutting_down_never_starts,
                ),
            ],
        );
    }

    /// A store whose find stalls (another instance holding the migration's
    /// lock) until the test sends on the returned `release`; `started`
    /// hears each find begin, and `finds` counts them.
    struct StalledStore {
        store: DataStore,
        release: std::sync::mpsc::Sender<()>,
        started: std::sync::mpsc::Receiver<()>,
        finds: Arc<std::sync::atomic::AtomicUsize>,
        /// The roaming root the find answers, where `NOTES` lands.
        roaming: std::path::PathBuf,
        _roots: tempfile::TempDir,
    }

    impl StalledStore {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};

            let roots = tempfile::tempdir().expect("temporary data root");
            let dirs = DataDirs {
                roaming: roots.path().join("roaming"),
                local: roots.path().join("local"),
            };
            std::fs::create_dir_all(&dirs.roaming).expect("the found root");
            std::fs::write(dirs.roaming.join("notes"), b"kept").expect("a kept value");
            let roaming = dirs.roaming.clone();
            let (release, released) = std::sync::mpsc::channel::<()>();
            let released = std::sync::Mutex::new(released);
            let (starting, started) = std::sync::mpsc::channel();
            let finds = Arc::new(AtomicUsize::new(0));
            let counting = Arc::clone(&finds);
            let store = DataStore::found_by(
                flui_platform::AppDataDir::try_new("app").expect("a name"),
                move |_| {
                    counting.fetch_add(1, Ordering::Relaxed);
                    let _ = starting.send(());
                    let _ = released
                        .lock()
                        .expect("one find at a time")
                        .recv_timeout(std::time::Duration::from_secs(30));
                    Ok(Some(flui_native::storage::FileStore::new(dirs.clone())))
                },
            );
            Self {
                store,
                release,
                started,
                finds,
                roaming,
                _roots: roots,
            }
        }
    }

    /// A call whose IO task runs once the pool shuts down never starts: it
    /// answers `Cancelled` and runs nowhere, on the pool's workers least of
    /// all.
    fn a_call_on_a_pool_shutting_down_never_starts() {
        let services = Arc::new(ExecutionServices::with_defaults());
        services.shutdown(std::time::Instant::now());
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let running = Arc::clone(&ran);
        let (reply, answer) = tokio::sync::oneshot::channel::<Result<(), StorageError>>();
        pollster::block_on(super::answer_off_io_workers(
            &Arc::downgrade(&services),
            reply,
            move || {
                running.store(true, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            },
        ));
        assert_eq!(
            pollster::block_on(answer).expect("answered"),
            Err(StorageError::Cancelled)
        );
        assert!(
            !ran.load(std::sync::atomic::Ordering::Relaxed),
            "the call never ran"
        );
    }

    /// The pool shuts down while a write runs (its store's find stalls until
    /// the shutdown has cancelled the IO task): the write the shutdown lets
    /// finish answers that it landed, and the disk holds what it answers.
    fn a_write_the_shutdown_lets_finish_answers_what_landed() {
        use std::time::Duration;

        let services = Arc::new(ExecutionServices::with_defaults());
        let StalledStore {
            store,
            release,
            started,
            finds: _,
            roaming,
            _roots,
        } = StalledStore::new();
        let storage = super::host_storage(store, Arc::downgrade(&services));
        let written = storage.publish(&NOTES, b"new".to_vec(), WriteMode::Replace);
        started
            .recv_timeout(Duration::from_secs(10))
            .expect("the write started");
        let cancelled = services.cancellation_root().clone();
        let releaser = std::thread::spawn(move || {
            // The IO task is cancelled before the write may go on.
            pollster::block_on(cancelled.cancelled());
            let _ = release.send(());
        });
        services.shutdown(std::time::Instant::now() + Duration::from_secs(10));
        releaser.join().expect("the write was let go");
        let answer = pollster::block_on(written);
        let disk = std::fs::read(roaming.join("notes")).expect("the value file");
        assert_eq!(
            answer,
            Ok(StoredVersion::of_bytes(b"new")),
            "a started write answers its outcome, never `Cancelled`"
        );
        assert_eq!(disk, b"new", "and the disk holds what it answered");
    }

    /// Stores whose first calls stall (another instance holds each one's
    /// migration lock) on FLUI's default pools, as many finds stalled as the
    /// IO lane has workers: IO work that does not touch a store still runs.
    /// Once the stalls end every call answers from the store found, the two
    /// calls of one store from its one find.
    fn a_store_call_that_waits_holds_no_io_worker() {
        use std::sync::atomic::Ordering;
        use std::time::Duration;

        let services = Arc::new(ExecutionServices::with_defaults());
        let (one, other) = (StalledStore::new(), StalledStore::new());
        let StalledStore {
            store,
            release: release_one,
            started: one_started,
            finds: one_finds,
            roaming: _,
            _roots: _one_roots,
        } = one;
        let StalledStore {
            store: other_store,
            release: release_other,
            started: other_started,
            finds: _,
            roaming: _,
            _roots: _other_roots,
        } = other;
        let storage = super::host_storage(store, Arc::downgrade(&services));
        let other_storage = super::host_storage(other_store, Arc::downgrade(&services));
        let reads = [
            storage.read(&NOTES, 1024),
            storage.read(&NOTES, 1024),
            other_storage.read(&NOTES, 1024),
        ];
        // Both finds have begun: on the lane's two workers, were the calls
        // run there.
        for started in [&one_started, &other_started] {
            started
                .recv_timeout(Duration::from_secs(10))
                .expect("the find begins");
        }
        let (ran, unrelated) = std::sync::mpsc::channel();
        services
            .spawn_io(Box::pin(async move {
                let _ = ran.send(());
            }))
            .expect("the IO lane takes the task");
        let ran = unrelated.recv_timeout(Duration::from_secs(10));
        release_one.send(()).expect("the find waits");
        release_other.send(()).expect("the find waits");
        assert!(
            ran.is_ok(),
            "unrelated IO work runs while the stores' calls wait"
        );
        for read in reads {
            let stored = pollster::block_on(read).expect("answered once the stall ended");
            assert_eq!(stored.bytes.as_deref(), Some(&b"kept"[..]));
        }
        assert_eq!(
            one_finds.load(Ordering::Relaxed),
            1,
            "one find for both calls of a store"
        );
        services.shutdown(std::time::Instant::now() + Duration::from_secs(5));
    }
}
