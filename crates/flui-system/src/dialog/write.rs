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
//! - **The replaced file's mode is kept** (Unix): the new file is given the
//!   permission bits of the one it replaces, where a file made afresh would
//!   get the process's defaults. Its owner, its access control list and its
//!   extended attributes are the new file's own.
//! - **Only a regular file is replaced.** A device or a pipe is written
//!   into.
//! - **Where no new file can be made beside it, the contents are written
//!   into the chosen file itself**: a folder that takes no other name (a
//!   sandbox's document folder, a read-only folder holding a writable
//!   file), a name too long to make a sibling's from, or a file that cannot
//!   be renamed over (a mount point, another user's file in a sticky
//!   folder). That write is not whole or nothing: one that fails midway
//!   leaves the file cut short, which is the price of saving there at all.
//!   A volume out of room is not such a case, since writing into the file
//!   would empty it first: the save fails and the file stands.

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
    /// No new file can be made beside it, be given its mode, or take its
    /// name; nothing of the chosen file changed, and writing into it may
    /// still work.
    NoSibling,
    /// The contents could not be written, or the folder could not be
    /// reached at all: writing into the chosen file would fare no better.
    Failed(io::Error),
}

/// Write `contents` into a new file beside `target`, synced and given the
/// mode of the file it `replaces`, then rename it over `target`.
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
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
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
        let written = file.write_all(contents);
        // Before the sync, so the mode is as durable as the contents.
        let stands_in = written.is_ok() && keeps_mode(&file, replaces);
        let synced = written.and_then(|()| file.sync_all());
        drop(file);
        let replaced = match synced {
            Ok(()) if stands_in => {
                std::fs::rename(&temporary, target).map_err(|_| NotReplaced::NoSibling)
            }
            Ok(()) => Err(NotReplaced::NoSibling),
            Err(error) => Err(NotReplaced::Failed(error)),
        };
        if replaced.is_err() {
            // The write's failure is the answer; the clean-up's is not.
            let _ = std::fs::remove_file(&temporary);
        }
        return replaced;
    }
    // Every name is taken.
    Err(NotReplaced::NoSibling)
}

/// Give `file` the permission bits of the file it `replaces`; `false` when
/// the file system refuses, so the new file would not stand in for the old
/// one.
#[cfg(unix)]
fn keeps_mode(file: &File, replaces: Option<&Metadata>) -> bool {
    replaces.is_none_or(|replaced| file.set_permissions(replaced.permissions()).is_ok())
}

/// A Windows file has no mode: its read-only attribute would keep the new
/// file from taking the old one's name, and its access control list is not
/// copied.
#[cfg(not(unix))]
fn keeps_mode(_: &File, _: Option<&Metadata>) -> bool {
    true
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
