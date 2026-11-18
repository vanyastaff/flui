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
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)?;
    let kind = file.metadata()?.file_type();
    let held = if kind.is_file() {
        Held::File
    } else if kind.is_dir() {
        Held::Folder
    } else {
        Held::Other
    };
    let final_name = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    Ok(Opened {
        file,
        held,
        final_name,
    })
}
