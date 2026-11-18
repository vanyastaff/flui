//! A pick's lasting access on macOS and iOS: a bookmark Foundation makes of
//! its URL, resolved after a restart into the URL again, whose
//! security-scoped resource is started for as long as the [`Access`] lives.
//!
//! macOS makes a security-scoped bookmark (`WithSecurityScope`), which an App
//! Sandbox resolves into a URL it may open once its access is started; the
//! process needs the `com.apple.security.files.bookmarks.app-scope`
//! entitlement to make one, and without it a pick carries no grant. iOS has
//! no security-scoped option: a bookmark made while the picker's access is
//! held resolves into a URL whose access the application starts again.
//!
//! Making and resolving a bookmark read the file system, so both run on a
//! work item (ADR-0360 §2). A bookmark Foundation calls stale (the file was
//! moved or renamed since) still resolves; a fresher one is made while the
//! access is held and handed over as the [`Access`]'s renewed grant.
//!
//! [`Access`]: crate::grant::Access

use super::{ErrorCode, errno, failure};
use crate::grant::GrantError;

/// `NSFileNoSuchFileError`.
const NO_SUCH_FILE: isize = 4;
/// `NSFileReadNoPermissionError`.
const NO_PERMISSION: isize = 257;
/// `NSFileReadNoSuchFileError`.
const READ_NO_SUCH_FILE: isize = 260;

/// What a bookmark that did not resolve answers, by its error's code: a file
/// Foundation could not find is `Missing`, one it may not reach `Revoked`,
/// any other failure the system's.
pub(crate) fn resolution_error(code: ErrorCode) -> GrantError {
    match code {
        ErrorCode::Cocoa(NO_SUCH_FILE | READ_NO_SUCH_FILE)
        | ErrorCode::Posix(errno::ENOENT | errno::ENOTDIR) => GrantError::Missing,
        ErrorCode::Cocoa(NO_PERMISSION) | ErrorCode::Posix(EPERM | EACCES) => GrantError::Revoked,
        other => GrantError::Failed(failure(other)),
    }
}

/// Operation not permitted, Darwin's.
const EPERM: i32 = 1;
/// Permission denied, Darwin's.
const EACCES: i32 = 13;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use foundation::{AppleGrants, bookmark};

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[expect(
    unsafe_code,
    reason = "a bookmark's resolution and the security-scoped access it starts"
)]
mod foundation {
    use std::rc::Rc;

    use flui_platform::capability::Resolver;
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{
        NSData, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
    };

    use super::resolution_error;
    use crate::apple::queue::WorkQueue;
    use crate::apple::{error_code, url_path};
    use crate::dialog::PickedFile;
    use crate::grant::{
        Access, AccessScope, Grant, GrantBackend, GrantCapability, GrantError, GrantKind,
        GrantResult,
    };

    /// How a bookmark is made: security-scoped on macOS, which an App Sandbox
    /// resolves into a URL it may open; minimal on iOS, which has no
    /// security-scoped option.
    #[cfg(target_os = "macos")]
    const CREATION: NSURLBookmarkCreationOptions = NSURLBookmarkCreationOptions::WithSecurityScope;
    #[cfg(target_os = "ios")]
    const CREATION: NSURLBookmarkCreationOptions = NSURLBookmarkCreationOptions::MinimalBookmark;

    /// How a bookmark is resolved: with its security scope on macOS, and
    /// never by asking the user (no volume mounted, no dialog shown).
    #[cfg(target_os = "macos")]
    const RESOLUTION: NSURLBookmarkResolutionOptions =
        NSURLBookmarkResolutionOptions::WithSecurityScope
            .union(NSURLBookmarkResolutionOptions::WithoutUI)
            .union(NSURLBookmarkResolutionOptions::WithoutMounting);
    #[cfg(target_os = "ios")]
    const RESOLUTION: NSURLBookmarkResolutionOptions = NSURLBookmarkResolutionOptions::WithoutUI
        .union(NSURLBookmarkResolutionOptions::WithoutMounting);

    /// The grant of the file at `url`: a bookmark of it, made while the
    /// process may reach it. `None` when Foundation makes none (a sandbox
    /// without the bookmark entitlement, a URL that names no file). Off the
    /// main thread only: it reads the file system.
    pub(crate) fn bookmark(url: &NSURL) -> Option<Grant> {
        let data = url
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                CREATION, None, None,
            )
            .ok()?;
        Grant::try_new(GrantKind::AppleBookmark, data.to_vec())
    }

    /// A security-scoped resource's access, started when it was made and
    /// stopped when it drops.
    struct SecurityScope(Retained<NSURL>);

    impl Drop for SecurityScope {
        fn drop(&mut self) {
            // SAFETY: balances the start that made this value.
            unsafe { self.0.stopAccessingSecurityScopedResource() };
        }
    }

    /// What the bookmark `payload` resolves to: the file's path while its
    /// access is held, a fresher bookmark when Foundation called it stale.
    /// Off the main thread only.
    fn resolve(payload: &[u8]) -> GrantResult {
        let data = NSData::with_bytes(payload);
        let mut stale = Bool::NO;
        // SAFETY: `stale` outlives the call, which writes it once.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                RESOLUTION,
                None,
                &raw mut stale,
            )
        }
        .map_err(|error| resolution_error(error_code(&error)))?;
        let malformed = GrantError::Failed(SystemFailure::MalformedAnswer);
        let file = url_path(&url)
            .and_then(PickedFile::try_new)
            .ok_or(malformed)?;
        // SAFETY: a method of a URL that takes nothing; a start that
        // succeeded is stopped once, when its scope drops.
        let started = unsafe { url.startAccessingSecurityScopedResource() };
        // Made while the access is held, which an App Sandbox requires.
        let renewed = if stale.as_bool() {
            bookmark(&url)
        } else {
            None
        };
        // A start that did not succeed leaves the path to the sandbox's
        // rule: the handle refuses one the process cannot open.
        let scope = started.then(|| AccessScope::new(SecurityScope(url)));
        Ok(Access::new(file, scope, renewed))
    }

    /// [`GrantBackend`] over Foundation's bookmarks, on the runtime's work
    /// queue.
    pub(crate) struct AppleGrants {
        queue: Rc<WorkQueue>,
    }

    impl AppleGrants {
        pub(crate) fn new(queue: Rc<WorkQueue>) -> Self {
            Self { queue }
        }
    }

    impl GrantBackend for AppleGrants {
        fn kind(&self) -> GrantKind {
            GrantKind::AppleBookmark
        }

        fn resolve(&self, grant: Grant, _max_bytes: u64, answer: Resolver<GrantResult>) {
            let busy = Err(GrantError::Unsupported(Unsupported::of::<GrantCapability>(
                UnsupportedReason::Unavailable,
            )));
            self.queue
                .run(answer, busy, move || resolve(grant.payload()));
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::resolution_error;
    use crate::apple::ErrorCode;
    use crate::grant::GrantError;

    #[test]
    fn a_bookmark_that_does_not_resolve_answers_why() {
        assert_eq!(resolution_error(ErrorCode::Cocoa(4)), GrantError::Missing);
        assert_eq!(resolution_error(ErrorCode::Cocoa(260)), GrantError::Missing);
        assert_eq!(resolution_error(ErrorCode::Posix(2)), GrantError::Missing);
        assert_eq!(resolution_error(ErrorCode::Cocoa(257)), GrantError::Revoked);
        assert_eq!(resolution_error(ErrorCode::Posix(1)), GrantError::Revoked);
        assert_eq!(resolution_error(ErrorCode::Posix(13)), GrantError::Revoked);
        assert_eq!(
            resolution_error(ErrorCode::Cocoa(259)),
            GrantError::Failed(SystemFailure::Framework(259)),
            "data that is no bookmark is the system's failure"
        );
    }
}
