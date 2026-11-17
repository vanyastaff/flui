//! The user's folders and what the operating system is.
//!
//! A widget acquires [`Os`] through `ctx.capability::<OsCapability>()`.
//! A folder is asked for and answered later, since finding one may reach
//! the file system (a redirected folder, a settings file); what the system
//! is answers at once.

use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

/// The OS capability: `ctx.capability::<OsCapability>()` hands out an
/// [`Os`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum OsCapability {}

impl Capability for OsCapability {
    type Handle = Os;
    const NAME: &'static str = "os";
}

/// A folder of the user's that the system names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KnownFolder {
    /// The user's home folder.
    Home,
    /// The desktop.
    Desktop,
    /// Documents.
    Documents,
    /// Downloads.
    Downloads,
    /// Pictures.
    Pictures,
    /// Music.
    Music,
    /// Videos.
    Videos,
    /// Where applications keep the user's settings: Windows' roaming
    /// application data, `~/Library/Application Support` on macOS,
    /// `$XDG_CONFIG_HOME` on Linux.
    Config,
    /// Where applications keep the user's data on this machine: Windows'
    /// local application data, `~/Library/Application Support` on macOS,
    /// `$XDG_DATA_HOME` on Linux.
    Data,
    /// Where applications keep what they can make again: Windows' local
    /// application data (Windows names no cache folder of its own),
    /// `~/Library/Caches` on macOS, `$XDG_CACHE_HOME` on Linux.
    Cache,
}

/// Why a folder was not found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FolderError {
    /// The user has no such folder on this system.
    #[error("the user has no such folder")]
    NotFound,
    /// This system names no such folder; the [`Unsupported`] says why.
    #[error("the folder service cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a folder request answers: the folder's absolute path.
pub type FolderResult = Result<PathBuf, FolderError>;

/// A version of the operating system: `major.minor.patch`, where Windows'
/// build number is its `patch` (10.0.26100).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OsVersion {
    /// The major version.
    pub major: u32,
    /// The minor version.
    pub minor: u32,
    /// The patch level, or Windows' build number.
    pub patch: u32,
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The processor architecture the operating system runs on, which an
/// emulated process differs from (an x86-64 build on an Arm64 Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Architecture {
    /// 32-bit x86.
    X86,
    /// x86-64.
    X86_64,
    /// 32-bit Arm.
    Arm,
    /// 64-bit Arm.
    Aarch64,
    /// One the system names and this type does not.
    Other,
}

/// What the operating system is.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OsInfo {
    /// The system's name as its maker writes it ("Windows 11", "macOS").
    pub name: String,
    /// Its version.
    pub version: OsVersion,
    /// The architecture it runs on.
    pub architecture: Architecture,
}

impl OsInfo {
    /// `name` at `version` on `architecture`.
    #[must_use]
    pub fn new(name: impl Into<String>, version: OsVersion, architecture: Architecture) -> Self {
        Self {
            name: name.into(),
            version,
            architecture,
        }
    }
}

/// The operating system as [`Os`] reaches it.
///
/// Owner-local. A folder is answered through its [`Resolver`], found off
/// the owner thread; [`info`](Self::info) answers at once and reads no
/// file.
pub trait OsBackend: 'static {
    /// Find `folder`.
    fn folder(&self, folder: KnownFolder, answer: Resolver<FolderResult>);

    /// What the system is.
    fn info(&self) -> OsInfo;
}

/// Widget-facing folders and system information.
///
/// Owner-thread (`!Send`), cheap to clone.
#[derive(Clone)]
pub struct Os {
    backend: Rc<dyn OsBackend>,
    completions: CompletionSource,
}

impl Os {
    /// A handle over `backend` for the acquisition `ctx`.
    #[must_use]
    pub fn new(backend: impl OsBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Find `folder`. A backend that answers a relative path answers
    /// [`SystemFailure::MalformedAnswer`]: a folder is absolute.
    pub fn folder(&self, folder: KnownFolder) -> Completion<FolderResult> {
        let (completion, caller) = self.completions.pending();
        let (found, answer) = self.completions.pending();
        self.backend.folder(folder, answer);
        let mut caller = Some(caller);
        found.on_complete(move |outcome| {
            let Some(caller) = caller.take() else {
                return;
            };
            match outcome {
                Ok(Ok(path)) if !path.is_absolute() => {
                    let _ = caller.resolve(Err(SystemFailure::MalformedAnswer.into()));
                }
                Ok(result) => drop(caller.resolve(result)),
                // The backend dropped its answer: so does the handle.
                Err(_) => drop(caller),
            }
        });
        completion
    }

    /// What the operating system is.
    #[must_use]
    pub fn info(&self) -> OsInfo {
        self.backend.info()
    }
}

impl fmt::Debug for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Os").finish_non_exhaustive()
    }
}
