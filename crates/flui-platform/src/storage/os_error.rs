//! What an operating-system failure means to a caller of the file store.
//!
//! Windows reports a sharing violation (another process holds the file
//! without letting it be replaced) only by its code: `std` maps it to no
//! `ErrorKind`. `ERROR_ACCESS_DENIED` covers both a refusal that lasts (a
//! read-only target, an ACL) and one that passes (a target pending delete),
//! so a failed replacement with that code is settled by probing the target:
//! a target that refuses even an open requesting no access is pending delete
//! and the write may be tried again; a read-only file, a directory, or a file
//! that refuses to be opened for writing is refused for good.

use std::io::{self, ErrorKind};
use std::path::Path;

use flui_platform_api::StorageError;

/// `ERROR_INVALID_FUNCTION`: a file system without byte-range locks.
#[cfg(windows)]
const ERROR_INVALID_FUNCTION: i32 = 1;
/// `ERROR_ACCESS_DENIED`.
#[cfg(windows)]
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_SHARING_VIOLATION`.
#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;
/// `ERROR_LOCK_VIOLATION`.
#[cfg(windows)]
const ERROR_LOCK_VIOLATION: i32 = 33;
/// `ERROR_NOT_SUPPORTED`: a redirector or file system that refuses locks.
#[cfg(windows)]
const ERROR_NOT_SUPPORTED: i32 = 50;

/// The error for an operating-system failure. A refused access is lasting
/// here; only [`replace_error`] probes whether it passes.
pub(super) fn storage_error(error: &io::Error) -> StorageError {
    #[cfg(windows)]
    if matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
    ) {
        return StorageError::Busy;
    }
    match error.kind() {
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => StorageError::Full,
        ErrorKind::ResourceBusy => StorageError::Busy,
        kind => StorageError::Inaccessible { kind },
    }
}

/// The error for a failed attempt to lock the storage root. `std` maps only
/// some "no locks here" codes to [`ErrorKind::Unsupported`]; the others are
/// recognised by code.
pub(super) fn lock_error(error: &io::Error) -> StorageError {
    #[cfg(windows)]
    let unsupported_code = matches!(
        error.raw_os_error(),
        Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
    );
    #[cfg(unix)]
    let unsupported_code = error.raw_os_error() == Some(ENOLCK);
    #[cfg(not(any(windows, unix)))]
    let unsupported_code = false;
    if unsupported_code || error.kind() == ErrorKind::Unsupported {
        StorageError::LockUnsupported
    } else {
        storage_error(error)
    }
}

/// `ENOLCK`: no locks are available (an NFS mount without a lock daemon).
#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
const ENOLCK: i32 = 37;
/// `ENOLCK`: no locks are available (an NFS mount without a lock daemon).
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const ENOLCK: i32 = 77;

/// The error for a failed replacement of `target`: a refused access is
/// settled by probing the target, never retried blindly.
pub(super) fn replace_error(error: &io::Error, target: &Path) -> StorageError {
    #[cfg(windows)]
    if is_access_denied(error) {
        return probe(target);
    }
    #[cfg(not(windows))]
    let _ = target;
    storage_error(error)
}

/// Whether Windows refused access, lastingly or not.
#[cfg(windows)]
fn is_access_denied(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_ACCESS_DENIED)
}

/// Why `target` refused to be replaced.
///
/// A directory or a read-only file is refused for good. Then the target is
/// opened with no access requested, which an ACL does not refuse (reading
/// attributes is granted through the directory): a refusal there means the
/// file is pending delete, a passing state, so [`StorageError::Busy`]. A
/// file that then refuses to be opened for writing is refused for good; one
/// that opens was refused only for now. `fs::metadata` cannot tell the
/// pending delete apart: on that refusal `std` reads the attributes from the
/// directory listing instead.
#[cfg(windows)]
fn probe(target: &Path) -> StorageError {
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;

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
    match OpenOptions::new().access_mode(0).open(target) {
        Err(error) if is_access_denied(&error) => return StorageError::Busy,
        Err(error) => return storage_error(&error),
        Ok(_) => {}
    }
    match OpenOptions::new().write(true).open(target) {
        Ok(_) => StorageError::Busy,
        Err(error) => storage_error(&error),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, ErrorKind};

    use flui_platform_api::StorageError;

    use super::{lock_error, storage_error};

    fn inaccessible(kind: ErrorKind) -> StorageError {
        StorageError::Inaccessible { kind }
    }

    /// Each row is an OS failure, the step it happened in, and the error the
    /// store reports for it.
    #[test]
    fn os_error_classification_table() {
        type Map = fn(&io::Error) -> StorageError;
        let storage: Map = storage_error;
        let lock: Map = lock_error;
        let mut rows: Vec<(&str, Map, io::Error, StorageError)> = vec![
            (
                "full disk",
                storage,
                ErrorKind::StorageFull.into(),
                StorageError::Full,
            ),
            (
                "quota",
                storage,
                ErrorKind::QuotaExceeded.into(),
                StorageError::Full,
            ),
            (
                "busy resource",
                storage,
                ErrorKind::ResourceBusy.into(),
                StorageError::Busy,
            ),
            (
                "missing file",
                storage,
                ErrorKind::NotFound.into(),
                inaccessible(ErrorKind::NotFound),
            ),
            (
                "lock: unsupported kind",
                lock,
                ErrorKind::Unsupported.into(),
                StorageError::LockUnsupported,
            ),
            (
                "lock: missing root",
                lock,
                ErrorKind::NotFound.into(),
                inaccessible(ErrorKind::NotFound),
            ),
        ];
        #[cfg(windows)]
        rows.extend([
            (
                "ERROR_SHARING_VIOLATION",
                storage,
                io::Error::from_raw_os_error(32),
                StorageError::Busy,
            ),
            (
                "ERROR_LOCK_VIOLATION",
                storage,
                io::Error::from_raw_os_error(33),
                StorageError::Busy,
            ),
            (
                "ERROR_ACCESS_DENIED outside a replacement",
                storage,
                io::Error::from_raw_os_error(5),
                inaccessible(ErrorKind::PermissionDenied),
            ),
            (
                "ERROR_DISK_FULL",
                storage,
                io::Error::from_raw_os_error(112),
                StorageError::Full,
            ),
            (
                "ERROR_HANDLE_DISK_FULL",
                storage,
                io::Error::from_raw_os_error(39),
                StorageError::Full,
            ),
            (
                "ERROR_PATH_NOT_FOUND",
                storage,
                io::Error::from_raw_os_error(3),
                inaccessible(ErrorKind::NotFound),
            ),
            (
                "lock: ERROR_NOT_SUPPORTED",
                lock,
                io::Error::from_raw_os_error(50),
                StorageError::LockUnsupported,
            ),
            (
                "lock: ERROR_INVALID_FUNCTION",
                lock,
                io::Error::from_raw_os_error(1),
                StorageError::LockUnsupported,
            ),
            (
                "lock: ERROR_CALL_NOT_IMPLEMENTED",
                lock,
                io::Error::from_raw_os_error(120),
                StorageError::LockUnsupported,
            ),
            (
                "lock: ERROR_LOCK_VIOLATION",
                lock,
                io::Error::from_raw_os_error(33),
                StorageError::Busy,
            ),
        ]);
        #[cfg(unix)]
        rows.extend([
            (
                "ENOSPC",
                storage,
                io::Error::from_raw_os_error(28),
                StorageError::Full,
            ),
            (
                "EACCES",
                storage,
                io::Error::from_raw_os_error(13),
                inaccessible(ErrorKind::PermissionDenied),
            ),
            (
                "EISDIR",
                storage,
                io::Error::from_raw_os_error(21),
                inaccessible(ErrorKind::IsADirectory),
            ),
            (
                "EBUSY",
                storage,
                io::Error::from_raw_os_error(16),
                StorageError::Busy,
            ),
            (
                "lock: ENOLCK",
                lock,
                io::Error::from_raw_os_error(super::ENOLCK),
                StorageError::LockUnsupported,
            ),
        ]);
        let wrong: Vec<String> = rows
            .iter()
            .filter_map(|(name, map, error, expected)| {
                let got = map(error);
                (got != *expected).then(|| format!("{name}: {got:?}, expected {expected:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "misclassified: {wrong:#?}");
    }
}
