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
//! scripted one included (ADR-0360): a URI is an [`ExternalUri`], whose
//! caller names the [`Scheme`]s it allows; a path is absolute and local, and
//! [`Launcher::open_path`] refuses a name of [`RUNNABLE_EXTENSIONS`]. That
//! list is the floor every host has, not the whole policy: a backend asks
//! its own system whether what it opened would run, off the owner thread,
//! and answers [`LaunchError::NotADocument`] the same way.

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

/// The file name extensions [`Launcher::open_path`] refuses on every host,
/// compared ignoring ASCII case: opening such a file runs a program or a
/// script, or sends the system's handler to another target, with the
/// associations a system FLUI supports has by default or commonly gains (a
/// Python or Java runtime). One list for every host, since a file made on one
/// system is opened on another; a name ending in a Windows class id
/// (`x.{3050f4d8-…}`) is refused too.
///
/// The list is a floor, not the policy: each backend also asks its system
/// whether the file it opened would run ([`LauncherBackend::open_path`]).
pub const RUNNABLE_EXTENSIONS: [&str; 56] = [
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
    "chm",
    "wsb",
    "theme",
    "themepack",
    "deskthemepack",
    "mht",
    "mhtml",
    // Scripts of a runtime an installation commonly adds.
    "py",
    "pyw",
    "pyz",
    "pyzw",
    "jar",
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
    "website",
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
    "inetloc",
    "fileloc",
];

/// The schemes whose URIs have an authority, which a browser reads even
/// without the `//` before it (`https:host`): an [`ExternalUri`] of one of
/// them must write the `//`, so the authority checks see every authority.
const AUTHORITY_SCHEMES: [&str; 6] = ["http", "https", "ftp", "ws", "wss", "file"];

/// The fields a `mailto` URI may carry in its query (RFC 6068 §2), compared
/// ignoring ASCII case. Others (`attach`, `in-reply-to`, custom headers) are
/// refused: some mail clients attach the local file an `attach` names.
const MAILTO_FIELDS: [&str; 5] = ["to", "cc", "bcc", "subject", "body"];

/// A URI scheme a caller lets a launch hand to the system's handler:
/// `https`, `mailto`, an application's own.
///
/// Held in ASCII lower case, so two spellings of one scheme are one value;
/// a URI's scheme is compared with it ignoring ASCII case, as RFC 3986 §3.1
/// compares schemes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scheme(Cow<'static, str>);

impl Scheme {
    /// `https`.
    pub const HTTPS: Self = Self(Cow::Borrowed("https"));
    /// `http`.
    pub const HTTP: Self = Self(Cow::Borrowed("http"));
    /// `mailto`. Its query may carry only `to`, `cc`, `bcc`, `subject` and
    /// `body` ([`InvalidUri::MailtoField`]).
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
/// The policy is the seam's, so every backend has it (ADR-0360): the URI is
/// at most [`MAX_EXTERNAL_URI_LEN`] bytes; holds no character RFC 3986 makes
/// it percent-encode (a space, `"`, `<`, `>`, `` ` ``, `{`, `|`, `}`, `^`,
/// `\`), which a shell would splice into its handler's command line, and no
/// character that hides how it reads; has a valid scheme that is not one of
/// [`REFUSED_SCHEMES`] and is one the caller allowed; writes the `//` of a
/// scheme that has an authority, which names no user before its host; and a
/// `mailto` query carries only `to`, `cc`, `bcc`, `subject` and `body`. The
/// URI is handed on as written: nothing decodes its percent-escapes, so an
/// escaped byte never becomes a delimiter or a control character on the way.
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
        for c in uri.chars() {
            if must_be_encoded(c) {
                return Err(InvalidUri::UnencodedCharacter);
            }
            if hides_how_it_reads(c) {
                return Err(InvalidUri::HiddenCharacter);
            }
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
        let has_authority = AUTHORITY_SCHEMES
            .iter()
            .any(|with| with.eq_ignore_ascii_case(scheme));
        if has_authority && !rest.starts_with("//") {
            return Err(InvalidUri::MissingAuthority);
        }
        if names_a_user(rest) {
            return Err(InvalidUri::Credentials);
        }
        if scheme.eq_ignore_ascii_case("mailto") && !has_only_mailto_fields(rest) {
            return Err(InvalidUri::MailtoField);
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

/// Whether `c` is an ASCII character RFC 3986 lets a URI carry only
/// percent-encoded: a space, `"`, `<`, `>`, `` ` ``, `{`, `|`, `}`, `^` or
/// `\`. Windows splices a URI into its handler's `"<app>" "%1"` command
/// line as it is written, so a `"` or a space there starts an argument of
/// the URI's own (the class of CVE-2018-1000006).
fn must_be_encoded(c: char) -> bool {
    matches!(
        c,
        ' ' | '"' | '<' | '>' | '`' | '{' | '|' | '}' | '^' | '\\'
    )
}

/// Whether `c` changes how a URI reads without being seen: a control
/// character (C0, DEL, C1); a bidirectional formatting character, which
/// shows the text after it reordered (`…/` U+202E `fdp.exe` reads as
/// `…/exe.pdf`); a zero-width or invisible formatting character, a soft
/// hyphen or a byte-order mark; or a space or separator outside ASCII
/// (U+00A0, U+2028). A URI has no use for any of them unencoded.
fn hides_how_it_reads(c: char) -> bool {
    c.is_control()
        || c.is_whitespace()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{17B4}'
                | '\u{17B5}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// Whether the part of a URI after its scheme's `:` has an authority with a
/// user in it: `//user:password@host`. `https://bank.example@evil.example/`
/// opens `evil.example`. A scheme that has an authority writes its `//`, so
/// every such authority starts here.
fn names_a_user(after_scheme: &str) -> bool {
    let Some(authority) = after_scheme.strip_prefix("//") else {
        return false;
    };
    let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
    authority[..end].contains('@')
}

/// Whether the query of a `mailto` URI, `after_scheme` being what follows
/// its `:`, names only fields of [`MAILTO_FIELDS`]. A field name holding a
/// `%` is refused rather than decoded: none of the admitted names needs one,
/// and a mail client that decodes `%61ttach` reads `attach`.
fn has_only_mailto_fields(after_scheme: &str) -> bool {
    let Some((_, query)) = after_scheme.split_once('?') else {
        return true;
    };
    let query = query.split_once('#').map_or(query, |(query, _)| query);
    query
        .split('&')
        .filter(|field| !field.is_empty())
        .all(|field| {
            let name = field.split_once('=').map_or(field, |(name, _)| name);
            MAILTO_FIELDS
                .iter()
                .any(|admitted| admitted.eq_ignore_ascii_case(name))
        })
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
    /// Its scheme has an authority (`http`, `https`, `ftp`, `ws`, `wss`)
    /// and the URI does not write the `//` before it (`https:host`), which a
    /// browser reads as an authority all the same.
    #[error("the URI does not write its authority's `//`")]
    MissingAuthority,
    /// It names a user, and maybe a password, before its host
    /// (`https://name@host`): the text before the `@` reads as the host to
    /// a person and is not one.
    #[error("the URI names a user before its host")]
    Credentials,
    /// It is a `mailto` URI whose query names a field other than `to`,
    /// `cc`, `bcc`, `subject` and `body` (an `attach`, which some mail
    /// clients honour with a local file).
    #[error("the mailto URI carries a field other than to, cc, bcc, subject and body")]
    MailtoField,
    /// It is longer than `limit` bytes.
    #[error("the URI is longer than {limit} bytes")]
    TooLong {
        /// [`MAX_EXTERNAL_URI_LEN`].
        limit: usize,
    },
    /// It holds a character RFC 3986 makes it percent-encode: a space,
    /// `"`, `<`, `>`, `` ` ``, `{`, `|`, `}`, `^` or `\`.
    #[error("the URI holds a character it must percent-encode")]
    UnencodedCharacter,
    /// It holds a character that changes how it reads without being seen:
    /// a control, bidirectional formatting, zero-width or invisible
    /// character, or a space or separator outside ASCII.
    #[error("the URI holds a hidden character")]
    HiddenCharacter,
}

/// Why the system did not launch what it was asked to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LaunchError {
    /// The path is relative: a launcher opens only absolute paths, since the
    /// system's handler does not share the application's working directory.
    #[error("the path is not absolute")]
    RelativePath,
    /// The path is not on a local drive: on Windows, any prefix but a drive
    /// letter (`C:\`, `\\?\C:\`), so a share (`\\server\share`,
    /// `\\?\UNC\…`), a device (`\\.\…`) or a namespace that reaches either
    /// (`\\?\GLOBALROOT\Device\Mup\…`). Reaching it could make the system
    /// connect to a server with the user's credentials.
    #[error("the path is not a local file")]
    NotLocal,
    /// [`Launcher::open_path`] was asked to open a program, a script or a
    /// shortcut: its name ends in one of [`RUNNABLE_EXTENSIONS`] or a class
    /// id, or the backend's system says the file it opened would run.
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
/// open has no name of [`runs_when_opened`]. Whatever touches the file
/// system (opening, resolving links, asking the system about the file) runs
/// off the owner thread, and its answer goes to the resolver of the request
/// that asked.
pub trait LauncherBackend: 'static {
    /// Open `uri` in the handler of its scheme.
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>);

    /// Open the file or folder at `path` in its default application.
    ///
    /// The backend opens the file first and judges what it opened, not the
    /// path, so a link swapped between a check and the open changes nothing:
    /// a file whose resolved name [`runs_when_opened`], or which its system
    /// calls dangerous to open (Windows `AssocIsDangerous`, a macOS
    /// application or Unix executable, a Linux executable or desktop entry),
    /// answers [`LaunchError::NotADocument`] and is not handed on.
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
    /// Refused without asking the system, and without touching the file
    /// system on the owner thread: a relative path
    /// ([`LaunchError::RelativePath`]), a path off the local drives
    /// ([`LaunchError::NotLocal`]), and a name of a program, a script or a
    /// shortcut ([`LaunchError::NotADocument`]). The backend judges the file
    /// it opens as well ([`LauncherBackend::open_path`]).
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

/// The path policy, decided from the path alone (nothing here touches the
/// file system, which may stall on the owner thread): `path` is absolute and
/// local, and an open names no file that [`runs_when_opened`]. The backend
/// answers whether the file exists and judges what it opens.
fn admit_path(path: &Path, verb: PathVerb) -> Result<(), LaunchError> {
    if !path.is_absolute() {
        return Err(LaunchError::RelativePath);
    }
    if !is_local_path(path) {
        return Err(LaunchError::NotLocal);
    }
    if verb == PathVerb::Open && runs_when_opened(path) {
        return Err(LaunchError::NotADocument);
    }
    Ok(())
}

/// Whether an absolute `path` is on a local drive: on Windows a drive
/// letter, plain or verbatim, is the only prefix admitted, and every other
/// (a share, a device, `\\?\GLOBALROOT` or a volume name) is not; on every
/// other system an absolute path has no prefix and is local.
///
/// Judged on the path as given, with no file system call, so a drive the
/// user mapped stays theirs. The rule every path a capability hands the
/// system meets (ADR-0360 §2): the launcher's verbs, and the files a share
/// sends.
#[must_use]
pub fn is_local_path(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        }
        _ => true,
    }
}

/// Whether the last component of `path` names a file that runs, or opens
/// another target, when opened: it ends in one of [`RUNNABLE_EXTENSIONS`]
/// or in a Windows class id (`.{…}`), read as Windows reads the name,
/// without the dots and spaces it drops from a name's end and without the
/// stream a `:` names (`run.exe:data` opens `run.exe`).
///
/// The floor every backend applies to the name of what it opened, beside
/// its own system's judgement ([`LauncherBackend::open_path`]).
#[must_use]
pub fn runs_when_opened(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    let unstreamed = name.split(':').next().unwrap_or_default();
    [name.as_ref(), unstreamed].into_iter().any(|name| {
        name.trim_end_matches(['.', ' '])
            .rsplit_once('.')
            .is_some_and(|(_, extension)| {
                (extension.starts_with('{') && extension.ends_with('}'))
                    || RUNNABLE_EXTENSIONS
                        .iter()
                        .any(|runnable| runnable.eq_ignore_ascii_case(extension))
            })
    })
}
