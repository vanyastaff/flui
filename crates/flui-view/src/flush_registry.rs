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

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use flui_platform_api::{Storage, StorageError, StorageName, StoredVersion, WriteMode};
use parking_lot::Mutex;

use crate::owner_notify::OwnerNotify;
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
/// Compared by `Arc` address. Both the pending slot and the committed
/// record hold the publisher's `Arc` itself — never a raw address or a
/// `Weak` — so the allocation stays alive as long as any record names it,
/// and a publisher made later can never be handed the same address and
/// pass for the one that committed.
struct PublisherIdentity;

/// The latest bytes published under one name.
#[expect(dead_code, reason = "written once the registry flushes")]
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
    #[expect(dead_code, reason = "compared once the registry rebases edits")]
    publisher: Arc<PublisherIdentity>,
}

/// The registry's state: one slot of unwritten bytes per published name,
/// and the last confirmed write per name.
#[derive(Default)]
struct Slots {
    by_name: HashMap<StorageName, Slot>,
    committed: HashMap<StorageName, CommittedWrite>,
}

/// What the host gave the registry to write with: the storage, the IO pool
/// that runs the writes, and the channel that tells the owner a write
/// finished.
#[expect(dead_code, reason = "written through once the registry flushes")]
struct RegistryHost {
    storage: Arc<dyn Storage>,
    writer: FlushWriter,
    notify: OwnerNotify,
}

/// What the registry's sides share.
struct Shared {
    slots: Mutex<Slots>,
    #[expect(dead_code, reason = "written through once the registry flushes")]
    host: RegistryHost,
}

/// The publishing side of the host's flush registry: where a document gets
/// its [`FlushPublisher`] and reads how its bytes stand.
///
/// Cheap to clone; every clone is the same registry.
///
/// Not yet wired: published bytes are kept, but nothing is written to the
/// storage, so no write is ever recorded as committed.
#[derive(Clone)]
pub struct FlushRegistry {
    shared: Arc<Shared>,
}

impl fmt::Debug for FlushRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushRegistry")
            .field("published", &self.shared.slots.lock().by_name.len())
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
        if self.shared.slots.lock().by_name.contains_key(&name) {
            FlushState::Pending
        } else {
            FlushState::Clean
        }
    }

    /// A future that resolves at the first change to what
    /// [`state`](Self::state) or [`committed`](Self::committed) answers for
    /// any name after it was taken. It wakes on the owner's turn, never on
    /// the IO thread that finished the write. Take it before reading the
    /// state.
    pub fn changed(&self) -> FlushChanged {
        FlushChanged {
            _shared: Arc::clone(&self.shared),
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
    pub fn publish(&self, bytes: Arc<[u8]>, mode: WriteMode, revision: Revision) {
        self.shared.slots.lock().by_name.insert(
            self.name,
            Slot {
                bytes,
                mode,
                revision,
                publisher: Arc::clone(&self.identity),
            },
        );
    }
}

/// Resolves at the next change to the registry; see
/// [`FlushRegistry::changed`].
#[must_use = "a future does nothing unless polled"]
pub struct FlushChanged {
    _shared: Arc<Shared>,
}

impl fmt::Debug for FlushChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushChanged").finish_non_exhaustive()
    }
}

impl Future for FlushChanged {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        // Nothing is written yet, so nothing changes.
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

    /// Settle the registry on the owner's turn: record the writes that
    /// finished, and return the wakers of the [`FlushChanged`] futures they
    /// resolved.
    ///
    /// Nothing is woken here: the registry's state is committed and its lock
    /// released before this returns, and the owner wakes the wakers itself,
    /// inside its single containment boundary, one by one, so a waker that
    /// panics does not lose the ones after it.
    #[must_use = "the owner wakes the returned wakers"]
    pub fn settle(&self) -> Vec<Waker> {
        Vec::new()
    }

    /// Write the latest unconfirmed bytes of every name before `deadline`,
    /// waiting for a write already in flight first. Synchronous, on the owner
    /// thread; runs no application code.
    #[must_use]
    pub fn flush_within(&self, deadline: Instant) -> FlushReport {
        // Nothing is written yet: every published name is unfinished.
        let _ = deadline;
        FlushReport {
            unfinished: self.shared.slots.lock().by_name.keys().copied().collect(),
            ..FlushReport::default()
        }
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

    use flui_platform_api::{StorageError, StorageName, StoredVersion, WriteMode};
    use flui_scheduler::AppLifecycleState;
    use flui_testing::storage::MemoryStorage;
    use parking_lot::Mutex;

    use super::{FlushHost, FlushState};
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

    fn flush(host: &FlushHost) -> super::FlushReport {
        host.flush_within(Instant::now() + Duration::from_secs(5))
    }

    /// Bytes published while the presentation is told it is detached are on
    /// the storage once the host's teardown flush returns, committed at the
    /// document's own revision.
    #[test]
    #[ignore = "contract: the teardown flush writes bytes published at Detached"]
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
    #[test]
    #[ignore = "contract: a publisher's own line rebases, another publisher conflicts"]
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
    #[test]
    #[ignore = "contract: a finished write wakes waiters on the owner's turn"]
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

    /// Counts its wakes, so a test can tell where a wake happened.
    #[derive(Default)]
    struct CountingWaker(AtomicUsize);

    impl std::task::Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
