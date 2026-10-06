//! What an operating-system failure means to a caller of the file store.
//!
//! Windows reports a sharing violation (another process holds the file
//! without letting it be replaced) only by its code: `std` maps it to no
//! `ErrorKind`. `ERROR_ACCESS_DENIED` covers both a refusal that lasts (a
//! read-only target, an ACL) and one that passes (a file pending delete, a
//! scanner holding it), so a failed replacement with that code is settled by
//! probing the target.

use std::io::{self, ErrorKind};
use std::path::Path;

use flui_platform_api::StorageError;

/// `ERROR_ACCESS_DENIED`.
#[cfg(windows)]
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_SHARING_VIOLATION`.
#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;
/// `ERROR_LOCK_VIOLATION`.
#[cfg(windows)]
const ERROR_LOCK_VIOLATION: i32 = 33;

/// An operating-system failure, sorted by what the caller can do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    /// Another process holds the file; trying again later may succeed.
    Busy,
    /// No space is left.
    Full,
    /// Windows refused access: lasting or passing, which only a probe of the
    /// target tells. Elsewhere a refused access is lasting and
    /// [`Inaccessible`](Self::Inaccessible).
    #[cfg(windows)]
    Denied,
    /// Anything else: the value cannot be reached.
    Inaccessible(ErrorKind),
}

/// Sort `error` by what the caller can do about it.
pub(super) fn classify(error: &io::Error) -> Failure {
    #[cfg(windows)]
    match error.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION) => return Failure::Busy,
        Some(ERROR_ACCESS_DENIED) => return Failure::Denied,
        _ => {}
    }
    match error.kind() {
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => Failure::Full,
        ErrorKind::ResourceBusy => Failure::Busy,
        kind => Failure::Inaccessible(kind),
    }
}

/// The error for a failure anywhere but the final replacement: a refused
/// access there is not passing.
pub(super) fn storage_error(error: &io::Error) -> StorageError {
    match classify(error) {
        Failure::Busy => StorageError::Busy,
        Failure::Full => StorageError::Full,
        #[cfg(windows)]
        Failure::Denied => StorageError::Inaccessible {
            kind: ErrorKind::PermissionDenied,
        },
        Failure::Inaccessible(kind) => StorageError::Inaccessible { kind },
    }
}

/// The error for a failed replacement of `target`: a refused access is
/// settled by probing the target, never retried blindly.
pub(super) fn replace_error(error: &io::Error, target: &Path) -> StorageError {
    #[cfg(windows)]
    if classify(error) == Failure::Denied {
        return probe(target);
    }
    #[cfg(not(windows))]
    let _ = target;
    storage_error(error)
}

/// Why `target` refused to be replaced: a directory, a read-only file or a
/// file that cannot be opened for writing is inaccessible at once; a file
/// that can be opened was refused only for now.
#[cfg(windows)]
fn probe(target: &Path) -> StorageError {
    use std::fs::{self, OpenOptions};

    let metadata = match fs::metadata(target) {
        Ok(metadata) => metadata,
        // No target: the directory itself refuses new entries.
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return StorageError::Inaccessible {
                kind: ErrorKind::PermissionDenied,
            };
        }
        Err(error) => return storage_error(&error),
    };
    if metadata.is_dir() {
        return StorageError::Inaccessible {
            kind: ErrorKind::IsADirectory,
        };
    }
    if metadata.permissions().readonly() {
        return StorageError::Inaccessible {
            kind: ErrorKind::PermissionDenied,
        };
    }
    match OpenOptions::new().write(true).open(target) {
        Ok(_) => StorageError::Busy,
        Err(error) => storage_error(&error),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, ErrorKind};

    use super::{Failure, classify};

    /// Each row is an OS failure and the class the store gives it.
    #[test]
    fn os_error_classification_table() {
        let mut rows: Vec<(&str, io::Error, Failure)> = vec![
            (
                "full disk by kind",
                io::Error::from(ErrorKind::StorageFull),
                Failure::Full,
            ),
            (
                "quota by kind",
                io::Error::from(ErrorKind::QuotaExceeded),
                Failure::Full,
            ),
            (
                "busy resource by kind",
                io::Error::from(ErrorKind::ResourceBusy),
                Failure::Busy,
            ),
            (
                "missing file",
                io::Error::from(ErrorKind::NotFound),
                Failure::Inaccessible(ErrorKind::NotFound),
            ),
        ];
        #[cfg(windows)]
        rows.extend([
            (
                "ERROR_SHARING_VIOLATION",
                io::Error::from_raw_os_error(32),
                Failure::Busy,
            ),
            (
                "ERROR_LOCK_VIOLATION",
                io::Error::from_raw_os_error(33),
                Failure::Busy,
            ),
            (
                "ERROR_ACCESS_DENIED",
                io::Error::from_raw_os_error(5),
                Failure::Denied,
            ),
            (
                "ERROR_DISK_FULL",
                io::Error::from_raw_os_error(112),
                Failure::Full,
            ),
            (
                "ERROR_HANDLE_DISK_FULL",
                io::Error::from_raw_os_error(39),
                Failure::Full,
            ),
            (
                "ERROR_PATH_NOT_FOUND",
                io::Error::from_raw_os_error(3),
                Failure::Inaccessible(ErrorKind::NotFound),
            ),
        ]);
        #[cfg(unix)]
        rows.extend([
            ("ENOSPC", io::Error::from_raw_os_error(28), Failure::Full),
            (
                "EACCES",
                io::Error::from_raw_os_error(13),
                Failure::Inaccessible(ErrorKind::PermissionDenied),
            ),
            (
                "EISDIR",
                io::Error::from_raw_os_error(21),
                Failure::Inaccessible(ErrorKind::IsADirectory),
            ),
            ("EBUSY", io::Error::from_raw_os_error(16), Failure::Busy),
        ]);
        let wrong: Vec<String> = rows
            .iter()
            .filter_map(|(name, error, expected)| {
                let got = classify(error);
                (got != *expected).then(|| format!("{name}: {got:?}, expected {expected:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "misclassified: {wrong:#?}");
    }
}
