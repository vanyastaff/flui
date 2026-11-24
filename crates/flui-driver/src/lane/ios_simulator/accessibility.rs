//! The Mac's accessibility API on the Simulator's window, through which the lane reads the iOS
//! app the way VoiceOver on iOS reads it.
//!
//! Simulator.app, and DeviceHub from Xcode 27, vends the booted device's accessibility elements to
//! AppKit as elements of its own window: Apple's `AccessibilityPlatformTranslation` asks the iOS
//! app for each element and serves it as an `NSAccessibilityElement` (`AXPMacPlatformElement`, as
//! `idb`'s class dump of the framework shows), so a client of the public `AXUIElement` API reads
//! the iOS app's elements as the children of a group in the window that is the device's screen,
//! framed in the Mac's global points, and presses them with `AXPress`. `idb` reads the same
//! translation through the framework itself; the lane reads only what the public API serves.
//!
//! [`HostAccessibility`] is that read, kept apart so the lane is held against a scripted Simulator
//! (`testing::ScriptedSimulatorScreen`) on every host; `MacAccessibility` is the Mac's, on
//! macOS. Both hand back the screen's elements as the wire's nodes before any matching: each
//! element's native role (`AXRole` or `AXRole/AXSubrole`), its label (`AXDescription`, else
//! `AXTitle`), value, hint (`AXHelp`) and identifier (`AXIdentifier`), its enabled, focused,
//! selected and checked states, and its frame mapped onto the app's window in device pixels
//! ([`super::tree::window_rect`]). [`super::tree::read`] matches them to the app's own tree.

#[cfg(target_os = "macos")]
mod mac;

use flui_protocol::geometry::LogicalSize;
use flui_protocol::tree::{Node, Tree};

#[cfg(target_os = "macos")]
pub use self::mac::MacAccessibility;
use super::device::Simulator;
use crate::error::{Error, Result};

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

/// Which screen a read is of: the device's window by its title, and the app's window it shows,
/// whose logical size and scale map the Mac's points onto the window's device pixels.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ScreenRequest {
    /// The title of the Simulator's window that shows the device ([`window_title`]).
    pub title: String,
    /// The app's window in points: a UIKit window fills its screen.
    pub size: LogicalSize,
    /// The window's scale.
    pub scale: f64,
}

impl ScreenRequest {
    /// The screen of the window titled `title`, showing an app window of `size` at `scale`.
    #[must_use]
    pub fn new(title: impl Into<String>, size: LogicalSize, scale: f64) -> Self {
        Self {
            title: title.into(),
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

/// What a read answers when no window has the title: the device is not on screen, since the
/// Simulator serves the tree only to its window.
#[must_use]
pub fn no_window(title: &str) -> Error {
    Error::not_found(format!(
        "no window titled {title:?} is on screen: the Simulator serves the device's \
         accessibility tree only through the window that shows it (Simulator.app, or DeviceHub \
         from Xcode 27), so open the device's window and keep it on the desktop"
    ))
}

/// The Mac's accessibility API on the Simulator's window: see the [module docs](self).
pub trait HostAccessibility: Send {
    /// The device's screen in the window `screen` names, as one root (the screen's group) whose
    /// children are the iOS app's elements, each projected as the [module docs](self) say,
    /// within `lane::walk`'s budgets.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when no window has the title or it shows no screen of the app's
    /// window's shape ([`no_window`], [`super::tree::choose_screen`]); [`Error::Ambiguous`] when
    /// two do; [`Error::NotSupported`] naming `macos_permission_accessibility` when the process
    /// is no trusted accessibility client; [`Error::Timeout`] when the Simulator did not answer.
    fn read(&mut self, screen: &ScreenRequest) -> Result<Tree>;

    /// Press, through `AXPress`, the element of `screen` that reads as `element` (its native
    /// role, label, frame and identifier, as [`Self::read`] gave them), as VoiceOver's activate
    /// command does.
    ///
    /// # Errors
    ///
    /// As [`Self::read`]; [`Error::NotFound`] when no element reads as `element` now;
    /// [`Error::ActionUnsupported`] when it offers no press; [`Error::Timeout`] with
    /// `may_have_run` when the press was not answered.
    fn press(&mut self, screen: &ScreenRequest, element: &Node) -> Result<()>;
}

/// Whether `found` is the element `wanted` describes: the same native role, label, frame and
/// identifier, the facts a read gives and a press finds an element by again.
#[must_use]
pub fn same_element(found: &Node, wanted: &Node) -> bool {
    found.native_role == wanted.native_role
        && found.name == wanted.name
        && found.rect == wanted.rect
        && found.automation_id == wanted.automation_id
}
