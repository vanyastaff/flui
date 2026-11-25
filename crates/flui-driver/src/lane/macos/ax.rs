//! The app's window as the macOS accessibility API shows it, the tree VoiceOver reads, projected
//! onto the wire's nodes: each element's role folded by [`roles::wire_role`], its name,
//! identifier, value, checked, expanded and selected states, whether it is enabled and focusable,
//! and its bounds in screen device pixels.
//!
//! The calls are the shared client's (`lane::ax`). The window's element is walked through the
//! bounded walk the Windows lane's reads use (`lane::walk`, which is apart from any OS API): a
//! walk cut short says so (`Tree::truncated`), and the search for an identifier is bounded the
//! same way. The first attribute that does not answer within the client's timeout fails the read
//! with a typed timeout instead of a node of defaults.
#![expect(
    unsafe_code,
    reason = "the accessibility API's C calls; each takes a live element and locals it writes"
)]

use std::ptr::NonNull;

use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFBoolean, CFRetained, CFType};

use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::{Checked, Role};

use crate::error::{Error, Result};
use crate::lane::ax::{self, MESSAGING_TIMEOUT_SECONDS, Visit, Walker, read_error, timeout};
use crate::lane::macos::frame;
use crate::lane::macos::pointer::Point;
use crate::lane::macos::press;
use crate::lane::macos::roles;
use crate::lane::macos::screen::{self, PointRect};
use crate::lane::walk::{self, Properties};

/// Whether this process is a trusted accessibility client, asked without a prompt.
pub(super) fn trusted() -> bool {
    ax::trusted()
}

/// One element as a node: see the [module docs](self).
fn describe(element: &AXUIElement, id: ElementId, scale: f64, reads: &mut Properties) -> Node {
    let mut read = |name: &'static str| reads.read(|| ax::attribute(element, name)).flatten();
    let ax_role = read("AXRole")
        .as_deref()
        .and_then(ax::string)
        .unwrap_or_default();
    let subrole = read("AXSubrole").as_deref().and_then(ax::string);
    let role = if ax_role.is_empty() {
        Role::Unknown
    } else {
        roles::wire_role(&ax_role, subrole.as_deref())
    };
    let mut node = Node::new(id, role, roles::native_role(&ax_role, subrole.as_deref()));
    let value = read("AXValue");
    // Static text carries its label as its value (the adapter puts a label there).
    node.name = ax::label(
        role,
        read("AXTitle").as_deref().and_then(ax::string),
        value.as_deref().and_then(ax::string),
    );
    node.automation_id = read("AXIdentifier").as_deref().and_then(ax::string);
    node.description = read("AXHelp").as_deref().and_then(ax::string);
    match role {
        Role::CheckBox | Role::Switch | Role::RadioButton => {
            node.checked = value
                .as_deref()
                .and_then(ax::boolean)
                .map(|on| if on { Checked::True } else { Checked::False });
        }
        Role::Tab => node.selected = value.as_deref().and_then(ax::boolean),
        Role::TextInput | Role::PasswordInput | Role::ComboBox => {
            // An element with a text value has one, the empty text of an empty field included.
            node.value = Some(value.as_deref().and_then(ax::string).unwrap_or_default());
        }
        _ => {}
    }
    if let Some(expanded) = read("AXExpanded").as_deref().and_then(ax::boolean) {
        node.expanded = Some(expanded);
    }
    if node.selected.is_none()
        && let Some(selected) = read("AXSelected").as_deref().and_then(ax::boolean)
    {
        node.selected = Some(selected);
    }
    node.disabled = read("AXEnabled")
        .as_deref()
        .and_then(ax::boolean)
        .is_some_and(|enabled| !enabled);
    node.focused = read("AXFocused")
        .as_deref()
        .and_then(ax::boolean)
        .unwrap_or(false);
    node.focusable = reads
        .read(|| ax::settable(element, "AXFocused"))
        .unwrap_or(false);
    if let Some((x, y, width, height)) = reads.read(|| ax::frame(element)).flatten() {
        node.rect = device_rect(Point::new(x, y), width, height, scale);
    }
    node
}

/// A rectangle at `position` of `width` by `height` points as device pixels at `scale`; `None`
/// for an empty or unaddressable one.
fn device_rect(position: Point, width: f64, height: f64, scale: f64) -> Option<Rect> {
    let top_left = screen::device_of(position, scale)?;
    let bottom_right =
        screen::device_of(Point::new(position.x + width, position.y + height), scale)?;
    let width = u32::try_from(i64::from(bottom_right.x) - i64::from(top_left.x)).ok()?;
    let height = u32::try_from(i64::from(bottom_right.y) - i64::from(top_left.y)).ok()?;
    (width > 0 && height > 0).then_some(Rect {
        x: top_left.x,
        y: top_left.y,
        width,
        height,
    })
}

/// The app `pid`'s element, its replies awaited [`MESSAGING_TIMEOUT_SECONDS`].
fn application(pid: i32) -> CFRetained<AXUIElement> {
    ax::application(pid, MESSAGING_TIMEOUT_SECONDS)
}

/// The app `pid`'s window whose frame is `frame`, among its windows.
fn window(pid: i32, frame: PointRect) -> Result<CFRetained<AXUIElement>> {
    let app = application(pid);
    for candidate in ax::elements(&app, "AXWindows").map_err(read_error)? {
        if let Some((x, y, width, height)) = ax::frame(&candidate).map_err(read_error)?
            && same_frame(PointRect::new(x, y, width, height), frame)
        {
            return Ok(candidate);
        }
    }
    Err(Error::not_found(format!(
        "the accessibility API shows no window of process {pid} at ({}, {}) of {} x {} points",
        frame.x, frame.y, frame.width, frame.height
    )))
}

/// Whether two frames are one window's, the window server and the accessibility API rounding
/// them apart by less than a point.
fn same_frame(a: PointRect, b: PointRect) -> bool {
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.width, b.width),
        (a.height, b.height),
    ]
    .iter()
    .all(|(a, b)| (a - b).abs() < 1.0)
}

/// The accessibility tree of the app `pid`'s window framed by `frame`, at `scale`, projected:
/// see the [module docs](self).
///
/// # Errors
///
/// [`Error::Timeout`] when the app did not answer in time; [`Error::NotFound`] when it shows no
/// such window; [`Error::Platform`] when the API failed otherwise.
pub(super) fn read(pid: i32, frame: PointRect, scale: f64) -> Result<Tree> {
    let root = Visit::root(window(pid, frame)?);
    let mut walker = Walker::new(|element: &AXUIElement, id, reads: &mut Properties| {
        describe(element, id, scale, reads)
    });
    let mut tree = walk::read(&mut walker, &root, timeout())?;
    if let Some(window) = tree.roots.pop() {
        tree.roots.push(frame::project(window));
    }
    Ok(tree)
}

/// Press the element of the app `pid`'s window framed by `frame` whose identifier is
/// `identifier`, through `AXPress`, as VoiceOver's activate command does.
///
/// # Errors
///
/// [`Error::NotFound`] when no element within the walk's budgets has the identifier; then as
/// `lane::ax::pressed`, the answer every `AXPress` gives: [`Error::ActionUnsupported`] when it
/// offers no press, [`Error::Timeout`] and [`Error::Gone`] (with `may_have_run` once the press
/// was sent, whatever the trust) when the app did not answer in time or the element went away,
/// and the Accessibility permission refused when it was taken back before the press was sent.
pub(super) fn press(pid: i32, frame: PointRect, scale: f64, identifier: &str) -> Result<()> {
    let root = Visit::root(window(pid, frame)?);
    let mut walker = Walker::new(|element: &AXUIElement, id, reads: &mut Properties| {
        describe(element, id, scale, reads)
    });
    let found = walk::find(&mut walker, root, identifier, timeout())?;
    press::answer(
        ax::actions(&found.element).map(|names| names.iter().any(|action| action == "AXPress")),
        || ax::perform(&found.element, "AXPress"),
        identifier,
        timeout(),
        trusted,
    )
}

/// The process the system sends keys to now: the focused application's.
pub(super) fn focused_pid() -> Option<i32> {
    // SAFETY: no arguments.
    let system = unsafe { AXUIElement::new_system_wide() };
    let focused = ax::attribute(&system, "AXFocusedApplication").ok()??;
    let focused = focused.downcast_ref::<AXUIElement>()?;
    let mut pid: libc::pid_t = 0;
    // SAFETY: a live element and a local the call writes.
    let error = unsafe { focused.pid(NonNull::from(&mut pid)) };
    (error == AXError::Success).then_some(pid)
}

/// Bring the app `pid` to the front and raise its window framed by `frame`; best effort, since the
/// next safety check reads whether it worked.
pub(super) fn bring_to_front(pid: i32, frame: PointRect) {
    let app = application(pid);
    let frontmost: &CFType = CFBoolean::new(true);
    // SAFETY: a live element, an attribute name and a boolean value.
    let _ = unsafe { app.set_attribute_value(&ax::name("AXFrontmost"), frontmost) };
    if let Ok(window) = window(pid, frame) {
        let _ = ax::perform(&window, "AXRaise");
    }
}
