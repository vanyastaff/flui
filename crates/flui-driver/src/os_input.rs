//! OS input on a person's desktop: never the default, always announced, guarded, and one session
//! at a time on a host (ADR-0238 §5).
//!
//! - [`InputPolicy::default`](flui_protocol::input::InputPolicy) is synthetic: a session takes
//!   the OS path only when it asks for it.
//! - [`admit`] holds the isolation rule: in process there is no OS input at all, and on the
//!   shared desktop it runs only with a lane that can tell the user's input from its own.
//! - An [`Announcement`] says, before the first OS event, what the run takes and for how long.
//! - [`OsInputLock`] is the host-wide lock two OS-input sessions on one desktop contend for: the
//!   second is refused `busy` instead of interleaving its events with the first's.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use flui_protocol::session::Isolation;

use crate::error::{Error, Result};

/// The isolation modes that keep OS input away from the person at the machine, which a refusal on
/// the shared desktop names.
const ISOLATED: &str = "a sandbox or a second session (Windows), Xvfb or a nested compositor \
                        (Linux), a second session or a VM (macOS), an emulator (Android), the \
                        Simulator (iOS) or headless Chrome";

/// Whether OS input may run in `isolation` with a lane that has a user-activity guard when
/// `guarded`.
///
/// # Errors
///
/// [`Error::NotSupported`] naming `os_input` in process, where no OS input exists, and naming
/// `user_activity_guard` on the shared desktop without a guard, with the isolation modes to use
/// instead.
pub fn admit(isolation: Isolation, guarded: bool) -> Result<()> {
    match isolation {
        Isolation::InProcess => Err(Error::NotSupported {
            capability: Some("os_input".into()),
            message: "an app hosted in process takes no OS input; attach an OS lane".into(),
        }),
        Isolation::Shared if !guarded => Err(Error::NotSupported {
            capability: Some("user_activity_guard".into()),
            message: format!(
                "this lane cannot tell your input from its own, so it takes no OS input on your \
                 desktop; run it in {ISOLATED}"
            ),
        }),
        _ => Ok(()),
    }
}

/// A device an OS-input run takes over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OsDevice {
    /// The mouse.
    Mouse,
    /// The keyboard.
    Keyboard,
    /// Touch.
    Touch,
    /// A pen.
    Pen,
}

impl OsDevice {
    fn name(self) -> &'static str {
        match self {
            Self::Mouse => "mouse",
            Self::Keyboard => "keyboard",
            Self::Touch => "touch",
            Self::Pen => "pen",
        }
    }
}

/// The line an OS-input run prints before its first event: what it takes, for about how long, and
/// how to stop it. A scenario declares its devices and duration; an ad-hoc call announces its own
/// single action.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Announcement {
    /// What the run takes.
    pub devices: Vec<OsDevice>,
    /// About how long it takes them.
    pub duration: Duration,
    /// Where its input lands.
    pub isolation: Isolation,
}

impl Announcement {
    /// A run taking `devices` for about `duration`, its input landing in `isolation`.
    #[must_use]
    pub fn new(devices: Vec<OsDevice>, duration: Duration, isolation: Isolation) -> Self {
        Self {
            devices,
            duration,
            isolation,
        }
    }
}

impl fmt::Display for Announcement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let devices: Vec<&str> = self.devices.iter().map(|device| device.name()).collect();
        let devices = match devices.as_slice() {
            [] => "no device".to_owned(),
            [one] => (*one).to_owned(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        let seconds = self.duration.as_secs().max(1);
        if self.isolation == Isolation::Shared {
            write!(
                f,
                "OS input: will take {devices} on this desktop for about {seconds} s; move the \
                 mouse or press a key to abort (best effort: isolation is the real protection)"
            )
        } else {
            write!(
                f,
                "OS input: will take {devices} in {} for about {seconds} s",
                self.isolation.name()
            )
        }
    }
}

/// The host-wide lock of an OS-input session: one per desktop at a time.
///
/// An OS file lock on a file in the user's temporary directory, so it is released when the process
/// holding it ends, however it ends.
#[derive(Debug)]
#[must_use = "the lock is released when this is dropped"]
pub struct OsInputLock {
    file: File,
    path: PathBuf,
}

impl OsInputLock {
    /// The lock file of this user's desktop.
    #[must_use]
    pub fn default_path() -> PathBuf {
        std::env::temp_dir().join("flui-driver-os-input.lock")
    }

    /// Take the lock at `path`.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] while another session holds it; [`Error::Io`] when the file cannot be
    /// opened.
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                file,
                path: path.to_owned(),
            }),
            Err(std::fs::TryLockError::WouldBlock) => Err(Error::Busy {
                message: format!(
                    "another session sends OS input on this desktop ({}); one runs at a time",
                    path.display()
                ),
            }),
            Err(std::fs::TryLockError::Error(error)) => Err(Error::Io(error)),
        }
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OsInputLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
