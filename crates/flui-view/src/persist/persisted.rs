//! [`Persisted`], the owner-local handle to one stored document.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::panic::{payload_text, retain_opaque_payload};
use flui_platform::{StorageError, Stored, StoredVersion, Unsupported};

use super::{CodecStep, Document, PersistError, ReadOnlyReason, Revision, SaveStatus};
use super::{StorageCapability, StorageHandle};
use crate::context::CrateToken;
use crate::flush_registry::FlushRegistry;
use crate::{LifecycleContext, LifecycleContextExt};

/// The first word of a stored document's header line.
const HEADER_MAGIC: &str = "flui-document";

/// What a write asks of storage until writing is wired.
const NOT_WIRED: Unsupported = Unsupported::new(flui_platform::UnsupportedReason::NotRegistered);

/// One [`Document`] kept for a widget: loaded from the UI runtime's storage,
/// written on every [`set`](Self::set), with its [`SaveStatus`].
///
/// Open it in `ViewState::init_state` and keep it in the state; clones share
/// the same document. It is owner-local (`!Send`): the value, the codec and
/// the status stay on the owner thread, and only bytes travel to the thread
/// that does the IO.
///
/// Writing is not wired yet: [`set`](Self::set) refuses every value, and
/// nothing is ever [`committed`](Self::committed).
pub struct Persisted<D: Document> {
    document: Rc<OpenDocument<D>>,
}

/// What the clones of one [`Persisted`] share.
struct OpenDocument<D> {
    /// The UI runtime's storage, or why it has none.
    storage: Result<StorageHandle, Unsupported>,
    /// The host's flush registry, which `set` publishes into.
    #[expect(dead_code, reason = "published into once set writes")]
    registry: Option<FlushRegistry>,
    /// Borrowed only between codec calls, storage requests and wakes.
    state: RefCell<DocumentState<D>>,
}

/// Where one document stands; see [`Persisted::status`].
struct DocumentState<D> {
    status: SaveStatus,
    /// The number of the latest load; an earlier load's result is not
    /// applied.
    generation: u64,
    /// The value the last applied load produced.
    loaded: Option<Loaded<D>>,
}

/// A loaded value and the stored file it came from.
struct Loaded<D> {
    value: Rc<D>,
    /// The version of the stored bytes, which a data write is based on.
    #[expect(dead_code, reason = "read by set once it writes")]
    base: StoredVersion,
    /// The revision the stored header records; 0 when nothing is stored.
    #[expect(dead_code, reason = "read by set once it writes")]
    revision: Revision,
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
        Self {
            document: Rc::new(OpenDocument {
                storage,
                registry: cx.flush_registry_in_crate(CrateToken::new()),
                state: RefCell::new(DocumentState {
                    status,
                    generation: 0,
                    loaded: None,
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
    /// the write: [`committed`](Self::committed) reaches the returned
    /// revision once the storage confirms it.
    ///
    /// # Errors
    ///
    /// Why the value was refused, with the status left as it was: no value
    /// is loaded to base the write on, the stored file is read-only, the
    /// revision counter is exhausted, or [`Document::encode`] panicked.
    pub fn set(&self, value: D) -> Result<Revision, PersistError> {
        drop(value);
        Err(PersistError::Storage(NOT_WIRED.into()))
    }

    /// The latest revision the storage confirmed, if any.
    #[must_use]
    pub fn committed(&self) -> Option<Revision> {
        None
    }

    /// Where the document stands against its storage.
    #[must_use]
    pub fn status(&self) -> SaveStatus {
        self.document.state.borrow().status.clone()
    }

    /// After a failed write, write the latest value again; otherwise do
    /// nothing.
    pub fn retry(&self) {}

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
        match outcome {
            Ok(loaded) => {
                let value = Rc::clone(&loaded.value);
                if latest {
                    state.status = SaveStatus::Clean;
                    state.loaded = Some(loaded);
                }
                Ok(value)
            }
            Err(error) => {
                if latest {
                    state.status = match &error {
                        PersistError::NewerVersion { found, supported } => {
                            SaveStatus::ReadOnly(ReadOnlyReason::NewerVersion {
                                found: *found,
                                supported: *supported,
                            })
                        }
                        other => SaveStatus::NotLoaded(Some(other.clone())),
                    };
                    state.loaded = None;
                }
                Err(error)
            }
        }
    }
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
