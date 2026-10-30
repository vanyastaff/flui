//! The storage capability (ADR-0152): how a [`Persisted`](super::Persisted)
//! document reaches the bytes it is kept in.

use std::fmt;
use std::sync::Arc;

use flui_platform::capability::Capability;
use flui_platform::{Storage, StorageFuture, StorageName, Stored, StoredVersion, WriteMode};

/// The storage capability: `cx.capability::<StorageCapability>()` hands out
/// a [`StorageHandle`].
///
/// The framework provides it wherever the application has a data directory;
/// an application replaces it, an encrypted store say, with
/// `Application::capability::<StorageCapability>(provider)`.
#[derive(Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum StorageCapability {}

impl Capability for StorageCapability {
    type Handle = StorageHandle;
    const NAME: &'static str = "storage";
}

/// The byte storage of one UI runtime, as a widget holds it.
///
/// Cheap to clone; clones share the store. Its calls answer on whatever
/// thread the store finishes on, the way [`Storage`] does.
#[derive(Clone)]
pub struct StorageHandle {
    store: Arc<dyn Storage>,
}

impl StorageHandle {
    /// A handle over `store`.
    #[must_use]
    pub fn new(store: Arc<dyn Storage>) -> Self {
        Self { store }
    }

    /// Read the value stored under `name`, refusing one longer than `limit`
    /// bytes; see [`Storage::read`].
    pub fn read(&self, name: &StorageName, limit: u64) -> StorageFuture<Stored> {
        self.store.read(name, limit)
    }

    /// Store `bytes` under `name` as `mode` allows; see [`Storage::publish`].
    pub fn publish(
        &self,
        name: &StorageName,
        bytes: Vec<u8>,
        mode: WriteMode,
    ) -> StorageFuture<StoredVersion> {
        self.store.publish(name, bytes, mode)
    }
}

impl fmt::Debug for StorageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageHandle").finish_non_exhaustive()
    }
}
