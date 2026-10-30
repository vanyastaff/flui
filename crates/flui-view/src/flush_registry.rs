//! The flush registry: the latest bytes of every persisted document, kept by
//! the host outside every UI runtime and written to storage from there.
//!
//! A document publishes its encoded bytes here on the owner thread, through
//! its own [`FlushPublisher`], made for the [`Storage`] the document acquired;
//! the host writes them to that storage on the IO pool, one write in flight
//! per value (a name in a storage), the latest bytes winning.
//! Because the registry lives outside the UI runtime, the host can still write
//! what was published when the UI runtime is gone or busy: at the end of the
//! application and when the session ends, without running any application
//! code.
//!
//! The [`FlushHost`] owns the registry: it builds it, flushes it and settles
//! it on the owner's turn. Documents get only the publishing side, a
//! [`FlushRegistry`].
//!
//! Per value the registry keeps at most one write in flight and the latest
//! bytes owed after it. Bytes published while a write is in flight replace
//! the owed bytes and are written once that write finishes. A failed write
//! keeps its bytes owed, with the failure, until newer bytes replace them or
//! the host's flush writes them; a write the IO pool refused, or dropped
//! unfinished, keeps them owed without a failure. The registry's lock is
//! never held while the storage, the IO pool or the owner's signal runs.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

use flui_platform::{Storage, StorageError, StorageFuture, StorageName, StoredVersion, WriteMode};
use parking_lot::{Condvar, Mutex};

use crate::owner_notify::{LifecycleEvent, OwnerNotify};
use crate::persist::Revision;

/// Where the bytes published under one name stand against the storage.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlushState {
    /// Every byte published under the name is stored, or nothing was
    /// published.
    Clean,
    /// Bytes were published and are not stored yet.
    Pending,
    /// The last write failed. The bytes stay in the registry until newer
    /// bytes replace them or the host's flush writes them. A write based on
    /// a version the storage no longer holds fails with
    /// [`StorageError::Conflict`].
    Failed(StorageError),
}

/// Who published a value's bytes: one [`FlushPublisher`] and its clones.
///
/// Compared by `Arc` address. Both the owed bytes and the committed record
/// hold the publisher's `Arc` itself — never a raw address or a `Weak` — so
/// the allocation stays alive as long as any record names it, and a
/// publisher made later can never be handed the same address and pass for
/// the one that committed.
struct PublisherIdentity;

/// One stored value: the storage that keeps it and its name there.
///
/// The storage is compared by its `Arc`'s address, never by its type, so two
/// stores of one type are two keys. Every record under a key (the owed
/// bytes, the committed write) holds that `Arc`, so its address cannot be
/// reused by another storage while the key is in the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ValueKey {
    store: usize,
    name: StorageName,
}

impl ValueKey {
    fn of(storage: &Arc<dyn Storage>, name: StorageName) -> Self {
        Self {
            store: Arc::as_ptr(storage).cast::<()>().addr(),
            name,
        }
    }
}

/// Bytes published under one value.
struct Slot {
    bytes: Arc<[u8]>,
    mode: WriteMode,
    revision: Revision,
    publisher: Arc<PublisherIdentity>,
    /// The storage the bytes are written to: the one the publishing document
    /// acquired.
    storage: Arc<dyn Storage>,
}

/// The last write the storage confirmed for one value.
struct CommittedWrite {
    revision: Revision,
    version: StoredVersion,
    /// The version that write was based on as published, before any
    /// rebase; `None` for a write that replaced whatever was stored.
    based_on: Option<StoredVersion>,
    /// The publisher whose line continues from this write; kept as its
    /// `Arc` (see [`PublisherIdentity`]).
    publisher: Arc<PublisherIdentity>,
    /// Keeps the key's storage address taken (see [`ValueKey`]).
    _storage: Arc<dyn Storage>,
}

/// One value's bytes that are not stored yet.
#[derive(Default)]
struct Entry {
    /// The latest bytes not being written: published since the write in
    /// flight started, or kept after a write that did not store them.
    owed: Option<Slot>,
    /// Whether a write of this value is in flight.
    writing: bool,
    /// Why the owed bytes' last write failed; `None` once newer bytes
    /// replace them.
    failure: Option<StorageError>,
}

impl Entry {
    fn state(&self) -> FlushState {
        match (&self.failure, &self.owed) {
            _ if self.writing => FlushState::Pending,
            (Some(error), Some(_)) => FlushState::Failed(error.clone()),
            (None, Some(_)) => FlushState::Pending,
            (_, None) => FlushState::Clean,
        }
    }

    fn is_clean(&self) -> bool {
        !self.writing && self.owed.is_none()
    }
}

/// What [`Slots::begin`] decided for one value.
enum Begin {
    /// Nothing to start: nothing owed, a write already in flight, or failed
    /// bytes left for the flush.
    Nothing,
    /// Write these bytes under this mode.
    Write(Slot, WriteMode),
}

/// The registry's state.
#[derive(Default)]
struct Slots {
    entries: HashMap<ValueKey, Entry>,
    committed: HashMap<ValueKey, CommittedWrite>,
    /// How many finished writes were recorded.
    changes: u64,
    /// How many of them the owner has settled: waiters taken before this
    /// count are resolved.
    settled: u64,
    /// The wakers of pending [`FlushChanged`] futures, each with the change
    /// count it was taken at.
    waiters: Vec<(u64, Waker)>,
}

fn count_change(changes: &mut u64) {
    *changes = changes
        .checked_add(1)
        .expect("BUG: a flush registry counted 2^64 changes");
}

impl Slots {
    /// Start writing the bytes owed under `key`, unless a write of it is
    /// in flight. Failed bytes are written again only when `retry_failed`.
    ///
    /// An edit based on the same version as the last confirmed write of its
    /// own publisher continues that publisher's line: it is rebased onto
    /// that write, whose bytes it replaces. Every other edit is written as
    /// published, and the storage's own comparison decides it.
    fn begin(&mut self, key: ValueKey, retry_failed: bool) -> Begin {
        let Some(entry) = self.entries.get_mut(&key) else {
            return Begin::Nothing;
        };
        if entry.writing || (entry.failure.is_some() && !retry_failed) {
            return Begin::Nothing;
        }
        let Some(slot) = entry.owed.take() else {
            return Begin::Nothing;
        };
        let mode = match (slot.mode, self.committed.get(&key)) {
            (WriteMode::IfUnchanged(base), Some(last))
                if Arc::ptr_eq(&last.publisher, &slot.publisher) && last.based_on == Some(base) =>
            {
                WriteMode::IfUnchanged(last.version)
            }
            (mode, _) => mode,
        };
        entry.failure = None;
        entry.writing = true;
        Begin::Write(slot, mode)
    }

    /// Record how the write of `slot` under `key` ended, and start the
    /// write of bytes published meanwhile. `None` is a write dropped before
    /// it finished.
    fn finish(
        &mut self,
        key: ValueKey,
        slot: Slot,
        result: Option<Result<StoredVersion, StorageError>>,
    ) -> Begin {
        // A write in flight keeps its entry; the default only keeps a drop
        // from panicking if that ever breaks.
        let entry = self.entries.entry(key).or_default();
        entry.writing = false;
        let next = match result {
            Some(Ok(version)) => {
                self.committed.insert(
                    key,
                    CommittedWrite {
                        revision: slot.revision,
                        version,
                        based_on: match slot.mode {
                            WriteMode::IfUnchanged(base) => Some(base),
                            _ => None,
                        },
                        publisher: slot.publisher,
                        _storage: slot.storage,
                    },
                );
                count_change(&mut self.changes);
                self.begin(key, false)
            }
            // Not written, and nothing wrong with the bytes: they stay owed
            // for the flush. Not restarted here, since a pool or storage
            // that is shutting down would refuse the next one the same way.
            None | Some(Err(StorageError::Cancelled | StorageError::Superseded)) => {
                entry.owed.get_or_insert(slot);
                Begin::Nothing
            }
            Some(Err(error)) => {
                if entry.owed.is_none() {
                    entry.owed = Some(slot);
                    entry.failure = Some(error);
                }
                count_change(&mut self.changes);
                self.begin(key, false)
            }
        };
        if self.entries.get(&key).is_some_and(Entry::is_clean) {
            self.entries.remove(&key);
        }
        next
    }
}

/// What the host gave the registry to write with: the IO pool that runs the
/// writes, and the channel that tells the owner a write finished. Each write
/// goes to the storage its bytes were published for.
struct RegistryHost {
    writer: FlushWriter,
    notify: OwnerNotify,
}

/// What the registry's sides share.
struct Shared {
    slots: Mutex<Slots>,
    /// Signalled whenever a write in flight ends, for the flush to wait on.
    write_ended: Condvar,
    host: RegistryHost,
}

impl Shared {
    /// Record how a write ended, start the next one, and signal the owner
    /// if what the registry answers changed. Runs with the lock released
    /// around everything but the record.
    fn finish(
        self: &Arc<Self>,
        key: ValueKey,
        slot: Slot,
        result: Option<Result<StoredVersion, StorageError>>,
    ) {
        let (next, changed) = {
            let mut slots = self.slots.lock();
            let before = slots.changes;
            let next = slots.finish(key, slot, result);
            (next, slots.changes != before)
        };
        self.write_ended.notify_all();
        self.start(key, next);
        if changed {
            self.signal();
        }
    }

    /// Hand a write [`Slots::begin`] started to the IO pool. A write the
    /// pool refuses is dropped here, which keeps its bytes owed.
    fn start(self: &Arc<Self>, key: ValueKey, begin: Begin) {
        if let Begin::Write(slot, mode) = begin {
            self.hand_to_pool(PendingWrite::new(Arc::clone(self), key, slot, mode));
        }
    }

    /// Run `write` on the IO pool. A refused write is dropped here, with
    /// the lock released, which keeps its bytes owed.
    fn hand_to_pool(&self, write: PendingWrite) {
        if let Err(refused) = (self.host.writer)(Box::pin(write)) {
            drop(refused);
        }
    }

    /// Tell the owner a write finished or was refused. A signal the owner
    /// cannot receive loses nothing: the record stays for its next settle.
    fn signal(&self) {
        let _undelivered = self.host.notify.notify(LifecycleEvent::FlushChanged);
    }
}

/// One write of one value, to the storage its bytes were published for: asks
/// it on its first poll and records the answer. Dropped before the answer, it
/// keeps its bytes owed — unless newer bytes were published meanwhile, which
/// then win. Bytes the storage had already accepted may still land; the flush
/// then writes them again.
struct PendingWrite {
    shared: Arc<Shared>,
    key: ValueKey,
    /// The bytes being written; `None` once the answer is recorded.
    slot: Option<Slot>,
    mode: WriteMode,
    answer: Option<StorageFuture<StoredVersion>>,
}

impl PendingWrite {
    fn new(shared: Arc<Shared>, key: ValueKey, slot: Slot, mode: WriteMode) -> Self {
        Self {
            shared,
            key,
            slot: Some(slot),
            mode,
            answer: None,
        }
    }
}

impl Future for PendingWrite {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        let Some(slot) = &this.slot else {
            return Poll::Ready(());
        };
        let answer = this.answer.get_or_insert_with(|| {
            slot.storage
                .publish(&this.key.name, slot.bytes.to_vec(), this.mode)
        });
        let Poll::Ready(result) = answer.as_mut().poll(cx) else {
            return Poll::Pending;
        };
        this.answer = None;
        let slot = this
            .slot
            .take()
            .expect("BUG: an unfinished write holds its bytes");
        this.shared.finish(this.key, slot, Some(result));
        Poll::Ready(())
    }
}

impl Drop for PendingWrite {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            self.shared.finish(self.key, slot, None);
        }
    }
}

/// Wakes the thread that blocks on a write in [`FlushHost::flush_within`].
struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll `future` on this thread until it finishes or `deadline` passes;
/// `false` when the deadline passed first. Polled at least once.
fn poll_until(mut future: Pin<&mut PendingWrite>, deadline: Instant) -> bool {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if future.as_mut().poll(&mut cx).is_ready() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::park_timeout(deadline - now);
    }
}

/// The publishing side of the host's flush registry: where a document gets
/// its [`FlushPublisher`] and reads how its bytes stand.
///
/// Cheap to clone; every clone is the same registry.
#[derive(Clone)]
pub struct FlushRegistry {
    shared: Arc<Shared>,
}

impl fmt::Debug for FlushRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushRegistry")
            .field("unwritten", &self.shared.slots.lock().entries.len())
            .finish_non_exhaustive()
    }
}

impl FlushRegistry {
    /// A new publisher of `name` in `storage`: the storage the publishing
    /// document acquired, which its bytes are written to. Each call is a
    /// different publisher; its clones are the same one.
    ///
    /// A publisher's own edits are one line: an edit based on the same
    /// version as its last confirmed write is rebased onto that write, whose
    /// bytes it replaces. Every other edit is written as published, and the
    /// storage decides it: an edit based on a version the storage no longer
    /// holds fails with [`StorageError::Conflict`]. The same name in two
    /// storages is two values.
    #[must_use]
    pub fn publisher(&self, name: StorageName, storage: Arc<dyn Storage>) -> FlushPublisher {
        FlushPublisher {
            shared: Arc::clone(&self.shared),
            key: ValueKey::of(&storage, name),
            storage,
            identity: Arc::new(PublisherIdentity),
        }
    }

    /// A future that resolves at the first write that finishes or fails,
    /// for any value, after it was taken: what changes a publisher's
    /// [`state`](FlushPublisher::state) or
    /// [`committed`](FlushPublisher::committed) other than its own
    /// publication. It wakes on the owner's turn, never on the IO thread
    /// that finished the write. Take it before reading the state.
    pub fn changed(&self) -> FlushChanged {
        let taken_at = self.shared.slots.lock().changes;
        FlushChanged {
            shared: Arc::clone(&self.shared),
            taken_at,
        }
    }
}

/// One document's right to publish one value: a name in a storage; see
/// [`FlushRegistry::publisher`].
#[derive(Clone)]
pub struct FlushPublisher {
    shared: Arc<Shared>,
    key: ValueKey,
    storage: Arc<dyn Storage>,
    identity: Arc<PublisherIdentity>,
}

impl fmt::Debug for FlushPublisher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushPublisher")
            .field("name", &self.key.name.as_str())
            .finish_non_exhaustive()
    }
}

impl FlushPublisher {
    /// The name this publisher writes.
    #[must_use]
    pub fn name(&self) -> StorageName {
        self.key.name
    }

    /// The document's revision last written for this value, by any
    /// publisher, as it was published, with the stored version the storage
    /// confirmed for it; `None` before the first confirmed write.
    #[must_use]
    pub fn committed(&self) -> Option<(Revision, StoredVersion)> {
        self.shared
            .slots
            .lock()
            .committed
            .get(&self.key)
            .map(|write| (write.revision, write.version))
    }

    /// Where the bytes published for this value stand.
    #[must_use]
    pub fn state(&self) -> FlushState {
        self.shared
            .slots
            .lock()
            .entries
            .get(&self.key)
            .map_or(FlushState::Clean, Entry::state)
    }

    /// Make `bytes` the latest value, to be written under `mode`, replacing
    /// bytes published earlier that are not written yet. `revision` is the
    /// document's own revision of these bytes, the one written in their
    /// header; the registry records it and never mints one. Owner thread;
    /// never waits for the storage.
    ///
    /// When no write of the value is in flight, the write starts on the IO
    /// pool now; otherwise it starts when the one in flight finishes.
    pub fn publish(&self, bytes: Arc<[u8]>, mode: WriteMode, revision: Revision) {
        let begin = {
            let mut slots = self.shared.slots.lock();
            let entry = slots.entries.entry(self.key).or_default();
            entry.owed = Some(Slot {
                bytes,
                mode,
                revision,
                publisher: Arc::clone(&self.identity),
                storage: Arc::clone(&self.storage),
            });
            entry.failure = None;
            slots.begin(self.key, false)
        };
        self.shared.start(self.key, begin);
    }
}

/// Resolves at the next change to the registry; see
/// [`FlushRegistry::changed`].
#[must_use = "a future does nothing unless polled"]
pub struct FlushChanged {
    shared: Arc<Shared>,
    /// The registry's change count when the future was taken.
    taken_at: u64,
}

impl fmt::Debug for FlushChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushChanged").finish_non_exhaustive()
    }
}

impl Future for FlushChanged {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut slots = self.shared.slots.lock();
        if slots.settled > self.taken_at {
            return Poll::Ready(());
        }
        let waker = cx.waker();
        if !slots
            .waiters
            .iter()
            .any(|(taken_at, known)| *taken_at == self.taken_at && known.will_wake(waker))
        {
            slots.waiters.push((self.taken_at, waker.clone()));
        }
        Poll::Pending
    }
}

/// One write the registry hands its [`FlushWriter`] to run on the IO pool.
pub type FlushWrite = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Runs a [`FlushWrite`] on the host's IO pool, or hands it back when the
/// pool refuses it; a refused write stays owed and the host's flush writes
/// its bytes.
pub type FlushWriter = Box<dyn Fn(FlushWrite) -> Result<(), FlushWrite> + Send + Sync>;

/// What [`FlushHost::flush_within`] wrote before its deadline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlushReport {
    /// Names whose latest bytes are now stored.
    pub written: Vec<StorageName>,
    /// Names whose write failed, with why.
    pub failed: Vec<(StorageName, StorageError)>,
    /// Names whose latest bytes were not stored by the deadline.
    pub unfinished: Vec<StorageName>,
}

impl FlushReport {
    /// Whether every published byte is stored.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty() && self.unfinished.is_empty()
    }
}

/// How long the host writes the flush registry when the user's session ends,
/// counted from the start of the `Ending` phase (ADR-0260): the framework's
/// share of the time the operating system gives a process before it ends it.
pub const SESSION_END_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);

/// The host's side of the flush registry: it builds the registry, settles it
/// on the owner's turn and flushes it when the application or the session
/// ends. Only the host holds it, and a clone of it flushes at exit; documents
/// get a [`FlushRegistry`].
#[derive(Clone)]
pub struct FlushHost {
    shared: Arc<Shared>,
}

impl fmt::Debug for FlushHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushHost").finish_non_exhaustive()
    }
}

impl FlushHost {
    /// A registry starting each write on the IO pool through `writer`, and
    /// signalling the owner through `notify` when a write finishes. Each
    /// write goes to the storage its publisher was made for.
    #[must_use]
    pub fn new(writer: FlushWriter, notify: OwnerNotify) -> Self {
        Self {
            shared: Arc::new(Shared {
                slots: Mutex::new(Slots::default()),
                write_ended: Condvar::new(),
                host: RegistryHost { writer, notify },
            }),
        }
    }

    /// The publishing side, for documents.
    #[must_use]
    pub fn registry(&self) -> FlushRegistry {
        FlushRegistry {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Settle the registry on the owner's turn: return the wakers of the
    /// [`FlushChanged`] futures that the writes finished since they were
    /// taken resolved.
    ///
    /// Nothing is woken here: the registry's state is committed and its lock
    /// released before this returns, and the owner wakes the wakers itself,
    /// inside its single containment boundary, one by one, so a waker that
    /// panics does not lose the ones after it.
    #[must_use = "the owner wakes the returned wakers"]
    pub fn settle(&self) -> Vec<Waker> {
        let mut slots = self.shared.slots.lock();
        slots.settled = slots.changes;
        let settled = slots.settled;
        let (wake, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut slots.waiters)
            .into_iter()
            .partition(|(taken_at, _)| *taken_at < settled);
        slots.waiters = waiting;
        wake.into_iter().map(|(_, waker)| waker).collect()
    }

    /// Write the latest unconfirmed bytes of every value before `deadline`,
    /// waiting for a write already in flight first. Synchronous, on the owner
    /// thread; runs no application code. Failed bytes are written again,
    /// each value at most once per flush.
    ///
    /// A write still unanswered at the deadline is handed to the IO pool to
    /// finish, and its name is reported unfinished.
    #[must_use]
    pub fn flush_within(&self, deadline: Instant) -> FlushReport {
        let shared = &self.shared;
        let owed: HashSet<ValueKey> = shared.slots.lock().entries.keys().copied().collect();
        let mut attempted = HashSet::new();
        loop {
            let mut slots = shared.slots.lock();
            if slots.entries.values().any(|entry| entry.writing) {
                if shared
                    .write_ended
                    .wait_until(&mut slots, deadline)
                    .timed_out()
                {
                    break;
                }
                continue;
            }
            let Some(key) = slots
                .entries
                .iter()
                .find(|(key, entry)| entry.owed.is_some() && !attempted.contains(*key))
                .map(|(key, _)| *key)
            else {
                break;
            };
            attempted.insert(key);
            let begin = slots.begin(key, true);
            drop(slots);
            let Begin::Write(slot, mode) = begin else {
                continue;
            };
            let mut write = PendingWrite::new(Arc::clone(shared), key, slot, mode);
            if !poll_until(Pin::new(&mut write), deadline) {
                shared.hand_to_pool(write);
                break;
            }
        }
        let slots = shared.slots.lock();
        let mut report = FlushReport::default();
        let mut touched = owed;
        touched.extend(attempted);
        for key in touched {
            match slots.entries.get(&key).map(Entry::state) {
                None | Some(FlushState::Clean) => report.written.push(key.name),
                Some(FlushState::Failed(error)) => report.failed.push((key.name, error)),
                Some(FlushState::Pending) => report.unfinished.push(key.name),
            }
        }
        report
    }
}

// In the crate: a published revision is the document's, and only the crate
// names one (`Revision::new` is crate-private).
#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Waker};
    use std::time::{Duration, Instant};

    use flui_platform::{Storage, StorageError, StorageName, StoredVersion, WriteMode};
    use flui_scheduler::AppLifecycleState;
    use flui_testing::storage::MemoryStorage;
    use parking_lot::Mutex;

    use super::{FlushHost, FlushState, FlushWrite};
    use crate::__runtime::LifecycleSource;
    use crate::owner_notify::{LifecycleEvent, OwnerNotify};
    use crate::persist::Revision;

    const SESSION: StorageName = StorageName::machine_local("session");
    const NOTES: StorageName = StorageName::from_static("notes");

    /// A host with no IO pool, so every write is left to the
    /// flush, recording the events it signals its owner with.
    fn host() -> (FlushHost, Arc<Mutex<Vec<LifecycleEvent>>>) {
        let signals = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&signals);
        let host = FlushHost::new(
            Box::new(Err),
            OwnerNotify::new(move |event| {
                log.lock().push(event);
                Ok(())
            }),
        );
        (host, signals)
    }

    /// The writes a host handed its IO pool, which the test runs by hand.
    type Pool = Arc<Mutex<Vec<FlushWrite>>>;

    /// A host whose IO pool accepts every write and runs it
    /// only when the test calls [`run_next`].
    fn host_with_pool() -> (FlushHost, Pool) {
        let pool = Pool::default();
        let queue = Arc::clone(&pool);
        let host = FlushHost::new(
            Box::new(move |write| {
                queue.lock().push(write);
                Ok(())
            }),
            OwnerNotify::new(|_| Ok(())),
        );
        (host, pool)
    }

    /// Poll the pool's oldest write once, outside the pool's lock: a write
    /// that finishes may hand the pool the next one. `true` when it finished.
    fn run_next(pool: &Pool) -> bool {
        let mut write = pool.lock().remove(0);
        let finished = write
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready();
        if !finished {
            pool.lock().insert(0, write);
        }
        finished
    }

    /// `storage` as a document acquires it: one `Arc`, the identity its
    /// publishers write to.
    fn shared(storage: &MemoryStorage) -> Arc<dyn Storage> {
        Arc::new(storage.clone())
    }

    fn flush(host: &FlushHost) -> super::FlushReport {
        host.flush_within(Instant::now() + Duration::from_secs(5))
    }

    /// Bytes published while the presentation is told it is detached are on
    /// the storage once the host's teardown flush returns, committed at the
    /// document's own revision.
    fn a_registry_entry_published_at_detached_is_written_at_teardown() {
        // Not the first revision, so a registry counting its own
        // publications would not report it.
        const CLOSING: Revision = Revision::new(7);
        let storage = MemoryStorage::new();
        let (host, _signals) = host();
        let store = shared(&storage);
        let publisher = host.registry().publisher(SESSION, Arc::clone(&store));
        let reader = publisher.clone();
        let lifecycle = LifecycleSource::new();
        let (_, _observation) = lifecycle
            .handle()
            .subscribe(move |state| {
                if state == AppLifecycleState::Detached {
                    publisher.publish(Arc::from(&b"closing"[..]), WriteMode::Replace, CLOSING);
                }
            })
            .expect("the presentation is open");

        lifecycle.begin_close();
        lifecycle
            .commit_terminal(AppLifecycleState::Detached)
            .expect("a closing presentation commits Detached");
        lifecycle.drain();
        let report = flush(&host);

        assert_eq!(
            storage.contents(&SESSION).as_deref(),
            Some(&b"closing"[..]),
            "the teardown flush writes what was published at Detached"
        );
        assert!(
            report.is_complete(),
            "nothing is left unwritten: {report:?}"
        );
        assert_eq!(
            reader.committed().map(|(revision, _)| revision),
            Some(CLOSING),
            "the committed revision is the document's, as published"
        );
    }

    /// A publisher's edit based on the version its own earlier write
    /// replaced is rebased onto that write; the same stale base from another
    /// publisher conflicts and leaves the stored value alone.
    fn a_publishers_own_line_rebases_and_another_conflicts() {
        let storage = MemoryStorage::new();
        let (host, _signals) = host();
        let store = shared(&storage);
        let registry = host.registry();
        let ours = registry.publisher(NOTES, Arc::clone(&store));
        let base = WriteMode::IfUnchanged(StoredVersion::ABSENT);

        ours.publish(Arc::from(&b"one"[..]), base, Revision::new(1));
        let _ = flush(&host);
        ours.publish(Arc::from(&b"two"[..]), base, Revision::new(2));
        let _ = flush(&host);
        let ours_written = storage.contents(&NOTES);
        let ours_state = ours.state();

        let theirs = registry.publisher(NOTES, Arc::clone(&store));
        theirs.publish(Arc::from(&b"theirs"[..]), base, Revision::new(3));
        let _ = flush(&host);

        assert_eq!(
            ours_written.as_deref(),
            Some(&b"two"[..]),
            "the publisher's own second edit is rebased and written"
        );
        assert_eq!(ours_state, FlushState::Clean);
        assert_eq!(
            theirs.state(),
            FlushState::Failed(StorageError::Conflict),
            "another publisher's stale base conflicts"
        );
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"two"[..]));
    }

    /// A finished write signals the owner and wakes `changed()` only when
    /// the owner settles the registry on its turn.
    fn a_finished_write_wakes_changed_on_the_owner_turn() {
        let storage = MemoryStorage::new();
        let (host, signals) = host();
        let store = shared(&storage);
        let registry = host.registry();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut cx = Context::from_waker(&waker);
        let mut changed = std::pin::pin!(registry.changed());
        let _ = changed.as_mut().poll(&mut cx);

        registry.publisher(SESSION, Arc::clone(&store)).publish(
            Arc::from(&b"open"[..]),
            WriteMode::Replace,
            Revision::new(1),
        );
        let _ = flush(&host);
        let after_write = counter.0.load(Ordering::SeqCst);
        let wake = host.settle();
        let after_settle = counter.0.load(Ordering::SeqCst);
        // The owner wakes what settle returned, inside its containment.
        for waker in wake {
            waker.wake();
        }

        assert_eq!(
            signals.lock().as_slice(),
            [LifecycleEvent::FlushChanged],
            "the finished write signalled the owner once"
        );
        assert_eq!(
            counter.0.load(Ordering::SeqCst),
            1,
            "the owner wakes the waiter once, from what settle returned"
        );
        assert_eq!(after_write, 0, "the finished write wakes nothing itself");
        assert_eq!(after_settle, 0, "settle wakes nothing itself");
        assert!(changed.as_mut().poll(&mut cx).is_ready());
    }

    /// A failed write keeps its bytes: the flush writes them later, and
    /// newer bytes published after the failure replace them.
    fn a_failed_write_keeps_its_bytes_until_replaced_or_flushed() {
        let storage = MemoryStorage::new();
        let (host, _signals) = host();
        let store = shared(&storage);
        let registry = host.registry();
        let notes = registry.publisher(NOTES, Arc::clone(&store));
        storage.fail_next(StorageError::Full);
        notes.publish(Arc::from(&b"one"[..]), WriteMode::Replace, Revision::new(1));

        let failed = flush(&host);
        let failed_state = notes.state();
        let flushed = flush(&host);
        let flushed_contents = storage.contents(&NOTES);

        storage.fail_next(StorageError::Full);
        notes.publish(Arc::from(&b"two"[..]), WriteMode::Replace, Revision::new(2));
        let _ = flush(&host);
        notes.publish(
            Arc::from(&b"three"[..]),
            WriteMode::Replace,
            Revision::new(3),
        );
        let replaced_state = notes.state();
        let _ = flush(&host);

        assert_eq!(failed.failed, [(NOTES, StorageError::Full)]);
        assert_eq!(failed_state, FlushState::Failed(StorageError::Full));
        assert!(
            flushed.is_complete(),
            "the next flush writes the kept bytes"
        );
        assert_eq!(flushed_contents.as_deref(), Some(&b"one"[..]));
        assert_eq!(
            replaced_state,
            FlushState::Pending,
            "newer bytes replace the failed ones"
        );
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"three"[..]));
        assert_eq!(
            notes.committed().map(|(revision, _)| revision),
            Some(Revision::new(3))
        );
    }

    /// Edits a publisher makes while its write is in flight are written once,
    /// after it, rebased onto it: the storage ends with the latest edit, not
    /// a conflict.
    fn same_writer_edits_rebase_instead_of_conflicting() {
        let storage = MemoryStorage::new();
        let (host, pool) = host_with_pool();
        let store = shared(&storage);
        let registry = host.registry();
        let notes = registry.publisher(NOTES, Arc::clone(&store));
        let loaded = WriteMode::IfUnchanged(StoredVersion::ABSENT);
        let barrier = storage.hold_commits();

        notes.publish(Arc::from(&b"one"[..]), loaded, Revision::new(1));
        let first_landed = run_next(&pool);
        notes.publish(Arc::from(&b"two"[..]), loaded, Revision::new(2));
        notes.publish(Arc::from(&b"three"[..]), loaded, Revision::new(3));
        let in_flight = pool.lock().len();
        barrier.commit();
        let first_finished = run_next(&pool);
        let second_finished = run_next(&pool);

        assert!(!first_landed, "the first write waits at the barrier");
        assert_eq!(in_flight, 1, "one write in flight per name");
        assert!(first_finished && second_finished);
        assert!(
            pool.lock().is_empty(),
            "the latest edit is the only next one"
        );
        assert_eq!(notes.state(), FlushState::Clean);
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"three"[..]));
        assert_eq!(
            notes.committed().map(|(revision, _)| revision),
            Some(Revision::new(3))
        );
    }

    /// An edit based on a version another writer stored, which the document
    /// reloaded, is written as published rather than rebased onto the
    /// publisher's older write; the edits after it continue its line.
    fn a_reloaded_base_is_written_as_published() {
        let storage = MemoryStorage::new();
        let (host, _signals) = host();
        let store = shared(&storage);
        let notes = host.registry().publisher(NOTES, Arc::clone(&store));
        let absent = WriteMode::IfUnchanged(StoredVersion::ABSENT);
        notes.publish(Arc::from(&b"one"[..]), absent, Revision::new(1));
        let _ = flush(&host);

        storage.put(&NOTES, &b"external"[..]);
        let reloaded = WriteMode::IfUnchanged(StoredVersion::of_bytes(b"external"));
        notes.publish(Arc::from(&b"two"[..]), reloaded, Revision::new(5));
        let _ = flush(&host);
        let after_reload = (notes.state(), storage.contents(&NOTES));
        notes.publish(Arc::from(&b"three"[..]), reloaded, Revision::new(6));
        let _ = flush(&host);

        assert_eq!(
            after_reload,
            (FlushState::Clean, Some(b"two".to_vec())),
            "the edit on the reloaded version is written"
        );
        assert_eq!(notes.state(), FlushState::Clean);
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"three"[..]));
        assert_eq!(
            notes.committed().map(|(revision, _)| revision),
            Some(Revision::new(6))
        );
    }

    /// The same name in two storages of one type is two values: each
    /// publisher's bytes go to its own storage, and one storage's failure
    /// is not the other's.
    fn two_storages_of_one_type_keep_a_name_apart() {
        let (first, second) = (MemoryStorage::new(), MemoryStorage::new());
        let (host, _signals) = host();
        let registry = host.registry();
        let ours = registry.publisher(NOTES, shared(&first));
        let theirs = registry.publisher(NOTES, shared(&second));
        first.fail_next(StorageError::Full);
        let absent = WriteMode::IfUnchanged(StoredVersion::ABSENT);

        ours.publish(Arc::from(&b"ours"[..]), absent, Revision::new(1));
        theirs.publish(Arc::from(&b"theirs"[..]), absent, Revision::new(1));
        let report = flush(&host);

        assert_eq!(report.failed, [(NOTES, StorageError::Full)]);
        assert_eq!(report.written, [NOTES]);
        assert_eq!(ours.state(), FlushState::Failed(StorageError::Full));
        assert_eq!(theirs.state(), FlushState::Clean);
        assert_eq!(first.contents(&NOTES), None);
        assert_eq!(second.contents(&NOTES).as_deref(), Some(&b"theirs"[..]));
        assert_eq!(ours.committed(), None);
        assert_eq!(
            theirs.committed().map(|(revision, _)| revision),
            Some(Revision::new(1))
        );
    }

    #[test]
    fn flush_registry_matrix() {
        crate::table_test::run_table(
            "flush_registry_matrix",
            &[
                (
                    "a_registry_entry_published_at_detached_is_written_at_teardown",
                    a_registry_entry_published_at_detached_is_written_at_teardown as fn(),
                ),
                (
                    "a_publishers_own_line_rebases_and_another_conflicts",
                    a_publishers_own_line_rebases_and_another_conflicts,
                ),
                (
                    "a_finished_write_wakes_changed_on_the_owner_turn",
                    a_finished_write_wakes_changed_on_the_owner_turn,
                ),
                (
                    "a_failed_write_keeps_its_bytes_until_replaced_or_flushed",
                    a_failed_write_keeps_its_bytes_until_replaced_or_flushed,
                ),
                (
                    "same_writer_edits_rebase_instead_of_conflicting",
                    same_writer_edits_rebase_instead_of_conflicting,
                ),
                (
                    "a_reloaded_base_is_written_as_published",
                    a_reloaded_base_is_written_as_published,
                ),
                (
                    "two_storages_of_one_type_keep_a_name_apart",
                    two_storages_of_one_type_keep_a_name_apart,
                ),
            ],
        );
    }

    /// Counts its wakes, so a test can tell where a wake happened.
    #[derive(Default)]
    struct CountingWaker(AtomicUsize);

    impl std::task::Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
