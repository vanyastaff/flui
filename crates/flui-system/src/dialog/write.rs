//! Writing a save's contents to the path the user chose, off the owner
//! thread.
//!
//! The contents replace the chosen file whole or not at all: they go into a
//! new file beside it, synced, which then takes its name, so a write that
//! fails leaves the chosen file as it was and no reader sees it half
//! written. What the user chose decides what is replaced:
//!
//! - **A link is written through.** A save to a symbolic link replaces the
//!   file the link leads to and leaves the link a link: the new file is made
//!   beside that file and takes its name.
//! - **The replaced file's mode is kept** (Unix): the new file is made with
//!   the permission bits of the one it replaces, before anything is written
//!   to it, so the contents are never readable by more users than the old
//!   file's were; a file made where none stood gets the process's defaults.
//!   Its owner, its access control list and its extended attributes are the
//!   new file's own.
//! - **Only a regular file is replaced.** A device or a pipe is written
//!   into.
//! - **Where no new file can be made beside it, or none can take its name by
//!   the file system's design, the contents are written into the chosen file
//!   itself**: a folder that takes no other name (a sandbox's document
//!   folder, a read-only folder holding a writable file), a name too long to
//!   make a sibling's from, a file on another device than its folder (a
//!   mount point), or another user's file in a sticky folder. That write is
//!   not whole or nothing: it empties the file first, so one that fails
//!   midway, or a process killed during it, leaves the file cut short, which
//!   is the price of saving there at all.
//! - **Any other failure leaves the chosen file as it was** and answers the
//!   system's code: a volume out of room, a file another process holds open
//!   without letting it be replaced (on Windows, open without
//!   `FILE_SHARE_DELETE`), a read-only file. The new file beside it is
//!   removed.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flui_platform::SystemFailure;
use flui_platform::capability::{CompletionSource, Resolver};

use super::{DialogError, SaveLocation, SaveResult};

/// Write `contents` to `path` on a thread of its own, then answer `caller`
/// with `location`, or with the failure of the write.
pub(super) fn write_file(
    completions: &CompletionSource,
    path: PathBuf,
    contents: Arc<[u8]>,
    location: SaveLocation,
    caller: Resolver<SaveResult>,
) {
    let (written, answer) = completions.pending::<Result<(), DialogError>>();
    let mut waiting = Some((caller, location));
    written.on_complete(move |outcome| {
        let Some((caller, location)) = waiting.take() else {
            return;
        };
        match outcome {
            Ok(Ok(())) => {
                // A retired owner has nobody left to answer.
                let _ = caller.resolve(Ok(Some(location)));
            }
            Ok(Err(error)) => {
                // A retired owner has nobody left to answer.
                let _ = caller.resolve(Err(error));
            }
            Err(_) => drop(caller),
        }
    });
    // The thread answers through the slot; a thread that cannot start leaves
    // the resolver there, and the failure is answered here.
    let slot = Arc::new(Mutex::new(Some(answer)));
    let writer = Arc::clone(&slot);
    let started = std::thread::Builder::new()
        .name("flui-system-save".to_owned())
        .spawn(move || {
            let written = write_replacing(&path, &contents).map_err(|error| write_failure(&error));
            let answer = writer
                .lock()
                .expect("BUG: the save slot's holders run no code under its lock")
                .take();
            if let Some(answer) = answer {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(written);
            }
        });
    if let Err(error) = started {
        let answer = slot
            .lock()
            .expect("BUG: the save slot's holders run no code under its lock")
            .take();
        if let Some(answer) = answer {
            let _ = answer.resolve(Err(write_failure(&error)));
        }
    }
}

/// How many names beside the chosen file a save tries for its new file
/// before it writes into the chosen file instead: each is taken only by one
/// this process is writing, or left by one that was killed.
const MAX_TEMPORARY_NAMES: u32 = 16;

/// How many links a chosen path may lead through before the system is left
/// to say that they lead nowhere: Linux's own bound.
const MAX_LINKS: u32 = 40;

/// Write `contents` to `path` as the module says: whole or not at all where
/// a new file can be made beside the file `path` names, into that file
/// otherwise.
fn write_replacing(path: &Path, contents: &[u8]) -> io::Result<()> {
    let target = linked_file(path)?;
    let replaced = match std::fs::metadata(&target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if replaced.as_ref().is_none_or(Metadata::is_file) {
        match write_beside(&target, contents, replaced.as_ref()) {
            Ok(()) => return Ok(()),
            Err(NotReplaced::Failed(error)) => return Err(error),
            Err(NotReplaced::NoSibling) => {}
        }
    }
    write_into(&target, contents)
}

/// The file a save to `path` writes: `path` itself, or, when it is a
/// symbolic link, the file the link leads to, whether that file exists yet
/// or not. A relative link is read from its own folder, as the system reads
/// it. Links that lead in a circle answer the system's own error.
fn linked_file(path: &Path) -> io::Result<PathBuf> {
    let mut file = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        let is_link = std::fs::symlink_metadata(&file)
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        if !is_link {
            // A file, nothing yet, or something that cannot be examined,
            // which the write itself answers for.
            return Ok(file);
        }
        let leads_to = std::fs::read_link(&file)?;
        file = match file.parent() {
            Some(folder) => folder.join(leads_to),
            None => leads_to,
        };
    }
    // Still a link after the bound: following it is the system's to refuse.
    std::fs::metadata(&file).map(|_| file)
}

/// Why contents did not reach a file through a new file beside it.
enum NotReplaced {
    /// No new file can be made beside it, or take its name, by the file
    /// system's design; nothing of the chosen file changed, and writing
    /// into it may still work.
    NoSibling,
    /// The contents could not be written, the folder could not be reached,
    /// or the new file could not take the chosen one's name for another
    /// reason (the chosen file held open, a read-only file): nothing of the
    /// chosen file changed, and writing into it would destroy it.
    Failed(io::Error),
}

/// Write `contents` into a new file beside `target`, made with the mode of
/// the file it `replaces` and synced, then rename it over `target`.
fn write_beside(
    target: &Path,
    contents: &[u8],
    replaces: Option<&Metadata>,
) -> Result<(), NotReplaced> {
    let (Some(folder), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(NotReplaced::Failed(ErrorKind::InvalidInput.into()));
    };
    for attempt in 0..MAX_TEMPORARY_NAMES {
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(name);
        temporary_name.push(format!(".{}-{attempt}.flui-save", std::process::id()));
        let temporary = folder.join(temporary_name);
        let mut file = match new_file(&temporary, replaces) {
            Ok(file) => file,
            Err(error) => match error.kind() {
                ErrorKind::AlreadyExists => continue,
                // The folder takes no new file, or no name that long.
                ErrorKind::PermissionDenied
                | ErrorKind::ReadOnlyFilesystem
                | ErrorKind::InvalidFilename => return Err(NotReplaced::NoSibling),
                _ => return Err(NotReplaced::Failed(error)),
            },
        };
        let synced = file.write_all(contents).and_then(|()| file.sync_all());
        drop(file);
        let replaced = synced
            .map_err(NotReplaced::Failed)
            .and_then(|()| std::fs::rename(&temporary, target).map_err(not_renamed));
        if replaced.is_err() {
            // The write's failure is the answer; the clean-up's is not.
            let _ = std::fs::remove_file(&temporary);
        }
        return replaced;
    }
    // Every name is taken.
    Err(NotReplaced::NoSibling)
}

/// Make the file `path` names, which must not exist yet, for writing: on
/// Unix with the permission bits of the file it `replaces` from the start
/// (the process's umask may only narrow them, and they are set exactly
/// before anything is written), so its contents are never readable by more
/// users than the replaced file's. A file the system will not give those
/// bits keeps the narrower ones it was made with.
#[cfg(unix)]
fn new_file(path: &Path, replaces: Option<&Metadata>) -> io::Result<File> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    // What `File::create` gives a file where none stood.
    const DEFAULT_MODE: u32 = 0o666;
    let mode = replaces.map_or(DEFAULT_MODE, |replaced| {
        replaced.permissions().mode() & 0o777
    });
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    if let Some(replaced) = replaces {
        // The bits the umask took, and the set-id and sticky bits, which
        // `open` does not set; refused by some file systems, which then
        // keep the narrower mode.
        let _ = file.set_permissions(replaced.permissions());
    }
    Ok(file)
}

/// Make the file `path` names, which must not exist yet, for writing. A
/// Windows file has no mode: its read-only attribute would keep the new
/// file from taking the old one's name, and its access control list is the
/// folder's to give.
#[cfg(not(unix))]
fn new_file(path: &Path, _: Option<&Metadata>) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// `EPERM`, the same on every Unix FLUI builds for: a rename the folder's
/// sticky bit refuses, since the chosen file is another user's.
#[cfg(unix)]
const EPERM: i32 = 1;

/// Why a new file could not take the chosen one's name. Only what the file
/// system refuses by its design lets the contents go into the chosen file
/// instead: another device than its folder (`EXDEV`, Windows'
/// `ERROR_NOT_SAME_DEVICE`), a mount point (`EBUSY`), another user's file in
/// a sticky folder (`EPERM`). Anything else, a file held open without
/// letting it be replaced or a read-only file among it, fails the save with
/// the chosen file as it was: writing into it would empty a file someone
/// else is using, or one its owner keeps from changing.
fn not_renamed(error: io::Error) -> NotReplaced {
    let by_design = match error.kind() {
        ErrorKind::CrossesDevices => true,
        #[cfg(unix)]
        ErrorKind::ResourceBusy => true,
        #[cfg(unix)]
        ErrorKind::PermissionDenied => error.raw_os_error() == Some(EPERM),
        _ => false,
    };
    if by_design {
        NotReplaced::NoSibling
    } else {
        NotReplaced::Failed(error)
    }
}

/// Write `contents` into `target` itself, emptied first, and sync it when
/// it is a regular file (a device or a pipe has nothing to sync).
fn write_into(target: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(target)?;
    file.write_all(contents)?;
    if file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        file.sync_all()?;
    }
    Ok(())
}

/// The failure a write reports: the system's code, or a write that took no
/// bytes without one ([`SystemFailure::MalformedAnswer`]).
fn write_failure(error: &io::Error) -> DialogError {
    DialogError::Failed(
        error
            .raw_os_error()
            .map_or(SystemFailure::MalformedAnswer, SystemFailure::Os),
    )
}
