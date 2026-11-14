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
use std::sync::Arc;

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
///
/// A file the system hands over as an open descriptor rather than a path (an
/// Android document a content provider serves) is held open by the value:
/// its [`path`](Self::path) names the descriptor, which stays readable while
/// any clone of the value lives and closes when the last one drops. Two
/// values are equal when they name the same path by the same name.
#[non_exhaustive]
#[derive(Clone)]
pub struct OpenedFile {
    /// Where the file is: for a held descriptor, the process's own name for
    /// it (`/proc/self/fd/N` on Linux and Android).
    pub path: PathBuf,
    /// What the file is called where it came from.
    name: Option<String>,
    /// What keeps the file reachable at `path`, shared by the clones.
    access: Option<Arc<dyn Held>>,
}

/// What an [`OpenedFile`] holds to keep its file reachable: an open
/// descriptor; it is released by its `Drop`.
trait Held: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> Held for T {}

impl OpenedFile {
    /// The file at `path`, called by its file name.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        Self {
            path,
            name,
            access: None,
        }
    }

    /// The file `descriptor` reads, called `name` where it came from, held
    /// open until the last clone drops. The descriptor is duplicated with
    /// close-on-exec, so no process this one starts inherits it, and the one
    /// passed in is closed.
    ///
    /// # Errors
    ///
    /// The I/O error of the duplication, which closes `descriptor`.
    #[cfg(unix)]
    pub fn try_from_descriptor(
        descriptor: std::os::fd::OwnedFd,
        name: Option<String>,
    ) -> std::io::Result<Self> {
        use std::os::fd::AsRawFd as _;
        // `try_clone` duplicates with `F_DUPFD_CLOEXEC`.
        let held = descriptor.try_clone()?;
        drop(descriptor);
        let path = if cfg!(any(target_os = "linux", target_os = "android")) {
            PathBuf::from(format!("/proc/self/fd/{}", held.as_raw_fd()))
        } else {
            PathBuf::from(format!("/dev/fd/{}", held.as_raw_fd()))
        };
        Ok(Self {
            path,
            name,
            access: Some(Arc::new(held)),
        })
    }

    /// What the file is called where it came from: its file name, or for a
    /// document a provider serves, the name the provider shows; `None` when
    /// it has none that is text.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl PartialEq for OpenedFile {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.name == other.name
    }
}

impl Eq for OpenedFile {}

impl std::fmt::Debug for OpenedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedFile")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("held", &self.access.is_some())
            .finish()
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
