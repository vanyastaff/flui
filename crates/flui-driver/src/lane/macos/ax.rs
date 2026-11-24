//! The app's window as the macOS accessibility API shows it, the tree VoiceOver reads, projected
//! onto the wire's nodes: each element's role folded by [`roles::wire_role`], its name,
//! identifier, value, checked, expanded and selected states, whether it is enabled and focusable,
//! and its bounds in screen device pixels.
//!
//! The window's element is walked through the bounded walk the Windows lane's reads use
//! (`lane::walk`, which is apart from any OS API): a walk cut short says so
//! (`Tree::truncated`), and the search for an identifier is bounded the same way. The client
//! waits [`MESSAGING_TIMEOUT_SECONDS`] for the app, and the first attribute that does not answer
//! fails the read with a typed timeout instead of a node of defaults. Text is cut while it is
//! read, to [`OUTLINE_TEXT_LIMIT`] characters, where the projection cuts the app's. Accessibility
//! elements live for one call on the lane's thread, so the lane stays `Send`.
#![expect(
    unsafe_code,
    reason = "the accessibility API's C calls; each takes a live element and locals it writes"
)]

use std::ptr::NonNull;
use std::time::Duration;

use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType, CGPoint, CGSize,
    ConcreteType,
};

use flui_protocol::tree::{ElementId, Node, OUTLINE_TEXT_LIMIT, Rect, Tree};
use flui_protocol::wire::{Checked, Role};

use crate::error::{Error, Result};
use crate::lane::macos::frame;
use crate::lane::macos::pointer::Point;
use crate::lane::macos::roles;
use crate::lane::macos::screen::{self, PointRect};
use crate::lane::walk::{self, Properties, Provider, ProviderError};

/// How long the client waits for the app to answer one call, in seconds.
pub const MESSAGING_TIMEOUT_SECONDS: f32 = 5.0;

fn timeout() -> Duration {
    Duration::from_secs_f32(MESSAGING_TIMEOUT_SECONDS)
}

/// Whether this process is a trusted accessibility client, asked without a prompt.
pub(super) fn trusted() -> bool {
    // SAFETY: no arguments; the call reads this process's trust.
    unsafe { AXIsProcessTrusted() }
}

/// What an accessibility error means for a read.
fn classify(error: AXError) -> ProviderError {
    match error {
        AXError::CannotComplete => ProviderError::Timeout,
        AXError::InvalidUIElement => ProviderError::Gone,
        AXError::APIDisabled => ProviderError::Failed(
            "the accessibility API is off for this process: it is not trusted".to_owned(),
        ),
        other => ProviderError::Failed(format!("the accessibility API answered {}", other.0)),
    }
}

/// An attribute's name, as the accessibility API spells it.
fn name(attribute: &'static str) -> CFRetained<CFString> {
    CFString::from_static_str(attribute)
}

/// `attribute` of `element`; `Ok(None)` when the element has no such attribute or no value.
fn attribute(
    element: &AXUIElement,
    attribute: &'static str,
) -> std::result::Result<Option<CFRetained<CFType>>, ProviderError> {
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: a live element, an attribute name, and a local the call writes a +1 value into.
    let error =
        unsafe { element.copy_attribute_value(&name(attribute), NonNull::from(&mut value)) };
    match error {
        AXError::Success => Ok(NonNull::new(value.cast_mut()).map(|value| {
            // SAFETY: a `Copy` call's value, owned by the caller with its +1 count.
            unsafe { CFRetained::from_raw(value) }
        })),
        AXError::NoValue | AXError::AttributeUnsupported => Ok(None),
        error => Err(classify(error)),
    }
}

fn string(value: &CFType) -> Option<String> {
    value.downcast_ref::<CFString>().and_then(text)
}

/// The first [`OUTLINE_TEXT_LIMIT`] characters of `value`, copied only that far; `None` for an
/// empty one.
fn text(value: &CFString) -> Option<String> {
    // Two UTF-16 units hold any character, so this many units hold the characters kept.
    let limit = CFIndex::try_from(OUTLINE_TEXT_LIMIT * 2).unwrap_or(CFIndex::MAX);
    let length = value.length();
    let text: String = if length > limit {
        // SAFETY: the range starts at the string's first unit and ends inside it, since the
        // string is longer than `limit` units.
        let cut = unsafe { CFString::with_substring(None, Some(value), CFRange::new(0, limit)) };
        match cut {
            Some(cut) => cut.to_string(),
            None => String::new(),
        }
    } else {
        value.to_string()
    };
    let text: String = text.chars().take(OUTLINE_TEXT_LIMIT).collect();
    (!text.is_empty()).then_some(text)
}

fn boolean(value: &CFType) -> Option<bool> {
    if let Some(flag) = value.downcast_ref::<CFBoolean>() {
        return Some(flag.as_bool());
    }
    value
        .downcast_ref::<CFNumber>()
        .and_then(CFNumber::as_i64)
        .map(|n| n != 0)
}

fn point(value: &CFType) -> Option<Point> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut point = CGPoint::new(0.0, 0.0);
    // SAFETY: a live value and a local point the call writes when the value holds one.
    let read = unsafe { value.value(AXValueType::CGPoint, NonNull::from(&mut point).cast()) };
    read.then_some(Point::new(point.x, point.y))
}

fn size(value: &CFType) -> Option<(f64, f64)> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut size = CGSize::new(0.0, 0.0);
    // SAFETY: a live value and a local size the call writes when the value holds one.
    let read = unsafe { value.value(AXValueType::CGSize, NonNull::from(&mut size).cast()) };
    read.then_some((size.width, size.height))
}

/// The items of `array` that are `T`s, each retained; the others are left out.
fn items<T: ConcreteType>(array: CFRetained<CFArray>) -> Vec<CFRetained<T>> {
    // SAFETY: an array the accessibility API returns holds Core Foundation objects, and each is
    // checked against `T`'s type before it is read as one.
    let array = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(array) };
    array
        .iter()
        .filter_map(|item| item.downcast::<T>().ok())
        .collect::<Vec<CFRetained<T>>>()
}

/// The elements `attribute` of `element` lists; none when it lists nothing.
fn elements(
    element: &AXUIElement,
    attribute_name: &'static str,
) -> std::result::Result<Vec<CFRetained<AXUIElement>>, ProviderError> {
    let listed =
        attribute(element, attribute_name)?.and_then(|value| value.downcast::<CFArray>().ok());
    Ok(match listed {
        Some(array) => items::<AXUIElement>(array),
        None => Vec::new(),
    })
}

/// The child of `parent` at `index`, read alone (`AXUIElementCopyAttributeValues` for one value),
/// so a walk copies no more children than its budget visits; `None` past the last child, which
/// the API answers as an index outside the array.
fn child_at(
    parent: &AXUIElement,
    index: CFIndex,
) -> std::result::Result<Option<CFRetained<AXUIElement>>, ProviderError> {
    let mut values: *const CFArray = std::ptr::null();
    // SAFETY: a live element, an attribute name, a range of one value, and a local the call writes
    // a +1 array into.
    let error = unsafe {
        parent.copy_attribute_values(&name("AXChildren"), index, 1, NonNull::from(&mut values))
    };
    match error {
        AXError::Success => {}
        AXError::NoValue | AXError::AttributeUnsupported | AXError::IllegalArgument => {
            return Ok(None);
        }
        error => return Err(classify(error)),
    }
    let Some(values) = NonNull::new(values.cast_mut()) else {
        return Ok(None);
    };
    // SAFETY: a `Copy` call's array, owned by the caller with its +1 count.
    let values: CFRetained<CFArray> = unsafe { CFRetained::from_raw(values) };
    Ok(items::<AXUIElement>(values).into_iter().next())
}

/// An element as the walk visits it: the element, its parent and its index among the parent's
/// children (none for the window, the walk's root).
struct Visit {
    element: CFRetained<AXUIElement>,
    parent: Option<CFRetained<AXUIElement>>,
    index: CFIndex,
}

/// The accessibility API as the walk reads it, for one read at `scale`.
struct Ax {
    scale: f64,
}

impl Provider for Ax {
    type Element = Visit;

    fn first_child(&mut self, visit: &Visit) -> std::result::Result<Option<Visit>, ProviderError> {
        Ok(child_at(&visit.element, 0)?.map(|element| Visit {
            element,
            parent: Some(visit.element.clone()),
            index: 0,
        }))
    }

    fn next_sibling(&mut self, visit: &Visit) -> std::result::Result<Option<Visit>, ProviderError> {
        let Some(parent) = &visit.parent else {
            return Ok(None);
        };
        let Some(index) = visit.index.checked_add(1) else {
            return Ok(None);
        };
        Ok(child_at(parent, index)?.map(|element| Visit {
            element,
            parent: Some(parent.clone()),
            index,
        }))
    }

    fn node(&mut self, visit: &Visit, id: ElementId) -> std::result::Result<Node, ProviderError> {
        let mut reads = Properties::new();
        let node = describe(&visit.element, id, self.scale, &mut reads);
        reads.finish(node)
    }
}

/// One element as a node: see the [module docs](self).
fn describe(element: &AXUIElement, id: ElementId, scale: f64, reads: &mut Properties) -> Node {
    let mut read = |name: &'static str| reads.read(|| attribute(element, name)).flatten();
    let ax_role = read("AXRole")
        .as_deref()
        .and_then(string)
        .unwrap_or_default();
    let subrole = read("AXSubrole").as_deref().and_then(string);
    let role = if ax_role.is_empty() {
        Role::Unknown
    } else {
        roles::wire_role(&ax_role, subrole.as_deref())
    };
    let mut node = Node::new(id, role, roles::native_role(&ax_role, subrole.as_deref()));
    let value = read("AXValue");
    // Static text carries its label as its value (the adapter puts a label there).
    node.name = read("AXTitle").as_deref().and_then(string).or_else(|| {
        (role == Role::Label)
            .then(|| value.as_deref().and_then(string))
            .flatten()
    });
    node.automation_id = read("AXIdentifier").as_deref().and_then(string);
    node.description = read("AXHelp").as_deref().and_then(string);
    match role {
        Role::CheckBox | Role::Switch | Role::RadioButton => {
            node.checked = value
                .as_deref()
                .and_then(boolean)
                .map(|on| if on { Checked::True } else { Checked::False });
        }
        Role::Tab => node.selected = value.as_deref().and_then(boolean),
        Role::TextInput | Role::PasswordInput | Role::ComboBox => {
            // An element with a text value has one, the empty text of an empty field included.
            node.value = Some(value.as_deref().and_then(string).unwrap_or_default());
        }
        _ => {}
    }
    if let Some(expanded) = read("AXExpanded").as_deref().and_then(boolean) {
        node.expanded = Some(expanded);
    }
    if node.selected.is_none()
        && let Some(selected) = read("AXSelected").as_deref().and_then(boolean)
    {
        node.selected = Some(selected);
    }
    node.disabled = read("AXEnabled")
        .as_deref()
        .and_then(boolean)
        .is_some_and(|enabled| !enabled);
    node.focused = read("AXFocused")
        .as_deref()
        .and_then(boolean)
        .unwrap_or(false);
    node.focusable = reads
        .read(|| settable(element, "AXFocused"))
        .unwrap_or(false);
    let position = reads
        .read(|| attribute(element, "AXPosition"))
        .flatten()
        .as_deref()
        .and_then(point);
    let extent = reads
        .read(|| attribute(element, "AXSize"))
        .flatten()
        .as_deref()
        .and_then(size);
    if let (Some(position), Some((width, height))) = (position, extent) {
        node.rect = device_rect(position, width, height, scale);
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

/// Whether `attribute` of `element` can be set.
fn settable(
    element: &AXUIElement,
    attribute: &'static str,
) -> std::result::Result<bool, ProviderError> {
    let mut settable: u8 = 0;
    // SAFETY: a live element, an attribute name, and a local the call writes.
    let error =
        unsafe { element.is_attribute_settable(&name(attribute), NonNull::from(&mut settable)) };
    match error {
        AXError::Success => Ok(settable != 0),
        AXError::NoValue | AXError::AttributeUnsupported => Ok(false),
        error => Err(classify(error)),
    }
}

/// The app `pid`'s element, its replies awaited [`MESSAGING_TIMEOUT_SECONDS`].
fn application(pid: i32) -> CFRetained<AXUIElement> {
    // SAFETY: any process id names an element; one with no app behind it answers every call
    // with an error.
    let app = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: a live element and a positive timeout. A refused timeout leaves the default one.
    let _ = unsafe { app.set_messaging_timeout(MESSAGING_TIMEOUT_SECONDS) };
    app
}

fn read_error(error: ProviderError) -> Error {
    match error {
        ProviderError::Timeout => Error::Timeout {
            awaited: crate::error::Awaited::Host,
            effect: None,
            message: format!(
                "the app's accessibility API did not answer within {} ms: the app may be hung",
                timeout().as_millis()
            ),
            retry: flui_protocol::error::ErrorCode::Timeout.default_retry(),
        },
        ProviderError::Gone => Error::gone(
            flui_protocol::error::HandleKind::Window,
            None,
            None,
            "the app's window went away while it was read",
        ),
        ProviderError::Failed(why) => Error::Platform {
            effect: None,
            message: format!("the OS accessibility tree could not be read: {why}"),
            retry: flui_protocol::error::Retry::Never,
        },
    }
}

/// The app `pid`'s window whose frame is `frame`, among its windows.
fn window(pid: i32, frame: PointRect) -> Result<CFRetained<AXUIElement>> {
    let app = application(pid);
    for candidate in elements(&app, "AXWindows").map_err(read_error)? {
        let position = attribute(&candidate, "AXPosition")
            .map_err(read_error)?
            .as_deref()
            .and_then(point);
        let extent = attribute(&candidate, "AXSize")
            .map_err(read_error)?
            .as_deref()
            .and_then(size);
        if let (Some(position), Some((width, height))) = (position, extent)
            && same_frame(PointRect::new(position.x, position.y, width, height), frame)
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
    let root = Visit {
        element: window(pid, frame)?,
        parent: None,
        index: 0,
    };
    let mut tree = walk::read(&mut Ax { scale }, &root, timeout())?;
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
/// [`Error::NotFound`] when no element within the walk's budgets has the identifier;
/// [`Error::ActionUnsupported`] when it offers no press; [`Error::Timeout`] (with `may_have_run`
/// for the press itself) when the app did not answer in time; [`Error::Gone`] when the element
/// went away.
pub(super) fn press(pid: i32, frame: PointRect, scale: f64, identifier: &str) -> Result<()> {
    let root = Visit {
        element: window(pid, frame)?,
        parent: None,
        index: 0,
    };
    let found = walk::find(&mut Ax { scale }, root, identifier, timeout())?;
    let offered = actions(&found.element)
        .map(|names| names.iter().any(|action| action == "AXPress").then_some(()));
    walk::pattern(offered, identifier, "AXPress", timeout())?;
    // SAFETY: a live element and an action name.
    let error = unsafe { found.element.perform_action(&name("AXPress")) };
    let answer = match error {
        AXError::Success => Ok(()),
        error => Err(classify(error)),
    };
    walk::invoked(answer, identifier, timeout())
}

/// The actions `element` offers, by name.
fn actions(element: &AXUIElement) -> std::result::Result<Vec<String>, ProviderError> {
    let mut names: *const CFArray = std::ptr::null();
    // SAFETY: a live element and a local the call writes a +1 array into.
    let error = unsafe { element.copy_action_names(NonNull::from(&mut names)) };
    match error {
        AXError::Success => {}
        AXError::NoValue | AXError::ActionUnsupported => return Ok(Vec::new()),
        error => return Err(classify(error)),
    }
    let Some(names) = NonNull::new(names.cast_mut()) else {
        return Ok(Vec::new());
    };
    // SAFETY: a `Copy` call's array, owned by the caller with its +1 count.
    let names: CFRetained<CFArray> = unsafe { CFRetained::from_raw(names) };
    let mut found = Vec::new();
    for name in items::<CFString>(names) {
        found.push(name.to_string());
    }
    Ok(found)
}

/// The process the system sends keys to now: the focused application's.
pub(super) fn focused_pid() -> Option<i32> {
    // SAFETY: no arguments.
    let system = unsafe { AXUIElement::new_system_wide() };
    let focused = attribute(&system, "AXFocusedApplication").ok()??;
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
    let _ = unsafe { app.set_attribute_value(&name("AXFrontmost"), frontmost) };
    if let Ok(window) = window(pid, frame) {
        // SAFETY: a live element and an action name.
        let _ = unsafe { window.perform_action(&name("AXRaise")) };
    }
}
