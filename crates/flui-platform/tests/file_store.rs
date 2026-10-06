//! The file store behind the `storage` feature: directories created on the
//! first write, values replaced atomically, inaccessible paths reported apart
//! from corrupt ones, and a write based on a stale version refused.
#![cfg(feature = "storage")]

use std::io::ErrorKind;
use std::path::Path;

use flui_platform::storage::{DataDirs, FileStore, data_dirs};
use flui_platform_api::{StorageError, StorageName, StoredVersion, WriteMode};

const NOTES: StorageName = StorageName::from_static("notes");
const SESSION: StorageName = StorageName::machine_local("notes");

/// Runs every case even after one fails, then panics listing the failing case
/// names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

fn separate_roots(dir: &Path) -> DataDirs {
    DataDirs {
        roaming: dir.join("roaming").join("app"),
        local: dir.join("local").join("app"),
    }
}

/// The entries of `dir` whose names start with `.tmp`: files a write staged
/// and left behind.
fn staged_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(".tmp"))
                .collect()
        })
        .unwrap_or_default()
}

fn first_write_creates_the_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());

    let missing = store.read(&NOTES, 1024).expect("a missing root reads");
    assert_eq!(missing.bytes, None);
    assert_eq!(missing.version, StoredVersion::ABSENT);
    assert!(!dirs.roaming.exists(), "reading creates nothing");

    let version = store
        .write(&NOTES, b"hello", WriteMode::Replace)
        .expect("the first write creates the root");
    assert_eq!(version, StoredVersion::of_bytes(b"hello"));
    let stored = store.read(&NOTES, 1024).expect("the value reads");
    assert_eq!(stored.bytes.as_deref(), Some(&b"hello"[..]));
    assert_eq!(stored.version, version);
    assert_eq!(staged_files(&dirs.roaming), Vec::<String>::new());
}

fn directory_in_place_of_the_file_is_inaccessible() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    std::fs::create_dir_all(dirs.roaming.join("notes").join("inside")).expect("a directory");
    let store = FileStore::new(dirs.clone());

    let inaccessible = StorageError::Inaccessible {
        kind: ErrorKind::IsADirectory,
    };
    assert_eq!(store.read(&NOTES, 1024), Err(inaccessible.clone()));
    assert_eq!(
        store.write(&NOTES, b"hello", WriteMode::Replace),
        Err(inaccessible)
    );
    assert_eq!(
        staged_files(&dirs.roaming),
        Vec::<String>::new(),
        "a failed write removes its staged file"
    );
}

fn a_stale_base_is_refused_with_conflict() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = FileStore::new(separate_roots(dir.path()));

    let first = store
        .write(
            &NOTES,
            b"first",
            WriteMode::IfUnchanged(StoredVersion::ABSENT),
        )
        .expect("the base matches an absent value");
    assert_eq!(
        store.write(
            &NOTES,
            b"stale",
            WriteMode::IfUnchanged(StoredVersion::ABSENT)
        ),
        Err(StorageError::Conflict)
    );
    let stored = store.read(&NOTES, 1024).expect("the value reads");
    assert_eq!(stored.bytes.as_deref(), Some(&b"first"[..]));

    store
        .write(&NOTES, b"second", WriteMode::IfUnchanged(first))
        .expect("the current base is accepted");
    store
        .write(&NOTES, b"last", WriteMode::Replace)
        .expect("a replacement needs no base");
    let stored = store.read(&NOTES, 1024).expect("the value reads");
    assert_eq!(stored.bytes.as_deref(), Some(&b"last"[..]));
}

fn a_locked_directory_refuses_based_writes_as_busy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    store
        .write(&NOTES, b"first", WriteMode::Replace)
        .expect("the root exists");

    // Another process sharing the directory, mid-write.
    let holder = std::fs::File::create(dirs.roaming.join(".flui-storage.lock")).expect("lock file");
    holder.lock().expect("the lock is free");

    let base = StoredVersion::of_bytes(b"first");
    assert_eq!(
        store.write(&NOTES, b"second", WriteMode::IfUnchanged(base)),
        Err(StorageError::Busy)
    );
    holder.unlock().expect("unlock");
    store
        .write(&NOTES, b"second", WriteMode::IfUnchanged(base))
        .expect("the write goes through once the lock is released");
}

fn a_value_over_the_limit_is_too_large() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = FileStore::new(separate_roots(dir.path()));
    store
        .write(&NOTES, b"0123456789", WriteMode::Replace)
        .expect("written");

    assert_eq!(
        store.read(&NOTES, 9),
        Err(StorageError::TooLarge { len: 10, limit: 9 })
    );
    let stored = store.read(&NOTES, 10).expect("a value at the limit reads");
    assert_eq!(stored.bytes.as_deref(), Some(&b"0123456789"[..]));
}

fn the_two_scopes_keep_equal_names_apart() {
    for shared_root in [false, true] {
        let dir = tempfile::tempdir().expect("temp dir");
        let dirs = if shared_root {
            DataDirs {
                roaming: dir.path().join("app"),
                local: dir.path().join("app"),
            }
        } else {
            separate_roots(dir.path())
        };
        let store = FileStore::new(dirs.clone());
        store
            .write(&NOTES, b"data", WriteMode::Replace)
            .expect("data");
        store
            .write(&SESSION, b"session", WriteMode::Replace)
            .expect("session");

        let data = store.read(&NOTES, 1024).expect("data reads");
        let session = store.read(&SESSION, 1024).expect("session reads");
        assert_eq!(
            data.bytes.as_deref(),
            Some(&b"data"[..]),
            "shared root: {shared_root}"
        );
        assert_eq!(
            session.bytes.as_deref(),
            Some(&b"session"[..]),
            "shared root: {shared_root}"
        );
        assert_eq!(
            std::fs::read(dirs.roaming.join("notes")).expect("data under the roaming root"),
            b"data"
        );
        if !shared_root {
            assert_eq!(
                std::fs::read(dirs.local.join("notes")).expect("session under the local root"),
                b"session"
            );
        }
    }
}

fn long_non_ascii_path_round_trips() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut root = dir.path().to_path_buf();
    while root.as_os_str().len() <= 300 {
        root.push("Заметки-笔记-ノート-данные");
    }
    let store = FileStore::new(DataDirs {
        roaming: root.join("roaming"),
        local: root.join("local"),
    });
    store
        .write(
            &NOTES,
            "текст 文本".as_bytes(),
            WriteMode::IfUnchanged(StoredVersion::ABSENT),
        )
        .expect("a long non-ASCII path is written");
    let stored = store.read(&NOTES, 1024).expect("and read back");
    assert_eq!(stored.bytes.as_deref(), Some("текст 文本".as_bytes()));
}

fn data_dirs_place_each_scope_under_the_application_name() {
    let app = StorageName::from_static("flui-notes");
    if let Some(dirs) = data_dirs(&app) {
        assert!(dirs.roaming.ends_with("flui-notes"), "{dirs:?}");
        assert!(dirs.local.ends_with("flui-notes"), "{dirs:?}");
        assert!(
            dirs.roaming.is_absolute() && dirs.local.is_absolute(),
            "{dirs:?}"
        );
    }
}

#[cfg(windows)]
fn read_only_target_is_inaccessible_not_busy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    store
        .write(&NOTES, b"kept", WriteMode::Replace)
        .expect("written");
    let target = dirs.roaming.join("notes");
    let mut permissions = std::fs::metadata(&target).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&target, permissions.clone()).expect("read-only");

    let result = store.write(&NOTES, b"lost", WriteMode::Replace);

    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "Windows: clears the attribute"
    )]
    permissions.set_readonly(false);
    std::fs::set_permissions(&target, permissions).expect("writable again for cleanup");
    assert_eq!(
        result,
        Err(StorageError::Inaccessible {
            kind: ErrorKind::PermissionDenied
        })
    );
    assert_eq!(std::fs::read(&target).expect("the old value"), b"kept");
    assert_eq!(staged_files(&dirs.roaming), Vec::<String>::new());
}

#[cfg(windows)]
fn a_target_held_without_delete_sharing_is_busy() {
    use std::os::windows::fs::OpenOptionsExt;

    /// `FILE_SHARE_READ`: the holder lets others read, not write or replace.
    const FILE_SHARE_READ: u32 = 1;

    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    store
        .write(&NOTES, b"kept", WriteMode::Replace)
        .expect("written");
    let target = dirs.roaming.join("notes");
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&target)
        .expect("a scanner holds the file");

    assert_eq!(
        store.write(&NOTES, b"later", WriteMode::Replace),
        Err(StorageError::Busy)
    );
    drop(holder);
    store
        .write(&NOTES, b"later", WriteMode::Replace)
        .expect("the write goes through once the holder lets go");
    assert_eq!(staged_files(&dirs.roaming), Vec::<String>::new());
}

#[test]
fn file_store_contract() {
    let mut cases: Vec<(&str, fn())> = vec![
        (
            "first_write_creates_the_directory",
            first_write_creates_the_directory,
        ),
        (
            "directory_in_place_of_the_file_is_inaccessible",
            directory_in_place_of_the_file_is_inaccessible,
        ),
        (
            "a_stale_base_is_refused_with_conflict",
            a_stale_base_is_refused_with_conflict,
        ),
        (
            "a_locked_directory_refuses_based_writes_as_busy",
            a_locked_directory_refuses_based_writes_as_busy,
        ),
        (
            "a_value_over_the_limit_is_too_large",
            a_value_over_the_limit_is_too_large,
        ),
        (
            "the_two_scopes_keep_equal_names_apart",
            the_two_scopes_keep_equal_names_apart,
        ),
        (
            "long_non_ascii_path_round_trips",
            long_non_ascii_path_round_trips,
        ),
        (
            "data_dirs_place_each_scope_under_the_application_name",
            data_dirs_place_each_scope_under_the_application_name,
        ),
    ];
    cases.extend(platform_cases());
    run_table("file_store_contract", &cases);
}

/// The rows only this platform can run.
#[cfg(windows)]
fn platform_cases() -> Vec<(&'static str, fn())> {
    vec![
        (
            "read_only_target_is_inaccessible_not_busy",
            read_only_target_is_inaccessible_not_busy,
        ),
        (
            "a_target_held_without_delete_sharing_is_busy",
            a_target_held_without_delete_sharing_is_busy,
        ),
    ]
}

/// The rows only this platform can run.
#[cfg(not(windows))]
fn platform_cases() -> Vec<(&'static str, fn())> {
    Vec::new()
}
