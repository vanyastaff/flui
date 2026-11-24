//! The Mac's accessibility API on the Simulator's window, through which the lane reads the iOS
//! app the way the Simulator shows it to the Mac.
//!
//! Simulator.app, and DeviceHub from Xcode 27, vends the booted device's accessibility elements to
//! AppKit as elements of its own window: Apple's `AccessibilityPlatformTranslation` asks the iOS
//! app for each element and serves it as an `NSAccessibilityElement` (`AXPMacPlatformElement`, as
//! `idb`'s class dump of the framework shows), so a client of the public `AXUIElement` API reads
//! the iOS app's elements as the children of a group in the window that is the device's screen,
//! framed in the Mac's global points, and presses them with `AXPress`. `idb` reads the same
//! translation through the framework itself; the lane reads only what the public API serves.
//! Apple documents none of it: what the bridge serves is measured by others and pinned by no run
//! of this lane yet (`docs/gaps.md`).
//!
//! [`HostAccessibility`] is that read, kept apart so the lane is held against a scripted Simulator
//! (`testing::ScriptedSimulatorScreen`) on every host; `MacAccessibility` is the Mac's, on macOS.
//! Both hand back the screen's elements as the API serves them ([`ScreenElement`]), before any
//! projection: [`super::tree`] projects and matches them, the same code for both.

#[cfg(target_os = "macos")]
mod mac;

use std::time::Duration;

use flui_protocol::error::{ErrorCode, HandleKind, Retry};
use flui_protocol::geometry::LogicalSize;

#[cfg(target_os = "macos")]
pub use self::mac::MacAccessibility;
use super::device::{Simulator, Udid};
use crate::error::{Error, Result};
use crate::lane::walk::ProviderError;

/// A rectangle in the Mac's global points: an element's `AXPosition` and `AXSize`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointFrame {
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width.
    pub width: f64,
    /// The height.
    pub height: f64,
}

impl PointFrame {
    /// The rectangle at `(x, y)` of `width` by `height` points.
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the rectangle is one an element can be placed by: finite, with an area.
    #[must_use]
    pub fn is_addressable(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

/// One element of the device's screen as the Mac's accessibility API serves it: its attributes
/// read as they are, before the lane names, places or matches it.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ScreenElement {
    /// `AXRole`.
    pub role: String,
    /// `AXSubrole`, if any.
    pub subrole: Option<String>,
    /// `AXDescription`: where the bridge puts an iOS element's label.
    pub description: Option<String>,
    /// `AXTitle`.
    pub title: Option<String>,
    /// `AXValue`, as text: a number as its digits, a flag as `1` or `0`.
    pub value: Option<String>,
    /// `AXHelp`: where the bridge puts an iOS element's hint.
    pub help: Option<String>,
    /// `AXIdentifier`.
    pub identifier: Option<String>,
    /// `AXEnabled`, when the element answers it.
    pub enabled: Option<bool>,
    /// `AXFocused`.
    pub focused: bool,
    /// `AXSelected`, when the element answers it.
    pub selected: Option<bool>,
    /// `AXPosition` and `AXSize`, when it has both.
    pub frame: Option<PointFrame>,
    /// Its children, in the API's order.
    pub children: Vec<ScreenElement>,
    /// Whether the read's budget left children of it unread.
    pub children_unread: bool,
}

impl ScreenElement {
    /// An element of `role` with nothing else read.
    #[must_use]
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            ..Self::default()
        }
    }
}

/// The device's screen as a read gives it: the screen's group's frame and the elements under it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HostScreen {
    /// The screen's group's frame in the Mac's points.
    pub frame: PointFrame,
    /// The group's children.
    pub elements: Vec<ScreenElement>,
    /// Whether the read's budget cut it short.
    pub truncated: bool,
}

impl HostScreen {
    /// The screen at `frame` showing `elements`, read whole.
    #[must_use]
    pub fn new(frame: PointFrame, elements: Vec<ScreenElement>) -> Self {
        Self {
            frame,
            elements,
            truncated: false,
        }
    }
}

/// Which screen a read is of: the device's window by its title and its simulator, and the app's
/// window it shows, whose logical size and scale map the Mac's points onto the window's device
/// pixels.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ScreenRequest {
    /// The title of the Simulator's window that shows the device ([`window_title`]).
    pub title: String,
    /// The simulator shown, which a window naming another is not ([`window_identity`]).
    pub udid: Udid,
    /// The app's window in points: a UIKit window fills its screen.
    pub size: LogicalSize,
    /// The window's scale.
    pub scale: f64,
}

impl ScreenRequest {
    /// The screen of simulator `udid`'s window titled `title`, showing an app window of `size`
    /// at `scale`.
    #[must_use]
    pub fn new(title: impl Into<String>, udid: Udid, size: LogicalSize, scale: f64) -> Self {
        Self {
            title: title.into(),
            udid,
            size,
            scale,
        }
    }
}

/// The title the Simulator gives the window that shows `simulator`: its name and its runtime,
/// apart by an en dash (`iPhone 16 – iOS 18.0`), the form Simulator.app titles a device window
/// with and DeviceHub keeps.
#[must_use]
pub fn window_title(simulator: &Simulator) -> String {
    format!("{} \u{2013} {}", simulator.name, simulator.runtime_name())
}

/// Refuse a title that names more than one listed simulator: the window is found by its title, and
/// no attribute a run has confirmed carries its simulator's UDID, so a title that `simulators`
/// gives two of (any state: DeviceHub keeps a window titled for a simulator that shut down) could
/// show another simulator's app.
///
/// # Errors
///
/// [`Error::Ambiguous`] naming the simulators that share `target`'s title.
pub fn check_title_names_one(simulators: &[Simulator], target: &Simulator) -> Result<()> {
    let title = window_title(target);
    let sharing: Vec<String> = simulators
        .iter()
        .filter(|simulator| window_title(simulator) == title)
        .map(|simulator| format!("{} ({})", simulator.udid, simulator.state))
        .collect();
    if sharing.len() <= 1 {
        return Ok(());
    }
    Err(Error::Ambiguous {
        message: format!(
            "{} simulators are titled {title:?}, and the lane finds a device's window by its \
             title: rename all but {} (`xcrun simctl rename`)",
            sharing.len(),
            target.udid
        ),
        candidates: sharing,
        retry: ErrorCode::Ambiguous.default_retry(),
    })
}

/// What a window's attributes say of the simulator it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowIdentity {
    /// An attribute names the simulator asked for.
    Named,
    /// An attribute names another simulator's UDID.
    Other,
    /// No attribute names a UDID.
    Unknown,
}

/// What `attributes`, the texts a window answers with (its `AXIdentifier` and `AXDocument`), say
/// of whether it shows simulator `udid`: a UDID among them is the window's simulator's. No run has
/// shown which attribute, if any, carries one, so a window naming none is `Unknown`, and the
/// title's uniqueness ([`check_title_names_one`]) stands for it.
#[must_use]
pub fn window_identity(udid: &Udid, attributes: &[Option<String>]) -> WindowIdentity {
    let mut named_other = false;
    for text in attributes.iter().flatten() {
        for word in text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')) {
            let Ok(found) = Udid::try_new(word) else {
                continue;
            };
            if found == *udid {
                return WindowIdentity::Named;
            }
            named_other = true;
        }
    }
    if named_other {
        WindowIdentity::Other
    } else {
        WindowIdentity::Unknown
    }
}

/// What a read answers when no window has the title: the device is not on screen, since the
/// Simulator serves the tree only to its window.
pub(crate) fn no_window(title: &str) -> Error {
    Error::not_found(format!(
        "no window titled {title:?} is on screen: the Simulator serves the device's \
         accessibility tree only through the window that shows it (Simulator.app, or DeviceHub \
         from Xcode 27), so open the device's window and keep it on the desktop; a window \
         titled in another language, or for a patch or beta runtime the listing names otherwise, \
         is not found either"
    ))
}

/// What a process that is no trusted accessibility client is refused with, wherever the API says
/// so: before a read, or by a call that answers `kAXErrorAPIDisabled` mid-read, the permission
/// taken back while the lane ran. The capability is the one the macOS lane names for the same
/// permission.
#[must_use]
pub fn untrusted() -> Error {
    Error::not_supported(
        "macos_permission_accessibility",
        "the process running the driver has no Accessibility permission, which reading the \
         Simulator's window takes; grant it under System Settings > Privacy & Security > \
         Accessibility, then start the process again",
    )
}

/// What a call of a read that failed with `error` answers. A failure while the process is no
/// trusted accessibility client (`trusted` asked after the failure) is the permission taken back,
/// [`untrusted`], whatever the call said, since `kAXErrorAPIDisabled` is one failure among the
/// API's others; a call that did not answer is a timeout naming `deadline`, the deadline each call
/// of the read ran under; an element gone is the window gone; any other failure is the platform's.
#[must_use]
pub fn read_failure(error: ProviderError, trusted: bool, deadline: Duration) -> Error {
    if !trusted {
        return untrusted();
    }
    match error {
        ProviderError::Timeout => Error::Timeout {
            effect: None,
            message: format!(
                "the Simulator's window did not answer a call of the read within {} ms",
                deadline.as_millis()
            ),
            retry: ErrorCode::Timeout.default_retry(),
        },
        ProviderError::Gone => Error::gone(
            HandleKind::Window,
            None,
            None,
            "the Simulator's window went away while it was read",
        ),
        ProviderError::Failed(why) => Error::Platform {
            effect: None,
            message: format!("the Simulator's window could not be read: {why}"),
            retry: Retry::Never,
        },
    }
}

/// The Mac's accessibility API on the Simulator's window: see the [module docs](self).
pub trait HostAccessibility: Send {
    /// The device's screen in the window `screen` names: the screen's frame and the iOS app's
    /// elements, read as [`ScreenElement`]s within `lane::walk`'s budgets.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when no window has the title or it shows no screen of the app's
    /// window's shape; [`Error::Ambiguous`] when two windows have the title or two screens fit
    /// ([`super::tree::find_screen`]); [`Error::NotSupported`] naming
    /// `macos_permission_accessibility` when the process is no trusted accessibility client
    /// ([`untrusted`]); [`Error::Timeout`] when the Simulator did not answer.
    fn read(&mut self, screen: &ScreenRequest) -> Result<HostScreen>;

    /// Press, through `AXPress`, the one element of `screen` that reads as `wanted` (an element a
    /// read gave), as VoiceOver's activate command does.
    ///
    /// # Errors
    ///
    /// As [`Self::read`]; [`Error::NotFound`] when no element reads as `wanted` now, and
    /// [`Error::Ambiguous`] when more than one does ([`super::tree::locate`]);
    /// [`Error::ActionUnsupported`] when it offers no press; [`Error::Timeout`] with
    /// `may_have_run` when the press was not answered.
    fn press(&mut self, screen: &ScreenRequest, wanted: &ScreenElement) -> Result<()>;
}
