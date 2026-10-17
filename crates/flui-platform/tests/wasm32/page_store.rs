//! The Web `FileStore` against IndexedDB.
//!
//! In a browser (`WASM_BINDGEN_USE_BROWSER=1`) these run against the page's
//! own IndexedDB, whose transaction scheduling is what makes a based write
//! atomic. Node has no IndexedDB: there a stand-in that runs one transaction
//! at a time takes its place, so the store's protocol and outcomes run on
//! every CI pass while the scheduling itself is the browser's.

use flui_platform::storage::{DataDirs, FileStore};
use flui_platform_api::{StorageError, StorageName, StoredVersion, WriteMode};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

const NOTES: StorageName = StorageName::from_static("notes");

#[wasm_bindgen(inline_js = r"
let restore = null;
export function provideIndexedDB() {
    if (globalThis.indexedDB) return;
    const records = new Map();
    let running = Promise.resolve();
    const later = callback => setTimeout(callback, 0);
    const database = {
        close() {},
        transaction(_store, _mode) {
            const transaction = { oncomplete: null, onabort: null, error: null };
            const pending = [];
            const request = action => {
                const request = { result: undefined, onsuccess: null };
                pending.push(() => { request.result = action(); request.onsuccess?.(); });
                return request;
            };
            const values = {
                get: key => request(() => records.get(key)?.slice()),
                put: (value, key) => request(() => { records.set(key, value.slice()); }),
            };
            transaction.objectStore = () => values;
            running = running.then(() => new Promise(done => {
                const step = () => {
                    const next = pending.shift();
                    if (next) { next(); later(step); }
                    else { transaction.oncomplete?.(); done(); }
                };
                later(step);
            }));
            return transaction;
        },
    };
    globalThis.indexedDB = {
        open() {
            const request = { result: database, onupgradeneeded: null, onsuccess: null, onerror: null };
            later(() => request.onsuccess?.());
            return request;
        },
    };
    restore = () => { delete globalThis.indexedDB; };
}
export function withdrawIndexedDB() {
    const own = Object.getOwnPropertyDescriptor(globalThis, 'indexedDB');
    Object.defineProperty(globalThis, 'indexedDB', { value: undefined, configurable: true, writable: true });
    restore = () => {
        if (own) Object.defineProperty(globalThis, 'indexedDB', own);
        else delete globalThis.indexedDB;
    };
}
export function restoreIndexedDB() {
    restore?.();
    restore = null;
}
")]
extern "C" {
    #[wasm_bindgen(js_name = provideIndexedDB)]
    fn provide_indexed_db();
    #[wasm_bindgen(js_name = withdrawIndexedDB)]
    fn withdraw_indexed_db();
    #[wasm_bindgen(js_name = restoreIndexedDB)]
    fn restore_indexed_db();
}

/// Puts the page's IndexedDB back as it was when dropped.
struct Database;

impl Database {
    fn provided() -> Self {
        provide_indexed_db();
        Self
    }

    fn withdrawn() -> Self {
        withdraw_indexed_db();
        Self
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        restore_indexed_db();
    }
}

/// A store under roots no other run of these tests used, so a browser's
/// IndexedDB kept from an earlier run holds nothing under them.
fn fresh_store() -> FileStore {
    let root = format!("flui-test-{}", js_sys::Math::random());
    FileStore::new(DataDirs {
        roaming: root.clone().into(),
        local: root.into(),
    })
}

#[wasm_bindgen_test]
async fn a_based_write_succeeds_from_its_base_and_conflicts_from_any_other() {
    let _database = Database::provided();
    let store = fresh_store();
    let first = store
        .write(
            &NOTES,
            b"first",
            WriteMode::IfUnchanged(StoredVersion::ABSENT),
        )
        .await
        .expect("a write based on the absent value lands on an absent value");
    assert_eq!(first, StoredVersion::of_bytes(b"first"));
    assert_eq!(
        store
            .write(
                &NOTES,
                b"stale",
                WriteMode::IfUnchanged(StoredVersion::ABSENT)
            )
            .await,
        Err(StorageError::Conflict),
        "the absent base is stale once a value is stored"
    );
    let second = store
        .write(&NOTES, b"second", WriteMode::IfUnchanged(first))
        .await
        .expect("a write based on the stored version lands");
    let stored = store.read(&NOTES, 1024).await.expect("read");
    assert_eq!(stored.bytes.as_deref(), Some(&b"second"[..]));
    assert_eq!(stored.version, second);
    assert_eq!(
        store
            .write(&NOTES, b"late", WriteMode::IfUnchanged(first))
            .await,
        Err(StorageError::Conflict),
        "an older version is not the base"
    );
    assert_eq!(
        store.write(&NOTES, b"replaced", WriteMode::Replace).await,
        Ok(StoredVersion::of_bytes(b"replaced")),
        "a replacing write takes no base"
    );
    assert_eq!(
        store.read(&NOTES, 4).await,
        Err(StorageError::TooLarge { len: 8, limit: 4 })
    );
}

#[wasm_bindgen_test]
async fn of_two_writes_from_one_base_exactly_one_lands() {
    let _database = Database::provided();
    let store = fresh_store();
    // Both transactions are started before either is awaited: the second
    // compares only after the first has replaced the value.
    let left = store.write(
        &NOTES,
        b"left",
        WriteMode::IfUnchanged(StoredVersion::ABSENT),
    );
    let right = store.write(
        &NOTES,
        b"right",
        WriteMode::IfUnchanged(StoredVersion::ABSENT),
    );
    let (left, right) = (left.await, right.await);
    assert_eq!(left, Ok(StoredVersion::of_bytes(b"left")));
    assert_eq!(right, Err(StorageError::Conflict));
    assert_eq!(
        store
            .read(&NOTES, 1024)
            .await
            .expect("read")
            .bytes
            .as_deref(),
        Some(&b"left"[..])
    );
}

#[wasm_bindgen_test]
async fn a_page_without_indexed_db_is_unavailable() {
    let _database = Database::withdrawn();
    let store = fresh_store();
    assert_eq!(
        store.read(&NOTES, 1024).await,
        Err(StorageError::Unavailable)
    );
    assert_eq!(
        store
            .write(
                &NOTES,
                b"value",
                WriteMode::IfUnchanged(StoredVersion::ABSENT)
            )
            .await,
        Err(StorageError::Unavailable)
    );
}
