//! [`FileStore`] on wasm32: one IndexedDB record per name.

use std::future::Future;
use std::path::PathBuf;

use flui_platform_api::{StorageError, StorageName, Stored, StoredVersion, WriteMode};
use js_sys::{Function, Promise, Uint8Array};
use tokio::sync::oneshot;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

use super::DataDirs;

/// The prefix machine-local values get under the local root, as on native
/// targets, so equal text in the two scopes names two records.
const MACHINE_LOCAL_DIR: &str = ".machine-local";

// Each call opens the database and closes it when its one transaction ends,
// so an idle connection never blocks a later upgrade from another tab.
// A based write compares and puts inside one readwrite transaction: the
// browser runs readwrite transactions over the same store one at a time
// across every connection of the origin, which is the cross-tab atomicity
// `localStorage` lacks. Failures reject with `'full'` or `'unavailable'`.
#[wasm_bindgen(inline_js = r"
const DATABASE = 'flui-storage';
const VALUES = 'values';
function open() {
    return new Promise((resolve, reject) => {
        if (!globalThis.indexedDB) { reject('unavailable'); return; }
        const request = globalThis.indexedDB.open(DATABASE, 1);
        request.onupgradeneeded = () => request.result.createObjectStore(VALUES);
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject('unavailable');
    });
}
function transact(mode, body) {
    return open().then(db => new Promise((resolve, reject) => {
        let transaction;
        try {
            transaction = db.transaction(VALUES, mode);
        } catch {
            db.close();
            reject('unavailable');
            return;
        }
        let answer;
        transaction.oncomplete = () => { db.close(); resolve(answer); };
        transaction.onabort = () => {
            db.close();
            reject(transaction.error?.name === 'QuotaExceededError' ? 'full' : 'unavailable');
        };
        body(transaction.objectStore(VALUES), value => { answer = value; });
    }));
}
export function readStoredValue(key) {
    return transact('readonly', (values, answer) => {
        const request = values.get(key);
        request.onsuccess = () => answer(request.result);
    });
}
export function writeStoredValue(key, bytes, matches) {
    return transact('readwrite', (values, answer) => {
        if (!matches) {
            values.put(bytes, key);
            answer(true);
            return;
        }
        const request = values.get(key);
        request.onsuccess = () => {
            const unchanged = matches(request.result);
            if (unchanged) values.put(bytes, key);
            answer(unchanged);
        };
    });
}
")]
extern "C" {
    /// The record under `key`, `undefined` when there is none.
    #[wasm_bindgen(js_name = readStoredValue)]
    fn read_stored_value(key: &str) -> Promise;
    /// Put `bytes` under `key`, after `matches` accepts the stored record
    /// when it is given; resolves to whether the put happened.
    #[wasm_bindgen(js_name = writeStoredValue)]
    fn write_stored_value(key: &str, bytes: &Uint8Array, matches: Option<&Function>) -> Promise;
}

/// A store of named byte values in the page's IndexedDB, one record each,
/// keyed by the root and the name.
///
/// Every call is one IndexedDB transaction, so a reader sees the old value or
/// the new one. A write based on a version ([`WriteMode::IfUnchanged`])
/// compares and replaces inside one readwrite transaction, which the browser
/// does not interleave with another readwrite transaction on the store from
/// any tab of the origin: of two writes from the same base exactly one
/// succeeds. A [`WriteMode::Replace`] takes no part in that comparison, as on
/// native targets.
///
/// The calls answer in a later task of the page, so they return futures; each
/// starts its transaction when called. Without IndexedDB (blocked by the
/// user, an opaque origin) every call is [`StorageError::Unavailable`].
#[derive(Debug)]
pub struct FileStore {
    roaming: PathBuf,
    local: PathBuf,
}

impl FileStore {
    /// A store under `dirs`, whose roots prefix the records' keys.
    #[must_use]
    pub fn new(dirs: DataDirs) -> Self {
        let DataDirs { roaming, local } = dirs;
        Self {
            roaming,
            local: local.join(MACHINE_LOCAL_DIR),
        }
    }

    /// Read the value stored under `name`; a missing value is `Ok` with no
    /// bytes and [`StoredVersion::ABSENT`].
    ///
    /// The limit is checked before the value is copied out of the browser.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unavailable`] without IndexedDB;
    /// [`StorageError::TooLarge`] for a value over `limit` bytes;
    /// [`StorageError::Inaccessible`] for a record this store did not write.
    pub fn read(
        &self,
        name: &StorageName,
        limit: u64,
    ) -> impl Future<Output = Result<Stored, StorageError>> + Send + use<> {
        let key = self.key(name);
        on_page(async move {
            let stored = JsFuture::from(read_stored_value(&key))
                .await
                .map_err(failure)?;
            if stored.is_undefined() {
                return Ok(Stored {
                    bytes: None,
                    version: StoredVersion::ABSENT,
                });
            }
            let stored = stored.dyn_into::<Uint8Array>().map_err(|_| foreign())?;
            let len = u64::from(stored.length());
            if len > limit {
                return Err(StorageError::TooLarge { len, limit });
            }
            let bytes = stored.to_vec();
            Ok(Stored {
                version: StoredVersion::of_bytes(&bytes),
                bytes: Some(bytes),
            })
        })
    }

    /// Store `bytes` under `name`; under [`WriteMode::IfUnchanged`] only if
    /// the stored version is still the base.
    ///
    /// Returns the version of the value now stored.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] when the stored version is not the base (a
    /// record this store did not write is never the base);
    /// [`StorageError::Full`] when the origin's quota refuses the value;
    /// [`StorageError::Unavailable`] without IndexedDB.
    pub fn write(
        &self,
        name: &StorageName,
        bytes: &[u8],
        mode: WriteMode,
    ) -> impl Future<Output = Result<StoredVersion, StorageError>> + Send + use<> {
        let key = self.key(name);
        let version = StoredVersion::of_bytes(bytes);
        let bytes = bytes.to_vec();
        on_page(async move {
            let base = match mode {
                WriteMode::Replace => None,
                WriteMode::IfUnchanged(base) => Some(base),
                // A mode this store does not know how to honour is refused
                // rather than guessed at.
                _ => return Err(StorageError::Unavailable),
            };
            // Lives until the transaction settles: the browser calls it from
            // inside the transaction, never after.
            let matches = base.map(|base| {
                Closure::<dyn FnMut(JsValue) -> bool>::new(move |stored: JsValue| {
                    holds_version(&stored, base)
                })
            });
            let written = JsFuture::from(write_stored_value(
                &key,
                &Uint8Array::from(bytes.as_slice()),
                matches
                    .as_ref()
                    .map(|matches| matches.as_ref().unchecked_ref()),
            ))
            .await
            .map_err(failure)?;
            if written.as_bool() == Some(true) {
                Ok(version)
            } else {
                Err(StorageError::Conflict)
            }
        })
    }

    fn key(&self, name: &StorageName) -> String {
        let root = if name.is_machine_local() {
            &self.local
        } else {
            &self.roaming
        };
        format!("{}/{}", root.display(), name.as_str())
    }
}

/// Run `work` on the page's task queue and hand its answer to whatever awaits
/// the returned future. The JavaScript values `work` holds stay in that task,
/// so the answer can travel through the host's `Send` IO futures.
fn on_page<T: Send + 'static>(
    work: impl Future<Output = Result<T, StorageError>> + 'static,
) -> impl Future<Output = Result<T, StorageError>> + Send {
    let (reply, answer) = oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = reply.send(work.await);
    });
    async move { answer.await.unwrap_or(Err(StorageError::Cancelled)) }
}

/// Whether the record `stored` (`undefined` when absent) has the version
/// `base`.
fn holds_version(stored: &JsValue, base: StoredVersion) -> bool {
    if stored.is_undefined() {
        return base == StoredVersion::ABSENT;
    }
    stored
        .dyn_ref::<Uint8Array>()
        .is_some_and(|stored| StoredVersion::of_bytes(&stored.to_vec()) == base)
}

/// The error a rejected transaction stands for.
fn failure(reason: JsValue) -> StorageError {
    if reason.as_string().as_deref() == Some("full") {
        StorageError::Full
    } else {
        StorageError::Unavailable
    }
}

/// A record holding something other than bytes: another script wrote it.
fn foreign() -> StorageError {
    StorageError::Inaccessible {
        kind: std::io::ErrorKind::InvalidData,
    }
}
