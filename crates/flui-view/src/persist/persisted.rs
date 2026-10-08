//! [`Persisted`], the owner-local handle to one stored document.

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use flui_platform_api::{Storage, StorageError};

use super::{Document, PersistError, Revision, SaveStatus};
use crate::LifecycleContext;
use crate::context::CrateToken;
use crate::flush_registry::FlushRegistry;

/// One [`Document`] kept for a widget: loaded from the UI runtime's storage,
/// written on every [`set`](Self::set), with its [`SaveStatus`].
///
/// Open it in `ViewState::init_state` and keep it in the state; clones share
/// the same document. It is owner-local (`!Send`): the value, the codec and
/// the status stay on the owner thread, and only bytes travel to the thread
/// that does the IO.
///
/// Not yet wired to storage: [`load`](Self::load) resolves to
/// [`PersistError::Storage`] with [`StorageError::Unavailable`],
/// [`set`](Self::set) refuses every value, and nothing is ever
/// [`committed`](Self::committed).
pub struct Persisted<D: Document> {
    document: Rc<OpenDocument<D>>,
}

/// What the clones of one [`Persisted`] share.
struct OpenDocument<D> {
    /// The UI runtime's storage, if it has one.
    storage: Option<Arc<dyn Storage>>,
    /// The host's flush registry, which `set` publishes into.
    #[expect(dead_code, reason = "published into once set writes")]
    registry: Option<FlushRegistry>,
    value_type: PhantomData<D>,
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
        Self {
            document: Rc::new(OpenDocument {
                storage: cx.storage(),
                registry: cx.flush_registry_in_crate(CrateToken::new()),
                value_type: PhantomData,
            }),
        }
    }

    /// Read the stored document: [`Document::initial`] when nothing is
    /// stored, the decoded value otherwise. The returned future runs on the
    /// owner thread; a later load replaces an earlier one, whose result is
    /// not applied.
    ///
    /// # Errors
    ///
    /// The [`PersistError`] that kept the document from loading: the
    /// storage's error, corrupt bytes, a body the document cannot decode, a
    /// newer format, a contained codec panic, or edits not yet stored.
    ///
    /// The future is not promised to be [`Unpin`]: pin it (`Box::pin`,
    /// `std::pin::pin!`) before polling it by hand.
    pub fn load(&self) -> impl Future<Output = Result<Rc<D>, PersistError>> + 'static {
        // An async block, so the future is `!Unpin` like the owner-local load
        // that replaces it; the captured value keeps it `!Send`.
        let failed: Result<Rc<D>, PersistError> =
            Err(PersistError::Storage(StorageError::Unavailable));
        async move { failed }
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
        Err(PersistError::Storage(StorageError::Unavailable))
    }

    /// The latest revision the storage confirmed, if any.
    #[must_use]
    pub fn committed(&self) -> Option<Revision> {
        None
    }

    /// Where the document stands against its storage.
    #[must_use]
    pub fn status(&self) -> SaveStatus {
        if self.document.storage.is_some() {
            SaveStatus::NotLoaded(None)
        } else {
            SaveStatus::Unavailable
        }
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
