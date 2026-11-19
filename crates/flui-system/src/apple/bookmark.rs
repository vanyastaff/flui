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
use crate::grant::{GrantError, ResolveHold};

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

/// Whether anybody still waits for a resolve: cleared when its hold drops,
/// read on the work item before a security scope is started, so a resolve
/// let go of starts no scope for nobody.
#[derive(Clone, Debug)]
pub(crate) struct Waited(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Waited {
    /// A resolve's wiring: hand `work` to `run`, which runs it on a work
    /// item, telling it whether anybody still waits; the hold returned is
    /// what tells it, so the resolve let go of starts no scope.
    pub(crate) fn hold<T: 'static>(
        run: impl FnOnce(Box<dyn FnOnce() -> T + Send>),
        work: impl FnOnce(&Self) -> T + Send + 'static,
    ) -> ResolveHold {
        let waited = Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            true,
        )));
        let hold = WaitedHold(waited.clone());
        run(Box::new(move || work(&waited)));
        ResolveHold::new(hold)
    }

    /// Start a scope with `start` when someone still waits, answering
    /// whether it started; nothing is started for a resolve let go of.
    pub(crate) fn start_scope(&self, start: impl FnOnce() -> bool) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire) && start()
    }
}

/// A resolve's hold: its drop tells the work item nobody waits any more.
#[derive(Debug)]
struct WaitedHold(Waited);

impl Drop for WaitedHold {
    fn drop(&mut self) {
        self.0.0.store(false, std::sync::atomic::Ordering::Release);
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

    use super::{Waited, resolution_error};
    use crate::apple::queue::WorkQueue;
    use crate::apple::{error_code, url_path};
    use crate::dialog::PickedFile;
    use crate::grant::{
        Access, AccessScope, Grant, GrantBackend, GrantCapability, GrantError, GrantKind,
        GrantResult, ResolveHold,
    };
    use crate::sandbox::Sandbox;

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
    /// No scope is started once nobody waits (`waited`). Off the main thread
    /// only.
    fn resolve(payload: &[u8], waited: &Waited) -> GrantResult {
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
        let started = waited.start_scope(|| {
            // SAFETY: a method of a URL that takes nothing; a start that
            // succeeded is stopped once, when its scope drops.
            unsafe { url.startAccessingSecurityScopedResource() }
        });
        // Made while the access is held, which an App Sandbox requires.
        let renewed = if stale.as_bool() {
            bookmark(&url)
        } else {
            None
        };
        // A start that did not succeed leaves the path to the sandbox's
        // rule: the handle refuses one the process cannot open.
        Ok(if started {
            Access::scoped(file, AccessScope::new(SecurityScope(url)), renewed)
        } else {
            Access::new(file, renewed)
        })
    }

    /// [`GrantBackend`] over Foundation's bookmarks, on the runtime's work
    /// queue, in the sandbox the process's environment describes.
    pub(crate) struct AppleGrants {
        queue: Rc<WorkQueue>,
        sandbox: Sandbox,
    }

    impl AppleGrants {
        pub(crate) fn new(queue: Rc<WorkQueue>, sandbox: Sandbox) -> Self {
            Self { queue, sandbox }
        }
    }

    impl GrantBackend for AppleGrants {
        fn takes(&self, grant: &Grant) -> bool {
            grant.made_by(GrantKind::AppleBookmark)
        }

        /// Read from the environment when the provider was made: no file
        /// system call.
        fn sandbox(&self, answer: Resolver<Sandbox>) {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(self.sandbox.clone());
        }

        fn resolve(
            &self,
            grant: Grant,
            _max_bytes: u64,
            answer: Resolver<GrantResult>,
        ) -> ResolveHold {
            let busy = Err(GrantError::Unsupported(Unsupported::of::<GrantCapability>(
                UnsupportedReason::Unavailable,
            )));
            // A resolution under way finishes; dropped before its scope, the
            // hold keeps the scope from being started for nobody.
            Waited::hold(
                |work| self.queue.run(answer, busy, work),
                move |waited| resolve(grant.payload(), waited),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::SystemFailure;

    use super::{Waited, resolution_error};
    use crate::apple::ErrorCode;
    use crate::grant::GrantError;

    /// What a resolve wired as `AppleGrants::resolve` wires it starts, run
    /// on its work item after its hold is kept or let go of.
    fn started_after(let_go: bool) -> (bool, bool) {
        let queued = std::cell::RefCell::new(None);
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let asking = std::sync::Arc::clone(&asked);
        let hold = Waited::hold(
            |work| *queued.borrow_mut() = Some(work),
            move |waited: &Waited| {
                waited.start_scope(|| {
                    asking.store(true, std::sync::atomic::Ordering::Relaxed);
                    true
                })
            },
        );
        if let_go {
            drop(hold);
        }
        let work = queued.into_inner().expect("the resolve was queued");
        let started = work();
        (started, asked.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[test]
    fn a_resolve_let_go_of_starts_no_scope() {
        assert_eq!(
            started_after(false),
            (true, true),
            "someone waits: the scope starts"
        );
        assert_eq!(
            started_after(true),
            (false, false),
            "nobody waits: no scope, and the system is not asked to start one"
        );
    }

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
