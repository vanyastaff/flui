//! [`Persisted`], the owner-local handle to one stored document.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_foundation::panic::{payload_text, retain_opaque_payload};
use flui_platform::{
    StorageError, Stored, StoredVersion, Unsupported, UnsupportedReason, WriteMode,
};

use super::{CodecStep, Document, PersistError, ReadOnlyReason, Revision, SaveStatus};
use super::{StorageCapability, StorageHandle};
use crate::context::CrateToken;
use crate::flush_registry::{FlushPublisher, FlushRegistry, FlushState};
use crate::{
    AppLifecycleState, AsyncDriver, CloseGuard, CloseHold, LifecycleSubscription, TaskToken,
    TimerHandle,
};
use crate::{LifecycleContext, LifecycleContextExt};

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
    /// The latest bytes handed to the registry.
    published: Option<Published>,
    /// The first failure since the last write that succeeded; later ones
    /// are logged.
    failure: Option<PersistError>,
    /// The close hold the status calls for.
    hold: Option<(HoldKind, CloseHold)>,
    /// The watch over the registry while a write is in flight, if one runs.
    watch: Option<TaskToken>,
    /// Whether the watch is still running.
    watching: bool,
    /// How many times the latest bytes were tried again after a busy store.
    busy_retries: usize,
    /// Whether a retry after a busy store is waiting out its back-off.
    busy_waiting: bool,
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

/// Which close hold a document keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HoldKind {
    /// A write in flight: [`CloseGuard::hold`].
    Work,
    /// A failed write: [`CloseGuard::require_decision`].
    Decision,
}

impl<D: Document> Clone for Persisted<D> {
    fn clone(&self) -> Self {
        Self {
            document: Rc::clone(&self.document),
        }
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
        Self {
            document: Rc::new(OpenDocument {
                storage,
                registry,
                driver: cx.async_driver(),
                timer: cx.timer(),
                hurry,
                _lifecycle: lifecycle,
                guard: cx.close_guard(),
                state: RefCell::new(DocumentState {
                    status,
                    generation: 0,
                    loaded: None,
                    published: None,
                    failure: None,
                    hold: None,
                    watch: None,
                    watching: false,
                    busy_retries: 0,
                    busy_waiting: false,
                }),
            }),
        }
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
            let (generation, read) = start?;
            let outcome = match read.await {
                Ok(stored) => decode_stored::<D>(stored),
                Err(error) => Err(PersistError::Storage(error)),
            };
            document.settle(generation, outcome)
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
            state.busy_retries = 0;
            state.busy_waiting = false;
            state.published = Some(Published {
                bytes: Arc::clone(&bytes),
                mode,
                revision,
            });
            state.status = SaveStatus::Saving;
            replaced
        };
        publisher.publish(bytes, mode, revision);
        document.refresh();
        // The replaced value's drop is application code: it runs with no
        // borrow held and after this edit is published, so an edit it makes
        // is published after this one and wins.
        drop(replaced);
        Ok(revision)
    }

    /// The latest revision the storage confirmed, if any.
    #[must_use]
    pub fn committed(&self) -> Option<Revision> {
        let (_, publisher) = self.document.registry.as_ref()?;
        publisher.committed().map(|(revision, _)| revision)
    }

    /// Where the document stands against its storage.
    #[must_use]
    pub fn status(&self) -> SaveStatus {
        self.document.refresh();
        self.document.state.borrow().status.clone()
    }

    /// After a failed write, write the latest value again; otherwise do
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
            state.busy_retries = 0;
            state.busy_waiting = false;
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

    /// Give up on a stored file that cannot be decoded: keep its bytes under
    /// a free `<name>-corrupt-N` name, then store [`Document::initial`] in
    /// its place. Does nothing unless the last load found the file corrupt
    /// or undecodable.
    pub fn start_over(&self) {}
}

/// A load in flight: its generation and the storage read.
type LoadStart = (u64, flui_platform::StorageFuture<Stored>);

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
        Ok((generation, storage.read(&D::NAME, D::MAX_BYTES)))
    }

    /// Apply a finished load if it is still the latest, and hand its
    /// outcome to the caller either way.
    fn settle(
        &self,
        generation: u64,
        outcome: Result<Loaded<D>, PersistError>,
    ) -> Result<Rc<D>, PersistError> {
        let mut state = self.state.borrow_mut();
        let latest = state.generation == generation;
        if latest {
            // Bytes written before this load belong to the value it replaces:
            // the status follows the load, not their write.
            state.published = None;
        }
        let (result, replaced) = match outcome {
            Ok(loaded) => {
                let value = Rc::clone(&loaded.value);
                let replaced = if latest {
                    state.status = SaveStatus::Clean;
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
        // A replaced value's drop is application code: no borrow is held.
        drop(replaced);
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

    /// Try the latest bytes again once a busy store's back-off is over.
    fn retry_busy(self: &Rc<Self>) {
        let republish = {
            let mut state = self.state.borrow_mut();
            if !state.busy_waiting {
                return;
            }
            state.busy_waiting = false;
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
                        state.busy_retries = 0;
                        state.busy_waiting = false;
                        if matches!(state.failure, Some(PersistError::Storage(_))) {
                            state.failure = None;
                        }
                    }
                    FlushState::Pending => {}
                    // A busy store is tried again on the runtime's clock, up
                    // to the last back-off; the status stays `Saving`.
                    FlushState::Failed(StorageError::Busy)
                        if state.failure.is_none()
                            && self.timer.is_some()
                            && (state.busy_waiting || state.busy_retries < BUSY_BACKOFF.len()) =>
                    {
                        state.busy_waiting = true;
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
            let spawn = saving && !state.watching && self.driver.is_some();
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
            let token = driver.spawn_local(Box::pin(watch(Rc::downgrade(self), registry.clone())));
            self.state.borrow_mut().watch = Some(token);
        }
    }
}

/// Follow the registry while `document` has a write in flight: each change
/// the registry settles brings the status and the close hold in line, and a
/// busy store is tried again after its back-off.
async fn watch<D: Document>(document: Weak<OpenDocument<D>>, registry: FlushRegistry) {
    loop {
        // Taken before the state is read, so a change in between resolves it.
        let changed = registry.changed();
        let Some(open) = document.upgrade() else {
            return;
        };
        open.refresh();
        let backoff = {
            let mut state = open.state.borrow_mut();
            if state.status != SaveStatus::Saving {
                state.watching = false;
                return;
            }
            state.busy_waiting.then(|| BUSY_BACKOFF[state.busy_retries])
        };
        let Some(delay) = backoff else {
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
        wait_or_hurry(&timer, delay, &hurry, guard.as_ref()).await;
        let Some(open) = document.upgrade() else {
            return;
        };
        open.retry_busy();
    }
}

/// Wait `delay` on the runtime's clock, or less: until the application is
/// hidden or paused, or a close is recorded while it waits.
async fn wait_or_hurry(
    timer: &TimerHandle,
    delay: Duration,
    hurry: &Hurry,
    guard: Option<&CloseGuard>,
) {
    let mut sleep = std::pin::pin!(timer.sleep(delay));
    let mut changed = guard.map(|guard| Box::pin(guard.changed()));
    std::future::poll_fn(|cx| {
        if hurry.due.replace(false) || guard.is_some_and(|guard| guard.pending().is_some()) {
            return Poll::Ready(());
        }
        let replaced = hurry.waker.replace(Some(cx.waker().clone()));
        drop(replaced);
        if sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(());
        }
        if let (Some(guard), Some(future)) = (guard, changed.as_mut())
            && future.as_mut().poll(cx).is_ready()
        {
            if guard.pending().is_some() {
                return Poll::Ready(());
            }
            *future = Box::pin(guard.changed());
            if future.as_mut().poll(cx).is_ready() {
                cx.waker().wake_by_ref();
            }
        }
        Poll::Pending
    })
    .await;
}

/// Asks a backed-off retry to run now; set when the application is hidden or
/// paused.
#[derive(Default)]
struct Hurry {
    due: Cell<bool>,
    /// The waiting retry's waker.
    waker: RefCell<Option<Waker>>,
}

impl Hurry {
    fn request(&self) {
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

/// The document a storage read holds: [`Document::initial`] when nothing is
/// stored, otherwise the body under a header naming this document in a
/// format this build reads, decoded with any codec panic contained.
fn decode_stored<D: Document>(stored: Stored) -> Result<Loaded<D>, PersistError> {
    let Some(bytes) = stored.bytes else {
        return Ok(Loaded {
            value: Rc::new(D::initial()),
            base: stored.version,
            revision: Revision::new(0),
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
