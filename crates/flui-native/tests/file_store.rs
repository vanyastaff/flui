//! The file store behind `AppConfig::with_storage_dir`: directories created on the
//! first write, values replaced atomically, inaccessible paths reported apart
//! from corrupt ones, a write based on a stale version refused, and a value
//! copied aside under a name no value can have. Native only: on wasm32 the
//! store is the page's IndexedDB.
#![cfg(not(target_arch = "wasm32"))]

use std::io::ErrorKind;
use std::path::Path;

use flui_native::storage::{DataDirs, FileStore, MemoryStore, data_dirs};
use flui_native::{HeadlessPlatform, Platform};
use flui_platform::{AppDataDir, StorageError, StorageName, StoredVersion, WriteMode};

use crate::run_table;

const NOTES: StorageName = StorageName::from_static("notes");
const SESSION: StorageName = StorageName::machine_local("notes");

fn separate_roots(dir: &Path) -> DataDirs {
    DataDirs {
        roaming: dir.join("roaming").join("app"),
        local: dir.join("local").join("app"),
    }
}

/// The entries of `dir` other than the value `notes`, the store's lock file
/// and its machine-local directory: what a write staged and left behind.
fn staged_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| {
                    !matches!(
                        name.as_str(),
                        "notes" | ".flui-storage.lock" | ".machine-local"
                    )
                })
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
    type Layout = fn(&Path) -> DataDirs;
    let layouts: [(&str, Layout); 3] = [
        ("separate roots", separate_roots),
        ("one root", |dir| DataDirs {
            roaming: dir.join("app"),
            local: dir.join("app"),
        }),
        ("one root spelled two ways", |dir| DataDirs {
            roaming: dir.join("app"),
            local: dir.join("other").join("..").join("app"),
        }),
    ];
    for (layout, dirs_in) in layouts {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("other")).expect("a sibling directory");
        let dirs = dirs_in(dir.path());
        let store = FileStore::new(dirs.clone());
        store
            .write(&NOTES, b"data", WriteMode::Replace)
            .expect("data");
        store
            .write(&SESSION, b"session", WriteMode::Replace)
            .expect("session");

        let data = store.read(&NOTES, 1024).expect("data reads");
        let session = store.read(&SESSION, 1024).expect("session reads");
        assert_eq!(data.bytes.as_deref(), Some(&b"data"[..]), "{layout}");
        assert_eq!(session.bytes.as_deref(), Some(&b"session"[..]), "{layout}");
        assert_eq!(
            std::fs::read(dirs.roaming.join("notes")).expect("data under the roaming root"),
            b"data",
            "{layout}"
        );
        assert_eq!(
            std::fs::read(dirs.local.join(".machine-local").join("notes"))
                .expect("session under the local root"),
            b"session",
            "{layout}"
        );
    }
}

/// A based write compares the stored length before reading: a stored value
/// far longer than the base is refused without being read, so this finishes
/// at once instead of reading a terabyte. Sparse files make that size free
/// on Unix; NTFS would allocate it, so the row runs there only.
#[cfg(unix)]
fn a_stale_base_is_refused_without_reading_a_long_value() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    std::fs::create_dir_all(&dirs.roaming).expect("root");
    std::fs::File::create(dirs.roaming.join("notes"))
        .and_then(|file| file.set_len(1 << 40))
        .expect("a sparse terabyte");
    let store = FileStore::new(dirs);
    assert_eq!(
        store.write(
            &NOTES,
            b"short",
            WriteMode::IfUnchanged(StoredVersion::of_bytes(b"short"))
        ),
        Err(StorageError::Conflict)
    );
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
    let app = AppDataDir::try_new("dev.flui.notes").expect("an identifier is a directory name");
    if let Some(dirs) = data_dirs(&app) {
        assert!(dirs.roaming.ends_with("dev.flui.notes"), "{dirs:?}");
        assert!(dirs.local.ends_with("dev.flui.notes"), "{dirs:?}");
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

/// A target pending delete (its delete-on-close handle closed while another
/// handle keeps it) refuses every new open with `ERROR_ACCESS_DENIED`, the
/// code an ACL gives; the state passes once the last handle closes, so the
/// write must be retryable, never `Inaccessible`.
#[cfg(windows)]
fn a_target_pending_delete_is_busy_not_inaccessible() {
    use std::os::windows::fs::OpenOptionsExt;

    const GENERIC_READ: u32 = 0x8000_0000;
    const DELETE: u32 = 0x0001_0000;
    const SHARE_ALL: u32 = 0x7;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    store
        .write(&NOTES, b"kept", WriteMode::Replace)
        .expect("written");
    let target = dirs.roaming.join("notes");
    let keeper = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .open(&target)
        .expect("a handle that keeps the file");
    let deleter = std::fs::OpenOptions::new()
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(&target)
        .expect("a delete-on-close handle");
    drop(deleter);
    assert_eq!(
        std::fs::OpenOptions::new()
            .access_mode(0)
            .open(&target)
            .map_err(|error| error.raw_os_error())
            .err(),
        Some(Some(5)),
        "the target is pending delete: even an attributes-only open is refused"
    );

    let result = store.write(&NOTES, b"later", WriteMode::Replace);
    drop(keeper);
    assert_eq!(result, Err(StorageError::Busy));
    store
        .write(&NOTES, b"later", WriteMode::Replace)
        .expect("the write goes through once the delete completes");
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

fn copy_aside_keeps_the_value_under_the_first_free_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    assert_eq!(store.copy_aside(&NOTES), Err(StorageError::NotFound));

    store
        .write(&NOTES, b"unreadable", WriteMode::Replace)
        .expect("the value is stored");
    // A copy an earlier run kept is never written over.
    std::fs::write(dirs.roaming.join("notes.corrupt-1"), b"kept before").expect("an old copy");

    let copied = store.copy_aside(&NOTES).expect("the value is copied");
    assert_eq!(copied.kept_as, "notes.corrupt-2");
    assert_eq!(copied.version, StoredVersion::of_bytes(b"unreadable"));
    let again = store.copy_aside(&NOTES).expect("a second copy");
    assert_eq!(again.kept_as, "notes.corrupt-3");
    for (file, bytes) in [
        ("notes.corrupt-1", &b"kept before"[..]),
        ("notes.corrupt-2", b"unreadable"),
        ("notes.corrupt-3", b"unreadable"),
    ] {
        assert_eq!(
            std::fs::read(dirs.roaming.join(file)).expect("the copy reads"),
            bytes,
            "{file}"
        );
    }
    let stored = store.read(&NOTES, 1024).expect("the value reads");
    assert_eq!(
        stored.bytes.as_deref(),
        Some(&b"unreadable"[..]),
        "the value stays"
    );

    // The copy's version is the base a fresh start writes over.
    store
        .write(&NOTES, b"fresh", WriteMode::IfUnchanged(copied.version))
        .expect("the copied value is replaced");

    // A machine-local value is copied within its own scope.
    store
        .write(&SESSION, b"session", WriteMode::Replace)
        .expect("the session is stored");
    let session = store.copy_aside(&SESSION).expect("the session is copied");
    assert_eq!(session.kept_as, "notes.corrupt-1");
    assert_eq!(
        std::fs::read(dirs.local.join(".machine-local").join("notes.corrupt-1"))
            .expect("the session copy reads"),
        b"session"
    );
}

/// Without a root a headless platform keeps each application's values in its
/// own memory: every store it hands out for that application sees them,
/// another application or another platform does not, and nothing reaches the
/// user's data directories.
fn a_headless_platform_keeps_values_in_memory_without_a_root() {
    let app = AppDataDir::try_new("dev.flui.headless-memory").expect("a directory name");
    let platform = HeadlessPlatform::new();
    let store = platform
        .data_store(&app)
        .expect("headless always has a store");
    store
        .write(&NOTES, b"kept", WriteMode::Replace)
        .expect("a memory write succeeds");

    let again = platform.data_store(&app).expect("a second store");
    let stored = again.read(&NOTES, 1024).expect("the value reads");
    assert_eq!(
        stored.bytes.as_deref(),
        Some(&b"kept"[..]),
        "one platform, one memory"
    );
    let other = HeadlessPlatform::new()
        .data_store(&app)
        .expect("another platform's store");
    assert_eq!(
        other.read(&NOTES, 1024).expect("reads").bytes,
        None,
        "another platform starts empty"
    );
    let neighbour = AppDataDir::try_new("dev.flui.headless-neighbour").expect("a directory name");
    assert_eq!(
        platform
            .data_store(&neighbour)
            .expect("the neighbour's store")
            .read(&NOTES, 1024)
            .expect("reads")
            .bytes,
        None,
        "another application on the same platform keeps its own values"
    );
    if let Some(dirs) = data_dirs(&app) {
        assert!(
            !dirs.roaming.exists() && !dirs.local.exists(),
            "the user's data directories are untouched: {dirs:?}"
        );
    }
}

/// A headless platform given a root keeps the values on disk under it, one
/// directory per scope and application.
fn a_headless_data_root_keeps_values_on_disk_under_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app = AppDataDir::try_new("app").expect("a directory name");
    let store = HeadlessPlatform::new()
        .with_data_root(dir.path())
        .data_store(&app)
        .expect("a store under the root");
    store
        .write(&NOTES, b"roaming", WriteMode::Replace)
        .expect("written");
    store
        .write(&SESSION, b"local", WriteMode::Replace)
        .expect("written");
    let dirs = separate_roots(dir.path());
    assert_eq!(
        std::fs::read(dirs.roaming.join("notes")).expect("the roaming file"),
        b"roaming"
    );
    assert_eq!(
        std::fs::read(dirs.local.join(".machine-local").join("notes")).expect("the local file"),
        b"local"
    );
}

/// The memory store answers as the file store does: a stale base is a
/// conflict, the two scopes keep equal names apart, a copy aside takes the
/// first free number in its scope, and a value over the limit is too large.
fn the_memory_store_compares_and_copies_as_the_file_store_does() {
    let store = MemoryStore::new();
    let first = store
        .write(
            &NOTES,
            b"one",
            WriteMode::IfUnchanged(StoredVersion::ABSENT),
        )
        .expect("written over nothing");
    store
        .write(&NOTES, b"two", WriteMode::IfUnchanged(first))
        .expect("written over its base");
    assert_eq!(
        store.write(&NOTES, b"three", WriteMode::IfUnchanged(first)),
        Err(StorageError::Conflict),
        "a stale base is refused"
    );
    store
        .write(&SESSION, b"session", WriteMode::Replace)
        .expect("the other scope");
    assert_eq!(
        store.read(&NOTES, 1024).expect("reads").bytes.as_deref(),
        Some(&b"two"[..])
    );
    assert_eq!(
        store.read(&SESSION, 1024).expect("reads").bytes.as_deref(),
        Some(&b"session"[..])
    );

    assert_eq!(
        store.copy_aside(&NOTES).expect("copied").kept_as,
        "notes.corrupt-1"
    );
    let second = store.copy_aside(&NOTES).expect("copied again");
    assert_eq!(second.kept_as, "notes.corrupt-2");
    assert_eq!(second.version, StoredVersion::of_bytes(b"two"));
    assert_eq!(
        store.copy_aside(&SESSION).expect("copied").kept_as,
        "notes.corrupt-1",
        "each scope counts its own copies"
    );
    assert_eq!(
        store.copy_aside(&StorageName::from_static("missing")),
        Err(StorageError::NotFound)
    );
    assert!(matches!(
        store.read(&NOTES, 2),
        Err(StorageError::TooLarge { len: 3, limit: 2 })
    ));
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
        (
            "copy_aside_keeps_the_value_under_the_first_free_name",
            copy_aside_keeps_the_value_under_the_first_free_name,
        ),
        (
            "a_headless_platform_keeps_values_in_memory_without_a_root",
            a_headless_platform_keeps_values_in_memory_without_a_root,
        ),
        (
            "a_headless_data_root_keeps_values_on_disk_under_it",
            a_headless_data_root_keeps_values_on_disk_under_it,
        ),
        (
            "the_memory_store_compares_and_copies_as_the_file_store_does",
            the_memory_store_compares_and_copies_as_the_file_store_does,
        ),
    ];
    cases.extend(platform_cases());
    run_table("file_store_contract", &cases);
}

/// A stored value, a copy of it and the directories the store creates for
/// them are reachable by their owner only, whatever the umask leaves to
/// others.
#[cfg(unix)]
fn stored_values_and_their_copies_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let mode_of = |path: &Path| {
        std::fs::metadata(path)
            .expect("the entry exists")
            .permissions()
            .mode()
            & 0o777
    };
    let dir = tempfile::tempdir().expect("temp dir");
    let dirs = separate_roots(dir.path());
    let store = FileStore::new(dirs.clone());
    store
        .write(&NOTES, b"private", WriteMode::Replace)
        .expect("the value is stored");
    store
        .write(&SESSION, b"private", WriteMode::Replace)
        .expect("the machine-local value is stored");
    let copied = store.copy_aside(&NOTES).expect("the value is copied");
    for file in ["notes", copied.kept_as.as_str()] {
        assert_eq!(mode_of(&dirs.roaming.join(file)), 0o600, "{file}");
    }
    let machine_local = dirs.local.join(".machine-local");
    assert_eq!(mode_of(&machine_local.join("notes")), 0o600);
    for created in [
        dir.path().join("roaming"),
        dirs.roaming.clone(),
        dirs.local,
        machine_local,
    ] {
        assert_eq!(mode_of(&created), 0o700, "{}", created.display());
    }
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
        (
            "a_target_pending_delete_is_busy_not_inaccessible",
            a_target_pending_delete_is_busy_not_inaccessible,
        ),
    ]
}

/// The rows only this platform can run.
#[cfg(unix)]
fn platform_cases() -> Vec<(&'static str, fn())> {
    vec![
        (
            "a_stale_base_is_refused_without_reading_a_long_value",
            a_stale_base_is_refused_without_reading_a_long_value,
        ),
        (
            "stored_values_and_their_copies_are_owner_only",
            stored_values_and_their_copies_are_owner_only,
        ),
    ]
}
