//! What the operating system is: its name, version and architecture.
//!
//! A widget acquires [`Os`] through `ctx.capability::<OsCapability>()`.
//! The user's folders are [`user_dirs`](crate::user_dirs); the application's
//! own storage is the storage capability.

use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{Capability, ProviderContext};

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

/// The operating system as [`Os`] reaches it: it answers at once and reads
/// no file.
pub trait OsBackend: 'static {
    /// What the system is.
    fn info(&self) -> OsInfo;
}

/// Widget-facing system information.
///
/// Owner-thread (`!Send`), cheap to clone.
#[derive(Clone)]
pub struct Os {
    backend: Rc<dyn OsBackend>,
}

impl Os {
    /// A handle over `backend`, for an acquisition, as every handle of the
    /// crate is made; it keeps nothing of the acquisition.
    #[must_use]
    pub fn new(backend: impl OsBackend, _acquisition: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
        }
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
