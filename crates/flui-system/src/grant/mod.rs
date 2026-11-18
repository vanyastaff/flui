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
//! path is one its [`Sandbox`] reaches, or one the access holds open (a
//! security-scoped resource on macOS and iOS, started when the grant was
//! resolved and stopped when the [`Access`] drops); the handle refuses any
//! other answer, whichever backend gave it.

#[cfg(any(windows, target_os = "linux", all(test, any(unix, windows))))]
pub(crate) mod path;
mod token;

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

pub use token::{Grant, GrantKind, MalformedGrant};

use crate::dialog::{PickedFile, PickedForm};
use crate::sandbox::Sandbox;

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
/// `Send`, since a backend starts the access where it resolves the grant,
/// off the owner thread (ADR-0360 §2), and hands it to the owner with the
/// answer.
#[must_use = "dropping the scope ends the access it holds"]
pub struct AccessScope(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any + Send>);

impl AccessScope {
    /// A scope whose drop drops `keep`: the value whose `Drop` ends the
    /// system's access.
    pub fn new(keep: impl Any + Send) -> Self {
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
/// answer. Its [`file`](Self::file) is a path the process
/// can open while the access lives, or, on a system that hands no path
/// over (Android), the file's name and contents read when it was resolved.
/// A system that keeps a grant only for a while may hand a fresher one
/// over ([`renewed`](Self::renewed)), which the application stores in the
/// old one's place.
#[derive(Debug)]
pub struct Access {
    file: PickedFile,
    renewed: Option<Grant>,
    scope: Option<AccessScope>,
}

impl Access {
    /// `file`, as a backend reacquired it, held open by `scope` where the
    /// system needs one, with a fresher grant where the system made one.
    #[must_use]
    pub fn new(file: PickedFile, scope: Option<AccessScope>, renewed: Option<Grant>) -> Self {
        Self {
            file,
            renewed,
            scope,
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
/// Owner-local; it resolves off the owner thread and answers through the
/// [`Resolver`], never calling back into the handle. A grant it receives is
/// of its [`kind`](Self::kind).
pub trait GrantBackend: 'static {
    /// The kind of grant it resolves, and its dialogs make.
    fn kind(&self) -> GrantKind;

    /// Reacquire what `grant` reaches; a system that hands the file over as
    /// its contents reads at most `max_bytes`.
    fn resolve(&self, grant: Grant, max_bytes: u64, answer: Resolver<GrantResult>);
}

/// Widget-facing grants: [`resolve`](Self::resolve) one an application
/// kept.
///
/// Owner-thread (`!Send`), cheap to clone. An answer settles at an owner
/// turn, never inside the call that asked.
#[derive(Clone)]
pub struct Grants {
    backend: Rc<dyn GrantBackend>,
    sandbox: Rc<Sandbox>,
    completions: CompletionSource,
}

impl Grants {
    /// A handle over `backend` for the acquisition `ctx`, in a process
    /// `sandbox` confines.
    pub fn new(backend: impl GrantBackend, sandbox: Sandbox, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            sandbox: Rc::new(sandbox),
            completions: ctx.completions().clone(),
        }
    }

    /// The sandbox the process runs in, as the platform's provider read it.
    #[must_use]
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// Reacquire the file `grant` reaches.
    ///
    /// A grant of another kind than this system's is [`GrantError::Foreign`]
    /// and no backend is asked. Whatever the backend answers is held to the
    /// layer's rule on every host: a path the [sandbox](Self::sandbox) does
    /// not reach and no [scope](Access::is_scoped) holds open is one the
    /// process cannot open, answered [`GrantError::Revoked`]; contents past
    /// `max_bytes` are [`GrantError::TooLarge`]; a folder handed over by its
    /// files, a download, or a fresher grant of another kind is
    /// [`SystemFailure::MalformedAnswer`].
    ///
    /// `max_bytes` bounds what a system that hands the file over as its
    /// contents (Android) reads; a reacquired path is the application's to
    /// read.
    pub fn resolve(&self, grant: &Grant, max_bytes: u64) -> Completion<GrantResult> {
        let (completion, caller) = self.completions.pending::<GrantResult>();
        let kind = self.backend.kind();
        if grant.kind() != kind {
            // The owner retiring first leaves nothing waiting.
            let _ = caller.resolve(Err(GrantError::Foreign));
            return completion;
        }
        let (answered, answer) = self.completions.pending::<GrantResult>();
        let sandbox = Rc::clone(&self.sandbox);
        let mut caller = Some(caller);
        answered.on_complete(move |outcome| {
            let Some(caller) = caller.take() else {
                return;
            };
            match outcome {
                Ok(result) => {
                    // The owner retiring first leaves nothing waiting.
                    let _ = caller.resolve(
                        result.and_then(|access| admit(access, &sandbox, kind, max_bytes)),
                    );
                }
                // The backend dropped its answer: so does the handle.
                Err(_) => drop(caller),
            }
        });
        self.backend.resolve(grant.clone(), max_bytes, answer);
        completion
    }
}

/// `access`, if the process can use it: a path `sandbox` reaches or a scope
/// holds open, contents within `max_bytes`, a fresher grant of `kind`.
fn admit(
    access: Access,
    sandbox: &Sandbox,
    kind: GrantKind,
    max_bytes: u64,
) -> Result<Access, GrantError> {
    if access
        .renewed
        .as_ref()
        .is_some_and(|renewed| renewed.kind() != kind)
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
        f.debug_struct("Grants")
            .field("sandbox", &self.sandbox)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_platform::SystemFailure;

    use super::{Access, AccessScope, Grant, GrantError, GrantKind, admit};
    use crate::dialog::{FolderFile, FolderPath, PickedFile};
    use crate::sandbox::Sandbox;

    fn contents(len: usize) -> PickedFile {
        PickedFile::try_from_contents("a.txt", Arc::from(vec![0; len])).expect("a file name")
    }

    #[test]
    fn an_answer_is_admitted_only_in_a_form_the_process_can_use() {
        let nowhere = Sandbox::try_confined(crate::sandbox::SandboxKind::AndroidApp, [])
            .expect("no folder to refuse");
        let admitted = |access: Access| admit(access, &nowhere, GrantKind::ContentUri, 4);
        assert!(admitted(Access::new(contents(4), None, None)).is_ok());
        assert_eq!(
            admitted(Access::new(contents(5), None, None)).err(),
            Some(GrantError::TooLarge {
                actual: 5,
                limit: 4
            })
        );
        let path = PickedFile::try_new(std::env::temp_dir().join("a.txt")).expect("absolute");
        assert_eq!(
            admitted(Access::new(path.clone(), None, None)).err(),
            Some(GrantError::Revoked),
            "a path the sandbox does not reach, with no scope"
        );
        assert!(admitted(Access::new(path, Some(AccessScope::new(())), None)).is_ok());
        let folder = PickedFile::try_from_folder(
            "f",
            vec![FolderFile::new(
                FolderPath::try_new("a").expect("a name"),
                Arc::from(Vec::new()),
            )],
        )
        .expect("a folder");
        assert_eq!(
            admitted(Access::new(folder, None, None)).err(),
            Some(SystemFailure::MalformedAnswer.into())
        );
        let other = Grant::try_new(GrantKind::AppleBookmark, &b"b"[..]).expect("a payload");
        assert_eq!(
            admitted(Access::new(contents(1), None, Some(other))).err(),
            Some(SystemFailure::MalformedAnswer.into()),
            "a fresher grant of another kind is no grant of this system's"
        );
    }
}
