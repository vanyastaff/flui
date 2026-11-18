//! A file a provider hands to another application, opened before it is
//! judged: what the descriptor holds and the name it ends at decide, never
//! what the path names, which can change after the check.
//!
//! The open follows links, so the file judged is the one the path leads to
//! now. `O_NONBLOCK` keeps a named pipe from holding the worker until a
//! writer comes, `O_NOCTTY` keeps a terminal from becoming the process's,
//! and the standard library adds `O_CLOEXEC`. The descriptor is examined
//! (`fstat`) before anything is read, and its final name is the one the
//! kernel gives it (`/proc/self/fd`). The launcher and the share sheet each
//! judge the result by their own rule.

/// What an opened path turned out to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// Anything else: a pipe, a device, a socket.
    Other,
    /// A file no name leads to any more (unlinked after the open), whose
    /// final name the kernel gives as `<name> (deleted)`.
    Unlinked,
}

/// A path opened for reading, with what it holds and its final name.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
#[derive(Debug)]
pub(crate) struct Opened {
    /// The open file, whose descriptor is what is handed on.
    pub(crate) file: std::fs::File,
    /// What the descriptor holds.
    pub(crate) held: Held,
    /// The name the descriptor ends at, links resolved.
    pub(crate) final_name: std::path::PathBuf,
}

#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
impl Opened {
    /// The last part of the final name, which the file is handed over
    /// under; `None` for the root.
    pub(crate) fn file_name(&self) -> Option<String> {
        self.final_name
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }
}

/// `path`, opened as the module says.
///
/// # Errors
///
/// The system's error for an open, an `fstat` or a read of the final name
/// that failed.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
pub(crate) fn open(path: &std::path::Path) -> std::io::Result<Opened> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)?;
    examine(file)
}

/// What the open `file` holds, by its `fstat`, and the name it ends at.
///
/// The name is read first and the `fstat` after it, so a file unlinked at
/// any point before the `fstat` counts no link and is [`Held::Unlinked`],
/// whatever name was read: the kernel names such a file `<name> (deleted)`,
/// which no rule about names can judge. One unlinked after the `fstat` was
/// still named by its link when the name was read.
///
/// # Errors
///
/// The system's error for an `fstat` or a read of the final name.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
pub(crate) fn examine(file: std::fs::File) -> std::io::Result<Opened> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::MetadataExt as _;

    let final_name = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    let metadata = file.metadata()?;
    let kind = metadata.file_type();
    let held = if metadata.nlink() == 0 {
        Held::Unlinked
    } else if kind.is_file() {
        Held::File
    } else if kind.is_dir() {
        Held::Folder
    } else {
        Held::Other
    };
    Ok(Opened {
        file,
        held,
        final_name,
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{Held, examine, open};

    /// A file unlinked between its open and the read of its name is judged
    /// by the `fstat` that already counted no link, not by the `(deleted)`
    /// name the kernel gives it then.
    #[test]
    fn a_file_unlinked_after_its_open_is_held_as_unlinked() {
        let folder = tempfile::tempdir().expect("a folder of the test's own");
        let script = folder.path().join("run.py");
        std::fs::write(&script, b"#!/bin/sh\n").expect("written");
        let file = std::fs::File::open(&script).expect("opened");
        std::fs::remove_file(&script).expect("unlinked");
        let opened = examine(file).expect("examined");
        assert_eq!(opened.held, Held::Unlinked);
        assert!(
            opened.final_name.to_string_lossy().ends_with("(deleted)"),
            "{}",
            opened.final_name.display()
        );
        let document = folder.path().join("report.pdf");
        std::fs::write(&document, b"%PDF").expect("written");
        assert_eq!(open(&document).expect("opened").held, Held::File);
    }
}
