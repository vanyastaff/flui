//! The app's window as UI Automation shows it, the tree Narrator reads, projected onto the wire's
//! nodes: each element's role folded by [`roles::wire_role`](super::roles::wire_role), its name,
//! automation id, value, toggle, expansion and selection, whether it is enabled and focusable,
//! and its bounds in screen device pixels.
//!
//! The raw view is walked, so nothing the app's adapter published is filtered out before the
//! projection sees it, within [`MAX_NODES`] elements and [`MAX_DEPTH`] levels; a walk cut short
//! says so (`Tree::truncated`). A UI Automation client and its COM apartment live for one read,
//! on the lane's thread: neither may cross to another thread, and the lane must stay `Send`. The
//! client waits [`TRANSACTION_TIMEOUT_MS`] for a provider, so an app that hangs fails the read
//! instead of holding the lane.
#![expect(
    unsafe_code,
    reason = "UI Automation's COM client; each call takes plain values or a live interface"
)]

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VT_BOOL, VT_BSTR, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded,
    IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationTreeWalker, TreeScope_Descendants, UIA_AutomationIdPropertyId,
    UIA_ExpandCollapseExpandCollapseStatePropertyId, UIA_FullDescriptionPropertyId,
    UIA_InvokePatternId, UIA_IsDialogPropertyId, UIA_IsExpandCollapsePatternAvailablePropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_PROPERTY_ID, UIA_SelectionItemIsSelectedPropertyId,
    UIA_ToggleToggleStatePropertyId, UIA_ValueValuePropertyId,
};
use windows::core::Interface;

use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::Checked;

use crate::error::Result;
use crate::lane::windows::roles::{self, ControlType, RoleFlags};
use crate::lane::windows::win32::{Apartment, Hwnd, lane_error};

/// The most elements one read walks.
pub const MAX_NODES: usize = 4096;

/// The deepest level one read walks.
pub const MAX_DEPTH: usize = 64;

/// How long the client waits for a provider to answer one call, in milliseconds.
pub const TRANSACTION_TIMEOUT_MS: u32 = 5_000;

/// How long the client waits to connect to a provider, in milliseconds.
const CONNECTION_TIMEOUT_MS: u32 = 2_000;

/// The longest text a node keeps of a name, value or description, in characters.
const MAX_TEXT: usize = 1_024;

/// The UI Automation tree of `window`, projected: see the [module docs](self).
///
/// # Errors
///
/// When COM or UI Automation is unavailable, or the window has no element.
pub(super) fn read(window: Hwnd) -> Result<Tree> {
    let _apartment = Apartment::enter()?;
    let automation = client()?;
    // SAFETY: a live client on this thread and a window handle the lane found.
    let root = unsafe { automation.ElementFromHandle(window.raw()) }.map_err(|error| {
        lane_error(format!(
            "UI Automation has no element for the app's window: {error}"
        ))
    })?;
    // SAFETY: a live client on this thread.
    let walker = unsafe { automation.RawViewWalker() }
        .map_err(|error| lane_error(format!("UI Automation's raw view is unavailable: {error}")))?;
    let mut walk = Walk {
        walker,
        next_id: 1,
        truncated: false,
    };
    let node = walk.visit(&root, 0);
    drop(walk.walker);
    drop(root);
    drop(automation);
    Ok(Tree::new(node.into_iter().collect(), walk.truncated))
}

/// Invoke the element of `window` whose automation id is `automation_id`, through its invoke
/// pattern, as a screen reader's activate command does.
///
/// # Errors
///
/// [`Error::NotFound`](crate::Error::NotFound) when no element has that id;
/// [`Error::ActionUnsupported`](crate::Error::ActionUnsupported) when it offers no invoke
/// pattern; when COM or UI Automation is unavailable or the invoke fails.
pub(super) fn invoke(window: Hwnd, automation_id: &str) -> Result<()> {
    let _apartment = Apartment::enter()?;
    let automation = client()?;
    // SAFETY: a live client on this thread and a window handle the lane found.
    let root = unsafe { automation.ElementFromHandle(window.raw()) }.map_err(|error| {
        lane_error(format!(
            "UI Automation has no element for the app's window: {error}"
        ))
    })?;
    // SAFETY: a live client on this thread; the value is a BSTR the condition copies.
    let condition = unsafe {
        automation
            .CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(automation_id))
    }
    .map_err(|error| lane_error(format!("UI Automation refused a condition: {error}")))?;
    // SAFETY: a live element and condition of this thread's client. A null result, no match, is
    // what the binding reports as an error.
    let element = unsafe { root.FindFirst(TreeScope_Descendants, &condition) }.map_err(|_| {
        crate::Error::not_found(format!(
            "UI Automation shows no element whose automation id is {automation_id:?}"
        ))
    })?;
    // SAFETY: a live element of this thread's client.
    let pattern: IUIAutomationInvokePattern = unsafe {
        element.GetCurrentPatternAs(UIA_InvokePatternId)
    }
    .map_err(|_| crate::Error::ActionUnsupported {
        message: format!("the element {automation_id:?} offers no invoke pattern"),
        retry: flui_protocol::error::ErrorCode::ActionUnsupported.default_retry(),
    })?;
    // SAFETY: a live pattern of this thread's client.
    unsafe { pattern.Invoke() }
        .map_err(|error| lane_error(format!("invoking {automation_id:?} failed: {error}")))
}

fn client() -> Result<IUIAutomation> {
    // SAFETY: COM is initialized on this thread (`Apartment`); no outer object.
    if let Ok(automation) = unsafe {
        CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    } {
        // SAFETY: plain values on a live interface.
        unsafe {
            let _ = automation.SetTransactionTimeout(TRANSACTION_TIMEOUT_MS);
            let _ = automation.SetConnectionTimeout(CONNECTION_TIMEOUT_MS);
        }
        if let Ok(automation) = automation.cast::<IUIAutomation>() {
            return Ok(automation);
        }
    }
    // SAFETY: as above.
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| lane_error(format!("UI Automation is not available: {error}")))
}

struct Walk {
    walker: IUIAutomationTreeWalker,
    next_id: u64,
    truncated: bool,
}

impl Walk {
    /// `element` and what is under it, within the walk's budgets.
    fn visit(&mut self, element: &IUIAutomationElement, depth: usize) -> Option<Node> {
        let id = ElementId::from_u64(self.next_id)?;
        self.next_id += 1;
        let mut node = describe(element, id);
        if depth >= MAX_DEPTH {
            // SAFETY: a live element of this thread's client.
            if unsafe { self.walker.GetFirstChildElement(element) }.is_ok() {
                node.children_unread = true;
                self.truncated = true;
            }
            return Some(node);
        }
        // SAFETY: as above. A null child or sibling ends the level, which the binding reports
        // as an error.
        let mut child = unsafe { self.walker.GetFirstChildElement(element) }.ok();
        while let Some(current) = child {
            if usize::try_from(self.next_id).unwrap_or(usize::MAX) > MAX_NODES {
                node.children_unread = true;
                self.truncated = true;
                break;
            }
            if let Some(read) = self.visit(&current, depth + 1) {
                node.children.push(read);
            }
            // SAFETY: as above.
            child = unsafe { self.walker.GetNextSiblingElement(&current) }.ok();
        }
        Some(node)
    }
}

/// One element as a node: see the [module docs](self). A property the provider cannot answer
/// reads as its default.
fn describe(element: &IUIAutomationElement, id: ElementId) -> Node {
    // SAFETY: each call reads one property of a live element of this thread's client.
    let control_id = unsafe { element.CurrentControlType() }.map_or(0, |control| control.0);
    let control = ControlType::from_id(control_id);
    // SAFETY: as above.
    let aria = unsafe { element.CurrentAriaRole() }.map_or_default(|role| role.to_string());
    let flags = RoleFlags::new(
        // SAFETY: as above.
        unsafe { element.CurrentIsPassword() }.is_ok_and(windows::core::BOOL::as_bool),
        flag(element, UIA_IsDialogPropertyId).unwrap_or(false),
    );
    let role = control.map_or(flui_protocol::wire::Role::Unknown, |control| {
        roles::wire_role(control, &aria, flags)
    });
    let native = control.map_or_else(
        || format!("Custom({control_id})"),
        |control| control.name().to_owned(),
    );
    let mut node = Node::new(id, role, native);
    // SAFETY: as above.
    node.name = text(unsafe { element.CurrentName() }.ok());
    // SAFETY: as above.
    node.automation_id = text(unsafe { element.CurrentAutomationId() }.ok());
    // SAFETY: as above.
    node.class_name = text(unsafe { element.CurrentClassName() }.ok());
    node.description = string(element, UIA_FullDescriptionPropertyId);
    if flag(element, UIA_IsValuePatternAvailablePropertyId) == Some(true) {
        // An element with a value pattern has a value, the empty text of an empty field
        // included.
        node.value = Some(string(element, UIA_ValueValuePropertyId).unwrap_or_default());
    }
    if flag(element, UIA_IsTogglePatternAvailablePropertyId) == Some(true) {
        node.checked = match integer(element, UIA_ToggleToggleStatePropertyId) {
            Some(0) => Some(Checked::False),
            Some(1) => Some(Checked::True),
            Some(2) => Some(Checked::Mixed),
            _ => None,
        };
    }
    if flag(element, UIA_IsExpandCollapsePatternAvailablePropertyId) == Some(true) {
        node.expanded = match integer(element, UIA_ExpandCollapseExpandCollapseStatePropertyId) {
            Some(state) if state == ExpandCollapseState_Expanded.0 => Some(true),
            Some(state) if state == ExpandCollapseState_Collapsed.0 => Some(false),
            _ => None,
        };
    }
    if flag(element, UIA_IsSelectionItemPatternAvailablePropertyId) == Some(true) {
        node.selected = flag(element, UIA_SelectionItemIsSelectedPropertyId);
    }
    // SAFETY: as above.
    node.disabled = unsafe { element.CurrentIsEnabled() }.is_ok_and(|enabled| !enabled.as_bool());
    // SAFETY: as above.
    node.focused =
        unsafe { element.CurrentHasKeyboardFocus() }.is_ok_and(windows::core::BOOL::as_bool);
    // SAFETY: as above.
    node.focusable =
        unsafe { element.CurrentIsKeyboardFocusable() }.is_ok_and(windows::core::BOOL::as_bool);
    // SAFETY: as above.
    if let Ok(bounds) = unsafe { element.CurrentBoundingRectangle() } {
        let width = u32::try_from(bounds.right.saturating_sub(bounds.left)).unwrap_or(0);
        let height = u32::try_from(bounds.bottom.saturating_sub(bounds.top)).unwrap_or(0);
        if width > 0 && height > 0 {
            node.rect = Some(Rect {
                x: bounds.left,
                y: bounds.top,
                width,
                height,
            });
        }
    }
    node
}

fn text(value: Option<windows::core::BSTR>) -> Option<String> {
    let value = value?.to_string();
    (!value.is_empty()).then(|| value.chars().take(MAX_TEXT).collect())
}

fn property(element: &IUIAutomationElement, id: UIA_PROPERTY_ID) -> Option<VARIANT> {
    // SAFETY: reads one property of a live element of this thread's client.
    unsafe { element.GetCurrentPropertyValue(id) }.ok()
}

fn kind(value: &VARIANT) -> VARENUM {
    // SAFETY: every VARIANT carries its type tag, whatever its value holds.
    unsafe { value.Anonymous.Anonymous.vt }
}

fn flag(element: &IUIAutomationElement, id: UIA_PROPERTY_ID) -> Option<bool> {
    let value = property(element, id)?;
    (kind(&value) == VT_BOOL)
        .then(|| bool::try_from(&value).ok())
        .flatten()
}

fn integer(element: &IUIAutomationElement, id: UIA_PROPERTY_ID) -> Option<i32> {
    let value = property(element, id)?;
    (kind(&value) == VT_I4)
        .then(|| i32::try_from(&value).ok())
        .flatten()
}

fn string(element: &IUIAutomationElement, id: UIA_PROPERTY_ID) -> Option<String> {
    let value = property(element, id)?;
    if kind(&value) != VT_BSTR {
        return None;
    }
    text(windows::core::BSTR::try_from(&value).ok())
}
