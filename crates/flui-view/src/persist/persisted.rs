//! [`Persisted`], the owner-local handle to one stored document.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_foundation::ListenerId;
use flui_foundation::notifier::{ChangeNotifier, Listenable, ListenerObserver};
use flui_foundation::panic::{payload_text, retain_opaque_payload};
use flui_platform::{
    CopiedAside, StorageError, StorageFuture, Stored, StoredVersion, Unsupported,
    UnsupportedReason, WriteMode,
};
use flui_scheduler::Instant;

use super::{CodecStep, Document, PersistError, ReadOnlyReason, Revision, SaveStatus};
use super::{StorageCapability, StorageHandle};
use crate::context::CrateToken;
use crate::flush_registry::{FlushPublisher, FlushRegistry, FlushState};
use crate::{
    AppLifecycleState, AsyncDriver, CloseGuard, CloseHold, LifecycleSubscription, TaskToken,
    TimerHandle,
};
use crate::{LifecycleContext, LifecycleContextExt, UnmountHook};

/// The first word of a stored document's header line.
const HEADER_MAGIC: &str = "flui-document";

/// What a write meets when the UI runtime's storage has no flush registry
/// beside it, as with a storage provider and no host behind it.
const NO_REGISTRY: Unsupported = Unsupported::new(UnsupportedReason::NotRegistered);

/// How long a write the store refused as busy waits before each retry; a
/// busy answer after the last is a failure.
const BUSY_BACKOFF: [Duration; 5] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
];

/// One [`Document`] kept for a widget: loaded from the UI runtime's storage,
/// written on every [`set`](Self::set), with its [`SaveStatus`].
///
/// Open it in `ViewState::init_state` and keep it in the state; clones share
/// the same document. It is owner-local (`!Send`): the value, the codec and
/// the status stay on the owner thread, and only bytes travel to the thread
/// that does the IO.
///
/// While a write is in flight the document holds its presentation's close
/// (a user's or the program's close waits; a session end goes through and
/// the framework writes the published bytes), and after a failed write it
/// requires a decision before any close the platform lets it refuse. Where
/// the platform cannot wait (the web, Android, iOS), what changed after the
/// last confirmed write is lost if the process is killed before the
/// application is hidden.
///
/// It is [`Listenable`]: its listeners are told when [`status`](Self::status)
/// or [`committed`](Self::committed) changes, so a widget can rebuild with
/// them. They are told on a later turn of the UI runtime's tasks, never from
/// inside a call on the document, and once for all the changes since the
/// last time they were told, also when the state is back on what they were
/// told by then: a reader may have seen it in between. A call that changes
/// nothing tells nobody. A listener may call any method of the document. The
/// listeners belong to the state that opened the document: they are dropped
/// when it unmounts, so one may hold a clone of the document, and a listener
/// added after that is not kept. A [`WeakPersisted`] from [`downgrade`](Self::downgrade) reads the
/// document without keeping it alive.
pub struct Persisted<D: Document> {
    document: Rc<OpenDocument<D>>,
}

/// What the clones of one [`Persisted`] share.
struct OpenDocument<D> {
    /// The UI runtime's storage, or why it has none.
    storage: Result<StorageHandle, Unsupported>,
    /// The host's flush registry, which `set` publishes into, with this
    /// document's publisher in it.
    registry: Option<(FlushRegistry, FlushPublisher)>,
    /// Where the watch over the registry runs.
    driver: Option<AsyncDriver>,
    /// The runtime's clock, which a busy store's retry waits on.
    timer: Option<TimerHandle>,
    /// Set when the application is hidden or paused: a waiting retry runs now.
    hurry: Rc<Hurry>,
    /// The lifecycle observation that sets `hurry`.
    _lifecycle: Option<LifecycleSubscription>,
    /// The presentation's close guard, which a write in flight or a failed
    /// one holds.
    guard: Option<CloseGuard>,
    /// The opening state's unmount, which drops the listeners.
    unmount: RefCell<Option<UnmountHook>>,
    /// Tells the document's listeners of changes.
    announcer: Announcer,
    /// Borrowed only between codec calls, storage requests and wakes.
    state: RefCell<DocumentState<D>>,
}

/// Where one document stands; see [`Persisted::status`].
struct DocumentState<D> {
    status: SaveStatus,
    /// The number of the latest load; an earlier load's result is not
    /// applied.
    generation: u64,
    /// The value the last applied load produced, and every value set since.
    loaded: Option<Loaded<D>>,
    /// The registry's last confirmed write when the last applied load
    /// issued its read: a later one means another document of this name
    /// wrote.
    committed_at_load: Option<(Revision, StoredVersion)>,
    /// The latest bytes handed to the registry.
    published: Option<Published>,
    /// The first failure since the last write that succeeded; later ones
    /// are logged.
    failure: Option<PersistError>,
    /// The close hold the status calls for.
    hold: Option<(HoldKind, CloseHold)>,
    /// The watch over the registry while a write is in flight or the
    /// document has listeners, if one runs.
    watch: Option<TaskToken>,
    /// Whether the watch is still running.
    watching: bool,
    /// How many times the latest bytes were tried again after a busy store.
    busy_retries: usize,
    /// When the retry waiting out a busy store's back-off is due, if one
    /// waits. A wait that ends is matched against it, so a wait made for
    /// an earlier edit never retries a later one.
    busy_deadline: Option<Instant>,
    /// The version of the stored file the latest load could not read, if
    /// it failed on the file's bytes: what a fresh start writes over.
    unreadable: Option<StoredVersion>,
    /// How far a fresh start [`Persisted::start_over`] began has come,
    /// until its value is handed to the registry.
    restart: Option<Restart>,
    /// The copy aside in flight, if one runs.
    copying: Option<TaskToken>,
}

/// A loaded value and the stored file it came from.
struct Loaded<D> {
    value: Rc<D>,
    /// The version of the stored bytes, which a data write is based on.
    base: StoredVersion,
    /// The revision of `value`: the stored header's when it was loaded, 0
    /// when nothing was stored, the latest set's after that.
    revision: Revision,
}

/// Bytes handed to the registry, kept for a retry.
struct Published {
    bytes: Arc<[u8]>,
    mode: WriteMode,
    revision: Revision,
}

/// The steps of a fresh start: keep the unreadable bytes, then write the
/// initial value over exactly those bytes.
#[derive(Clone, Copy)]
enum Restart {
    /// The unreadable bytes, which the failed load read at this version,
    /// are not kept yet.
    Copying(StoredVersion),
    /// They are kept; the initial value is written only over this version,
    /// the one the failed load read.
    Copied(StoredVersion),
}

/// Which close hold a document keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HoldKind {
    /// A write in flight: [`CloseGuard::hold`].
    Work,
    /// A failed write: [`CloseGuard::require_decision`].
    Decision,
}

/// What a document's listeners are told about: its status, the latest
/// revision the storage confirmed and the revision of the value in memory.
type Reported = (SaveStatus, Option<Revision>, Option<Revision>);

/// Tells a document's listeners of changes, one delivery at a time on the
/// UI runtime's tasks.
struct Announcer {
    notifier: ChangeNotifier,
    /// What the listeners were last told, or what stood at open.
    told: RefCell<Reported>,
    /// Whether a delivery is queued and has not started yet.
    queued: Cell<bool>,
    /// Whether the state moved off what the listeners were told since the
    /// last delivery, even if it is back there now: a reader may have seen
    /// the state in between, so the delivery tells them.
    moved: Cell<bool>,
    /// The latest delivery's task.
    delivery: RefCell<Option<TaskToken>>,
    /// Set when the state that opened the document unmounts: nobody is
    /// told again, and a listener added later is not kept.
    closed: Cell<bool>,
}

impl Announcer {
    /// The opening state unmounted: drop the queued delivery, then the
    /// listeners, whose captures may hold clones of the document.
    fn close(&self) {
        self.closed.set(true);
        let delivery = self.delivery.borrow_mut().take();
        drop(delivery);
        Listenable::remove_all_listeners(&self.notifier);
    }
}

impl<D: Document> Clone for Persisted<D> {
    fn clone(&self) -> Self {
        Self {
            document: Rc::clone(&self.document),
        }
    }
}

/// A [`Persisted`] that does not keep its document alive, for a listener of
/// the document to hold: [`upgrade`](Self::upgrade) gives the document while
/// another clone still holds it.
pub struct WeakPersisted<D: Document> {
    document: Weak<OpenDocument<D>>,
}

impl<D: Document> WeakPersisted<D> {
    /// The document, unless every [`Persisted`] of it is gone.
    #[must_use]
    pub fn upgrade(&self) -> Option<Persisted<D>> {
        self.document
            .upgrade()
            .map(|document| Persisted { document })
    }
}

impl<D: Document> Clone for WeakPersisted<D> {
    fn clone(&self) -> Self {
        Self {
            document: Weak::clone(&self.document),
        }
    }
}

impl<D: Document> fmt::Debug for WeakPersisted<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakPersisted")
            .field("name", &D::NAME.as_str())
            .field("alive", &(self.document.strong_count() > 0))
            .finish()
    }
}

impl<D: Document> fmt::Debug for Persisted<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Persisted")
            .field("name", &D::NAME.as_str())
            .field("status", &self.status())
            .finish()
    }
}

impl<D: Document> Persisted<D> {
    /// The document [`D::NAME`](Document::NAME) in the storage of the UI runtime
    /// `cx` belongs to. Call it from `init_state` or
    /// `did_change_dependencies`; nothing is read until [`load`](Self::load).
    #[must_use]
    pub fn open(cx: &dyn LifecycleContext) -> Self {
        let storage = cx.capability::<StorageCapability>();
        let status = if storage.is_ok() {
            SaveStatus::NotLoaded(None)
        } else {
            SaveStatus::Unavailable
        };
        // Writes go to the storage the document acquired, whichever
        // provider that is.
        let registry = storage.as_ref().ok().and_then(|handle| {
            cx.flush_registry_in_crate(CrateToken::new())
                .map(|registry| {
                    let publisher = registry.publisher(D::NAME, Arc::clone(handle.store()));
                    (registry, publisher)
                })
        });
        let committed = registry
            .as_ref()
            .and_then(|(_, publisher)| publisher.committed())
            .map(|(revision, _)| revision);
        let hurry = Rc::new(Hurry::default());
        let lifecycle = cx.lifecycle_handle().and_then(|handle| {
            let hurry = Rc::downgrade(&hurry);
            handle
                .subscribe(move |state| {
                    if matches!(state, AppLifecycleState::Hidden | AppLifecycleState::Paused)
                        && let Some(hurry) = hurry.upgrade()
                    {
                        hurry.request();
                    }
                })
                .ok()
                .map(|(_, subscription)| subscription)
        });
        let document = Rc::new(OpenDocument {
            storage,
            registry,
            driver: cx.async_driver(),
            timer: cx.timer(),
            hurry,
            _lifecycle: lifecycle,
            guard: cx.close_guard(),
            unmount: RefCell::new(None),
            announcer: Announcer {
                notifier: ChangeNotifier::new(),
                told: RefCell::new((status.clone(), committed, None)),
                queued: Cell::new(false),
                moved: Cell::new(false),
                delivery: RefCell::new(None),
                closed: Cell::new(false),
            },
            state: RefCell::new(DocumentState {
                status,
                generation: 0,
                loaded: None,
                published: None,
                committed_at_load: None,
                failure: None,
                hold: None,
                watch: None,
                watching: false,
                busy_retries: 0,
                busy_deadline: None,
                unreadable: None,
                restart: None,
                copying: None,
            }),
        });
        // The listeners belong to the state that opened the document: they go
        // when it unmounts, whichever clones of the document live on.
        let closing = Rc::downgrade(&document);
        let hook = cx.on_unmount(Box::new(move || {
            if let Some(document) = closing.upgrade() {
                document.announcer.close();
            }
        }));
        *document.unmount.borrow_mut() = Some(hook);
        Self { document }
    }

    /// Read the stored document: [`Document::initial`] when nothing is
    /// stored, the decoded value otherwise. The returned future runs on the
    /// owner thread; a later load replaces an earlier one, whose result is
    /// returned but not applied. Dropping the future cancels the read.
    ///
    /// # Errors
    ///
    /// The [`PersistError`] that kept the document from loading: the
    /// storage's error ([`StorageError::Unsupported`] when the UI runtime has
    /// no storage, [`StorageError::TooLarge`] for a file over
    /// [`Document::MAX_BYTES`]), corrupt bytes, a body the document cannot
    /// decode, a newer format, a contained codec panic, or edits not yet
    /// stored.
    ///
    /// The future is not promised to be [`Unpin`]: pin it (`Box::pin`,
    /// `std::pin::pin!`) before polling it by hand.
    pub fn load(&self) -> impl Future<Output = Result<Rc<D>, PersistError>> + 'static {
        let document = Rc::clone(&self.document);
        document.refresh();
        let start = document.start_load();
        async move {
            let LoadStart {
                generation,
                baseline,
                read,
            } = start?;
            let (outcome, read) = match read.await {
                Ok(stored) => {
                    let version = stored.version;
                    (decode_stored::<D>(stored), Some(version))
                }
                Err(error) => (Err(PersistError::Storage(error)), None),
            };
            document.settle(generation, baseline, outcome, read)
        }
    }

    /// Make `value` the document's value and hand it to the storage,
    /// returning the revision it will be stored as. Saving does not wait for
    /// the write: the status is [`SaveStatus::Saving`] until the storage
    /// confirms it, and [`committed`](Self::committed) then reaches the
    /// returned revision.
    ///
    /// A data document is written only over the stored version it was
    /// loaded from (or its own later writes), so another writer's change is
    /// a [`StorageError::Conflict`]; a [machine-local](flui_platform::StorageName::machine_local)
    /// one replaces whatever is stored.
    ///
    /// # Errors
    ///
    /// Why the value was refused: the UI runtime has no storage
    /// ([`StorageError::Unsupported`]), no value is loaded to base the write
    /// on ([`PersistError::NotLoaded`]), the stored file is read-only, the
    /// revision counter is exhausted, or [`Document::encode`] panicked. The
    /// value in memory and the file stay as they were; after an encode panic
    /// the status is [`SaveStatus::Failed`] until a write succeeds.
    pub fn set(&self, value: D) -> Result<Revision, PersistError> {
        let document = &self.document;
        let (revision, mode) = document.next_write()?;
        let Some((_, publisher)) = &document.registry else {
            return Err(PersistError::Storage(NO_REGISTRY.into()));
        };
        let body = match catch_unwind(AssertUnwindSafe(|| value.encode())) {
            Ok(body) => body,
            Err(payload) => {
                let message = payload_text(&*payload)
                    .unwrap_or("a panic without a message")
                    .to_owned();
                retain_opaque_payload(payload);
                let error = PersistError::Panicked {
                    during: CodecStep::Encode,
                    message,
                };
                document.fail(error.clone());
                return Err(error);
            }
        };
        let bytes: Arc<[u8]> = Arc::from(framed::<D>(revision, &body));
        let replaced = {
            let mut state = document.state.borrow_mut();
            // A load still in flight read the file before this edit: its
            // result is not applied over it.
            state.generation = state.generation.wrapping_add(1);
            let loaded = state
                .loaded
                .as_mut()
                .expect("BUG: next_write found a loaded value");
            loaded.revision = revision;
            let replaced = std::mem::replace(&mut loaded.value, Rc::new(value));
            state.failure = None;
            document.clear_busy(&mut state);
            state.published = Some(Published {
                bytes: Arc::clone(&bytes),
                mode,
                revision,
            });
            state.status = SaveStatus::Saving;
            replaced
        };
        publisher.publish(bytes, mode, revision);
        // The replaced value is dropped after this edit is published, so an
        // edit its drop makes is published after this one and wins.
        then_retire(|| document.refresh(), replaced);
        Ok(revision)
    }

    /// A handle that does not keep this document alive, for its listeners
    /// to hold.
    #[must_use]
    pub fn downgrade(&self) -> WeakPersisted<D> {
        WeakPersisted {
            document: Rc::downgrade(&self.document),
        }
    }

    /// The latest revision the storage confirmed, if any.
    #[must_use]
    pub fn committed(&self) -> Option<Revision> {
        self.document.committed()
    }

    /// The revision of the value in memory while it is the stored one: the
    /// stored file's after a load ([`Revision::NONE`] when nothing was
    /// stored), a set value's once the storage confirms it. `None` before a
    /// load succeeds and while a value is not yet stored, so a revision read
    /// here always names bytes on disk: an edit lost before its write
    /// commits never matches what a later load reads back.
    #[must_use]
    pub fn revision(&self) -> Option<Revision> {
        self.document.refresh();
        self.document.revision()
    }

    /// Where the document stands against its storage.
    #[must_use]
    pub fn status(&self) -> SaveStatus {
        self.document.refresh();
        self.document.state.borrow().status.clone()
    }

    /// After a failed write, write the latest value again, or take up a
    /// failed [`start_over`](Self::start_over) where it stopped; otherwise do
    /// nothing.
    pub fn retry(&self) {
        let document = &self.document;
        document.refresh();
        let republish = {
            let mut state = document.state.borrow_mut();
            if !matches!(state.status, SaveStatus::Failed(_)) {
                return;
            }
            state.failure = None;
            document.clear_busy(&mut state);
            if state.restart.is_some() {
                state.status = SaveStatus::Saving;
                drop(state);
                document.restart();
                return;
            }
            let republish = state.published.as_ref().map(|published| {
                (
                    Arc::clone(&published.bytes),
                    published.mode,
                    published.revision,
                )
            });
            state.status = if republish.is_some() {
                SaveStatus::Saving
            } else {
                SaveStatus::Clean
            };
            republish
        };
        if let (Some((bytes, mode, revision)), Some((_, publisher))) =
            (republish, &document.registry)
        {
            publisher.publish(bytes, mode, revision);
        }
        document.refresh();
    }

    /// Give up on a stored file that cannot be read: keep its bytes under a
    /// free name `<name>.corrupt-<n>` beside it
    /// ([`Storage::copy_aside`](flui_platform::Storage::copy_aside)), then
    /// store [`Document::initial`] over exactly those bytes. Does nothing
    /// unless the last load found the file corrupt or could not decode it.
    ///
    /// The fresh start runs on the UI runtime's tasks; the status is
    /// [`SaveStatus::Saving`] until the initial value is stored, and the
    /// value is loaded, so [`set`](Self::set) works, once the copy is kept.
    /// A failed step leaves the status [`SaveStatus::Failed`], and
    /// [`retry`](Self::retry) takes up the fresh start where it stopped: a
    /// kept copy is not made again. A file that changed since the failed load
    /// read it is never written over: that write is a [`StorageError::Conflict`].
    pub fn start_over(&self) {
        let document = &self.document;
        document.refresh();
        {
            let mut state = document.state.borrow_mut();
            if !matches!(
                &state.status,
                SaveStatus::NotLoaded(Some(
                    PersistError::Corrupt { .. }
                        | PersistError::Decode(_)
                        | PersistError::Panicked {
                            during: CodecStep::Decode,
                            ..
                        }
                ))
            ) {
                return;
            }
            let Some(base) = state.unreadable else {
                return;
            };
            // A load still in flight is not applied over the fresh start.
            state.generation = state.generation.wrapping_add(1);
            state.restart = Some(Restart::Copying(base));
            state.status = SaveStatus::Saving;
        }
        document.restart();
    }
}

impl<D: Document> Listenable for Persisted<D> {
    /// The document follows the registry while it has listeners, so a
    /// commit by another document of this name is told too.
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        let announcer = &self.document.announcer;
        let id = announcer.notifier.add_observer(observer);
        if announcer.closed.get() {
            // The opening state is gone: the listener would never be told,
            // and is not kept.
            announcer.notifier.remove_listener(id);
            return id;
        }
        self.document.refresh();
        id
    }

    fn remove_listener(&self, id: ListenerId) {
        self.document.announcer.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.document.announcer.notifier.remove_all_listeners();
    }
}

/// A load in flight.
struct LoadStart {
    /// The generation that makes it the latest load.
    generation: u64,
    /// The registry's last confirmed write when the read was issued: a later
    /// one means the bytes read may no longer be stored.
    baseline: Option<(Revision, StoredVersion)>,
    /// The storage read.
    read: StorageFuture<Stored>,
}

impl<D: Document> OpenDocument<D> {
    /// Begin a load: refuse it over unsaved edits or without storage,
    /// otherwise make it the latest and issue the read.
    fn start_load(&self) -> Result<LoadStart, PersistError> {
        let mut state = self.state.borrow_mut();
        if matches!(state.status, SaveStatus::Saving | SaveStatus::Failed(_)) {
            return Err(PersistError::UnsavedChanges);
        }
        let storage = self
            .storage
            .as_ref()
            .map_err(|refusal| PersistError::Storage(StorageError::Unsupported(*refusal)))?;
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        drop(state);
        // Taken before the read is issued, so a write confirmed after it is
        // never taken for one the bytes read include.
        let baseline = self.committed_write();
        Ok(LoadStart {
            generation,
            baseline,
            read: storage.read(&D::NAME, D::MAX_BYTES),
        })
    }

    /// Apply a finished load if it is still the latest, and hand its
    /// outcome to the caller either way.
    fn settle(
        self: &Rc<Self>,
        generation: u64,
        baseline: Option<(Revision, StoredVersion)>,
        outcome: Result<Loaded<D>, PersistError>,
        read: Option<StoredVersion>,
    ) -> Result<Rc<D>, PersistError> {
        let mut state = self.state.borrow_mut();
        let latest = state.generation == generation;
        if latest {
            // Bytes written before this load belong to the value it replaces:
            // the status follows the load, not their write.
            state.published = None;
            state.unreadable = outcome.is_err().then_some(read).flatten();
        }
        let (result, replaced) = match outcome {
            Ok(loaded) => {
                let value = Rc::clone(&loaded.value);
                let replaced = if latest {
                    state.status = SaveStatus::Clean;
                    state.committed_at_load = baseline;
                    state.loaded.replace(loaded)
                } else {
                    Some(loaded)
                };
                (Ok(value), replaced)
            }
            Err(error) => {
                let replaced = if latest {
                    state.status = match &error {
                        PersistError::NewerVersion { found, supported } => {
                            SaveStatus::ReadOnly(ReadOnlyReason::NewerVersion {
                                found: *found,
                                supported: *supported,
                            })
                        }
                        other => SaveStatus::NotLoaded(Some(other.clone())),
                    };
                    state.loaded.take()
                } else {
                    None
                };
                (Err(error), replaced)
            }
        };
        drop(state);
        // Announced before the replaced value's drop: a drop that panics
        // leaves the settled load told.
        then_retire(|| self.announce(), replaced);
        result
    }

    /// The revision and write mode of the next value `set` writes, or why
    /// none is written.
    fn next_write(self: &Rc<Self>) -> Result<(Revision, WriteMode), PersistError> {
        self.refresh();
        let state = self.state.borrow();
        match &state.status {
            SaveStatus::Unavailable => {
                let refusal = self.storage.as_ref().err().copied().unwrap_or(NO_REGISTRY);
                return Err(PersistError::Storage(StorageError::Unsupported(refusal)));
            }
            SaveStatus::ReadOnly(ReadOnlyReason::NewerVersion { found, supported }) => {
                return Err(PersistError::NewerVersion {
                    found: *found,
                    supported: *supported,
                });
            }
            SaveStatus::ReadOnly(ReadOnlyReason::LockUnsupported) => {
                return Err(PersistError::Storage(StorageError::Unsupported(
                    Unsupported::new(UnsupportedReason::NotOnThisDevice),
                )));
            }
            _ => {}
        }
        let loaded = state.loaded.as_ref().ok_or(PersistError::NotLoaded)?;
        let revision = loaded
            .revision
            .get()
            .checked_add(1)
            .map(Revision::new)
            .ok_or(PersistError::Exhausted)?;
        let mode = if D::NAME.is_machine_local() {
            WriteMode::Replace
        } else {
            WriteMode::IfUnchanged(loaded.base)
        };
        Ok((revision, mode))
    }

    /// Run the next step of the fresh start [`Persisted::start_over`] began.
    fn restart(self: &Rc<Self>) {
        let step = self.state.borrow().restart;
        match step {
            None => {}
            Some(Restart::Copied(base)) => self.write_initial(base),
            Some(Restart::Copying(base)) => {
                let (Ok(storage), Some(driver), Some(_)) =
                    (&self.storage, &self.driver, &self.registry)
                else {
                    self.fail(PersistError::Storage(NO_REGISTRY.into()));
                    return;
                };
                let copy = storage.copy_aside(&D::NAME);
                // Spawning wakes the frame hook, which may panic; the copy is
                // then cancelled, and the fresh start is left failed, so
                // `retry` copies again. The hook's panic is raised, or kept
                // while a panic already unwinds.
                let spawned = catch_unwind(AssertUnwindSafe(|| {
                    driver.spawn_local(Box::pin(copy_aside(Rc::downgrade(self), copy, base)))
                }));
                let token = match spawned {
                    Ok(token) => token,
                    Err(payload) => {
                        let failed = catch_unwind(AssertUnwindSafe(|| {
                            self.fail(PersistError::Storage(StorageError::Cancelled));
                        }));
                        if let Err(again) = failed {
                            retain_opaque_payload(again);
                        }
                        if std::thread::panicking() {
                            retain_opaque_payload(payload);
                            return;
                        }
                        resume_unwind(payload);
                    }
                };
                let stale = self.state.borrow_mut().copying.replace(token);
                drop(stale);
                self.refresh();
            }
        }
    }

    /// Load [`Document::initial`] and hand it to the registry, written only
    /// over the stored version `base`.
    fn write_initial(self: &Rc<Self>, base: StoredVersion) {
        let Some((_, publisher)) = &self.registry else {
            self.fail(PersistError::Storage(NO_REGISTRY.into()));
            return;
        };
        // A panic in it is contained and recorded: the fresh start is left
        // failed and retryable, never `Saving` with nothing to finish it.
        let value = match contained_initial::<D>() {
            Ok(value) => value,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let body = match catch_unwind(AssertUnwindSafe(|| value.encode())) {
            Ok(body) => body,
            Err(payload) => {
                let message = payload_text(&*payload)
                    .unwrap_or("a panic without a message")
                    .to_owned();
                retain_opaque_payload(payload);
                // The failure is recorded before the value's drop, which is
                // application code and may panic in turn.
                self.fail(PersistError::Panicked {
                    during: CodecStep::Encode,
                    message,
                });
                drop(value);
                return;
            }
        };
        // The first revision of a file that starts over.
        let revision = Revision::new(1);
        let bytes: Arc<[u8]> = Arc::from(framed::<D>(revision, &body));
        let mode = WriteMode::IfUnchanged(base);
        let replaced = {
            let mut state = self.state.borrow_mut();
            state.restart = None;
            state.unreadable = None;
            state.failure = None;
            self.clear_busy(&mut state);
            state.published = Some(Published {
                bytes: Arc::clone(&bytes),
                mode,
                revision,
            });
            state.status = SaveStatus::Saving;
            state.loaded.replace(Loaded {
                value: Rc::new(value),
                base,
                revision,
            })
        };
        publisher.publish(bytes, mode, revision);
        // A replaced value is dropped after the initial value is published.
        then_retire(|| self.refresh(), replaced);
    }

    /// End a busy store's retries: the next refusal starts from the first
    /// back-off, and a hurry asked for before it is not carried into it.
    fn clear_busy(&self, state: &mut DocumentState<D>) {
        state.busy_retries = 0;
        state.busy_deadline = None;
        self.hurry.disarm();
    }

    /// Try the latest bytes again once the back-off that ends at `deadline`
    /// is over, unless that wait was replaced since.
    fn retry_busy(self: &Rc<Self>, deadline: Instant) {
        let republish = {
            let mut state = self.state.borrow_mut();
            if state.busy_deadline != Some(deadline) {
                return;
            }
            state.busy_deadline = None;
            state.busy_retries += 1;
            state.published.as_ref().map(|published| {
                (
                    Arc::clone(&published.bytes),
                    published.mode,
                    published.revision,
                )
            })
        };
        if let (Some((bytes, mode, revision)), Some((_, publisher))) = (republish, &self.registry) {
            publisher.publish(bytes, mode, revision);
        }
        self.refresh();
    }

    /// Record a failure that kept a value from being published.
    fn fail(self: &Rc<Self>, error: PersistError) {
        {
            let mut state = self.state.borrow_mut();
            if let Some(first) = &state.failure {
                tracing::warn!(
                    document = D::NAME.as_str(),
                    %error,
                    %first,
                    "a document failed again; the first failure is kept"
                );
            } else {
                state.failure = Some(error);
            }
        }
        self.refresh();
    }

    /// Bring the status in line with the registry, keep the close hold the
    /// status calls for, and keep the watch running while a write is in
    /// flight.
    fn refresh(self: &Rc<Self>) {
        let (released, stale_watch, spawn) = {
            let mut state = self.state.borrow_mut();
            if let (Some(_), Some((_, publisher))) = (&state.published, &self.registry) {
                let flush = publisher.state();
                match &flush {
                    FlushState::Clean => {
                        self.clear_busy(&mut state);
                        if matches!(state.failure, Some(PersistError::Storage(_))) {
                            state.failure = None;
                        }
                    }
                    FlushState::Pending => {}
                    // A busy store is tried again on the runtime's clock, up
                    // to the last back-off; the status stays `Saving`.
                    FlushState::Failed(StorageError::Busy)
                        if let Some(timer) = &self.timer
                            && state.failure.is_none()
                            && (state.busy_deadline.is_some()
                                || state.busy_retries < BUSY_BACKOFF.len()) =>
                    {
                        if state.busy_deadline.is_none() {
                            let backoff = BUSY_BACKOFF[state.busy_retries];
                            // A clock with no room for the back-off fails the
                            // write instead of waiting forever.
                            match timer.now().checked_add(backoff) {
                                Some(deadline) => {
                                    state.busy_deadline = Some(deadline);
                                    self.hurry.arm();
                                }
                                None => {
                                    state.failure = Some(PersistError::Storage(StorageError::Busy));
                                }
                            }
                        }
                    }
                    FlushState::Failed(error) => {
                        if state.failure.is_none() {
                            state.failure = Some(PersistError::Storage(error.clone()));
                        }
                    }
                }
                state.status = match (&state.failure, flush) {
                    (Some(failure), _) => SaveStatus::Failed(failure.clone()),
                    (None, FlushState::Clean) => SaveStatus::Clean,
                    (None, _) => SaveStatus::Saving,
                };
            } else if let Some(failure) = &state.failure {
                state.status = SaveStatus::Failed(failure.clone());
            }
            let wanted = match state.status {
                SaveStatus::Saving => Some(HoldKind::Work),
                SaveStatus::Failed(_) => Some(HoldKind::Decision),
                _ => None,
            };
            let released = if state.hold.as_ref().map(|(kind, _)| *kind) == wanted {
                None
            } else {
                let taken = wanted.zip(self.guard.as_ref()).map(|(kind, guard)| {
                    let hold = match kind {
                        HoldKind::Work => guard.hold(),
                        HoldKind::Decision => guard.require_decision(),
                    };
                    (kind, hold)
                });
                std::mem::replace(&mut state.hold, taken)
            };
            let saving = state.status == SaveStatus::Saving;
            // Listeners follow the registry too: another document of this
            // name may commit, which changes `committed()` here.
            let watched = saving || self.announcer.notifier.has_listeners();
            let spawn = watched && !state.watching && self.driver.is_some();
            let stale_watch = if spawn {
                state.watching = true;
                state.watch.take()
            } else {
                None
            };
            (released, stale_watch, spawn)
        };
        // Released after the borrow: a hold's release signals the owner.
        drop(released);
        drop(stale_watch);
        if spawn && let (Some(driver), Some((registry, _))) = (&self.driver, &self.registry) {
            // Spawning wakes the frame hook, which may panic; the task is then
            // cancelled, and nothing is left watching, so the next refresh
            // spawns it again. While a panic already unwinds, the hook's is
            // kept, not raised.
            let spawned = catch_unwind(AssertUnwindSafe(|| {
                driver.spawn_local(Box::pin(watch(Rc::downgrade(self), registry.clone())))
            }));
            match spawned {
                Ok(token) => self.state.borrow_mut().watch = Some(token),
                Err(payload) => {
                    self.state.borrow_mut().watching = false;
                    if std::thread::panicking() {
                        retain_opaque_payload(payload);
                        return;
                    }
                    resume_unwind(payload);
                }
            }
        }
        self.announce();
    }

    /// The registry's last confirmed write of this value, by any document.
    fn committed_write(&self) -> Option<(Revision, StoredVersion)> {
        let (_, publisher) = self.registry.as_ref()?;
        publisher.committed()
    }

    /// The latest revision the storage confirmed, if any.
    fn committed(&self) -> Option<Revision> {
        let (_, publisher) = self.registry.as_ref()?;
        publisher.committed().map(|(revision, _)| revision)
    }

    /// The revision of the value in memory, if it is the stored one.
    fn revision(&self) -> Option<Revision> {
        let state = self.state.borrow();
        let (SaveStatus::Clean, Some(loaded)) = (&state.status, &state.loaded) else {
            return None;
        };
        let Some((_, publisher)) = &self.registry else {
            return Some(loaded.revision);
        };
        // `Clean` is the value's, shared with every document of its name in
        // the runtime: the bytes on disk are this document's only when it
        // wrote last, or, when it wrote nothing since its load, while no
        // document of its name wrote after that load.
        let on_disk = if state.published.is_some() {
            publisher.committed_here() == Some(loaded.revision)
        } else {
            publisher.committed() == state.committed_at_load
        };
        on_disk.then_some(loaded.revision)
    }

    /// What the listeners are told about, as it stands.
    fn reported(&self) -> Reported {
        let status = self.state.borrow().status.clone();
        (status, self.committed(), self.revision())
    }

    /// Queue a delivery to the listeners if what they were told no longer
    /// stands, noting the move for a delivery already queued. Runs no
    /// listener: they run in the delivery, on a later turn of the runtime's
    /// tasks.
    fn announce(self: &Rc<Self>) {
        let announcer = &self.announcer;
        let Some(driver) = &self.driver else {
            return;
        };
        if announcer.closed.get() {
            return;
        }
        let now = self.reported();
        if *announcer.told.borrow() == now {
            return;
        }
        // Kept also while a delivery is queued: the state may be back on
        // what was told by the time it runs.
        announcer.moved.set(true);
        if announcer.queued.get() {
            return;
        }
        let document = Rc::downgrade(self);
        // Spawning wakes the frame hook, which may panic; the task is then
        // cancelled, and nothing stays queued, so the next announcement tries
        // again. While a panic already unwinds, the hook's is kept, not raised.
        let spawned = catch_unwind(AssertUnwindSafe(|| {
            driver.spawn_local(Box::pin(async move { deliver(&document) }))
        }));
        let token = match spawned {
            Ok(token) => token,
            Err(payload) => {
                if std::thread::panicking() {
                    retain_opaque_payload(payload);
                    return;
                }
                resume_unwind(payload);
            }
        };
        announcer.queued.set(true);
        // The task it replaces has run, or is the delivery running now,
        // whose cancellation is honoured once its poll returns.
        let finished = announcer.delivery.borrow_mut().replace(token);
        drop(finished);
    }
}

/// Run `step`, then drop `replaced`, a value the step's edit replaced, whose
/// drop is application code: it runs with no borrow held and after the step,
/// even when the step panics, so a panic in either never drops the other
/// while unwinding. The drop's panic is raised over the step's, which is
/// kept.
fn then_retire<T>(step: impl FnOnce(), replaced: T) {
    let stepped = catch_unwind(AssertUnwindSafe(step));
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(replaced))) {
        if let Err(first) = stepped {
            retain_opaque_payload(first);
        }
        resume_unwind(payload);
    }
    if let Err(payload) = stepped {
        resume_unwind(payload);
    }
}

/// Tell `document`'s listeners what changed since they were last told, if
/// anything still differs.
fn deliver<D: Document>(document: &Weak<OpenDocument<D>>) {
    let Some(open) = document.upgrade() else {
        return;
    };
    // Brought in line with the registry while still queued, so the refresh
    // queues nothing more. A refresh that panics (a frame hook it wakes)
    // ends this delivery, and the next announcement queues another.
    let announcer = &open.announcer;
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| open.refresh())) {
        announcer.queued.set(false);
        resume_unwind(payload);
    }
    announcer.queued.set(false);
    let now = open.reported();
    let changed = {
        let mut told = announcer.told.borrow_mut();
        let changed = announcer.moved.replace(false) || *told != now;
        *told = now;
        changed
    };
    // Recorded before the listeners run: a change they make queues the
    // next delivery, and a panic in one leaves the next delivery to come.
    if changed {
        announcer.notifier.notify_listeners();
    }
}

/// Follow the registry while `document` has a write in flight or listeners:
/// each change the registry settles brings the status, the close hold and the
/// listeners in line, and a busy store is tried again after its back-off.
async fn watch<D: Document>(document: Weak<OpenDocument<D>>, registry: FlushRegistry) {
    // However the watch ends, a panic in user code it runs (a listener's
    // frame hook) included, the next refresh may start another.
    let _watching = Watching(document.clone());
    loop {
        // Taken before the state is read, so a change in between resolves it.
        let changed = registry.changed();
        let Some(open) = document.upgrade() else {
            return;
        };
        open.refresh();
        let (saving, deadline) = {
            let state = open.state.borrow();
            (state.status == SaveStatus::Saving, state.busy_deadline)
        };
        if !saving {
            if !open.announcer.notifier.has_listeners() {
                return;
            }
            drop(open);
            changed.await;
            continue;
        }
        let Some(deadline) = deadline else {
            drop(open);
            changed.await;
            continue;
        };
        let (timer, hurry, guard) = (
            open.timer
                .clone()
                .expect("BUG: a busy retry waits only with a timer"),
            Rc::clone(&open.hurry),
            open.guard.clone(),
        );
        drop(open);
        // A registry change can replace the wait (a newer edit, or one that
        // went through): the loop reads the deadline again.
        let woke = wait_or_hurry(&timer, deadline, &hurry, guard.as_ref(), changed).await;
        let Some(open) = document.upgrade() else {
            return;
        };
        if woke == Woke::Due {
            open.retry_busy(deadline);
        }
    }
}

/// Marks a document's watch ended when the watch's future is dropped.
struct Watching<D: Document>(Weak<OpenDocument<D>>);

impl<D: Document> Drop for Watching<D> {
    fn drop(&mut self) {
        if let Some(open) = self.0.upgrade()
            && let Ok(mut state) = open.state.try_borrow_mut()
        {
            state.watching = false;
        }
    }
}

/// Keep the unreadable bytes of `document`, then write its initial value
/// over them.
async fn copy_aside<D: Document>(
    document: Weak<OpenDocument<D>>,
    copy: StorageFuture<CopiedAside>,
    base: StoredVersion,
) {
    let copied = copy.await;
    let Some(open) = document.upgrade() else {
        return;
    };
    match copied {
        Ok(CopiedAside { kept_as, version }) => {
            tracing::info!(
                document = D::NAME.as_str(),
                kept_as,
                "an unreadable document was kept aside; it starts over"
            );
            if version != base {
                tracing::warn!(
                    document = D::NAME.as_str(),
                    "the document changed after the load that could not read it"
                );
            }
            // Written over the file the failed load read, never over one
            // that replaced it since: the storage refuses that as a conflict.
            open.state.borrow_mut().restart = Some(Restart::Copied(base));
            open.write_initial(base);
        }
        Err(error) => open.fail(PersistError::Storage(error)),
    }
}

/// Why [`wait_or_hurry`] returned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Woke {
    /// The deadline passed, or the retry was hurried.
    Due,
    /// The registry changed first.
    Changed,
}

/// Wait until `deadline` on the runtime's clock, or less: until the
/// application is hidden or paused, or a close is recorded while it waits;
/// or until the registry changes.
async fn wait_or_hurry(
    timer: &TimerHandle,
    deadline: Instant,
    hurry: &Hurry,
    guard: Option<&CloseGuard>,
    registry_changed: impl Future<Output = ()>,
) -> Woke {
    let mut sleep = std::pin::pin!(timer.sleep_until(deadline));
    let mut registry_changed = std::pin::pin!(registry_changed);
    let mut changed = guard.map(|guard| Box::pin(guard.changed()));
    std::future::poll_fn(|cx| {
        if hurry.due.replace(false) || guard.is_some_and(|guard| guard.pending().is_some()) {
            return Poll::Ready(Woke::Due);
        }
        let waker = cx.waker().clone();
        let replaced = hurry.waker.replace(Some(waker));
        drop(replaced);
        if sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Woke::Due);
        }
        if let (Some(guard), Some(future)) = (guard, changed.as_mut())
            && future.as_mut().poll(cx).is_ready()
        {
            if guard.pending().is_some() {
                return Poll::Ready(Woke::Due);
            }
            *future = Box::pin(guard.changed());
            if future.as_mut().poll(cx).is_ready() {
                cx.waker().wake_by_ref();
            }
        }
        if registry_changed.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Woke::Changed);
        }
        Poll::Pending
    })
    .await
}

/// Asks a backed-off retry to run now; requested when the application is
/// hidden or paused, and kept only while a busy store's retries run.
#[derive(Default)]
struct Hurry {
    /// Whether a busy store's retries run: a request outside them is
    /// dropped, so it never hurries a later write's first back-off.
    armed: Cell<bool>,
    due: Cell<bool>,
    /// The waiting retry's waker.
    waker: RefCell<Option<Waker>>,
}

impl Hurry {
    fn arm(&self) {
        self.armed.set(true);
    }

    fn disarm(&self) {
        self.armed.set(false);
        self.due.set(false);
    }

    fn request(&self) {
        if !self.armed.get() {
            return;
        }
        self.due.set(true);
        let waker = self.waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// `body` under the header naming document `D` at `revision`.
fn framed<D: Document>(revision: Revision, body: &[u8]) -> Vec<u8> {
    let header = format!(
        "{HEADER_MAGIC} {} {} {}\n",
        D::NAME.as_str(),
        D::VERSION,
        revision.get()
    );
    let mut bytes = Vec::with_capacity(header.len() + body.len());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(body);
    bytes
}

/// [`Document::initial`], with a panic in it contained and reported.
fn contained_initial<D: Document>() -> Result<D, PersistError> {
    catch_unwind(D::initial).map_err(|payload| {
        let message = payload_text(&*payload)
            .unwrap_or("a panic without a message")
            .to_owned();
        retain_opaque_payload(payload);
        PersistError::Panicked {
            during: CodecStep::Initial,
            message,
        }
    })
}

/// The document a storage read holds: [`Document::initial`] when nothing is
/// stored, otherwise the body under a header naming this document in a
/// format this build reads, decoded with any codec panic contained.
fn decode_stored<D: Document>(stored: Stored) -> Result<Loaded<D>, PersistError> {
    let Some(bytes) = stored.bytes else {
        return Ok(Loaded {
            value: Rc::new(contained_initial::<D>()?),
            base: stored.version,
            revision: Revision::NONE,
        });
    };
    let header = parse_header(&bytes, D::NAME.as_str())?;
    if header.version > D::VERSION {
        return Err(PersistError::NewerVersion {
            found: header.version,
            supported: D::VERSION,
        });
    }
    let body = &bytes[header.body_start..];
    let decoded = catch_unwind(AssertUnwindSafe(|| D::decode(header.version, body)));
    let value = match decoded {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return Err(PersistError::Decode(error)),
        Err(payload) => {
            let message = payload_text(&*payload)
                .unwrap_or("a panic without a message")
                .to_owned();
            retain_opaque_payload(payload);
            return Err(PersistError::Panicked {
                during: CodecStep::Decode,
                message,
            });
        }
    };
    Ok(Loaded {
        value: Rc::new(value),
        base: stored.version,
        revision: header.revision,
    })
}

/// A stored document's header line, parsed.
struct Header {
    version: u32,
    revision: Revision,
    /// Where the body starts, after the header's newline.
    body_start: usize,
}

/// Parse `flui-document <name> <version> <revision>\n` at the start of
/// `bytes`, requiring `<name>` to be `name`.
fn parse_header(bytes: &[u8], name: &str) -> Result<Header, PersistError> {
    let corrupt = |detail: &str| PersistError::Corrupt {
        detail: detail.to_owned(),
    };
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| corrupt("no header line"))?;
    let line =
        std::str::from_utf8(&bytes[..end]).map_err(|_| corrupt("the header is not UTF-8"))?;
    let mut fields = line.split(' ');
    if fields.next() != Some(HEADER_MAGIC) {
        return Err(corrupt("the header does not start with `flui-document`"));
    }
    let stored_name = fields
        .next()
        .ok_or_else(|| corrupt("the header has no name"))?;
    if stored_name != name {
        return Err(PersistError::Corrupt {
            detail: format!("the header names `{stored_name}`, not `{name}`"),
        });
    }
    let version = fields
        .next()
        .and_then(|field| field.parse::<u32>().ok())
        .filter(|version| *version > 0)
        .ok_or_else(|| corrupt("the header's version is not a number from 1"))?;
    let revision = fields
        .next()
        .and_then(|field| field.parse::<u64>().ok())
        .ok_or_else(|| corrupt("the header's revision is not a number"))?;
    if fields.next().is_some() {
        return Err(corrupt("the header has more than four fields"));
    }
    Ok(Header {
        version,
        revision: Revision::new(revision),
        body_start: end + 1,
    })
}
