//! The launcher over Launch Services and the Finder: `NSWorkspace`'s
//! `openURL:` for a URI and a document, `activateFileViewerSelectingURLs:`
//! to reveal, and `NSFileManager`'s `trashItemAtURL:` to trash. No command
//! line: nothing reaches `open(1)` or a shell.
//!
//! A URI goes to Launch Services as written, which hands it to the
//! application the user chose for its scheme; the seam admitted only the
//! schemes its caller named. An open first opens the file and judges what
//! the descriptor holds (ADR-0360 §2): its final path (`F_GETPATH`, which
//! names the file every link the open followed led to) must pass the floor
//! (`runs_when_opened`), must not be a Unix executable, and must not be an
//! application bundle, as Launch Services' `NSURLIsApplicationKey` says;
//! only then is that final path handed on. Every request runs as a work
//! item off the main thread (`queue::WorkQueue`), since each touches the file
//! system or asks Launch Services, which may wait on a volume.

use std::path::Path;

use flui_platform::launch::{LaunchError, runs_when_opened};

use super::{ErrorCode, errno, failure};

/// What the file an open opened is, as its descriptor and Launch Services
/// say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenedKind {
    /// A regular file; `executable` when any of its execute bits is set,
    /// which makes Launch Services run it in Terminal.
    Regular { executable: bool },
    /// A folder; `application` when Launch Services calls it an
    /// application bundle, which opening launches.
    Folder { application: bool },
    /// Anything else (a device, a socket): nothing a document is.
    Other,
}

/// Whether the file an open opened, at `final_path`, may be handed on: a
/// document or a plain folder. A name that runs when opened, an executable
/// and an application bundle are not documents.
pub(super) fn judge(final_path: &Path, kind: OpenedKind) -> Result<(), LaunchError> {
    let runs = runs_when_opened(final_path)
        || matches!(
            kind,
            OpenedKind::Regular { executable: true }
                | OpenedKind::Folder { application: true }
                | OpenedKind::Other
        );
    if runs {
        Err(LaunchError::NotADocument)
    } else {
        Ok(())
    }
}

/// The error a file system call on the request's path answered.
pub(super) fn path_error(code: ErrorCode) -> LaunchError {
    match code {
        ErrorCode::Posix(errno::ENOENT | errno::ENOTDIR)
        // NSFileNoSuchFileError, NSFileReadNoSuchFileError.
        | ErrorCode::Cocoa(4 | 260) => LaunchError::NotFound,
        ErrorCode::Posix(errno::EACCES | errno::EPERM)
        // NSFileReadNoPermissionError, NSFileWriteNoPermissionError.
        | ErrorCode::Cocoa(257 | 513) => LaunchError::Denied,
        code => LaunchError::Failed(failure(code)),
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacLauncher;

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "the opened file's final path, Foundation's resource keys"
)]
mod appkit {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use flui_platform::capability::Resolver;
    use flui_platform::launch::{
        ExternalUri, LaunchError, LaunchResult, LauncherBackend, LauncherCapability, is_local_path,
    };
    use flui_platform::{Unsupported, UnsupportedReason};
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSFileManager, NSNumber, NSString, NSURL};

    use super::super::ErrorCode;
    use super::super::appkit::{error_code, file_url};
    use super::super::queue::WorkQueue;
    use super::{OpenedKind, judge, path_error};

    /// What a request answers when the runtime has no place for it, or when
    /// Launch Services declined a handler it has.
    fn not_now() -> LaunchError {
        LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
            UnsupportedReason::Unavailable,
        ))
    }

    /// [`LauncherBackend`] over Launch Services and the Finder.
    pub(crate) struct MacLauncher {
        queue: Rc<WorkQueue>,
    }

    impl MacLauncher {
        pub(crate) fn new(queue: Rc<WorkQueue>) -> Self {
            Self { queue }
        }
    }

    /// Hand `url` to the application Launch Services picks for it.
    fn hand_on(url: &NSURL) -> LaunchResult {
        let workspace = NSWorkspace::sharedWorkspace();
        if workspace.URLForApplicationToOpenURL(url).is_none() {
            return Err(LaunchError::NoHandler);
        }
        if workspace.openURL(url) {
            Ok(())
        } else {
            Err(not_now())
        }
    }

    /// The path the file `file` holds is at now, every link resolved.
    fn final_path(file: &std::fs::File) -> Result<PathBuf, LaunchError> {
        let mut buffer = vec![0_u8; usize::try_from(libc::PATH_MAX).unwrap_or(1024)];
        // SAFETY: the descriptor is open for the call, and `F_GETPATH`
        // writes at most `PATH_MAX` bytes, a NUL included, into a buffer of
        // that size that outlives the call.
        let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
        if status == -1 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(path_error(ErrorCode::Posix(errno)));
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        buffer.truncate(length);
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buffer)))
    }

    /// Whether Launch Services calls the folder at `url` an application.
    fn is_application(url: &NSURL) -> bool {
        // SAFETY: Foundation's resource key constants live for the process.
        let key = unsafe { objc2_foundation::NSURLIsApplicationKey };
        let Ok(values) = url.resourceValuesForKeys_error(&NSArray::from_slice(&[key])) else {
            // A folder whose kind cannot be read is not handed on.
            return true;
        };
        values
            .objectForKey(key)
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .is_some_and(|value| value.boolValue())
    }

    /// Open the document at `path`: open it, judge what was opened, and
    /// hand on its final path while the file is still open.
    fn open_document(path: &Path) -> LaunchResult {
        let io_error = |error: &std::io::Error| {
            path_error(ErrorCode::Posix(error.raw_os_error().unwrap_or(0)))
        };
        let file = std::fs::File::open(path).map_err(|error| io_error(&error))?;
        let metadata = file.metadata().map_err(|error| io_error(&error))?;
        let final_path = final_path(&file)?;
        if !is_local_path(&final_path) {
            return Err(LaunchError::NotLocal);
        }
        let url = file_url(&final_path).ok_or(LaunchError::NotFound)?;
        let kind = if metadata.is_file() {
            OpenedKind::Regular {
                executable: metadata.permissions().mode() & 0o111 != 0,
            }
        } else if metadata.is_dir() {
            OpenedKind::Folder {
                application: is_application(&url),
            }
        } else {
            OpenedKind::Other
        };
        judge(&final_path, kind)?;
        let handed = hand_on(&url);
        drop(file);
        handed
    }

    /// Show `path` selected in the Finder, once it exists.
    fn reveal(path: &Path) -> LaunchResult {
        std::fs::symlink_metadata(path)
            .map_err(|error| path_error(ErrorCode::Posix(error.raw_os_error().unwrap_or(0))))?;
        let url = file_url(path).ok_or(LaunchError::NotFound)?;
        NSWorkspace::sharedWorkspace()
            .activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*url]));
        Ok(())
    }

    /// Move `path` to the trash of its volume.
    fn trash(path: &Path) -> LaunchResult {
        let url = file_url(path).ok_or(LaunchError::NotFound)?;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| match error_code(&error) {
                // NSFeatureUnsupportedError: the volume has no trash.
                ErrorCode::Cocoa(3328) => {
                    LaunchError::Unsupported(Unsupported::of::<LauncherCapability>(
                        UnsupportedReason::NotOnThisDevice,
                    ))
                }
                code => path_error(code),
            })
    }

    impl LauncherBackend for MacLauncher {
        fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
            self.queue.run(answer, Err(not_now()), move || {
                let url = NSURL::URLWithString(&NSString::from_str(uri.as_str()))
                    .ok_or(LaunchError::NoHandler)?;
                hand_on(&url)
            });
        }

        fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
            self.queue
                .run(answer, Err(not_now()), move || open_document(&path));
        }

        fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
            self.queue
                .run(answer, Err(not_now()), move || reveal(&path));
        }

        fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
            self.queue.run(answer, Err(not_now()), move || trash(&path));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use flui_platform::SystemFailure;
    use flui_platform::launch::LaunchError;

    use super::super::{ErrorCode, errno};
    use super::{OpenedKind, judge, path_error};

    #[test]
    fn only_a_document_or_a_plain_folder_is_handed_on() {
        let cases: [(&str, OpenedKind, Result<(), LaunchError>); 7] = [
            (
                "/Users/a/report.pdf",
                OpenedKind::Regular { executable: false },
                Ok(()),
            ),
            (
                "/Users/a/Projects",
                OpenedKind::Folder { application: false },
                Ok(()),
            ),
            // A link named like a document that led to a script: judged by
            // the final path.
            (
                "/Users/a/run.command",
                OpenedKind::Regular { executable: false },
                Err(LaunchError::NotADocument),
            ),
            (
                "/Users/a/tool",
                OpenedKind::Regular { executable: true },
                Err(LaunchError::NotADocument),
            ),
            (
                "/Applications/Mail.app",
                OpenedKind::Folder { application: true },
                Err(LaunchError::NotADocument),
            ),
            // A bundle Launch Services calls an application under a name
            // the floor does not list.
            (
                "/Users/a/Thing.bundle",
                OpenedKind::Folder { application: true },
                Err(LaunchError::NotADocument),
            ),
            (
                "/dev/null",
                OpenedKind::Other,
                Err(LaunchError::NotADocument),
            ),
        ];
        for (path, kind, expected) in cases {
            assert_eq!(judge(Path::new(path), kind), expected, "{path} {kind:?}");
        }
    }

    #[test]
    fn a_file_system_error_is_named_where_the_contract_names_it() {
        let cases = [
            (ErrorCode::Posix(errno::ENOENT), LaunchError::NotFound),
            (ErrorCode::Posix(errno::ENOTDIR), LaunchError::NotFound),
            (ErrorCode::Cocoa(4), LaunchError::NotFound),
            (ErrorCode::Posix(errno::EACCES), LaunchError::Denied),
            (ErrorCode::Posix(errno::EPERM), LaunchError::Denied),
            (ErrorCode::Cocoa(513), LaunchError::Denied),
            (
                ErrorCode::Posix(5),
                LaunchError::Failed(SystemFailure::Os(5)),
            ),
            (
                ErrorCode::Cocoa(640),
                LaunchError::Failed(SystemFailure::Framework(640)),
            ),
        ];
        for (code, expected) in cases {
            assert_eq!(path_error(code), expected, "{code:?}");
        }
    }
}
