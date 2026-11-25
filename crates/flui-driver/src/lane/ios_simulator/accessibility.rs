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
//! projection: [`super::tree`] projects and matches them, the same code for both. The walk over
//! the screen's elements is pure too ([`walk_screen`] over a [`ScreenSource`]), so what it marks
//! as unread is the same for the Mac's API and a test's source.

#[cfg(target_os = "macos")]
mod mac;

use std::time::Duration;

use flui_protocol::error::{ErrorCode, HandleKind, Retry};
use flui_protocol::geometry::LogicalSize;

#[cfg(target_os = "macos")]
pub use self::mac::MacAccessibility;
use super::device::{Simulator, Udid};
use crate::error::{Error, Result};
use crate::lane::ax;
use crate::lane::walk::{MAX_DEPTH, MAX_NODES, ProviderError};

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
    /// Whether the read stopped before it had seen all its children: the walk's budget or depth
    /// ran out, or the element went away mid-read ([`walk_screen`]).
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
    /// Whether the read stopped before it had seen all the group's own children, as
    /// [`ScreenElement::children_unread`] says of an element's.
    pub children_unread: bool,
    /// Whether the read stopped short anywhere: a budget, the depth, or an element gone mid-read.
    pub truncated: bool,
}

impl HostScreen {
    /// The screen at `frame` showing `elements`, read whole.
    #[must_use]
    pub fn new(frame: PointFrame, elements: Vec<ScreenElement>) -> Self {
        Self {
            frame,
            elements,
            children_unread: false,
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
    /// The simulator shown: a window that names any other UUID is not its window
    /// ([`window_identity`]).
    pub udid: Udid,
    /// How many other listed simulators share [`Self::title`], whatever their state: while one
    /// does, a window that names no UUID could be its, and is not taken ([`screen_title`]). Only
    /// the default device set is listed (`xcrun simctl list devices`): a simulator of another set
    /// (`simctl --set`, Xcode's previews) is not counted, so its window, when it names no UUID,
    /// is not told apart from the device's.
    pub others_titled: usize,
    /// The app's window in points: a UIKit window fills its screen.
    pub size: LogicalSize,
    /// The window's scale.
    pub scale: f64,
}

impl ScreenRequest {
    /// The screen of simulator `udid`'s window titled `title`, which `others_titled` other listed
    /// simulators share ([`screen_title`] gives both), showing an app window of `size` at `scale`.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        udid: Udid,
        others_titled: usize,
        size: LogicalSize,
        scale: f64,
    ) -> Self {
        Self {
            title: title.into(),
            udid,
            others_titled,
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

/// The title of `target`'s window as `listing`, read now, gives it, and how many other listed
/// simulators share it in any state (DeviceHub keeps a window titled for a simulator that shut
/// down, and one whose runtime went keeps the window it had). `target` is found in `listing` by
/// its UDID, never by the name it had when the lane connected.
///
/// # Errors
///
/// [`Error::NotFound`] when `listing` no longer lists `target`; [`Error::InvalidArgument`] when it
/// lists it under another title than `target`'s, renamed since the lane connected, since the lane
/// would look for a window titled for another device.
pub fn screen_title(listing: &[Simulator], target: &Simulator) -> Result<(String, usize)> {
    let Some(listed) = listing
        .iter()
        .find(|simulator| simulator.udid == target.udid)
    else {
        return Err(Error::not_found(format!(
            "simulator {} is no longer listed by `xcrun simctl list devices`: it was deleted \
             since the lane connected",
            target.udid
        )));
    };
    let title = window_title(listed);
    let connected = window_title(target);
    if title != connected {
        return Err(Error::invalid_argument(format!(
            "simulator {} is titled {title:?} now, not {connected:?} as when the lane connected: \
             start a session again",
            target.udid
        )));
    }
    let others = listing
        .iter()
        .filter(|simulator| simulator.udid != target.udid && window_title(simulator) == title)
        .count();
    Ok((title, others))
}

/// What a window's attributes say of the simulator it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowIdentity {
    /// An attribute names the simulator asked for.
    Named,
    /// An attribute names a UUID that is not the simulator asked for.
    Other,
    /// No attribute names a UUID.
    Unknown,
}

/// What `attributes`, the texts a window answers with (its `AXIdentifier` and `AXDocument`), say
/// of whether it shows simulator `udid`, by an allow-list: a window that names `udid` is its; one
/// that names any other UUID is not, whatever the UUID is (another simulator's, from any device
/// set, a document's), so a window is never taken on a UUID nobody checked; one that names none is
/// `Unknown`, taken only while no other listed simulator shares its title
/// ([`ScreenRequest::others_titled`]). Every run of characters in a UUID's 8-4-4-4-12 form is
/// read as a UUID, whatever stands beside it, a hexadecimal digit included: `udid` found
/// anywhere makes the window `Named`; any other run of the form, and no `udid`, makes it `Other`,
/// so a run glued to the text around it (`SimDevice<UUID>`, `<UUID>abc`) never reads as naming
/// none. A text longer than [`IDENTITY_TEXT_LIMIT`] UTF-16 units is not read at all and names
/// something else, so a window that answers one is taken only when another of its texts names
/// `udid`, and never on a text read in part. Every host's window goes through here. No run has
/// shown which attribute, if any, carries a UDID, or in what form.
#[must_use]
pub fn window_identity(udid: &Udid, attributes: &[Option<String>]) -> WindowIdentity {
    let mut names_other = false;
    for text in attributes.iter().flatten() {
        if text.encode_utf16().nth(IDENTITY_TEXT_LIMIT).is_some() {
            names_other = true;
            continue;
        }
        for run in uuid_runs(text) {
            if run.eq_ignore_ascii_case(udid.as_str()) {
                return WindowIdentity::Named;
            }
            names_other = true;
        }
    }
    if names_other {
        WindowIdentity::Other
    } else {
        WindowIdentity::Unknown
    }
}

/// The length of a UUID's 8-4-4-4-12 form.
const UUID_LEN: usize = 36;

/// Every run of [`UUID_LEN`] characters in `text` in a UUID's 8-4-4-4-12 form, whatever stands
/// beside it, overlapping runs included.
fn uuid_runs(text: &str) -> impl Iterator<Item = &str> + '_ {
    (0..=text.len().saturating_sub(UUID_LEN)).filter_map(move |start| {
        let run = text.get(start..start + UUID_LEN)?;
        run.bytes()
            .enumerate()
            .all(|(at, byte)| {
                if matches!(at, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
            .then_some(run)
    })
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

/// The most UTF-16 units of a window's `AXIdentifier` or `AXDocument` that [`window_identity`]
/// reads, each whole, since it must see all of a text to know which UUIDs it names: far past any
/// document path or identifier, and far past the outline's cut. A longer text names something
/// else, never read in part; a host copies at most one unit more than this, so it can tell.
pub const IDENTITY_TEXT_LIMIT: usize = 64 * 1024;

/// Whether the screen stayed where the read placed its elements: `before`, the screen's frame
/// the search read, and `after`, the same group's frame read once the walk is done. Every
/// element's frame is placed on the app's window from `before`, so a screen moved or resized in
/// between, or one with no frame left, would place them all off by the move.
///
/// # Errors
///
/// [`Error::Busy`] when `after` is not `before`: read again.
pub fn screen_steady(before: PointFrame, after: Option<PointFrame>) -> Result<()> {
    if after == Some(before) {
        return Ok(());
    }
    Err(Error::Busy {
        message: format!(
            "the device's screen moved or was resized while it was read (from {before:?} to \
             {after:?}), so its elements cannot be placed on the app's window: read again"
        ),
        effect: None,
        retry: ErrorCode::Busy.default_retry(),
    })
}

/// What a call of a read that failed with `error` answers. A failure while the process is no
/// trusted accessibility client (`trusted` asked after the failure) is the permission taken back,
/// [`untrusted`], whatever the call said, since `kAXErrorAPIDisabled` is one failure among the
/// API's others; a call that did not answer is a timeout naming `deadline`, the one the read gave
/// the element the call was asked of, or, as `None`, saying that element refused it and kept its
/// own; an element gone is the window gone; any other failure is the platform's.
#[must_use]
pub fn read_failure(error: ProviderError, trusted: bool, deadline: Option<Duration>) -> Error {
    if !trusted {
        return untrusted();
    }
    match error {
        ProviderError::Timeout => Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: None,
            message: format!(
                "the Simulator's window did not answer a call of the read {}",
                ax::deadline_clause(deadline)
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

/// What pressing the element `label` of the Simulator's window answered: `offered`, whether it
/// offers `AXPress`, and then `perform`, the press, made only when it does, with `deadline` the
/// one the read gave this element (`None`: it refused it) and `trusted` asked after a failure.
/// The answer is the one every `AXPress` gives (`lane::ax::pressed`, the macOS lane's too), the
/// refusal for a process no longer trusted being [`untrusted`]: a press already sent and not
/// answered, or whose element went away, keeps `may_have_run` whatever the trust.
///
/// # Errors
///
/// [`Error::NotSupported`] as [`untrusted`]; [`Error::ActionUnsupported`] when it offers no
/// press; [`Error::Timeout`] when a call was not answered and [`Error::Gone`] when the element
/// went away, each with `may_have_run` once the press was sent; [`Error::Platform`] when a call
/// failed otherwise.
pub fn pressed(
    offered: std::result::Result<bool, ProviderError>,
    perform: impl FnOnce() -> std::result::Result<(), ProviderError>,
    label: &str,
    deadline: Option<Duration>,
    trusted: impl Fn() -> bool,
) -> Result<()> {
    ax::pressed(offered, perform, label, deadline, trusted, untrusted)
}

/// The elements of a device's screen as [`walk_screen`] reads them: the Mac's accessibility API,
/// or a test's tree.
pub trait ScreenSource {
    /// One element, with what it takes to read and press it again within the read.
    type Element;

    /// The child of `parent` at `index`; `None` past the last.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Gone`] when `parent` went away, which cuts its children short; any other
    /// failure fails the walk.
    fn child_at(
        &mut self,
        parent: &Self::Element,
        index: usize,
    ) -> std::result::Result<Option<Self::Element>, ProviderError>;

    /// `element`'s attributes as they are served.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Gone`] when the element went away, which leaves it out; any other failure
    /// fails the walk.
    fn element(
        &mut self,
        element: &Self::Element,
    ) -> std::result::Result<ScreenElement, ProviderError>;
}

/// An element a walk read, with its source's handle and its children.
#[derive(Debug)]
pub struct Walked<E> {
    /// What it read, its children left out ([`Self::detached`] puts them in).
    pub element: ScreenElement,
    /// The source's element, to press it within the same read.
    pub handle: E,
    /// Its children, read.
    pub children: Vec<Walked<E>>,
}

impl<E> Walked<E> {
    /// The element with its children, without the handles.
    #[must_use]
    pub fn detached(&self) -> ScreenElement {
        let mut element = self.element.clone();
        element.children = self
            .children
            .iter()
            .map(Self::detached)
            .collect::<Vec<ScreenElement>>();
        element
    }
}

/// What a walk of a screen read: its elements, and what it left unread.
#[derive(Debug)]
pub struct ScreenWalk<E> {
    /// The screen's elements.
    pub elements: Vec<Walked<E>>,
    /// Whether the screen's own children were left unread.
    pub children_unread: bool,
    /// Whether the walk stopped short anywhere: by its budget or depth, or by an element gone.
    pub truncated: bool,
    /// Whether one of the cuts behind `truncated` was an element gone (a parent while its
    /// children were listed, an element while its attributes were read, or one past the depth
    /// before it said whether it has a child): what a source that answers a gone element as a
    /// refusal is asked about.
    pub met_gone: bool,
}

impl<E> ScreenWalk<E> {
    /// The walk as a read of the screen at `frame`, without the handles.
    #[must_use]
    pub fn screen(&self, frame: PointFrame) -> HostScreen {
        let mut screen = HostScreen::new(
            frame,
            self.elements
                .iter()
                .map(Walked::detached)
                .collect::<Vec<ScreenElement>>(),
        );
        screen.children_unread = self.children_unread;
        screen.truncated = self.truncated;
        screen
    }
}

/// A parent's children as [`list_children`] lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildList<E> {
    /// The children listed, in the API's order.
    pub children: Vec<E>,
    /// What left the list short of the parent's children, if anything.
    pub cut: Option<ListCut>,
}

impl<E> ChildList<E> {
    /// Whether the list is short of the parent's children.
    #[must_use]
    pub const fn is_cut(&self) -> bool {
        self.cut.is_some()
    }
}

/// What left a [`ChildList`] short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListCut {
    /// The parent went away before its children were all listed: a read again may list them.
    ParentGone,
    /// A child is there past the bound: a read again with the same bound stops there again.
    Bound,
}

/// The children of a parent, `child_at` asked for each index in turn, at most `bound` of them:
/// the list ends whole when `child_at` answers `None`, and is cut when the parent is gone
/// ([`ProviderError::Gone`]) or a child is there at index `bound`, which is asked before the list
/// is called cut, so a parent of exactly `bound` children is listed whole. A `bound` of zero asks
/// only whether a first child is there. Both the screen's walk ([`walk_screen`]) and the window's
/// search (`tree::find_screen`) list through it.
///
/// # Errors
///
/// Any other failure of `child_at`, as it answered.
pub fn list_children<E>(
    mut child_at: impl FnMut(usize) -> std::result::Result<Option<E>, ProviderError>,
    bound: usize,
) -> std::result::Result<ChildList<E>, ProviderError> {
    let mut children = Vec::new();
    loop {
        let index = children.len();
        let cut = match child_at(index) {
            Ok(Some(child)) if index < bound => {
                children.push(child);
                continue;
            }
            Ok(Some(_)) => Some(ListCut::Bound),
            Err(ProviderError::Gone) => Some(ListCut::ParentGone),
            Ok(None) => None,
            Err(error) => return Err(error),
        };
        return Ok(ChildList { children, cut });
    }
}

/// The elements under `screen`, read from `source` within [`MAX_NODES`] elements and
/// [`MAX_DEPTH`] levels, each level listed by [`list_children`] within what is left of the
/// budget, then each child read in turn. A level stops short the same way whatever stops it, the
/// node budget, its parent gone mid-read, or one of its elements gone while read: the parent's
/// children are marked unread and the walk truncated, never a shorter list read as whole, since
/// an element that went away could have been the twin of one read; an element on the deepest
/// level read has its children marked unread when it has a child or went away before it said.
/// An element that went away while its own attributes were read is left out, its level marked
/// so. Every cut is recorded once, with its cause, and what the walk reports (`truncated`,
/// `met_gone`) is read from those records.
///
/// # Errors
///
/// The first failure of `source` that is neither of those, as it answered.
pub fn walk_screen<S: ScreenSource>(
    source: &mut S,
    screen: &S::Element,
) -> std::result::Result<ScreenWalk<S::Element>, ProviderError> {
    let mut walk = ScreenWalker {
        visited: 0,
        cut_by_bound: false,
        cut_by_gone: false,
    };
    let level = walk.children_of(source, screen, 0)?;
    Ok(ScreenWalk {
        elements: level.children,
        children_unread: level.unread,
        truncated: walk.cut_by_bound || walk.cut_by_gone,
        met_gone: walk.cut_by_gone,
    })
}

/// What cut a level of a walk short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkCut {
    /// The budget, or the depth: a read again cuts it the same way.
    Bound,
    /// An element went away: the parent while its children were listed, an element while it was
    /// read, or one past the depth before it said whether it has a child.
    Gone,
}

impl From<ListCut> for WalkCut {
    fn from(cut: ListCut) -> Self {
        match cut {
            ListCut::ParentGone => Self::Gone,
            ListCut::Bound => Self::Bound,
        }
    }
}

/// A walk's count of elements and the cuts it met, each recorded once by [`Self::cut`].
struct ScreenWalker {
    visited: usize,
    cut_by_bound: bool,
    cut_by_gone: bool,
}

/// One parent's children as a walk read them, and whether some were left unread.
struct Level<E> {
    children: Vec<Walked<E>>,
    unread: bool,
}

impl ScreenWalker {
    /// Record that a level was cut short by `cause`: the one record the walk's `truncated` and
    /// `met_gone` are read from.
    fn cut(&mut self, cause: WalkCut) {
        match cause {
            WalkCut::Bound => self.cut_by_bound = true,
            WalkCut::Gone => self.cut_by_gone = true,
        }
    }

    /// The children of `parent`, `depth` levels under the screen, each with its own.
    fn children_of<S: ScreenSource>(
        &mut self,
        source: &mut S,
        parent: &S::Element,
        depth: usize,
    ) -> std::result::Result<Level<S::Element>, ProviderError> {
        let listed = list_children(
            |index| source.child_at(parent, index),
            MAX_NODES.saturating_sub(self.visited),
        )?;
        self.visited += listed.children.len();
        let mut unread = false;
        if let Some(cut) = listed.cut {
            unread = true;
            self.cut(cut.into());
        }
        let mut children = Vec::with_capacity(listed.children.len());
        for handle in listed.children {
            let mut element = match source.element(&handle) {
                Ok(element) => element,
                // An element that went away while it was read is left out, and its level read
                // short: it could have been the twin of one read.
                Err(ProviderError::Gone) => {
                    unread = true;
                    self.cut(WalkCut::Gone);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let grandchildren = if depth + 1 >= MAX_DEPTH {
                // Past the depth only whether a child is there is asked: one there, or the
                // element gone before it said, leaves its children unread.
                let probe = list_children(|index| source.child_at(&handle, index), 0)?;
                if let Some(cut) = probe.cut {
                    element.children_unread = true;
                    self.cut(cut.into());
                }
                Vec::new()
            } else {
                let level = self.children_of(source, &handle, depth + 1)?;
                element.children_unread = level.unread;
                level.children
            };
            children.push(Walked {
                element,
                handle,
                children: grandchildren,
            });
        }
        Ok(Level { children, unread })
    }
}

/// The Mac's accessibility API on the Simulator's window: see the [module docs](self).
pub trait HostAccessibility: Send {
    /// The device's screen in the window `screen` names: the screen's frame and the iOS app's
    /// elements, read as [`ScreenElement`]s by [`walk_screen`].
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] when no window has the title, every window of it names another UUID,
    /// or it shows no screen of the app's window's shape; [`Error::Ambiguous`] when two windows
    /// name the simulator, or none does and two name no UUID, or the one that names no UUID
    /// shares its title with another listed simulator ([`super::tree::choose_window`]), or two
    /// screens fit ([`super::tree::find_screen`]); [`Error::NotSupported`] naming
    /// `macos_permission_accessibility` when the process is no trusted accessibility client
    /// ([`untrusted`]); [`Error::Timeout`] when the Simulator did not answer; [`Error::Busy`]
    /// when the screen moved, was resized or went away during the read ([`screen_steady`],
    /// [`super::tree::read_screen`]).
    fn read(&mut self, screen: &ScreenRequest) -> Result<HostScreen>;

    /// Press, through `AXPress`, the one element of `screen` that reads as `wanted` (an element a
    /// read gave), as VoiceOver's activate command does.
    ///
    /// # Errors
    ///
    /// As [`Self::read`]; [`Error::NotFound`] when no element reads as `wanted` now, and
    /// [`Error::Ambiguous`] when more than one does, or when one does but the read stopped short,
    /// so a second may sit in the part not read ([`super::tree::locate`]);
    /// [`Error::ActionUnsupported`] when it offers no press; [`Error::Timeout`] with
    /// `may_have_run` when the press was not answered.
    fn press(&mut self, screen: &ScreenRequest, wanted: &ScreenElement) -> Result<()>;
}
