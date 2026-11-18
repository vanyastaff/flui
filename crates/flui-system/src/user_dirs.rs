//! Where the user keeps their documents, downloads, pictures and the rest.
//!
//! A widget acquires [`UserDirs`] through
//! `ctx.capability::<UserDirCapability>()` and asks for a [`UserDir`]. The
//! answer is the folder the user's system names for it, or none: a system
//! may name no folder for a kind, or the user may have switched it off. The
//! application's own storage is not here: it is the storage capability.

use std::{fmt, path::PathBuf, rc::Rc};

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

/// The user-folder capability: `ctx.capability::<UserDirCapability>()`
/// hands out [`UserDirs`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum UserDirCapability {}

impl Capability for UserDirCapability {
    type Handle = UserDirs;
    const NAME: &'static str = "user-dirs";
}

/// A folder the user keeps one kind of file in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UserDir {
    /// The user's home folder.
    Home,
    /// The desktop.
    Desktop,
    /// Documents.
    Documents,
    /// Downloads.
    Downloads,
    /// Music.
    Music,
    /// Pictures.
    Pictures,
    /// Videos.
    Videos,
    /// Templates a file manager offers new files from.
    Templates,
    /// Files the user shares with others.
    PublicShare,
}

/// Why a folder could not be named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UserDirError {
    /// The system names no user folders here, as the [`Unsupported`] says.
    #[error("user folders are not available: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name: the file that
    /// names the folders could not be read, or says something no folder is.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a folder request answers: the folder, an absolute path, or `None`
/// when the system names none for that kind.
pub type UserDirResult = Result<Option<PathBuf>, UserDirError>;

/// The system's user folders as [`UserDirs`] reaches them.
///
/// Owner-local; it reads the file system off the owner thread and answers
/// through the [`Resolver`], never calling back into the handle. A folder it
/// answers is absolute.
pub trait UserDirBackend: 'static {
    /// Name the folder of `dir`.
    fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>);
}

/// Widget-facing user folders.
///
/// Owner-thread (`!Send`), cheap to clone. An answer settles at an owner
/// turn, never inside the call that asked.
#[derive(Clone)]
pub struct UserDirs {
    backend: Rc<dyn UserDirBackend>,
    completions: CompletionSource,
}

impl UserDirs {
    /// A handle over `backend` for the acquisition `ctx`.
    pub fn new(backend: impl UserDirBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// The folder the system names for `dir`, read when asked: a user who
    /// moved their documents since is answered the new folder.
    pub fn locate(&self, dir: UserDir) -> Completion<UserDirResult> {
        let (completion, answer) = self.completions.pending::<UserDirResult>();
        self.backend.locate(dir, answer);
        completion
    }
}

impl fmt::Debug for UserDirs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserDirs").finish_non_exhaustive()
    }
}
