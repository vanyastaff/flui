//! The app's window as UI Automation shows it, the tree Narrator reads, projected onto the wire's
//! nodes: each element's role folded by [`roles::wire_role`], its name,
//! automation id, value, toggle, expansion and selection, whether it is enabled and focusable,
//! and its bounds in screen device pixels.
//!
//! The raw view is walked, so nothing the app's adapter published is filtered out before the
//! projection sees it, through the bounded walk of [`walk`]: a walk cut short says so
//! (`Tree::truncated`), and the search for an automation id is bounded the same way. The client
//! waits [`TRANSACTION_TIMEOUT_MS`] for a provider, and the first property that times out fails
//! the read with a typed timeout instead of a node of defaults. Text is cut while it is read, to
//! [`OUTLINE_TEXT_LIMIT`] characters, where the projection cuts the app's. A UI Automation client
//! and its COM apartment live for one call, on the lane's thread: neither may cross to another
//! thread, and the lane must stay `Send`.
#![expect(
    unsafe_code,
    reason = "UI Automation's COM client; each call takes plain values or a live interface"
)]

use std::time::Duration;

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VT_BOOL, VT_BSTR, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded,
    IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationTreeWalker, UIA_ExpandCollapseExpandCollapseStatePropertyId,
    UIA_FullDescriptionPropertyId, UIA_InvokePatternId, UIA_IsDialogPropertyId,
    UIA_IsExpandCollapsePatternAvailablePropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId, UIA_IsValuePatternAvailablePropertyId, UIA_PROPERTY_ID,
    UIA_SelectionItemIsSelectedPropertyId, UIA_ToggleToggleStatePropertyId,
    UIA_ValueValuePropertyId,
};
use windows::core::{BSTR, HRESULT, Interface};

use flui_protocol::tree::{ElementId, Node, OUTLINE_TEXT_LIMIT, Rect, Tree};
use flui_protocol::wire::Checked;

use crate::error::Result;
use crate::lane::windows::roles::{self, ControlType, RoleFlags};
use crate::lane::windows::walk::{self, Properties, Provider, ProviderError};
use crate::lane::windows::win32::{Apartment, Hwnd, lane_error};

/// How long the client waits for a provider to answer one call, in milliseconds.
pub const TRANSACTION_TIMEOUT_MS: u32 = 5_000;

/// How long the client waits to connect to a provider, in milliseconds.
const CONNECTION_TIMEOUT_MS: u32 = 2_000;

/// `UIA_E_TIMEOUT`: the provider did not answer within the transaction timeout.
const UIA_E_TIMEOUT: HRESULT = HRESULT(0x8013_1505_u32.cast_signed());
/// `UIA_E_ELEMENTNOTAVAILABLE`: the element went away.
const UIA_E_ELEMENTNOTAVAILABLE: HRESULT = HRESULT(0x8004_0201_u32.cast_signed());
/// `RPC_E_DISCONNECTED`, `RPC_S_SERVER_UNAVAILABLE`: the provider's process went away.
const GONE_PROCESS: [HRESULT; 2] = [
    HRESULT(0x8001_0108_u32.cast_signed()),
    HRESULT(0x8007_06BA_u32.cast_signed()),
];

fn timeout() -> Duration {
    Duration::from_millis(u64::from(TRANSACTION_TIMEOUT_MS))
}

fn classify(error: &windows::core::Error) -> ProviderError {
    let code = error.code();
    if code == UIA_E_TIMEOUT {
        ProviderError::Timeout
    } else if code == UIA_E_ELEMENTNOTAVAILABLE || GONE_PROCESS.contains(&code) {
        ProviderError::Gone
    } else {
        ProviderError::Failed(error.to_string())
    }
}

/// A client and its raw view, for one call.
struct Uia {
    automation: IUIAutomation,
    walker: IUIAutomationTreeWalker,
}

impl Uia {
    fn open() -> Result<Self> {
        let automation = client()?;
        // SAFETY: a live client on this thread.
        let walker = unsafe { automation.RawViewWalker() }.map_err(|error| {
            lane_error(format!("UI Automation's raw view is unavailable: {error}"))
        })?;
        Ok(Self { automation, walker })
    }

    fn root(&self, window: Hwnd) -> Result<IUIAutomationElement> {
        // SAFETY: a live client on this thread and a window handle the lane found.
        unsafe { self.automation.ElementFromHandle(window.raw()) }.map_err(|error| {
            lane_error(format!(
                "UI Automation has no element for the app's window: {error}"
            ))
        })
    }

    /// An answer that may be no object (the end of a level for the walker, a pattern the element
    /// does not offer): a null object is what the binding reports as an error with no code, told
    /// apart from a provider's failure.
    fn optional<T>(
        answer: windows::core::Result<T>,
    ) -> std::result::Result<Option<T>, ProviderError> {
        match answer {
            Ok(found) => Ok(Some(found)),
            Err(error) if error.code().is_ok() => Ok(None),
            Err(error) => Err(classify(&error)),
        }
    }
}

impl Provider for Uia {
    type Element = IUIAutomationElement;

    fn first_child(
        &mut self,
        element: &IUIAutomationElement,
    ) -> std::result::Result<Option<IUIAutomationElement>, ProviderError> {
        // SAFETY: a live element of this thread's client.
        Self::optional(unsafe { self.walker.GetFirstChildElement(element) })
    }

    fn next_sibling(
        &mut self,
        element: &IUIAutomationElement,
    ) -> std::result::Result<Option<IUIAutomationElement>, ProviderError> {
        // SAFETY: as above.
        Self::optional(unsafe { self.walker.GetNextSiblingElement(element) })
    }

    fn node(
        &mut self,
        element: &IUIAutomationElement,
        id: ElementId,
    ) -> std::result::Result<Node, ProviderError> {
        let mut reader = Reader {
            element,
            reads: Properties::new(),
        };
        let node = reader.describe(id);
        reader.reads.finish(node)
    }
}

/// The UI Automation tree of `window`, projected: see the [module docs](self).
///
/// # Errors
///
/// [`Error::Timeout`](crate::Error::Timeout) when the app's provider did not answer in time; when
/// COM or UI Automation is unavailable, or the window has no element.
pub(super) fn read(window: Hwnd) -> Result<Tree> {
    let _apartment = Apartment::enter()?;
    let mut uia = Uia::open()?;
    let root = uia.root(window)?;
    walk::read(&mut uia, &root, timeout())
}

/// Invoke the element of `window` whose automation id is `automation_id`, through its invoke
/// pattern, as a screen reader's activate command does.
///
/// # Errors
///
/// [`Error::NotFound`](crate::Error::NotFound) when no element within the walk's budgets has that
/// id; [`Error::Timeout`](crate::Error::Timeout) when the provider did not answer in time;
/// as [`walk::pattern`] for the element's invoke pattern and [`walk::invoked`] for the invoke;
/// when COM or UI Automation is unavailable.
pub(super) fn invoke(window: Hwnd, automation_id: &str) -> Result<()> {
    let _apartment = Apartment::enter()?;
    let mut uia = Uia::open()?;
    let root = uia.root(window)?;
    let element = walk::find(&mut uia, root, automation_id, timeout())?;
    let pattern: IUIAutomationInvokePattern = walk::pattern(
        // SAFETY: a live element of this thread's client.
        Uia::optional(unsafe { element.GetCurrentPatternAs(UIA_InvokePatternId) }),
        automation_id,
        "invoke",
        timeout(),
    )?;
    // SAFETY: a live pattern of this thread's client.
    let answer = unsafe { pattern.Invoke() }.map_err(|error| classify(&error));
    walk::invoked(answer, automation_id, timeout())
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

/// Reads one element's properties through [`Properties`]: a property the provider does not offer
/// reads as its default, and after the first it did not answer no further call is made.
struct Reader<'a> {
    element: &'a IUIAutomationElement,
    reads: Properties,
}

impl Reader<'_> {
    fn keep<T>(&mut self, call: impl FnOnce() -> windows::core::Result<T>) -> Option<T> {
        self.reads.read(|| call().map_err(|error| classify(&error)))
    }

    fn property(&mut self, id: UIA_PROPERTY_ID) -> Option<VARIANT> {
        let element = self.element;
        // SAFETY: reads one property of a live element of this thread's client.
        self.keep(|| unsafe { element.GetCurrentPropertyValue(id) })
    }

    fn flag(&mut self, id: UIA_PROPERTY_ID) -> Option<bool> {
        let value = self.property(id)?;
        (kind(&value) == VT_BOOL)
            .then(|| bool::try_from(&value).ok())
            .flatten()
    }

    fn integer(&mut self, id: UIA_PROPERTY_ID) -> Option<i32> {
        let value = self.property(id)?;
        (kind(&value) == VT_I4)
            .then(|| i32::try_from(&value).ok())
            .flatten()
    }

    fn string(&mut self, id: UIA_PROPERTY_ID) -> Option<String> {
        let value = self.property(id)?;
        if kind(&value) != VT_BSTR {
            return None;
        }
        BSTR::try_from(&value).ok().as_ref().and_then(text)
    }

    fn bstr(&mut self, call: impl FnOnce() -> windows::core::Result<BSTR>) -> Option<String> {
        self.keep(call).as_ref().and_then(text)
    }

    fn describe(&mut self, id: ElementId) -> Node {
        let element = self.element;
        // SAFETY: each call reads one property of a live element of this thread's client.
        let control_id = self
            .keep(|| unsafe { element.CurrentControlType() })
            .map_or(0, |control| control.0);
        let control = ControlType::from_id(control_id);
        // SAFETY: as above.
        let aria = self
            .bstr(|| unsafe { element.CurrentAriaRole() })
            .unwrap_or_default();
        // SAFETY: as above.
        let password = self
            .keep(|| unsafe { element.CurrentIsPassword() })
            .is_some_and(windows::core::BOOL::as_bool);
        let dialog = self.flag(UIA_IsDialogPropertyId).unwrap_or(false);
        let role = control.map_or(flui_protocol::wire::Role::Unknown, |control| {
            roles::wire_role(control, &aria, RoleFlags::new(password, dialog))
        });
        let native = control.map_or_else(
            || format!("Custom({control_id})"),
            |control| control.name().to_owned(),
        );
        let mut node = Node::new(id, role, native);
        // SAFETY: as above.
        node.name = self.bstr(|| unsafe { element.CurrentName() });
        // SAFETY: as above.
        node.automation_id = self.bstr(|| unsafe { element.CurrentAutomationId() });
        // SAFETY: as above.
        node.class_name = self.bstr(|| unsafe { element.CurrentClassName() });
        node.description = self.string(UIA_FullDescriptionPropertyId);
        if self.flag(UIA_IsValuePatternAvailablePropertyId) == Some(true) {
            // An element with a value pattern has a value, the empty text of an empty field
            // included.
            node.value = Some(self.string(UIA_ValueValuePropertyId).unwrap_or_default());
        }
        if self.flag(UIA_IsTogglePatternAvailablePropertyId) == Some(true) {
            node.checked = match self.integer(UIA_ToggleToggleStatePropertyId) {
                Some(0) => Some(Checked::False),
                Some(1) => Some(Checked::True),
                Some(2) => Some(Checked::Mixed),
                _ => None,
            };
        }
        if self.flag(UIA_IsExpandCollapsePatternAvailablePropertyId) == Some(true) {
            node.expanded = match self.integer(UIA_ExpandCollapseExpandCollapseStatePropertyId) {
                Some(state) if state == ExpandCollapseState_Expanded.0 => Some(true),
                Some(state) if state == ExpandCollapseState_Collapsed.0 => Some(false),
                _ => None,
            };
        }
        if self.flag(UIA_IsSelectionItemPatternAvailablePropertyId) == Some(true) {
            node.selected = self.flag(UIA_SelectionItemIsSelectedPropertyId);
        }
        // SAFETY: as above.
        node.disabled = self
            .keep(|| unsafe { element.CurrentIsEnabled() })
            .is_some_and(|enabled| !enabled.as_bool());
        // SAFETY: as above.
        node.focused = self
            .keep(|| unsafe { element.CurrentHasKeyboardFocus() })
            .is_some_and(windows::core::BOOL::as_bool);
        // SAFETY: as above.
        node.focusable = self
            .keep(|| unsafe { element.CurrentIsKeyboardFocusable() })
            .is_some_and(windows::core::BOOL::as_bool);
        // SAFETY: as above.
        if let Some(bounds) = self.keep(|| unsafe { element.CurrentBoundingRectangle() }) {
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
}

/// The first [`OUTLINE_TEXT_LIMIT`] characters of `value`, decoded only that far; `None` for an
/// empty one.
fn text(value: &BSTR) -> Option<String> {
    let units: &[u16] = value;
    let text: String = char::decode_utf16(units.iter().copied())
        .take(OUTLINE_TEXT_LIMIT)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    (!text.is_empty()).then_some(text)
}

fn kind(value: &VARIANT) -> VARENUM {
    // SAFETY: every VARIANT carries its type tag, whatever its value holds.
    unsafe { value.Anonymous.Anonymous.vt }
}
