//! Opening a URI, a file or a folder in the system's handler, and moving a
//! file to the trash (ADR-0154 §3, §4).
//!
//! A widget acquires a [`Launcher`] through
//! `cx.capability::<LauncherCapability>()` in a lifecycle hook. The marker
//! and the handle live in this contract crate because framework widgets (a
//! link) name them; the operating system's handler reaches the handle
//! through a [`LauncherBackend`] that a capability crate's provider builds.
//!
//! What may be launched is decided here, once, for every backend:
//! [`ExternalUri`] refuses the schemes that would run code or read local
//! data in the system's handler, and a path must be absolute.

use std::{
    fmt,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::capability::{Capability, Completion, CompletionSource, ProviderContext, Resolver};
use crate::unsupported::UnsupportedReason;

/// The launcher capability: `cx.capability::<LauncherCapability>()` hands out
/// a [`Launcher`].
#[derive(Debug)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum LauncherCapability {}

impl Capability for LauncherCapability {
    type Handle = Launcher;
    const NAME: &'static str = "launcher";
}

/// The longest URI a launcher hands to the system, in bytes.
pub const MAX_EXTERNAL_URI_LEN: usize = 8 * 1024;

/// The schemes no launcher hands to the system's handler: they run script
/// (`javascript`, `vbscript`), carry their own document (`data`), name a
/// page of the handler itself (`about`, `blob`), or read local files, which
/// go through [`Launcher::open_path`] instead (`file`).
pub const REFUSED_SCHEMES: [&str; 6] = ["javascript", "vbscript", "data", "about", "blob", "file"];

/// An absolute URI a launcher may hand to the system's handler.
///
/// The policy is the seam's, so every backend has it: the scheme is valid
/// (RFC 3986 §3.1) and not one of [`REFUSED_SCHEMES`], the URI holds no
/// control character and is at most [`MAX_EXTERNAL_URI_LEN`] bytes. An
/// application cannot widen it; it can check a URI further before asking.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ExternalUri {
    uri: String,
    /// The length of the scheme, before its `:`.
    scheme_len: usize,
}

impl ExternalUri {
    /// `uri` checked against the launch policy.
    ///
    /// # Errors
    ///
    /// The first rule `uri` breaks, as an [`InvalidUri`].
    pub fn try_new(uri: impl Into<String>) -> Result<Self, InvalidUri> {
        let uri: String = uri.into();
        if uri.len() > MAX_EXTERNAL_URI_LEN {
            return Err(InvalidUri::TooLong {
                limit: MAX_EXTERNAL_URI_LEN,
            });
        }
        if uri.chars().any(char::is_control) {
            return Err(InvalidUri::ControlCharacter);
        }
        let scheme_len = uri.find(':').ok_or(InvalidUri::NotAbsolute)?;
        let scheme = &uri[..scheme_len];
        let mut chars = scheme.chars();
        let valid = chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !valid {
            return Err(InvalidUri::NotAbsolute);
        }
        if REFUSED_SCHEMES
            .iter()
            .any(|refused| scheme.eq_ignore_ascii_case(refused))
        {
            return Err(InvalidUri::RefusedScheme);
        }
        Ok(Self { uri, scheme_len })
    }

    /// The URI as given.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.uri
    }

    /// The scheme as given, before its `:`; compare it ignoring ASCII case.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.uri[..self.scheme_len]
    }
}

impl fmt::Debug for ExternalUri {
    /// The scheme only: the rest of a URI can carry a user's data.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalUri")
            .field("scheme", &self.scheme())
            .finish_non_exhaustive()
    }
}

/// Why a string is not an [`ExternalUri`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidUri {
    /// It has no scheme, or its scheme is not valid: a relative reference
    /// has no handler.
    #[error("the URI has no valid scheme")]
    NotAbsolute,
    /// Its scheme is one of [`REFUSED_SCHEMES`].
    #[error("the URI's scheme is not handed to the system")]
    RefusedScheme,
    /// It is longer than `limit` bytes.
    #[error("the URI is longer than {limit} bytes")]
    TooLong {
        /// [`MAX_EXTERNAL_URI_LEN`].
        limit: usize,
    },
    /// It holds a control character.
    #[error("the URI holds a control character")]
    ControlCharacter,
}

/// Why the system did not launch what it was asked to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchError {
    /// The path is relative: a launcher opens only absolute paths, since the
    /// system's handler does not share the application's working directory.
    #[error("the path is not absolute")]
    RelativePath,
    /// The path does not exist.
    #[error("the path does not exist")]
    NotFound,
    /// Nothing on the system handles the scheme or the kind of file.
    #[error("nothing handles it")]
    NoHandler,
    /// The user, the sandbox or the system refused.
    #[error("the system refused")]
    Denied,
    /// The service the backend launches through is missing or not running
    /// (no desktop portal, no file manager); the reason says whether trying
    /// later may help.
    #[error("the launch service is unavailable: {0:?}")]
    Unavailable(UnsupportedReason),
    /// The system answered with an error the launcher does not name; `code`
    /// is the system's, for diagnostics only.
    #[error("the system failed with code {code}")]
    Failed {
        /// The operating system's error code.
        code: i32,
    },
}

/// What a launch answers: done, or why not.
pub type LaunchResult = Result<(), LaunchError>;

/// The system's handler as a [`Launcher`] reaches it.
///
/// Owner-local. A backend answers each request through its [`Resolver`]
/// when the system answers, at once or later; it never calls back into the
/// handle. A path it receives is absolute.
pub trait LauncherBackend: 'static {
    /// Open `uri` in the handler of its scheme.
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>);

    /// Open the file or folder at `path` in its default application.
    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>);

    /// Show `path` in the file manager, selected in its folder where the
    /// system can select it.
    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>);

    /// Move the file or folder at `path` to the trash.
    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>);
}

/// Widget-facing launcher for one presentation.
///
/// Owner-thread (`!Send`), cheap to clone. Every request answers with a
/// [`Completion`], which settles at an owner turn after the system answers,
/// never inside the call that asked.
#[derive(Clone)]
pub struct Launcher {
    backend: Rc<dyn LauncherBackend>,
    completions: CompletionSource,
}

impl Launcher {
    /// A handle over `backend` for the acquisition `cx`.
    #[must_use]
    pub fn new(backend: impl LauncherBackend, cx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: cx.completions().clone(),
        }
    }

    /// Open `uri` in the handler of its scheme: a browser for `https`, a
    /// mail client for `mailto`.
    pub fn open_uri(&self, uri: &ExternalUri) -> Completion<LaunchResult> {
        let (completion, answer) = self.completions.pending();
        self.backend.open_uri(uri.clone(), answer);
        completion
    }

    /// Open the file or folder at `path` in its default application.
    pub fn open_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.with_absolute(path, LauncherBackend::open_path)
    }

    /// Show `path` in the file manager.
    pub fn reveal_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.with_absolute(path, LauncherBackend::reveal_path)
    }

    /// Move the file or folder at `path` to the trash.
    pub fn trash_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.with_absolute(path, LauncherBackend::trash_path)
    }

    /// Ask `backend` about `path` when it is absolute; answer
    /// [`LaunchError::RelativePath`] without asking otherwise.
    fn with_absolute(
        &self,
        path: &Path,
        ask: impl FnOnce(&dyn LauncherBackend, PathBuf, Resolver<LaunchResult>),
    ) -> Completion<LaunchResult> {
        let (completion, answer) = self.completions.pending();
        if path.is_absolute() {
            ask(&*self.backend, path.to_path_buf(), answer);
        } else {
            // The owner retiring first leaves nothing waiting for the answer.
            let _ = answer.resolve(Err(LaunchError::RelativePath));
        }
        completion
    }
}

impl fmt::Debug for Launcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launcher").finish_non_exhaustive()
    }
}
