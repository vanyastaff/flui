//! Access to a picked file that outlasts the process.
//!
//! An open dialog asked to [keep access](crate::dialog::OpenRequest::keep_access)
//! hands each pick over with a [`Grant`] ([`PickedFile::grant`]): bytes the
//! application stores and, after a restart, hands to [`Grants::resolve`],
//! acquired through `ctx.capability::<GrantCapability>()`. The answer is an
//! [`Access`]: the file again, reachable while the access is held, or why it
//! is not ([`GrantError`]).
//!
//! The layer never hands out a path the process cannot open. A reacquired
//! path is one its [`Sandbox`] reaches, or one a scope this crate's own
//! providers started holds open (a security-scoped resource on macOS and
//! iOS, started when the grant was resolved and stopped when the [`Access`]
//! drops); the handle refuses any other answer, whichever backend gave it.

#[cfg(any(windows, target_os = "linux", all(test, any(unix, windows))))]
pub(crate) mod path;
mod resolving;
mod token;

use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

pub use resolving::{ResolveHold, Resolving};
pub use token::{Grant, GrantKind, MalformedGrant};

use crate::dialog::{PickedFile, PickedForm};
use crate::sandbox::Sandbox;
use resolving::{ResolveState, SandboxSlot, TooManyWaiting};

/// The grant capability: `ctx.capability::<GrantCapability>()` hands out
/// [`Grants`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum GrantCapability {}

impl Capability for GrantCapability {
    type Handle = Grants;
    const NAME: &'static str = "grants";
}

/// Why a grant reacquired nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GrantError {
    /// The grant was made by another kind of system (a path a Windows
    /// machine kept, resolved on a Mac through synced settings): only its
    /// own kind resolves it. Nothing was asked.
    #[error("the grant was made by another kind of system")]
    Foreign,
    /// The system no longer grants the access: the user or the system
    /// withdrew it (Android pruned the oldest persisted permission, the
    /// document portal forgot the document), or the system resolved the
    /// grant to a file the process may not open. Pick the file again.
    #[error("the system no longer grants access to the file")]
    Revoked,
    /// The file is not where the grant reached it: it was deleted, or moved
    /// where the system does not follow it (a bookmark follows a move
    /// within its volume, a path follows none).
    #[error("the file the grant reached is not there")]
    Missing,
    /// The grant names a path off the local drives (a share, a pipe, a
    /// device), which the layer does not reach (ADR-0360 §2). Nothing was
    /// asked of the file system.
    #[error("the grant names a path off the local drives")]
    NotLocal,
    /// The file holds more than the request reads, on a system that hands
    /// a reacquired file over as its contents (Android).
    #[error("the file holds {actual} bytes, more than the {limit} the request reads")]
    TooLarge {
        /// The file's size, as the system declared it or as far as it was
        /// read.
        actual: u64,
        /// The `max_bytes` [`Grants::resolve`] was asked with.
        limit: u64,
    },
    /// The system resolves no grant here, as the [`Unsupported`] says.
    #[error("grants are not resolved here: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name; the
    /// [`SystemFailure`] says where the failure came from.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a resolve answers: the file again, or why it is not reached.
pub type GrantResult = Result<Access, GrantError>;

/// What keeps a reacquired file open to the process while it is held: a
/// security-scoped resource's access on macOS and iOS, which its drop
/// stops.
///
/// Only this crate's providers make one, so a scope that lets a path past
/// the sandbox's rule is always a system's started access. `Send`, since a
/// provider starts the access where it resolves the grant, off the owner
/// thread (ADR-0360 §2), and hands it to the owner with the answer.
#[must_use = "dropping the scope ends the access it holds"]
pub struct AccessScope(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any + Send>);

impl AccessScope {
    /// A scope whose drop drops `keep`: the value whose `Drop` ends the
    /// system's access.
    #[cfg(any(target_os = "macos", target_os = "ios", test, feature = "testing"))]
    pub(crate) fn new(keep: impl Any + Send) -> Self {
        Self(Box::new(keep))
    }
}

impl fmt::Debug for AccessScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessScope").finish_non_exhaustive()
    }
}

/// A file a [`Grant`] reacquired, reachable while this value is held.
///
/// It crosses from the thread that resolved it to the owner with the
/// answer. Its [`file`](Self::file) is a path the process can open while
/// the access lives, or, on a system that hands no path over (Android), the
/// file's name and contents read when it was resolved. A system that keeps
/// a grant only for a while may hand a fresher one over
/// ([`renewed`](Self::renewed)), which the application stores in the old
/// one's place.
#[derive(Debug)]
pub struct Access {
    file: PickedFile,
    renewed: Option<Grant>,
    scope: Option<AccessScope>,
}

impl Access {
    /// `file`, as a backend reacquired it, with a fresher grant where the
    /// system made one. A backend outside this crate hands no scope over:
    /// its path is reached where the sandbox reaches it.
    #[must_use]
    pub fn new(file: PickedFile, renewed: Option<Grant>) -> Self {
        Self {
            file,
            renewed,
            scope: None,
        }
    }

    /// `file` held open by `scope`, the access a system started for it.
    #[cfg(any(target_os = "macos", target_os = "ios", test, feature = "testing"))]
    pub(crate) fn scoped(file: PickedFile, scope: AccessScope, renewed: Option<Grant>) -> Self {
        Self {
            file,
            renewed,
            scope: Some(scope),
        }
    }

    /// The file: a path the process opens while this value is held, or its
    /// name and contents.
    #[must_use]
    pub fn file(&self) -> &PickedFile {
        &self.file
    }

    /// A fresher grant to store in place of the one resolved: a macOS or
    /// iOS bookmark Foundation calls stale (the file moved or was renamed),
    /// made again while the access is held. `None` when the grant resolved
    /// stays as good.
    #[must_use]
    pub fn renewed(&self) -> Option<&Grant> {
        self.renewed.as_ref()
    }

    /// Whether the access holds a scope the system started for it, which
    /// ends when this value drops.
    #[must_use]
    pub fn is_scoped(&self) -> bool {
        self.scope.is_some()
    }
}

/// The system's grants as [`Grants`] reach them.
///
/// Owner-local; it reads the file system off the owner thread and answers
/// through the [`Resolver`]s, never calling back into the handle. A grant it
/// receives is one it [takes](Self::takes).
pub trait GrantBackend: 'static {
    /// Whether it resolves `grant`: one its system made (a real backend
    /// takes the [`GrantKind`] its dialogs make, and never the scripted
    /// double's grants).
    fn takes(&self, grant: &Grant) -> bool;

    /// Read the sandbox the process runs in: off the owner thread where the
    /// file system describes it (a Flatpak's `/.flatpak-info`). Asked again
    /// at a later resolve when it answered nothing (the resolver dropped).
    fn sandbox(&self, answer: Resolver<Sandbox>);

    /// Reacquire what `grant` reaches; a system that hands the file over as
    /// its contents reads at most `max_bytes`. The resolve lasts while the
    /// handle holds what this returns: its drop stops the work nobody waits
    /// for any more.
    fn resolve(&self, grant: Grant, max_bytes: u64, answer: Resolver<GrantResult>) -> ResolveHold;
}

/// Widget-facing grants: [`resolve`](Self::resolve) one an application
/// kept.
///
/// Owner-thread (`!Send`), cheap to clone. An answer settles at an owner
/// turn, never inside the call that asked.
#[derive(Clone)]
pub struct Grants {
    backend: Rc<dyn GrantBackend>,
    sandbox: Rc<SandboxSlot>,
    completions: CompletionSource,
}

impl Grants {
    /// A handle over `backend` for the acquisition `ctx`; the backend is
    /// asked for the process's sandbox here, and each resolve waits for it.
    pub fn new(backend: impl GrantBackend, ctx: &ProviderContext<'_>) -> Self {
        let backend: Rc<dyn GrantBackend> = Rc::new(backend);
        let completions = ctx.completions().clone();
        let sandbox = SandboxSlot::reading(read_sandbox(&*backend, &completions));
        Self {
            backend,
            sandbox,
            completions,
        }
    }

    /// Reacquire the file `grant` reaches.
    ///
    /// A grant of another kind than this system's is [`GrantError::Foreign`]
    /// and no backend is asked. Whatever the backend answers is held to the
    /// layer's rule on every host: a path the process's sandbox does not
    /// reach and no scope its system started holds open is one the process
    /// cannot open, answered [`GrantError::Revoked`]; contents past
    /// `max_bytes` are [`GrantError::TooLarge`]; a folder handed over by its
    /// files, a download, or a fresher grant of another kind is
    /// [`SystemFailure::MalformedAnswer`].
    ///
    /// `max_bytes` bounds what a system that hands the file over as its
    /// contents (Android) reads; a reacquired path is the application's to
    /// read. A resolve waits for the process's sandbox, which is read again
    /// when the backend answered it nothing before; past
    /// [`MAX_WAITING`](Self::MAX_WAITING) resolves already waiting, one more
    /// is refused with `Unsupported` (`Unavailable`), worth asking again.
    /// Dropping the [`Resolving`] before it answers delivers nothing, and
    /// stops what its platform can stop ([`Resolving`] says what).
    pub fn resolve(&self, grant: &Grant, max_bytes: u64) -> Resolving {
        let (completion, caller) = self.completions.pending::<GrantResult>();
        let state = Rc::new(ResolveState::default());
        if !self.backend.takes(grant) {
            // The owner retiring first leaves nothing waiting.
            let _ = caller.resolve(Err(GrantError::Foreign));
            return Resolving::new(completion, state);
        }
        let caller = Rc::new(Cell::new(Some(caller)));
        let backend = Rc::clone(&self.backend);
        let completions = self.completions.clone();
        let grant = grant.clone();
        let held = Rc::downgrade(&state);
        let starting = Rc::clone(&caller);
        let start = move |sandbox: Option<Rc<Sandbox>>| {
            // A resolve dropped while the sandbox was read starts nothing.
            let (Some(state), Some(caller)) = (held.upgrade(), starting.take()) else {
                return;
            };
            state.started();
            let Some(sandbox) = sandbox else {
                // The backend answered no sandbox this time: nothing is
                // reached, and the next resolve asks again.
                let _ = caller.resolve(Err(unavailable()));
                return;
            };
            let (answered, answer) = completions.pending::<GrantResult>();
            let mut caller = Some(caller);
            let resolved = grant.clone();
            answered.on_complete(move |outcome| {
                let Some(caller) = caller.take() else {
                    return;
                };
                match outcome {
                    Ok(result) => {
                        // The owner retiring first leaves nothing waiting.
                        let _ = caller.resolve(
                            result.and_then(|access| admit(access, &sandbox, &resolved, max_bytes)),
                        );
                    }
                    // The backend dropped its answer: so does the handle.
                    Err(_) => drop(caller),
                }
            });
            state.hold(backend.resolve(grant, max_bytes, answer));
        };
        let reader = Rc::clone(&self.backend);
        let completions = self.completions.clone();
        match self
            .sandbox
            .when_read(start, || read_sandbox(&*reader, &completions))
        {
            Ok(Some(ticket)) => state.waits_in(&self.sandbox, ticket),
            Ok(None) => {}
            Err(TooManyWaiting) => {
                if let Some(caller) = caller.take() {
                    // The owner retiring first leaves nothing waiting.
                    let _ = caller.resolve(Err(unavailable()));
                }
            }
        }
        Resolving::new(completion, state)
    }

    /// The most resolves of one handle that wait for its sandbox at once.
    pub const MAX_WAITING: usize = SandboxSlot::MAX_WAITING;
}

/// Ask `backend` for the process's sandbox.
fn read_sandbox(backend: &dyn GrantBackend, completions: &CompletionSource) -> Completion<Sandbox> {
    let (read, answer) = completions.pending::<Sandbox>();
    backend.sandbox(answer);
    read
}

/// The refusal of a resolve that cannot run now and may later.
fn unavailable() -> GrantError {
    GrantError::Unsupported(Unsupported::of::<GrantCapability>(
        UnsupportedReason::Unavailable,
    ))
}

/// `access`, if the process can use it: a path `sandbox` reaches or a scope
/// holds open, contents within `max_bytes`, a fresher grant of the system
/// that made `resolved`.
fn admit(
    access: Access,
    sandbox: &Sandbox,
    resolved: &Grant,
    max_bytes: u64,
) -> Result<Access, GrantError> {
    if access
        .renewed
        .as_ref()
        .is_some_and(|renewed| !renewed.same_origin(resolved))
    {
        return Err(SystemFailure::MalformedAnswer.into());
    }
    match access.file.form() {
        PickedForm::Path(path) if access.is_scoped() || sandbox.reaches(path) => Ok(()),
        PickedForm::Path(_) => Err(GrantError::Revoked),
        PickedForm::Contents { contents, .. } => {
            let actual = u64::try_from(contents.len()).unwrap_or(u64::MAX);
            if actual > max_bytes {
                Err(GrantError::TooLarge {
                    actual,
                    limit: max_bytes,
                })
            } else {
                Ok(())
            }
        }
        PickedForm::Folder { .. } | PickedForm::Download { .. } => {
            Err(SystemFailure::MalformedAnswer.into())
        }
    }?;
    Ok(access)
}

impl fmt::Debug for Grants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grants").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_platform::SystemFailure;

    use super::{Access, AccessScope, Grant, GrantError, GrantKind, admit};
    use crate::dialog::{FolderFile, FolderPath, PickedFile};
    use crate::sandbox::android_app;

    fn contents(len: usize) -> PickedFile {
        PickedFile::try_from_contents("a.txt", Arc::from(vec![0; len])).expect("a file name")
    }

    #[test]
    fn an_answer_is_admitted_only_in_a_form_the_process_can_use() {
        let nowhere = android_app();
        let resolved = Grant::try_new(GrantKind::ContentUri, &b"content://a"[..]).expect("a URI");
        let admitted = |access: Access| admit(access, &nowhere, &resolved, 4);
        assert!(admitted(Access::new(contents(4), None)).is_ok());
        assert_eq!(
            admitted(Access::new(contents(5), None)).err(),
            Some(GrantError::TooLarge {
                actual: 5,
                limit: 4
            })
        );
        let path = PickedFile::try_new(std::env::temp_dir().join("a.txt")).expect("absolute");
        assert_eq!(
            admitted(Access::new(path.clone(), None)).err(),
            Some(GrantError::Revoked),
            "a path the sandbox does not reach, with no scope"
        );
        assert!(admitted(Access::scoped(path, AccessScope::new(()), None)).is_ok());
        let folder = PickedFile::try_from_folder(
            "f",
            vec![FolderFile::new(
                FolderPath::try_new("a").expect("a name"),
                Arc::from(Vec::new()),
            )],
        )
        .expect("a folder");
        assert_eq!(
            admitted(Access::new(folder, None)).err(),
            Some(SystemFailure::MalformedAnswer.into())
        );
        let other = Grant::try_new(GrantKind::AppleBookmark, &b"b"[..]).expect("a payload");
        assert_eq!(
            admitted(Access::new(contents(1), Some(other))).err(),
            Some(SystemFailure::MalformedAnswer.into()),
            "a fresher grant of another kind is no grant of this system's"
        );
    }
}
