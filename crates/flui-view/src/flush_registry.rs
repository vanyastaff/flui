//! The flush registry: the latest bytes of every persisted document, kept by
//! the host outside every realm and written to storage from there.
//!
//! A document publishes its encoded bytes here on the owner thread; the host
//! writes them through its [`Storage`] on the IO pool, one write in flight
//! per name, the latest bytes winning. Because the registry lives outside the
//! realm, the host can still write what was published when the realm is gone
//! or busy: at the end of the application and when the session ends, without
//! running any application code.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use flui_platform_api::{Storage, StorageError, StorageName, StoredVersion, WriteMode};
use parking_lot::Mutex;

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
    /// bytes replace them or the host's flush writes them.
    Failed(StorageError),
}

/// The latest bytes published under one name.
struct Slot {
    #[expect(dead_code, reason = "written once the registry flushes")]
    bytes: Arc<[u8]>,
    #[expect(dead_code, reason = "written once the registry flushes")]
    mode: WriteMode,
    revision: Revision,
}

/// The registry's state: one slot per published name.
#[derive(Default)]
struct Slots {
    by_name: HashMap<StorageName, Slot>,
}

/// The host's record of the latest bytes of every persisted document, and
/// of what the storage confirmed.
///
/// Cheap to clone; every clone is the same registry. A widget reaches it
/// through [`LifecycleContext::flush_registry`](crate::LifecycleContext::flush_registry);
/// the host builds it with [`FlushRegistryHost::new`].
///
/// Not yet wired: published bytes are kept, but nothing is written to the
/// storage, so nothing is ever committed.
#[derive(Clone)]
pub struct FlushRegistry {
    slots: Arc<Mutex<Slots>>,
    #[expect(dead_code, reason = "written through once the registry flushes")]
    host: Arc<RegistryHost>,
}

/// What the host gave the registry to write with: the storage, and the IO
/// pool that runs the writes.
#[expect(dead_code, reason = "written through once the registry flushes")]
struct RegistryHost {
    storage: Arc<dyn Storage>,
    writer: FlushWriter,
}

impl fmt::Debug for FlushRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushRegistry")
            .field("published", &self.slots.lock().by_name.len())
            .finish_non_exhaustive()
    }
}

impl FlushRegistry {
    /// Make `bytes` the latest value of `name`, to be written under `mode`,
    /// replacing bytes published earlier that are not written yet, and
    /// return the revision they were published as. Owner thread; never
    /// waits for the storage.
    pub fn publish(&self, name: StorageName, bytes: Arc<[u8]>, mode: WriteMode) -> Revision {
        let mut slots = self.slots.lock();
        let revision = slots.by_name.get(&name).map_or(Revision::FIRST, |slot| {
            slot.revision
                .next()
                .expect("BUG: one name is published fewer than u64::MAX times")
        });
        slots.by_name.insert(
            name,
            Slot {
                bytes,
                mode,
                revision,
            },
        );
        revision
    }

    /// The latest revision of `name` the storage confirmed, with the stored
    /// version it confirmed it as; `None` before the first confirmation.
    #[must_use]
    pub fn committed(&self, name: StorageName) -> Option<(Revision, StoredVersion)> {
        let _ = name;
        None
    }

    /// Where the bytes published under `name` stand.
    #[must_use]
    pub fn outcome(&self, name: StorageName) -> FlushState {
        if self.slots.lock().by_name.contains_key(&name) {
            FlushState::Pending
        } else {
            FlushState::Clean
        }
    }
}

/// One write the registry hands its [`FlushWriter`] to run on the IO pool.
pub type FlushWrite = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Runs a [`FlushWrite`] on the host's IO pool, or hands it back when the
/// pool refuses it; a refused write stays owed and the host's flush writes
/// its bytes.
pub type FlushWriter = Box<dyn Fn(FlushWrite) -> Result<(), FlushWrite> + Send + Sync>;

/// What [`FlushRegistryHost::flush_within`] wrote before its deadline.
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

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::FlushRegistry {}
}

/// The host's side of a [`FlushRegistry`]: building it and flushing it when
/// the application or the session ends.
///
/// Sealed: [`FlushRegistry`] is its only implementation. The host imports it
/// as `use flui_view::__runtime::FlushRegistryHost as _;`.
pub trait FlushRegistryHost: sealed::Sealed {
    /// A registry writing through `storage`, starting each write on the IO
    /// pool through `writer`.
    fn new(storage: Arc<dyn Storage>, writer: FlushWriter) -> Self;

    /// Write the latest unconfirmed bytes of every name before `deadline`,
    /// waiting for a write already in flight first. Synchronous, on the owner
    /// thread; runs no application code.
    fn flush_within(&self, deadline: Instant) -> FlushReport;
}

impl FlushRegistryHost for FlushRegistry {
    fn new(storage: Arc<dyn Storage>, writer: FlushWriter) -> Self {
        Self {
            slots: Arc::new(Mutex::new(Slots::default())),
            host: Arc::new(RegistryHost { storage, writer }),
        }
    }

    fn flush_within(&self, deadline: Instant) -> FlushReport {
        // Nothing is written yet: every published name is unfinished.
        let _ = deadline;
        FlushReport {
            unfinished: self.slots.lock().by_name.keys().copied().collect(),
            ..FlushReport::default()
        }
    }
}
