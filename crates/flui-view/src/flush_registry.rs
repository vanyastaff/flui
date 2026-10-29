//! The flush registry: the latest bytes of every persisted document, kept by
//! the host outside every UI runtime and written to storage from there.
//!
//! A document publishes its encoded bytes here on the owner thread, through
//! its own [`FlushPublisher`]; the host writes them through its [`Storage`]
//! on the IO pool, one write in flight per name, the latest bytes winning.
//! Because the registry lives outside the UI runtime, the host can still write
//! what was published when the UI runtime is gone or busy: at the end of the
//! application and when the session ends, without running any application
//! code.
//!
//! The [`FlushHost`] owns the registry: it builds it, flushes it and settles
//! it on the owner's turn. Documents get only the publishing side, a
//! [`FlushRegistry`].
//!
//! Per name the registry keeps at most one write in flight and the latest
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
    /// a version another publisher replaced fails with
    /// [`StorageError::Conflict`].
    Failed(StorageError),
}

/// Who published a name's bytes: one [`FlushPublisher`] and its clones.
///
/// Compared by `Arc` address. Both the owed bytes and the committed record
/// hold the publisher's `Arc` itself — never a raw address or a `Weak` — so
/// the allocation stays alive as long as any record names it, and a
/// publisher made later can never be handed the same address and pass for
/// the one that committed.
struct PublisherIdentity;

/// Bytes published under one name.
struct Slot {
    bytes: Arc<[u8]>,
    mode: WriteMode,
    revision: Revision,
    publisher: Arc<PublisherIdentity>,
}

/// The last write the storage confirmed for one name.
struct CommittedWrite {
    revision: Revision,
    version: StoredVersion,
    /// The publisher whose line the next edit must continue to be rebased
    /// rather than conflict; kept as its `Arc` (see [`PublisherIdentity`]).
    publisher: Arc<PublisherIdentity>,
}

/// One name's bytes that are not stored yet.
#[derive(Default)]
struct Entry {
    /// The latest bytes not being written: published since the write in
    /// flight started, or kept after a write that did not store them.
    owed: Option<Slot>,
    /// Whether a write of this name is in flight.
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

/// What [`Slots::begin`] decided for one name.
enum Begin {
    /// Nothing to start: nothing owed, a write already in flight, or failed
    /// bytes left for the flush.
    Nothing,
    /// Write these bytes under this mode.
    Write(Slot, WriteMode),
    /// The write would conflict; the failure is recorded and the bytes stay
    /// owed.
    Refused,
}

/// The registry's state.
#[derive(Default)]
struct Slots {
    entries: HashMap<StorageName, Entry>,
    committed: HashMap<StorageName, CommittedWrite>,
    /// How many finished or refused writes were recorded.
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
    /// Start writing the bytes owed under `name`, unless a write of it is
    /// in flight. Failed bytes are written again only when `retry_failed`.
    ///
    /// An edit based on a version is checked against the last write the
    /// storage confirmed: from the publisher that made that write it is
    /// rebased onto it, from another publisher it conflicts unless it was
    /// based on exactly that version.
    fn begin(&mut self, name: StorageName, retry_failed: bool) -> Begin {
        let Some(entry) = self.entries.get_mut(&name) else {
            return Begin::Nothing;
        };
        if entry.writing || (entry.failure.is_some() && !retry_failed) {
            return Begin::Nothing;
        }
        let Some(slot) = entry.owed.take() else {
            return Begin::Nothing;
        };
        let mode = match (slot.mode, self.committed.get(&name)) {
            (WriteMode::IfUnchanged(_), Some(last))
                if Arc::ptr_eq(&last.publisher, &slot.publisher) =>
            {
                WriteMode::IfUnchanged(last.version)
            }
            (WriteMode::IfUnchanged(base), Some(last)) if base != last.version => {
                entry.owed = Some(slot);
                entry.failure = Some(StorageError::Conflict);
                count_change(&mut self.changes);
                return Begin::Refused;
            }
            (mode, _) => mode,
        };
        entry.failure = None;
        entry.writing = true;
        Begin::Write(slot, mode)
    }

    /// Record how the write of `slot` under `name` ended, and start the
    /// write of bytes published meanwhile. `None` is a write dropped before
    /// it finished.
    fn finish(
        &mut self,
        name: StorageName,
        slot: Slot,
        result: Option<Result<StoredVersion, StorageError>>,
    ) -> Begin {
        // A write in flight keeps its entry; the default only keeps a drop
        // from panicking if that ever breaks.
        let entry = self.entries.entry(name).or_default();
        entry.writing = false;
        let next = match result {
            Some(Ok(version)) => {
                self.committed.insert(
                    name,
                    CommittedWrite {
                        revision: slot.revision,
                        version,
                        publisher: slot.publisher,
                    },
                );
                count_change(&mut self.changes);
                self.begin(name, false)
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
                self.begin(name, false)
            }
        };
        if self.entries.get(&name).is_some_and(Entry::is_clean) {
            self.entries.remove(&name);
        }
        next
    }
}

/// What the host gave the registry to write with: the storage, the IO pool
/// that runs the writes, and the channel that tells the owner a write
/// finished.
struct RegistryHost {
    storage: Arc<dyn Storage>,
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
        name: StorageName,
        slot: Slot,
        result: Option<Result<StoredVersion, StorageError>>,
    ) {
        let (next, changed) = {
            let mut slots = self.slots.lock();
            let before = slots.changes;
            let next = slots.finish(name, slot, result);
            (next, slots.changes != before)
        };
        self.write_ended.notify_all();
        self.start(name, next);
        if changed {
            self.signal();
        }
    }

    /// Hand a write [`Slots::begin`] started to the IO pool. A write the
    /// pool refuses is dropped here, which keeps its bytes owed.
    fn start(self: &Arc<Self>, name: StorageName, begin: Begin) {
        if let Begin::Write(slot, mode) = begin {
            self.hand_to_pool(PendingWrite::new(Arc::clone(self), name, slot, mode));
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

/// One write of one name: asks the storage on its first poll and records
/// the answer. Dropped before the answer, it keeps its bytes owed — unless
/// newer bytes were published meanwhile, which then win. Bytes the storage
/// had already accepted may still land; the flush then writes them again.
struct PendingWrite {
    shared: Arc<Shared>,
    name: StorageName,
    /// The bytes being written; `None` once the answer is recorded.
    slot: Option<Slot>,
    mode: WriteMode,
    answer: Option<StorageFuture<StoredVersion>>,
}

impl PendingWrite {
    fn new(shared: Arc<Shared>, name: StorageName, slot: Slot, mode: WriteMode) -> Self {
        Self {
            shared,
            name,
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
            this.shared
                .host
                .storage
                .publish(&this.name, slot.bytes.to_vec(), this.mode)
        });
        let Poll::Ready(result) = answer.as_mut().poll(cx) else {
            return Poll::Pending;
        };
        this.answer = None;
        let slot = this
            .slot
            .take()
            .expect("BUG: an unfinished write holds its bytes");
        this.shared.finish(this.name, slot, Some(result));
        Poll::Ready(())
    }
}

impl Drop for PendingWrite {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            self.shared.finish(self.name, slot, None);
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
    /// A new publisher of `name`. Each call is a different publisher; its
    /// clones are the same one.
    ///
    /// A publisher's own edits are one line: an edit based on the version its
    /// own previous write replaced is rebased onto that write instead of
    /// conflicting. An edit based on a version another publisher replaced
    /// fails with [`StorageError::Conflict`].
    #[must_use]
    pub fn publisher(&self, name: StorageName) -> FlushPublisher {
        FlushPublisher {
            shared: Arc::clone(&self.shared),
            name,
            identity: Arc::new(PublisherIdentity),
        }
    }

    /// The document's revision last written for `name`, as it was
    /// published, with the stored version the storage confirmed for it;
    /// `None` before the first confirmed write.
    #[must_use]
    pub fn committed(&self, name: StorageName) -> Option<(Revision, StoredVersion)> {
        self.shared
            .slots
            .lock()
            .committed
            .get(&name)
            .map(|write| (write.revision, write.version))
    }

    /// Where the bytes published under `name` stand.
    #[must_use]
    pub fn state(&self, name: StorageName) -> FlushState {
        self.shared
            .slots
            .lock()
            .entries
            .get(&name)
            .map_or(FlushState::Clean, Entry::state)
    }

    /// A future that resolves at the first write that finishes, fails or is
    /// refused, for any name, after it was taken: what changes
    /// [`state`](Self::state) or [`committed`](Self::committed) other than
    /// the publisher's own publication. It wakes on the owner's turn, never
    /// on the IO thread that finished the write. Take it before reading the
    /// state.
    pub fn changed(&self) -> FlushChanged {
        let taken_at = self.shared.slots.lock().changes;
        FlushChanged {
            shared: Arc::clone(&self.shared),
            taken_at,
        }
    }
}

/// One document's right to publish under one name; see
/// [`FlushRegistry::publisher`].
#[derive(Clone)]
pub struct FlushPublisher {
    shared: Arc<Shared>,
    name: StorageName,
    identity: Arc<PublisherIdentity>,
}

impl fmt::Debug for FlushPublisher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushPublisher")
            .field("name", &self.name.as_str())
            .finish_non_exhaustive()
    }
}

impl FlushPublisher {
    /// The name this publisher writes.
    #[must_use]
    pub fn name(&self) -> StorageName {
        self.name
    }

    /// Make `bytes` the latest value of the name, to be written under
    /// `mode`, replacing bytes published earlier that are not written yet.
    /// `revision` is the document's own revision of these bytes, the one
    /// written in their header; the registry records it and never mints one.
    /// Owner thread; never waits for the storage.
    ///
    /// When no write of the name is in flight, the write starts on the IO
    /// pool now; otherwise it starts when the one in flight finishes.
    pub fn publish(&self, bytes: Arc<[u8]>, mode: WriteMode, revision: Revision) {
        let (begin, changed) = {
            let mut slots = self.shared.slots.lock();
            let entry = slots.entries.entry(self.name).or_default();
            entry.owed = Some(Slot {
                bytes,
                mode,
                revision,
                publisher: Arc::clone(&self.identity),
            });
            entry.failure = None;
            let before = slots.changes;
            let begin = slots.begin(self.name, false);
            (begin, slots.changes != before)
        };
        self.shared.start(self.name, begin);
        if changed {
            self.shared.signal();
        }
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

/// The host's side of the flush registry: it builds the registry, settles it
/// on the owner's turn and flushes it when the application or the session
/// ends. Only the host holds it; documents get a [`FlushRegistry`].
pub struct FlushHost {
    shared: Arc<Shared>,
}

impl fmt::Debug for FlushHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushHost").finish_non_exhaustive()
    }
}

impl FlushHost {
    /// A registry writing through `storage`, starting each write on the IO
    /// pool through `writer`, and signalling the owner through `notify` when
    /// a write finishes.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>, writer: FlushWriter, notify: OwnerNotify) -> Self {
        Self {
            shared: Arc::new(Shared {
                slots: Mutex::new(Slots::default()),
                write_ended: Condvar::new(),
                host: RegistryHost {
                    storage,
                    writer,
                    notify,
                },
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

    /// Write the latest unconfirmed bytes of every name before `deadline`,
    /// waiting for a write already in flight first. Synchronous, on the owner
    /// thread; runs no application code. Failed bytes are written again,
    /// each name at most once per flush.
    ///
    /// A write still unanswered at the deadline is handed to the IO pool to
    /// finish, and its name is reported unfinished.
    #[must_use]
    pub fn flush_within(&self, deadline: Instant) -> FlushReport {
        let shared = &self.shared;
        let owed: HashSet<StorageName> = shared.slots.lock().entries.keys().copied().collect();
        let mut attempted = HashSet::new();
        let mut refused = false;
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
            let Some(name) = slots
                .entries
                .iter()
                .find(|(name, entry)| entry.owed.is_some() && !attempted.contains(*name))
                .map(|(name, _)| *name)
            else {
                break;
            };
            attempted.insert(name);
            let before = slots.changes;
            let begin = slots.begin(name, true);
            refused |= slots.changes != before;
            drop(slots);
            let Begin::Write(slot, mode) = begin else {
                continue;
            };
            let mut write = PendingWrite::new(Arc::clone(shared), name, slot, mode);
            if !poll_until(Pin::new(&mut write), deadline) {
                shared.hand_to_pool(write);
                break;
            }
        }
        if refused {
            shared.signal();
        }
        let slots = shared.slots.lock();
        let mut report = FlushReport::default();
        let mut touched = owed;
        touched.extend(attempted);
        for name in touched {
            match slots.entries.get(&name).map(Entry::state) {
                None | Some(FlushState::Clean) => report.written.push(name),
                Some(FlushState::Failed(error)) => report.failed.push((name, error)),
                Some(FlushState::Pending) => report.unfinished.push(name),
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

    use flui_platform::{StorageError, StorageName, StoredVersion, WriteMode};
    use flui_scheduler::AppLifecycleState;
    use flui_testing::storage::MemoryStorage;
    use parking_lot::Mutex;

    use super::{FlushHost, FlushState, FlushWrite};
    use crate::__runtime::LifecycleSource;
    use crate::owner_notify::{LifecycleEvent, OwnerNotify};
    use crate::persist::Revision;

    const SESSION: StorageName = StorageName::machine_local("session");
    const NOTES: StorageName = StorageName::from_static("notes");

    /// A host over `storage` with no IO pool, so every write is left to the
    /// flush, recording the events it signals its owner with.
    fn host(storage: &MemoryStorage) -> (FlushHost, Arc<Mutex<Vec<LifecycleEvent>>>) {
        let signals = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&signals);
        let host = FlushHost::new(
            Arc::new(storage.clone()),
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

    /// A host over `storage` whose IO pool accepts every write and runs it
    /// only when the test calls [`run_next`].
    fn host_with_pool(storage: &MemoryStorage) -> (FlushHost, Pool) {
        let pool = Pool::default();
        let queue = Arc::clone(&pool);
        let host = FlushHost::new(
            Arc::new(storage.clone()),
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
        let (host, _signals) = host(&storage);
        let publisher = host.registry().publisher(SESSION);
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
            host.registry()
                .committed(SESSION)
                .map(|(revision, _)| revision),
            Some(CLOSING),
            "the committed revision is the document's, as published"
        );
    }

    /// A publisher's edit based on the version its own earlier write
    /// replaced is rebased onto that write; the same stale base from another
    /// publisher conflicts and leaves the stored value alone.
    fn a_publishers_own_line_rebases_and_another_conflicts() {
        let storage = MemoryStorage::new();
        let (host, _signals) = host(&storage);
        let registry = host.registry();
        let ours = registry.publisher(NOTES);
        let base = WriteMode::IfUnchanged(StoredVersion::ABSENT);

        ours.publish(Arc::from(&b"one"[..]), base, Revision::new(1));
        let _ = flush(&host);
        ours.publish(Arc::from(&b"two"[..]), base, Revision::new(2));
        let _ = flush(&host);
        let ours_written = storage.contents(&NOTES);
        let ours_state = registry.state(NOTES);

        registry
            .publisher(NOTES)
            .publish(Arc::from(&b"theirs"[..]), base, Revision::new(3));
        let _ = flush(&host);

        assert_eq!(
            ours_written.as_deref(),
            Some(&b"two"[..]),
            "the publisher's own second edit is rebased and written"
        );
        assert_eq!(ours_state, FlushState::Clean);
        assert_eq!(
            registry.state(NOTES),
            FlushState::Failed(StorageError::Conflict),
            "another publisher's stale base conflicts"
        );
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"two"[..]));
    }

    /// A finished write signals the owner and wakes `changed()` only when
    /// the owner settles the registry on its turn.
    fn a_finished_write_wakes_changed_on_the_owner_turn() {
        let storage = MemoryStorage::new();
        let (host, signals) = host(&storage);
        let registry = host.registry();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut cx = Context::from_waker(&waker);
        let mut changed = std::pin::pin!(registry.changed());
        let _ = changed.as_mut().poll(&mut cx);

        registry.publisher(SESSION).publish(
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
        let (host, _signals) = host(&storage);
        let registry = host.registry();
        let notes = registry.publisher(NOTES);
        storage.fail_next(StorageError::Full);
        notes.publish(Arc::from(&b"one"[..]), WriteMode::Replace, Revision::new(1));

        let failed = flush(&host);
        let failed_state = registry.state(NOTES);
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
        let replaced_state = registry.state(NOTES);
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
            registry.committed(NOTES).map(|(revision, _)| revision),
            Some(Revision::new(3))
        );
    }

    /// Edits a publisher makes while its write is in flight are written once,
    /// after it, rebased onto it: the storage ends with the latest edit, not
    /// a conflict.
    fn same_writer_edits_rebase_instead_of_conflicting() {
        let storage = MemoryStorage::new();
        let (host, pool) = host_with_pool(&storage);
        let registry = host.registry();
        let notes = registry.publisher(NOTES);
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
        assert_eq!(registry.state(NOTES), FlushState::Clean);
        assert_eq!(storage.contents(&NOTES).as_deref(), Some(&b"three"[..]));
        assert_eq!(
            registry.committed(NOTES).map(|(revision, _)| revision),
            Some(Revision::new(3))
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
