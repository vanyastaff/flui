//! What the operating system opens the application with: URLs (a link the
//! user followed, a custom-scheme URL another application launched) and
//! files (a document opened with the application), as the host reports them
//! to the application's handler (ADR-0254 §4).
//!
//! The host hands over what the operating system gave it, bounded: what it
//! carries is untrusted input, so the number of items and the length of each
//! URL are checked here, once, for every host, before anything above the host
//! parses them. Parsing and the decision to open belong to the UI runtime
//! (`UiRuntime::open_url`, ADR-0330 §5).

use std::path::PathBuf;

use crate::WindowId;

/// The most URLs and files one [`Opened`] carries; the host drops the rest.
pub const MAX_OPENED_ITEMS: usize = 64;

/// The longest URL, in bytes, an [`Opened`] carries and the UI runtime opens;
/// a longer one is dropped whole, never cut.
pub const MAX_OPENED_URL_LEN: usize = 8192;

/// URLs and files the operating system opened the application with.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The URLs, as the operating system gave them, in its order.
    pub urls: Vec<String>,
    /// The files, in the operating system's order.
    pub files: Vec<OpenedFile>,
    /// Where the operating system addressed them.
    pub target: OpenTarget,
    /// Whether they launched the application or reached it running.
    pub launch: Launch,
}

impl Opened {
    /// What the operating system opened, bounded: an item past
    /// [`MAX_OPENED_ITEMS`] (URLs first, then files) and a URL longer than
    /// [`MAX_OPENED_URL_LEN`] bytes are dropped with a warning that says how
    /// many, never read further.
    #[must_use]
    pub fn new(
        urls: impl IntoIterator<Item = String>,
        files: impl IntoIterator<Item = OpenedFile>,
        target: OpenTarget,
        launch: Launch,
    ) -> Self {
        let mut too_long = 0_usize;
        let mut over = 0_usize;
        let mut kept = 0_usize;
        let mut admit = |is_url_too_long: bool| {
            if is_url_too_long {
                too_long += 1;
                false
            } else if kept == MAX_OPENED_ITEMS {
                over += 1;
                false
            } else {
                kept += 1;
                true
            }
        };
        let urls: Vec<String> = urls
            .into_iter()
            .filter(|url| admit(url.len() > MAX_OPENED_URL_LEN))
            .collect();
        let files: Vec<OpenedFile> = files.into_iter().filter(|_| admit(false)).collect();
        if too_long > 0 || over > 0 {
            tracing::warn!(
                too_long,
                over,
                max_items = MAX_OPENED_ITEMS,
                max_url_len = MAX_OPENED_URL_LEN,
                "the operating system opened more than the application takes; the rest is dropped"
            );
        }
        Self {
            urls,
            files,
            target,
            launch,
        }
    }

    /// Whether it carries neither a URL nor a file.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty() && self.files.is_empty()
    }
}

/// A file the operating system opened the application with.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedFile {
    /// Where the file is.
    pub path: PathBuf,
}

impl OpenedFile {
    /// The file at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

/// Where the operating system addressed what it opened.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenTarget {
    /// The window the operating system gave it to: an iOS scene, an Android
    /// activity.
    Window(WindowId),
    /// The application as a whole: a desktop host leaves the window to the
    /// application, which knows which one was active last.
    Application,
}

/// Whether what was opened launched the application.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Launch {
    /// It started the application: delivered before the first frame.
    Cold,
    /// It reached the application while it ran.
    Running,
}
