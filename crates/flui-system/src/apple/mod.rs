//! What the macOS and iOS providers share. Foundation, Grand Central
//! Dispatch and the uniform type identifiers are the same on both systems,
//! so the part of a provider that touches only them lives here once:
//!
//! - [`version`]: the system's version and the gates above its floor, one
//!   version type per system;
//! - [`queue`]: the requests that leave the main thread, bounded per UI
//!   runtime;
//! - [`modal`]: the caller's answer while a sheet or a presented controller
//!   is up;
//! - [`content_types`]: what a dialog's filters ask the system to allow,
//!   and the contract's rule a pick is held to;
//! - [`share`]: what a share is handed, its files checked first;
//! - [`user_dirs`]: the user's folders as Foundation names them;
//! - [`os`]: what the system is;
//! - an `NSError`'s code ([`ErrorCode`]) and the [`SystemFailure`] it is,
//!   and a path as a file URL and back, in the file system's bytes.
//!
//! The rules are functions over plain values, compiled and tested on every
//! host; the Foundation calls are each file's `foundation` module, built
//! for macOS and iOS.

pub(crate) mod bookmark;
pub(crate) mod content_types;
mod floor;
pub(crate) mod modal;
pub(crate) mod os;
pub(crate) mod queue;
pub(crate) mod share;
pub(crate) mod user_dirs;
pub(crate) mod version;

use flui_platform::SystemFailure;

/// An `NSError`'s code, by what it is a code of: what [`failure`] and each
/// provider's own mapping read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorCode {
    /// A POSIX `errno`: the error's own, in `NSPOSIXErrorDomain`, or the one
    /// a Cocoa error carries as its underlying error.
    Posix(i32),
    /// A code of `NSCocoaErrorDomain` with no POSIX error under it.
    Cocoa(isize),
    /// A code of any other domain (ServiceManagement's, `NSOSStatus`).
    Other(isize),
}

/// The [`SystemFailure`] an error no variant of the service's names is.
pub(crate) fn failure(code: ErrorCode) -> SystemFailure {
    match code {
        ErrorCode::Posix(errno) => SystemFailure::Os(errno),
        ErrorCode::Cocoa(code) | ErrorCode::Other(code) => {
            SystemFailure::Framework(i64::try_from(code).unwrap_or(i64::MAX))
        }
    }
}

/// The code of a failed file system call, as every provider maps one: its
/// `errno`, or zero for an error the system gave no code for.
pub(crate) fn io_code(error: &std::io::Error) -> ErrorCode {
    ErrorCode::Posix(error.raw_os_error().unwrap_or(0))
}

/// The `errno` values the providers name, Darwin's.
pub(crate) mod errno {
    /// Operation not permitted.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) const EPERM: i32 = 1;
    /// No such file or directory.
    pub(crate) const ENOENT: i32 = 2;
    /// Permission denied.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) const EACCES: i32 = 13;
    /// The file exists.
    #[cfg(any(target_os = "ios", all(test, not(target_arch = "wasm32"))))]
    pub(crate) const EEXIST: i32 = 17;
    /// A component of the path is not a directory.
    pub(crate) const ENOTDIR: i32 = 20;
}

#[cfg(target_os = "macos")]
pub(crate) use foundation::PATH_MAX;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use foundation::{error_code, file_url, main_thread, url_path};

/// The Foundation calls both systems' providers make.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[expect(
    unsafe_code,
    reason = "a path as the file system's bytes, Foundation's error constants"
)]
mod foundation {
    use std::path::Path;

    use flui_platform::capability::Capability;
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_foundation::{NSDictionary, NSError, NSString, NSURL};

    use super::ErrorCode;

    /// The main thread, for a service AppKit or UIKit runs there; `C`
    /// refused with `NotYetOnThisHost` on a runtime whose owner thread is
    /// another one (the headless host), before the framework is touched.
    pub(crate) fn main_thread<C: Capability>() -> Result<MainThreadMarker, Unsupported> {
        MainThreadMarker::new()
            .ok_or_else(|| Unsupported::of::<C>(UnsupportedReason::NotYetOnThisHost))
    }

    /// `path` as a file URL, from its bytes as the file system has them, so
    /// a name that is not UTF-8 (a FAT, exFAT, SMB or NFS volume may hold
    /// one) names the same file; `None` for a path holding a NUL, which no
    /// file has. `directory` says whether it names a folder.
    pub(crate) fn file_url(path: &Path, directory: bool) -> Option<Retained<NSURL>> {
        use std::os::unix::ffi::OsStrExt as _;

        let bytes = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let pointer = std::ptr::NonNull::new(bytes.as_ptr().cast_mut())?;
        // SAFETY: a NUL-terminated byte string that outlives the call, which
        // copies it into the URL.
        Some(unsafe {
            NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                pointer, directory, None,
            )
        })
    }

    /// The longest path Darwin's calls take or answer, in bytes, its NUL
    /// included (`PATH_MAX`).
    pub(crate) const PATH_MAX: usize = 1024;
    const _: () = assert!(libc::PATH_MAX == 1024);

    /// The path a file URL names, as the file system's bytes; `None` for a
    /// URL that names no file. `getFileSystemRepresentation:` answers `NO`
    /// where `fileSystemRepresentation` would raise an exception.
    pub(crate) fn url_path(url: &NSURL) -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStrExt as _;

        if !url.isFileURL() {
            return None;
        }
        // On the stack: a panel's delegate asks this of every item shown.
        let mut buffer = [0_u8; PATH_MAX];
        let pointer = std::ptr::NonNull::from(&mut buffer).cast::<std::ffi::c_char>();
        // SAFETY: the buffer outlives the call and holds the length given,
        // which the method writes at most, a NUL included.
        let written = unsafe { url.getFileSystemRepresentation_maxLength(pointer, buffer.len()) };
        if !written {
            return None;
        }
        let path = std::ffi::CStr::from_bytes_until_nul(&buffer).ok()?;
        Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            path.to_bytes(),
        )))
    }

    /// What `error`'s code is a code of: its POSIX code where it carries
    /// one, else its own code by its domain.
    pub(crate) fn error_code(error: &NSError) -> ErrorCode {
        // SAFETY: Foundation's error domain and key constants are immutable
        // strings that live for the process.
        let (posix, cocoa, underlying_key) = unsafe {
            (
                objc2_foundation::NSPOSIXErrorDomain,
                objc2_foundation::NSCocoaErrorDomain,
                objc2_foundation::NSUnderlyingErrorKey,
            )
        };
        let code = error.code();
        let domain = error.domain();
        if &*domain == posix {
            return ErrorCode::Posix(i32::try_from(code).unwrap_or(i32::MAX));
        }
        let info: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = error.userInfo();
        let underlying = info
            .objectForKey(underlying_key)
            .and_then(|object| object.downcast::<NSError>().ok());
        match underlying {
            Some(underlying) if &*underlying.domain() == posix => {
                ErrorCode::Posix(i32::try_from(underlying.code()).unwrap_or(i32::MAX))
            }
            _ if &*domain == cocoa => ErrorCode::Cocoa(code),
            _ => ErrorCode::Other(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::{ErrorCode, failure};

    #[test]
    fn an_errors_posix_code_is_the_oss_and_any_other_the_frameworks() {
        assert_eq!(failure(ErrorCode::Posix(28)), SystemFailure::Os(28));
        assert_eq!(
            failure(ErrorCode::Cocoa(3328)),
            SystemFailure::Framework(3328)
        );
        assert_eq!(
            failure(ErrorCode::Other(-9878)),
            SystemFailure::Framework(-9878)
        );
    }
}
