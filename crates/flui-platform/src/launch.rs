//! Opening a URI, a file or a folder in the system's handler, and moving a
//! file to the trash (ADR-0154 §3, §4).
//!
//! A widget acquires a [`Launcher`] through
//! `ctx.capability::<LauncherCapability>()` in a lifecycle hook. The marker
//! and the handle live in this contract crate because framework widgets (a
//! link) name them; the operating system's handler reaches the handle
//! through a [`LauncherBackend`] that a capability crate's provider builds.
//!
//! What may be launched is decided here, once, for every backend, the
//! scripted one included (ADR-0359): a URI is an [`ExternalUri`], whose
//! caller names the [`Scheme`]s it allows; a path is absolute and local, and
//! [`Launcher::open_path`] opens documents only, never a program, a script or
//! a shortcut.

use std::{
    borrow::Cow,
    fmt,
    path::{Component, Path, PathBuf, Prefix},
    rc::Rc,
};

use crate::capability::{Capability, Completion, CompletionSource, ProviderContext, Resolver};
use crate::system_failure::SystemFailure;
use crate::unsupported::Unsupported;

/// The launcher capability: `ctx.capability::<LauncherCapability>()` hands out
/// a [`Launcher`].
#[derive(Debug)]
#[expect(
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

/// The schemes no launcher hands to the system's handler, whoever allows
/// them: they run script (`javascript`, `vbscript`), carry their own document
/// (`data`), name a page of the handler itself (`about`, `blob`), or read
/// local files, which go through [`Launcher::open_path`] instead (`file`).
/// [`Scheme::try_new`] refuses each of them.
pub const REFUSED_SCHEMES: [&str; 6] = ["javascript", "vbscript", "data", "about", "blob", "file"];

/// The file name extensions [`Launcher::open_path`] refuses, compared
/// ignoring ASCII case: opening such a file runs a program or a script, or
/// sends the system's handler to another target, with the default
/// associations of a system FLUI supports. One list for every host, since a
/// file made on one system is opened on another.
pub const RUNNABLE_EXTENSIONS: [&str; 41] = [
    // Windows programs and installers.
    "exe",
    "com",
    "scr",
    "pif",
    "cpl",
    "msc",
    "msi",
    "msp",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "application",
    "appref-ms",
    "diagcab",
    "hta",
    // Windows scripts.
    "bat",
    "cmd",
    "ps1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "reg",
    // Windows shortcuts and searches: they open another target.
    "lnk",
    "url",
    "scf",
    "library-ms",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    // A Linux desktop entry runs the command it names.
    "desktop",
    // macOS applications, Terminal scripts and Automator workflows, and the
    // Finder's location files.
    "app",
    "command",
    "tool",
    "terminal",
    "workflow",
    "webloc",
];

/// A URI scheme a caller lets a launch hand to the system's handler:
/// `https`, `mailto`, an application's own.
///
/// Held in ASCII lower case, so two spellings of one scheme are one value;
/// a URI's scheme is compared with it ignoring ASCII case, as RFC 3986 §3.1
/// compares schemes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheme(Cow<'static, str>);

impl Scheme {
    /// `https`.
    pub const HTTPS: Self = Self(Cow::Borrowed("https"));
    /// `http`.
    pub const HTTP: Self = Self(Cow::Borrowed("http"));
    /// `mailto`.
    pub const MAILTO: Self = Self(Cow::Borrowed("mailto"));
    /// `tel`.
    pub const TEL: Self = Self(Cow::Borrowed("tel"));

    /// `name` as a scheme a launch may hand to the system.
    ///
    /// `None` when `name` is not a scheme (RFC 3986 §3.1: an ASCII letter,
    /// then letters, digits, `+`, `-` and `.`), or is one of
    /// [`REFUSED_SCHEMES`], which no caller can allow.
    #[must_use]
    pub fn try_new(name: impl Into<Cow<'static, str>>) -> Option<Self> {
        let mut name: Cow<'static, str> = name.into();
        if !is_scheme(&name) || is_refused(&name) {
            return None;
        }
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            name.to_mut().make_ascii_lowercase();
        }
        Some(Self(name))
    }

    /// The scheme, in ASCII lower case.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An absolute URI a launcher may hand to the system's handler.
///
/// The policy is the seam's, so every backend has it (ADR-0359): the URI is
/// at most [`MAX_EXTERNAL_URI_LEN`] bytes, holds no character that hides how
/// it reads, has a valid scheme that is not one of [`REFUSED_SCHEMES`] and is
/// one the caller allowed, and names no user before its host. The URI is
/// handed on as written: nothing decodes its percent-escapes, so an escaped
/// byte never becomes a delimiter or a control character on the way.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ExternalUri {
    uri: String,
    /// The length of the scheme, before its `:`.
    scheme_len: usize,
}

impl ExternalUri {
    /// `uri` checked against the launch policy, with `allowed` the schemes
    /// the caller lets through: `&[Scheme::HTTPS]` for a link that must open
    /// in a browser.
    ///
    /// The length is checked before anything else reads or copies `uri`.
    ///
    /// # Errors
    ///
    /// The first rule `uri` breaks, as an [`InvalidUri`].
    pub fn try_new(uri: &str, allowed: &[Scheme]) -> Result<Self, InvalidUri> {
        if uri.len() > MAX_EXTERNAL_URI_LEN {
            return Err(InvalidUri::TooLong {
                limit: MAX_EXTERNAL_URI_LEN,
            });
        }
        if uri.chars().any(hides_how_it_reads) {
            return Err(InvalidUri::ControlCharacter);
        }
        let scheme_len = uri.find(':').ok_or(InvalidUri::NotAbsolute)?;
        let (scheme, rest) = (&uri[..scheme_len], &uri[scheme_len + 1..]);
        if !is_scheme(scheme) {
            return Err(InvalidUri::NotAbsolute);
        }
        if is_refused(scheme) {
            return Err(InvalidUri::RefusedScheme);
        }
        if !allowed
            .iter()
            .any(|allowed| allowed.as_str().eq_ignore_ascii_case(scheme))
        {
            return Err(InvalidUri::SchemeNotAllowed);
        }
        if names_a_user(rest) {
            return Err(InvalidUri::Credentials);
        }
        Ok(Self {
            uri: uri.to_owned(),
            scheme_len,
        })
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

/// Whether `scheme` is one by RFC 3986 §3.1: an ASCII letter, then ASCII
/// letters, digits, `+`, `-` and `.`.
fn is_scheme(scheme: &str) -> bool {
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Whether `scheme` is one of [`REFUSED_SCHEMES`], ignoring ASCII case.
fn is_refused(scheme: &str) -> bool {
    REFUSED_SCHEMES
        .iter()
        .any(|refused| scheme.eq_ignore_ascii_case(refused))
}

/// Whether `c` changes how a URI reads without being seen: a control
/// character (C0, DEL, C1), or a bidirectional formatting character, which
/// shows the text after it reordered (`…/` U+202E `fdp.exe` reads as
/// `…/exe.pdf`).
fn hides_how_it_reads(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Whether the part of a URI after its scheme's `:` has an authority with a
/// user in it: `//user:password@host`. `https://bank.example@evil.example/`
/// opens `evil.example`. A browser reads `\` as `/` here, so either starts
/// the authority.
fn names_a_user(after_scheme: &str) -> bool {
    let Some(authority) = after_scheme
        .strip_prefix(['/', '\\'])
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
    else {
        return false;
    };
    let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    authority[..end].contains('@')
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
    /// Its scheme is not one the caller allowed.
    #[error("the URI's scheme is not one the caller allows")]
    SchemeNotAllowed,
    /// It names a user, and maybe a password, before its host
    /// (`https://name@host`): the text before the `@` reads as the host to
    /// a person and is not one.
    #[error("the URI names a user before its host")]
    Credentials,
    /// It is longer than `limit` bytes.
    #[error("the URI is longer than {limit} bytes")]
    TooLong {
        /// [`MAX_EXTERNAL_URI_LEN`].
        limit: usize,
    },
    /// It holds a control character or a bidirectional formatting
    /// character, which changes how it reads without being seen.
    #[error("the URI holds a control or formatting character")]
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
    /// The path names a network share or a device (`\\server\share`,
    /// `\\.\device` on Windows), not a local file: reaching it would make
    /// the system connect to the server, with the user's credentials.
    #[error("the path is not a local file")]
    NotLocal,
    /// [`Launcher::open_path`] was asked to open a program, a script or a
    /// shortcut: the path, or what it links to, ends in one of
    /// [`RUNNABLE_EXTENSIONS`]. Opening it would run code or another target.
    #[error("the path is not a document")]
    NotADocument,
    /// The path does not exist.
    #[error("the path does not exist")]
    NotFound,
    /// Nothing on the system handles the scheme or the kind of file.
    #[error("nothing handles it")]
    NoHandler,
    /// The user, the sandbox or the system refused.
    #[error("the system refused")]
    Denied,
    /// The service the backend launches through is not here (no desktop
    /// portal, no file manager); the [`Unsupported`] says why and whether
    /// trying later may help.
    #[error("the launch service cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the launcher does not name; the
    /// [`SystemFailure`] says where the failure came from, for diagnostics.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

impl LaunchError {
    /// What is not here, when that is why the launch failed.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::RelativePath
            | Self::NotLocal
            | Self::NotADocument
            | Self::NotFound
            | Self::NoHandler
            | Self::Denied
            | Self::Failed(_) => None,
        }
    }
}

/// What a launch answers: done, or why not.
pub type LaunchResult = Result<(), LaunchError>;

/// The system's handler as a [`Launcher`] reaches it.
///
/// Owner-local. A backend answers each request through its [`Resolver`]
/// when the system answers, at once or later; it never calls back into the
/// handle. A path it receives is absolute and local, and one it is asked to
/// open names no file of [`RUNNABLE_EXTENSIONS`].
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

/// What a path request asks of the system: an open checks more than the
/// others, since only an open runs what the path names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathVerb {
    Open,
    Reveal,
    Trash,
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
    /// A handle over `backend` for the acquisition `ctx`.
    #[must_use]
    pub fn new(backend: impl LauncherBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Open `uri` in the handler of its scheme: a browser for `https`, a
    /// mail client for `mailto`.
    pub fn open_uri(&self, uri: &ExternalUri) -> Completion<LaunchResult> {
        let (completion, answer) = self.completions.pending();
        self.backend.open_uri(uri.clone(), answer);
        completion
    }

    /// Open the document or folder at `path` in its default application.
    ///
    /// Refused without asking the system: a relative path
    /// ([`LaunchError::RelativePath`]), a network share or a device
    /// ([`LaunchError::NotLocal`]), and a program, a script or a shortcut
    /// ([`LaunchError::NotADocument`]), judged by its name and, where it is
    /// a link, by the name of what it links to.
    pub fn open_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.ask_about(path, PathVerb::Open, LauncherBackend::open_path)
    }

    /// Show `path` in the file manager. A relative path and a network share
    /// are refused as [`open_path`](Self::open_path) refuses them.
    pub fn reveal_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.ask_about(path, PathVerb::Reveal, LauncherBackend::reveal_path)
    }

    /// Move the file or folder at `path` to the trash. A relative path and a
    /// network share are refused as [`open_path`](Self::open_path) refuses
    /// them.
    pub fn trash_path(&self, path: &Path) -> Completion<LaunchResult> {
        self.ask_about(path, PathVerb::Trash, LauncherBackend::trash_path)
    }

    /// Ask `backend` about `path` when the policy admits it for `verb`;
    /// answer the refusal without asking otherwise.
    fn ask_about(
        &self,
        path: &Path,
        verb: PathVerb,
        ask: impl FnOnce(&dyn LauncherBackend, PathBuf, Resolver<LaunchResult>),
    ) -> Completion<LaunchResult> {
        let (completion, answer) = self.completions.pending();
        match admit_path(path, verb) {
            Ok(()) => ask(&*self.backend, path.to_path_buf(), answer),
            Err(refusal) => {
                // The owner retiring first leaves nothing waiting for the answer.
                let _ = answer.resolve(Err(refusal));
            }
        }
        completion
    }
}

impl fmt::Debug for Launcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launcher").finish_non_exhaustive()
    }
}

/// The path policy: `path` is absolute and local as given, and an open
/// names no file of [`RUNNABLE_EXTENSIONS`], neither as given nor as it
/// resolves. A path that does not resolve (it does not exist yet) is judged
/// as given, and the backend answers whether it exists.
///
/// Locality is judged on the path as given: a drive the user mapped to a
/// share resolves to the share and is still the user's choice, while a
/// share named in the path is the caller's.
fn admit_path(path: &Path, verb: PathVerb) -> Result<(), LaunchError> {
    if !path.is_absolute() {
        return Err(LaunchError::RelativePath);
    }
    if !is_local(path) {
        return Err(LaunchError::NotLocal);
    }
    if verb == PathVerb::Open
        && (is_runnable(path)
            || std::fs::canonicalize(path).is_ok_and(|resolved| is_runnable(&resolved)))
    {
        return Err(LaunchError::NotADocument);
    }
    Ok(())
}

/// Whether `path` names no network share and no device. Only Windows paths
/// carry such a prefix; on every other system a path is local.
fn is_local(path: &Path) -> bool {
    !matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::DeviceNS(_)
            )
    )
}

/// Whether the last component of `path` ends in one of
/// [`RUNNABLE_EXTENSIONS`], as Windows reads the name: without the dots and
/// spaces it drops from a name's end, and without the stream a `:` names
/// (`run.exe:data` opens `run.exe`).
fn is_runnable(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    let unstreamed = name.split(':').next().unwrap_or_default();
    [name.as_ref(), unstreamed].into_iter().any(|name| {
        name.trim_end_matches(['.', ' '])
            .rsplit_once('.')
            .is_some_and(|(_, extension)| {
                RUNNABLE_EXTENSIONS
                    .iter()
                    .any(|runnable| runnable.eq_ignore_ascii_case(extension))
            })
    })
}
