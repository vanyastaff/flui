//! [`FileStore`] on wasm32: one IndexedDB record per name.

use std::future::Future;
use std::path::PathBuf;

use flui_platform::{
    CopiedAside, StorageError, StorageName, Stored, StoredVersion, Unsupported, UnsupportedReason,
    WriteMode,
};
use js_sys::{Array, Function, Promise, Uint8Array};
use tokio::sync::oneshot;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

use super::DataDirs;

/// The prefix machine-local values get under the local root, as on native
/// targets, so equal text in the two scopes names two records.
const MACHINE_LOCAL_DIR: &str = ".machine-local";

/// What joins a value's name and a copy's number in the name
/// [`FileStore::copy_aside`] keeps the copy under, as on native targets.
/// A [`StorageName`] cannot hold `.`, so no value is ever named like a copy.
const COPY_INFIX: &str = ".corrupt-";

// Each call opens the database and closes it when its one transaction ends,
// so an idle connection never blocks a later upgrade from another tab.
// A based write compares and puts inside one readwrite transaction: the
// browser runs readwrite transactions over the same store one at a time
// across every connection of the origin, which is the cross-tab atomicity
// `localStorage` lacks. Failures reject with `'full'` or `'unavailable'`.
#[wasm_bindgen(inline_js = r"
const DATABASE = 'flui-storage';
const VALUES = 'values';
const LENGTHS = 'lengths';
function open() {
    return new Promise((resolve, reject) => {
        if (!globalThis.indexedDB) { reject('unavailable'); return; }
        const request = globalThis.indexedDB.open(DATABASE, 2);
        request.onupgradeneeded = () => {
            const db = request.result;
            for (const store of [VALUES, LENGTHS]) {
                if (!db.objectStoreNames.contains(store)) db.createObjectStore(store);
            }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject('unavailable');
    });
}
function transact(mode, body) {
    return open().then(db => new Promise((resolve, reject) => {
        let transaction;
        try {
            transaction = db.transaction([VALUES, LENGTHS], mode);
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
        body(transaction.objectStore(VALUES), transaction.objectStore(LENGTHS), value => { answer = value; });
    }));
}
// A cursor tells a missing key from one holding `undefined`: the record is
// `[value]`, or `undefined` when the key holds nothing.
function found(request) {
    return request.result ? [request.result.value] : undefined;
}
// Each value's length is a record of its own, put in the same transaction as
// the value, so an oversized value is refused, and a based write of another
// length conflicts, without the value being loaded. A value with no length
// record (another script wrote it) is loaded and measured.
function lengthThen(lengths, key, next) {
    const request = lengths.get(key);
    request.onsuccess = () => next(typeof request.result === 'number' ? request.result : undefined);
}
export function readStoredValue(key, limit) {
    return transact('readonly', (values, lengths, answer) => {
        lengthThen(lengths, key, length => {
            if (length !== undefined && length > limit) { answer({ tooLarge: length }); return; }
            const request = values.openCursor(key);
            request.onsuccess = () => answer(found(request));
        });
    });
}
export function writeStoredValue(key, bytes, matches, baseLength) {
    return transact('readwrite', (values, lengths, answer) => {
        const put = () => { values.put(bytes, key); lengths.put(bytes.length, key); };
        if (!matches) { put(); answer(true); return; }
        lengthThen(lengths, key, length => {
            if (length !== undefined && length !== baseLength) { answer(false); return; }
            const request = values.openCursor(key);
            request.onsuccess = () => {
                const unchanged = matches(found(request));
                if (unchanged) put();
                answer(unchanged);
            };
        });
    });
}
// The copy claims the first free `<key><infix><n>` and puts the value there
// in the same readwrite transaction that reads it, so no other copy from
// any tab claims the same name and the copy holds exactly the bytes read.
export function copyStoredValue(key, infix) {
    return transact('readwrite', (values, lengths, answer) => {
        const request = values.openCursor(key);
        request.onsuccess = () => {
            const record = found(request);
            if (!record) { answer(undefined); return; }
            const value = record[0];
            if (!(value instanceof Uint8Array)) { answer('foreign'); return; }
            const claim = n => {
                const free = values.count(key + infix + n);
                free.onsuccess = () => {
                    if (free.result > 0) { claim(n + 1); return; }
                    values.put(value, key + infix + n);
                    lengths.put(value.length, key + infix + n);
                    answer([n, value]);
                };
            };
            claim(1);
        };
    });
}
")]
extern "C" {
    /// The record under `key` as `[value]`, `undefined` when there is none,
    /// or `{ tooLarge: length }` when its recorded length passes `limit`.
    #[wasm_bindgen(js_name = readStoredValue)]
    fn read_stored_value(key: &str, limit: f64) -> Promise;
    /// Put `bytes` under `key`, after `matches` accepts the stored record
    /// when it is given; a recorded length other than `base_length` is a
    /// mismatch without the value being loaded. Resolves to whether the put
    /// happened.
    #[wasm_bindgen(js_name = writeStoredValue)]
    fn write_stored_value(
        key: &str,
        bytes: &Uint8Array,
        matches: Option<&Function>,
        base_length: Option<f64>,
    ) -> Promise;
    /// Copy the record under `key` to the first free `key + infix + n`,
    /// `n` from 1, in one transaction. Resolves to `[n, bytes]`, `undefined`
    /// when `key` holds nothing, or `'foreign'` when it holds no bytes.
    #[wasm_bindgen(js_name = copyStoredValue)]
    fn copy_stored_value(key: &str, infix: &str) -> Promise;
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
/// user, an opaque origin) every call is [`StorageError::Unsupported`]
/// (`Unavailable`).
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
    /// The limit is checked against the value's length record before the
    /// value is loaded; a value without one is loaded and measured.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unsupported`] (`Unavailable`) without IndexedDB;
    /// [`StorageError::TooLarge`] for a value over `limit` bytes;
    /// [`StorageError::Inaccessible`] for a record this store did not write.
    pub fn read(
        &self,
        name: &StorageName,
        limit: u64,
    ) -> impl Future<Output = Result<Stored, StorageError>> + Send + use<> {
        let key = self.key(name);
        on_page(async move {
            let record = JsFuture::from(read_stored_value(&key, js_length(limit)))
                .await
                .map_err(failure)?;
            if let Some(len) = recorded_too_large(&record) {
                return Err(StorageError::TooLarge { len, limit });
            }
            let Some(stored) = record_value(&record) else {
                return Ok(Stored {
                    bytes: None,
                    version: StoredVersion::ABSENT,
                });
            };
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
    /// [`StorageError::Unsupported`] (`Unavailable`) without IndexedDB.
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
                _ => return Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into()),
            };
            // Lives until the transaction settles: the browser calls it from
            // inside the transaction, never after.
            let matches = base.map(|base| {
                Closure::<dyn FnMut(JsValue) -> bool>::new(move |record: JsValue| {
                    holds_version(&record, base)
                })
            });
            let written = JsFuture::from(write_stored_value(
                &key,
                &Uint8Array::from(bytes.as_slice()),
                matches
                    .as_ref()
                    .map(|matches| matches.as_ref().unchecked_ref()),
                base.and_then(|base| base.byte_len()).map(js_length),
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

    /// Copy the value stored under `name` to the first free name
    /// `<name>.corrupt-<n>` beside it, counting `n` from 1, and leave `name`
    /// as it is.
    ///
    /// Reading the value, claiming the copy's name and putting the copy are
    /// one readwrite transaction, so two copies, from any tab of the origin,
    /// never share a name, and the returned version is the version of
    /// exactly the bytes copied.
    ///
    /// # Errors
    ///
    /// [`StorageError::NotFound`] when `name` holds no value;
    /// [`StorageError::Inaccessible`] for a record this store did not write;
    /// [`StorageError::Full`] when the origin's quota refuses the copy;
    /// [`StorageError::Unsupported`] (`Unavailable`) without IndexedDB.
    pub fn copy_aside(
        &self,
        name: &StorageName,
    ) -> impl Future<Output = Result<CopiedAside, StorageError>> + Send + use<> {
        let key = self.key(name);
        let name = name.as_str();
        on_page(async move {
            let copied = JsFuture::from(copy_stored_value(&key, COPY_INFIX))
                .await
                .map_err(failure)?;
            if copied.is_undefined() {
                return Err(StorageError::NotFound);
            }
            let copied = copied.dyn_into::<Array>().map_err(|_| foreign())?;
            let n = copied.get(0).as_f64().ok_or_else(foreign)?;
            let bytes = copied
                .get(1)
                .dyn_into::<Uint8Array>()
                .map_err(|_| foreign())?
                .to_vec();
            Ok(CopiedAside {
                kept_as: format!("{name}{COPY_INFIX}{n}"),
                version: StoredVersion::of_bytes(&bytes),
            })
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

/// The value a record (`[value]`, or `undefined` for a missing key) holds;
/// a stored `undefined` is a value, not an absent record.
fn record_value(record: &JsValue) -> Option<JsValue> {
    record.dyn_ref::<Array>().map(|record| record.get(0))
}

/// Whether `record` has the version `base`. A record of another length is
/// never copied out to compare.
fn holds_version(record: &JsValue, base: StoredVersion) -> bool {
    let Some(stored) = record_value(record) else {
        return base == StoredVersion::ABSENT;
    };
    stored.dyn_ref::<Uint8Array>().is_some_and(|stored| {
        base.byte_len() == Some(u64::from(stored.length()))
            && StoredVersion::of_bytes(&stored.to_vec()) == base
    })
}

/// The error a rejected transaction stands for.
fn failure(reason: JsValue) -> StorageError {
    if reason.as_string().as_deref() == Some("full") {
        StorageError::Full
    } else {
        Unsupported::new(UnsupportedReason::Unavailable).into()
    }
}

/// A record holding something other than bytes: another script wrote it.
fn foreign() -> StorageError {
    StorageError::Inaccessible {
        kind: std::io::ErrorKind::InvalidData,
    }
}

/// A byte count as the JavaScript number IndexedDB compares it with,
/// saturating past what any array can hold.
fn js_length(len: u64) -> f64 {
    f64::from(u32::try_from(len).unwrap_or(u32::MAX))
}

/// The recorded length a read refused, when `record` is `{ tooLarge }`.
fn recorded_too_large(record: &JsValue) -> Option<u64> {
    if record.is_undefined() || record.is_instance_of::<Array>() {
        return None;
    }
    let len = js_sys::Reflect::get(record, &JsValue::from_str("tooLarge"))
        .ok()?
        .as_f64()?;
    // A length this store recorded: a non-negative integer an array held.
    u32::try_from(len as i64).ok().map(u64::from)
}
