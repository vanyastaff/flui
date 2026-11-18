//! Sharing text, a link or files through the system's share sheet.
//!
//! A widget acquires [`Share`] through `ctx.capability::<ShareCapability>()`
//! in a lifecycle hook of a widget in a window: the share sheet is attached
//! to that window.

use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver, WindowRef,
};
use flui_platform::launch::{ExternalUri, is_local_path};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

/// The share capability: `ctx.capability::<ShareCapability>()` hands out a
/// [`Share`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum ShareCapability {}

impl Capability for ShareCapability {
    type Handle = Share;
    const NAME: &'static str = "share";
}

/// What to share: text, a link, files, or several of them, with a title the
/// share sheet shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShareRequest {
    /// What the share sheet calls the content, if set.
    pub title: Option<String>,
    /// Text to share, if set.
    pub text: Option<String>,
    /// A link to share, if set, checked as a launch is
    /// ([`ExternalUri`]).
    pub uri: Option<ExternalUri>,
    /// Files to share: absolute and on a local drive, as
    /// [`is_local_path`] judges a path.
    pub files: Vec<PathBuf>,
}

impl ShareRequest {
    /// The longest text or title a share carries, in bytes.
    pub const MAX_TEXT_LEN: usize = 64 * 1024;

    /// The most files one share carries.
    pub const MAX_FILES: usize = 64;

    /// Nothing to share yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the share sheet calls the content.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Share `text`.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Share the link `uri`.
    #[must_use]
    pub fn uri(mut self, uri: ExternalUri) -> Self {
        self.uri = Some(uri);
        self
    }

    /// Also share the file at `path`.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>) -> Self {
        self.files.push(path.into());
        self
    }

    /// The first thing wrong with the request, which no share sheet is
    /// shown for.
    fn check(&self) -> Result<(), MalformedShare> {
        if self.text.is_none() && self.uri.is_none() && self.files.is_empty() {
            return Err(MalformedShare::Empty);
        }
        let is_text = |text: &String| text.len() <= Self::MAX_TEXT_LEN && !text.contains('\0');
        if !self.title.as_ref().is_none_or(is_text) || !self.text.as_ref().is_none_or(is_text) {
            return Err(MalformedShare::Text);
        }
        if self.files.len() > Self::MAX_FILES {
            return Err(MalformedShare::TooManyFiles);
        }
        for path in &self.files {
            check_file(path)?;
        }
        Ok(())
    }
}

/// A file to share is absolute and local.
fn check_file(path: &Path) -> Result<(), MalformedShare> {
    if !path.is_absolute() {
        Err(MalformedShare::RelativePath)
    } else if !is_local_path(path) {
        Err(MalformedShare::NotLocal)
    } else {
        Ok(())
    }
}

/// What is wrong with a [`ShareRequest`] that [`Share`] refuses before the
/// system is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedShare {
    /// The request carries neither text, a link nor a file.
    #[error("the share carries nothing")]
    Empty,
    /// The title or the text is longer than
    /// [`ShareRequest::MAX_TEXT_LEN`] or holds a NUL.
    #[error("the share's title or text cannot be sent as written")]
    Text,
    /// The request carries more than [`ShareRequest::MAX_FILES`] files.
    #[error("the share carries too many files")]
    TooManyFiles,
    /// A file's path is relative.
    #[error("a shared file's path is not absolute")]
    RelativePath,
    /// A file is not on a local drive: sending it could make the system
    /// connect to a server with the user's credentials.
    #[error("a shared file is not local")]
    NotLocal,
}

/// Why a share did not reach the share sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShareError {
    /// The request is malformed, as the [`MalformedShare`] says. Nothing was
    /// shown.
    #[error("the share request is malformed: {0}")]
    InvalidRequest(#[from] MalformedShare),
    /// The window the share sheet would attach to has closed. Nothing was
    /// shown.
    #[error("the window closed")]
    WindowClosed,
    /// A shared file does not exist.
    #[error("a shared file does not exist")]
    NotFound,
    /// The system has no share sheet, or cannot share this content; the
    /// [`Unsupported`] says why.
    #[error("the share service cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a share answers: `Ok` once the system's share sheet has the content.
/// The user picking a target, or closing the sheet, is not reported: not
/// every system tells.
pub type ShareResult = Result<(), ShareError>;

/// The system's share sheet as [`Share`] reaches it.
///
/// Owner-local. A request it receives is well formed and its window is
/// open; it answers through the [`Resolver`] once the sheet has the content
/// or the system refused. Whatever touches the file system runs off the
/// owner thread.
pub trait ShareBackend: 'static {
    /// Show the share sheet attached to `window` with `request`'s content.
    fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>);
}

/// Widget-facing sharing for one window.
///
/// Owner-thread (`!Send`), cheap to clone. A share answers with a
/// [`Completion`], which settles at an owner turn after the system answers.
#[derive(Clone)]
pub struct Share {
    backend: Rc<dyn ShareBackend>,
    completions: CompletionSource,
    window: WindowRef,
}

impl Share {
    /// A handle over `backend` for the acquisition `ctx`, bound to the
    /// acquiring widget's window.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] with [`UnsupportedReason::NoWindow`] when the widget
    /// is in no window: the share sheet is attached to one.
    pub fn try_new(
        backend: impl ShareBackend,
        ctx: &ProviderContext<'_>,
    ) -> Result<Self, Unsupported> {
        let window = ctx.window().ok_or(Unsupported::of::<ShareCapability>(
            UnsupportedReason::NoWindow,
        ))?;
        Ok(Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
            window,
        })
    }

    /// Show the share sheet with `request`'s content.
    ///
    /// Refused without asking the system: a malformed request
    /// ([`ShareError::InvalidRequest`]) and a closed window
    /// ([`ShareError::WindowClosed`]).
    pub fn share(&self, request: ShareRequest) -> Completion<ShareResult> {
        let (completion, answer) = self.completions.pending();
        let refusal = match request.check() {
            Err(malformed) => Some(ShareError::InvalidRequest(malformed)),
            Ok(()) if self.window.lifetime().is_closed() => Some(ShareError::WindowClosed),
            Ok(()) => None,
        };
        match refusal {
            // The owner retiring first leaves nothing waiting for the answer.
            Some(error) => drop(answer.resolve(Err(error))),
            None => self.backend.share(&self.window, request, answer),
        }
        completion
    }
}

impl fmt::Debug for Share {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Share")
            .field("window", &self.window.id())
            .finish_non_exhaustive()
    }
}
