//! [`FileStore`]: one file per name, replaced atomically.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use flui_platform_api::{StorageError, StorageName, Stored, StoredVersion, WriteMode};

use super::DataDirs;
use super::os_error::{lock_error, replace_error, storage_error};

/// The file in each root that a write based on a version locks while it
/// compares and replaces. Every process sharing the root locks the same file.
const LOCK_FILE: &str = ".flui-storage.lock";

/// The directory under the local root that machine-local values live in, so
/// they stay apart from roaming values even when both roots name the same
/// directory (Linux), however the two paths are spelled. A [`StorageName`]
/// cannot start with `.`, so no value is ever named like it.
const MACHINE_LOCAL_DIR: &str = ".machine-local";

/// The start of every file this store stages a write in. Specific to this
/// store, so a sweep never touches another program's temporary files.
const STAGED_PREFIX: &str = ".flui-staged-";

/// How old a staged file must be before a based write sweeps it away as the
/// leftover of a killed writer. A live write keeps its file younger than
/// this: it writes, flushes and renames in one call.
const STALE_STAGED_AGE: Duration = Duration::from_secs(60);

/// A store of named byte values, one file each, under the two roots of a
/// [`DataDirs`].
///
/// Each call blocks on the disk until it is done: run it on an IO thread.
/// Calls on the same root from several threads or processes are safe: a
/// value is replaced by one rename of a fully written file, and a write
/// based on a version holds the root's lock while it compares and replaces;
/// [`WriteMode::Replace`] takes no lock, and the last writer wins.
pub struct FileStore {
    roaming: PathBuf,
    local: PathBuf,
    /// Called at each step of a write; a test interrupts or pauses the write
    /// there.
    #[cfg(test)]
    step_hook: Option<StepHook>,
}

impl FileStore {
    /// A store under `dirs`. Nothing is created until the first write.
    ///
    /// Roaming values live directly under `dirs.roaming`; machine-local ones
    /// under `dirs.local/.machine-local`, so equal text in the two scopes
    /// names two files even when both roots are one directory.
    #[must_use]
    pub fn new(dirs: DataDirs) -> Self {
        let DataDirs { roaming, local } = dirs;
        Self {
            roaming,
            local: local.join(MACHINE_LOCAL_DIR),
            #[cfg(test)]
            step_hook: None,
        }
    }

    /// Read the value stored under `name`.
    ///
    /// A missing value, or a missing root, is `Ok` with no bytes and
    /// [`StoredVersion::ABSENT`].
    ///
    /// # Errors
    ///
    /// [`StorageError::TooLarge`] for a value over `limit` bytes, found from
    /// its length before it is read. When the file grew past the limit while
    /// it was being read, at most `limit + 1` bytes are read and
    /// [`TooLarge::len`](StorageError::TooLarge) is a lower bound of its
    /// length. [`StorageError::Busy`] while another
    /// process holds the file; [`StorageError::Inaccessible`] when it cannot
    /// be read (no permission, a directory in its place).
    pub fn read(&self, name: &StorageName, limit: u64) -> Result<Stored, StorageError> {
        let target = self.path_of(name);
        let metadata = match fs::metadata(&target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Stored {
                    bytes: None,
                    version: StoredVersion::ABSENT,
                });
            }
            Err(error) => return Err(storage_error(&error)),
        };
        if metadata.is_dir() {
            return Err(StorageError::Inaccessible {
                kind: ErrorKind::IsADirectory,
            });
        }
        if metadata.len() > limit {
            return Err(StorageError::TooLarge {
                len: metadata.len(),
                limit,
            });
        }
        let file = match File::open(&target) {
            Ok(file) => file,
            // Removed since its length was read.
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Stored {
                    bytes: None,
                    version: StoredVersion::ABSENT,
                });
            }
            Err(error) => return Err(storage_error(&error)),
        };
        // The file may have grown since its length was read: read one byte
        // past the limit at most, never the whole file.
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        file.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| storage_error(&error))?;
        let len = u64::try_from(bytes.len()).expect("BUG: a byte length fits in u64");
        if len > limit {
            return Err(StorageError::TooLarge { len, limit });
        }
        Ok(Stored {
            version: StoredVersion::of_bytes(&bytes),
            bytes: Some(bytes),
        })
    }

    /// Store `bytes` under `name`, creating the root on the first write.
    ///
    /// The bytes go to a uniquely named file in the root, are flushed to the
    /// disk, and replace the value with one rename, so the old value stays
    /// whole until the new one is complete. Under
    /// [`WriteMode::IfUnchanged`] the root's lock is held from comparing the
    /// stored version to replacing it.
    ///
    /// Returns the version of the value now stored. A write that fails
    /// leaves the old value and no staged file behind.
    ///
    /// # Errors
    ///
    /// [`StorageError::Conflict`] when the stored version is not the base;
    /// [`StorageError::Busy`] while another writer holds the lock or another
    /// process holds the file without letting it be replaced (trying again
    /// later may succeed); [`StorageError::LockUnsupported`] for a based write
    /// on a file system without locks; [`StorageError::Full`];
    /// [`StorageError::Inaccessible`] for a target or root that refuses the
    /// write for good (no permission, a read-only file, a directory in its
    /// place), never retried.
    pub fn write(
        &self,
        name: &StorageName,
        bytes: &[u8],
        mode: WriteMode,
    ) -> Result<StoredVersion, StorageError> {
        let root = self.root_of(name);
        let target = root.join(name.as_str());
        fs::create_dir_all(root).map_err(|error| storage_error(&error))?;

        // Held until this function returns: past the rename.
        let _lock = match mode {
            WriteMode::Replace => None,
            WriteMode::IfUnchanged(base) => {
                let lock = lock_root(root)?;
                sweep_stale_staged_files(root);
                if !holds_version(&target, base)? {
                    return Err(StorageError::Conflict);
                }
                Some(lock)
            }
            // A mode this store does not know how to honour is refused
            // rather than guessed at.
            _ => return Err(StorageError::Unavailable),
        };
        if self.reach(Step::VersionChecked).is_break() {
            return Err(StorageError::Cancelled);
        }

        let staged = tempfile::Builder::new()
            .prefix(STAGED_PREFIX)
            // `std` opens the file: it takes paths past 260 characters on
            // Windows, and marks nothing temporary on the file that becomes
            // the value.
            .make_in(root, |path| {
                OpenOptions::new().write(true).create_new(true).open(path)
            })
            .map_err(|error| storage_error(&error))?;
        if self.reach(Step::Staged).is_break() {
            return Err(interrupted(staged.into_temp_path()));
        }
        let (head, tail) = bytes.split_at(bytes.len() / 2);
        staged
            .as_file()
            .write_all(head)
            .map_err(|error| storage_error(&error))?;
        if self.reach(Step::HalfWritten).is_break() {
            return Err(interrupted(staged.into_temp_path()));
        }
        staged
            .as_file()
            .write_all(tail)
            .and_then(|()| staged.as_file().sync_all())
            .map_err(|error| storage_error(&error))?;
        if self.reach(Step::Flushed).is_break() {
            return Err(interrupted(staged.into_temp_path()));
        }

        // Closed before the rename; deleted on drop unless the rename took it.
        let mut staged = staged.into_temp_path();
        // `std::fs::rename`, not `NamedTempFile::persist`: on Windows `std`
        // passes a `\\?\` path, so a target past 260 characters works, and
        // falls back to POSIX-semantics replacement when the plain move is
        // refused.
        fs::rename(&staged, &target).map_err(|error| replace_error(&error, &target))?;
        staged.disable_cleanup(true);
        sync_directory(root);
        if self.reach(Step::Replaced).is_break() {
            return Err(StorageError::Cancelled);
        }
        Ok(StoredVersion::of_bytes(bytes))
    }

    /// The root `name` lives under.
    fn root_of(&self, name: &StorageName) -> &Path {
        if name.is_machine_local() {
            &self.local
        } else {
            &self.roaming
        }
    }

    /// The file holding `name`'s value.
    fn path_of(&self, name: &StorageName) -> PathBuf {
        self.root_of(name).join(name.as_str())
    }

    #[cfg(not(test))]
    #[expect(
        clippy::unused_self,
        reason = "the test build reads the step hook from `self`"
    )]
    fn reach(&self, _step: Step) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

/// A point a write passes; a test stops the write there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The root is locked (for a based write) and the version compared.
    VersionChecked,
    /// The staged file exists, empty.
    Staged,
    /// Half the bytes are in the staged file.
    HalfWritten,
    /// Every byte is in the staged file and flushed to the disk.
    Flushed,
    /// The staged file replaced the target.
    Replaced,
}

/// Lock `root` for a based write.
fn lock_root(root: &Path) -> Result<File, StorageError> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(LOCK_FILE))
        .map_err(|error| storage_error(&error))?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(StorageError::Busy),
        Err(TryLockError::Error(error)) => Err(lock_error(&error)),
    }
}

/// Whether `target` holds the value `base` names. The lengths are compared
/// first, so a value of another length, however long, is never read; one of
/// the same length is read and hashed, never past that length.
fn holds_version(target: &Path, base: StoredVersion) -> Result<bool, StorageError> {
    let file = match File::open(target) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(base == StoredVersion::ABSENT);
        }
        Err(error) => return Err(storage_error(&error)),
    };
    let len = file
        .metadata()
        .map_err(|error| storage_error(&error))?
        .len();
    let Some(base_len) = base.byte_len() else {
        return Ok(false);
    };
    if len != base_len {
        return Ok(false);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(len.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| storage_error(&error))?;
    Ok(StoredVersion::of_bytes(&bytes) == base)
}

/// Remove files this store staged in `root` that are older than
/// [`STALE_STAGED_AGE`]: the leftovers of writers killed before their rename.
/// Called under the root's lock; a file that cannot be inspected or removed
/// is left for a later sweep.
fn sweep_stale_staged_files(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGED_PREFIX))
        {
            continue;
        }
        let stale = entry.metadata().is_ok_and(|metadata| {
            metadata.is_file()
                && metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age > STALE_STAGED_AGE)
        });
        if stale && let Err(error) = fs::remove_file(entry.path()) {
            tracing::debug!(%error, path = %entry.path().display(), "removing a stale staged file failed");
        }
    }
}

/// Make the rename itself durable where the file system needs the
/// directory flushed for it. The value is already replaced: a failure here
/// only weakens durability against power loss, so it is logged, not
/// reported.
fn sync_directory(root: &Path) {
    #[cfg(unix)]
    if let Err(error) = File::open(root).and_then(|directory| directory.sync_all()) {
        tracing::debug!(%error, root = %root.display(), "flushing the storage directory failed");
    }
    #[cfg(not(unix))]
    let _ = root;
}

/// Leave the staged file where a killed process would leave it.
fn interrupted(mut staged: tempfile::TempPath) -> StorageError {
    staged.disable_cleanup(true);
    StorageError::Cancelled
}

/// What a test runs at each [`Step`] of a write.
#[cfg(test)]
type StepHook = std::sync::Arc<dyn Fn(Step) -> ControlFlow<()> + Send + Sync>;

#[cfg(test)]
impl FileStore {
    fn with_step_hook(
        mut self,
        hook: impl Fn(Step) -> ControlFlow<()> + Send + Sync + 'static,
    ) -> Self {
        self.step_hook = Some(std::sync::Arc::new(hook));
        self
    }

    fn reach(&self, step: Step) -> ControlFlow<()> {
        self.step_hook
            .as_ref()
            .map_or(ControlFlow::Continue(()), |hook| hook(step))
    }
}

impl std::fmt::Debug for FileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileStore")
            .field("roaming", &self.roaming)
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::ops::ControlFlow;
    use std::sync::mpsc;
    use std::time::Duration;

    use flui_platform_api::{StorageError, StorageName, StoredVersion, WriteMode};

    use super::{FileStore, STAGED_PREFIX, STALE_STAGED_AGE, Step};
    use crate::storage::DataDirs;
    use crate::table_test::run_table;

    const NOTES: StorageName = StorageName::from_static("notes");
    const OLD: &[u8] = b"flui-document notes 1 1\n{\"titles\":[\"old\"]}";
    const NEW: &[u8] = b"flui-document notes 1 2\n{\"titles\":[\"new\",\"longer\"]}";

    /// The files in `dir` this store staged a write in.
    fn staged_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .expect("the root exists")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(STAGED_PREFIX))
            })
            .map(|entry| entry.path())
            .collect()
    }

    /// A writer killed before its rename leaves its staged file; a later
    /// based write removes it once it is older than a live write could be,
    /// and leaves a young staged file and another program's file alone.
    #[test]
    fn a_based_write_sweeps_stale_staged_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dirs_in(&dir).roaming;
        for _ in 0..2 {
            assert_eq!(
                interrupted_at(&dir, Step::Flushed).write(&NOTES, NEW, WriteMode::Replace),
                Err(StorageError::Cancelled)
            );
        }
        let orphans = staged_files(&root);
        assert_eq!(orphans.len(), 2, "both killed writers left a file");
        let old = std::time::SystemTime::now() - STALE_STAGED_AGE - Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&orphans[0])
            .and_then(|file| file.set_modified(old))
            .expect("age the first orphan");
        let foreign = root.join(".tmp-another-program");
        std::fs::write(&foreign, b"theirs").expect("another program's file");
        std::fs::File::options()
            .write(true)
            .open(&foreign)
            .and_then(|file| file.set_modified(old))
            .expect("age it too");

        FileStore::new(dirs_in(&dir))
            .write(&NOTES, OLD, WriteMode::IfUnchanged(StoredVersion::ABSENT))
            .expect("the based write lands");

        assert_eq!(staged_files(&root), vec![orphans[1].clone()]);
        assert!(foreign.exists(), "another program's file is not swept");
    }

    fn dirs_in(dir: &tempfile::TempDir) -> DataDirs {
        DataDirs {
            roaming: dir.path().join("roaming"),
            local: dir.path().join("local"),
        }
    }

    /// A store whose writes stop at `step` the way a killed process would.
    fn interrupted_at(dir: &tempfile::TempDir, step: Step) -> FileStore {
        FileStore::new(dirs_in(dir)).with_step_hook(move |reached| {
            if reached == step {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
    }

    /// Write `OLD` (or nothing), interrupt a based write of `NEW` at `step`,
    /// check that a fresh store reads `expected`, then that a based write
    /// from that state still succeeds.
    fn interrupt(step: Step, with_old: bool, expected: Option<&[u8]>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = if with_old {
            FileStore::new(dirs_in(&dir))
                .write(&NOTES, OLD, WriteMode::Replace)
                .expect("the old value is written")
        } else {
            StoredVersion::ABSENT
        };
        assert_eq!(
            interrupted_at(&dir, step).write(&NOTES, NEW, WriteMode::IfUnchanged(base)),
            Err(StorageError::Cancelled),
            "the write stops at {step:?}"
        );

        let next_run = FileStore::new(dirs_in(&dir));
        let stored = next_run.read(&NOTES, 1024).expect("the value reads");
        assert_eq!(
            stored.bytes.as_deref(),
            expected,
            "after stopping at {step:?}"
        );
        next_run
            .write(&NOTES, b"after", WriteMode::IfUnchanged(stored.version))
            .expect("a based write after the interruption succeeds");
    }

    fn interrupted_after_the_lock_keeps_the_old_value() {
        interrupt(Step::VersionChecked, true, Some(OLD));
    }

    fn interrupted_with_an_empty_staged_file_keeps_the_old_value() {
        interrupt(Step::Staged, true, Some(OLD));
    }

    fn interrupted_half_written_keeps_the_old_value() {
        interrupt(Step::HalfWritten, true, Some(OLD));
    }

    fn interrupted_after_the_flush_keeps_the_old_value() {
        interrupt(Step::Flushed, true, Some(OLD));
    }

    fn interrupted_after_the_replace_reads_the_new_value() {
        interrupt(Step::Replaced, true, Some(NEW));
    }

    fn interrupted_first_write_reads_as_absent() {
        interrupt(Step::HalfWritten, false, None);
    }

    /// A process killed at any step of a write leaves the old whole value
    /// or the new whole value, never a mix, and the next write proceeds.
    ///
    /// Limit: `sync_all` matters only on power loss, which no test here can
    /// cause; a killed process loses nothing the kernel already holds.
    #[test]
    fn file_store_interruption_matrix() {
        run_table(
            "file_store_interruption_matrix",
            &[
                (
                    "interrupted_after_the_lock_keeps_the_old_value",
                    interrupted_after_the_lock_keeps_the_old_value,
                ),
                (
                    "interrupted_with_an_empty_staged_file_keeps_the_old_value",
                    interrupted_with_an_empty_staged_file_keeps_the_old_value,
                ),
                (
                    "interrupted_half_written_keeps_the_old_value",
                    interrupted_half_written_keeps_the_old_value,
                ),
                (
                    "interrupted_after_the_flush_keeps_the_old_value",
                    interrupted_after_the_flush_keeps_the_old_value,
                ),
                (
                    "interrupted_after_the_replace_reads_the_new_value",
                    interrupted_after_the_replace_reads_the_new_value,
                ),
                (
                    "interrupted_first_write_reads_as_absent",
                    interrupted_first_write_reads_as_absent,
                ),
            ],
        );
    }

    /// Two stores on one directory, in two threads, both write from the
    /// same base. The first is held between comparing the version and
    /// replacing the file until the second has tried once; exactly one write
    /// lands and the other is refused, and the file holds the winner's
    /// bytes.
    #[test]
    fn two_threads_interleave_and_exactly_one_conflicts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (checked_tx, checked_rx) = mpsc::channel::<()>();
        let (tried_tx, tried_rx) = mpsc::channel::<()>();
        let tried_rx = std::sync::Mutex::new(tried_rx);

        let first = FileStore::new(dirs_in(&dir)).with_step_hook(move |step| {
            if step == Step::VersionChecked {
                checked_tx.send(()).expect("the second writer waits");
                // Bounded, so a store that lets the second write through
                // without waiting still finishes and fails the assertions.
                let _ = tried_rx
                    .lock()
                    .expect("one writer")
                    .recv_timeout(Duration::from_secs(10));
            }
            ControlFlow::Continue(())
        });
        let second = FileStore::new(dirs_in(&dir));

        let (first_result, second_result) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                first.write(
                    &NOTES,
                    b"first",
                    WriteMode::IfUnchanged(StoredVersion::ABSENT),
                )
            });
            let second = scope.spawn(move || {
                checked_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("the first writer compared its version");
                let mut result = second.write(
                    &NOTES,
                    b"second",
                    WriteMode::IfUnchanged(StoredVersion::ABSENT),
                );
                tried_tx.send(()).expect("the first writer is waiting");
                while result == Err(StorageError::Busy) {
                    std::thread::sleep(Duration::from_millis(1));
                    result = second.write(
                        &NOTES,
                        b"second",
                        WriteMode::IfUnchanged(StoredVersion::ABSENT),
                    );
                }
                result
            });
            (
                first.join().expect("first writer"),
                second.join().expect("second writer"),
            )
        });

        let results = [&first_result, &second_result];
        let conflicts = results
            .iter()
            .filter(|result| ***result == Err(StorageError::Conflict))
            .count();
        assert_eq!(
            conflicts, 1,
            "exactly one write is refused: {first_result:?}, {second_result:?}"
        );
        let winner: &[u8] = if first_result.is_ok() {
            b"first"
        } else {
            b"second"
        };
        let stored = FileStore::new(dirs_in(&dir))
            .read(&NOTES, 1024)
            .expect("the value reads");
        assert_eq!(stored.bytes.as_deref(), Some(winner));
    }
}
