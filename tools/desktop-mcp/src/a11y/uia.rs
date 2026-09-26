//! Windows UI Automation backend.
//!
//! Lives on the worker thread: [`uiautomation::UIAutomation::new`] joins that
//! thread to the COM multithreaded apartment, and every `UIElement` this
//! backend holds stays on it. Calls into providers carry UI Automation's own
//! timeouts ([`crate::os::uia_with_timeouts`]), so a hung application fails
//! a call instead of holding the thread.
//!
//! A tree read walks the control view one element at a time: each step
//! fetches the next child or sibling together with every property the
//! [`Node`] shape reports (a cache request with `TreeScope::Element`), so an
//! element costs about two cross-process calls and nothing past the read's
//! budget is ever fetched, however many children a provider claims. The
//! runtime id (the handle's identity) is read live per element.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use uiautomation::core::UICacheRequest;
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UIRangeValuePattern, UIScrollItemPattern,
    UISelectionItemPattern, UITogglePattern, UIValuePattern,
};
use uiautomation::types::{
    ControlType, ExpandCollapseState, Handle, ToggleState, TreeScope, UIProperty,
};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use super::{
    AccessibilityBackend, Action, ActionName, Checked, ClickPoint, Node, Read, Role, role_from_aria,
};
use crate::cache::ElementCache;
use crate::error::{Effect, ToolError, ToolResult};
use crate::geometry::Rect;

/// The pattern-availability properties the actions depend on, each with
/// the action(s) it makes available. `set_value` needs either of two
/// patterns; `expand` and `collapse` share one, and the element's state says
/// which of them applies.
const PATTERNS: &[(UIProperty, &[ActionName])] = &[
    (UIProperty::IsInvokePatternAvailable, &[ActionName::Invoke]),
    (UIProperty::IsTogglePatternAvailable, &[ActionName::Toggle]),
    (UIProperty::IsValuePatternAvailable, &[ActionName::SetValue]),
    (
        UIProperty::IsRangeValuePatternAvailable,
        &[ActionName::SetValue],
    ),
    (
        UIProperty::IsSelectionItemPatternAvailable,
        &[ActionName::Select],
    ),
    (
        UIProperty::IsExpandCollapsePatternAvailable,
        &[ActionName::Expand, ActionName::Collapse],
    ),
    (
        UIProperty::IsScrollItemPatternAvailable,
        &[ActionName::ScrollIntoView],
    ),
];

/// The property that says whether `action` is available, and the pattern
/// name UI Automation gives it; `None` for an action this backend has no
/// pattern for, which the callers report as unsupported.
///
/// `ActionName` is `#[non_exhaustive]`, so this match cannot be the check that
/// every action has a pattern; `every_action_name_has_a_uia_pattern` is.
fn pattern_of(action: ActionName) -> Option<(UIProperty, &'static str)> {
    Some(match action {
        ActionName::Invoke => (UIProperty::IsInvokePatternAvailable, "Invoke"),
        ActionName::Toggle => (UIProperty::IsTogglePatternAvailable, "Toggle"),
        ActionName::SetValue => (UIProperty::IsValuePatternAvailable, "Value"),
        ActionName::Select => (UIProperty::IsSelectionItemPatternAvailable, "SelectionItem"),
        ActionName::Focus => (UIProperty::IsKeyboardFocusable, "keyboard focus"),
        ActionName::Expand | ActionName::Collapse => (
            UIProperty::IsExpandCollapsePatternAvailable,
            "ExpandCollapse",
        ),
        ActionName::ScrollIntoView => (UIProperty::IsScrollItemPatternAvailable, "ScrollItem"),
        _ => return None,
    })
}

/// How many elements one read (`accessibility_tree`, `find`, a `wait_for`
/// poll) fetches at most, across all the windows it reads; counting a node's
/// left-out children spends it too. Well under the element cache's capacity,
/// so every handle a response carries stays resolvable.
pub const NODE_BUDGET: usize = 5_000;

/// How often, and how far apart, `focus` reads back whether the element
/// took keyboard focus.
const FOCUS_POLLS: u32 = 5;
const FOCUS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

use super::CLIPPED_CHARS as MAX_PROPERTY_CHARS;

/// How many bytes of strings one read reports at most, across all its
/// elements; past it the read stops, marked truncated.
const READ_BYTES: usize = 16 << 20;

/// How many parents a walk up to the desktop takes at most: a hostile
/// provider can report an endless or cyclic chain, and the walk runs on the
/// one worker every tool shares.
const ANCESTOR_LIMIT: usize = 256;

/// How long one such walk may take at most, whatever the count: a provider
/// can answer every parent at the edge of the call timeout.
const ANCESTOR_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

/// How far past the budget or the depth a node's remaining children are
/// counted before the count stops being exact.
const OMITTED_COUNT_CAP: usize = 256;

/// Properties every [`Node`] reads from the cache.
const NODE_PROPERTIES: &[UIProperty] = &[
    UIProperty::Name,
    UIProperty::ControlType,
    UIProperty::AriaRole,
    UIProperty::AutomationId,
    UIProperty::ClassName,
    UIProperty::BoundingRectangle,
    UIProperty::IsEnabled,
    UIProperty::HasKeyboardFocus,
    UIProperty::IsKeyboardFocusable,
    UIProperty::RangeValueValue,
    UIProperty::ToggleToggleState,
    UIProperty::ExpandCollapseExpandCollapseState,
    UIProperty::SelectionItemIsSelected,
    UIProperty::ProcessId,
    UIProperty::IsPassword,
    UIProperty::ValueIsReadOnly,
    UIProperty::RangeValueIsReadOnly,
];

/// `UIA_E_ELEMENTNOTAVAILABLE`: the provider removed the element.
const E_ELEMENT_NOT_AVAILABLE: i32 = 0x8004_0201_u32 as i32;
/// `RPC_E_DISCONNECTED`: the providing process went away.
const E_DISCONNECTED: i32 = 0x8001_0108_u32 as i32;
/// `RPC_S_SERVER_UNAVAILABLE`: the providing process exited.
const E_SERVER_UNAVAILABLE: i32 = 0x8007_06BA_u32 as i32;
/// `RPC_S_CALL_FAILED`: the providing process died during the call.
const E_CALL_FAILED: i32 = 0x8007_06BE_u32 as i32;
/// `CO_E_OBJNOTCONNECTED`: the provider object was disconnected.
const E_NOT_CONNECTED: i32 = 0x8004_01FD_u32 as i32;
/// `ERROR_INVALID_WINDOW_HANDLE`: the window was destroyed.
const E_INVALID_WINDOW: i32 = 0x8007_0578_u32 as i32;
/// `UIA_E_ELEMENTNOTENABLED`: the element is disabled.
const E_ELEMENT_NOT_ENABLED: i32 = 0x8004_0200_u32 as i32;

/// An issued element, with the process it belonged to when issued: a handle
/// is refused once that process is gone, even if UI Automation (keyed by
/// window handle for Win32 controls) would follow a recycled handle into
/// another application.
#[derive(Debug, Clone)]
struct Held {
    element: UIElement,
    /// The runtime id it was issued under, `None` for an anonymous one.
    runtime: Option<Vec<i32>>,
    /// Its control type when issued. A runtime id derived from a recycled
    /// native window can repeat for a replacement in the same process; one
    /// of another control type is told apart by this. Not its class name or
    /// automation id: an action can change those on the same control (a web
    /// toggle's CSS classes, a re-keyed DOM id). `None` when it could not be
    /// read: such a handle is not acted on.
    control_type: Option<i32>,
    pid: u32,
    started: Option<u64>,
}

impl Held {
    /// Whether `fresh`, read live, is still the element this handle was
    /// issued for: the same runtime id, in the same process (by pid and
    /// start time), of the same control type. The one rule every path uses,
    /// so a read, an action's precheck and its readback agree. `None` when
    /// it cannot be told: a property could not be read.
    fn same_as(&self, fresh: &UIElement, started_now: Option<u64>) -> Option<bool> {
        let id = self.runtime.as_deref()?;
        let runtime = crate::os::runtime_id(fresh.as_ref()).ok()?;
        let control_type = cached_i32(fresh, UIProperty::ControlType)?;
        let pid = cached_pid(fresh)?;
        Some(
            runtime.as_deref() == Some(id)
                && Some(control_type) == self.control_type
                && pid == self.pid
                && started_now == self.started,
        )
    }
}

/// UI Automation client state for the session.
pub struct Uia {
    automation: UIAutomation,
    /// The control view, walked one sibling at a time so a node's children
    /// are fetched only as far as the budget reaches.
    walker: UITreeWalker,
    single: UICacheRequest,
    /// A text control's value, read on its own: the element a handle keeps
    /// is the one from the walk, and a value cached there (a whole document)
    /// would stay in memory as long as the handle.
    value: UICacheRequest,
    elements: ElementCache<Identity, Held>,
    anonymous: u64,
    /// Process start times looked up during the current read, one lookup
    /// per process rather than per element.
    starts: HashMap<u32, Option<u64>>,
}

impl std::fmt::Debug for Uia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uia")
            .field("handles", &self.elements.len())
            .finish_non_exhaustive()
    }
}

fn platform(context: &str) -> impl FnOnce(uiautomation::Error) -> ToolError + '_ {
    move |e| ToolError::platform(context, e)
}

impl Uia {
    /// Joins this thread to the COM MTA and prepares the cache requests.
    /// Refused without call timeouts: a client that can wait on a hung
    /// provider forever would hold the one desktop thread, every queued tool
    /// and the shutdown release with it.
    pub fn new() -> Result<Self, String> {
        // The first client joins the thread to COM; the one with timeouts is
        // created on top of it and is the one used.
        let _plain = UIAutomation::new().map_err(|e| e.to_string())?;
        let automation = crate::os::uia_with_timeouts()
            .map(UIAutomation::from)
            .ok_or_else(|| {
                "no UI Automation client with call timeouts (CUIAutomation8) is available"
                    .to_owned()
            })?;
        let walker = automation
            .get_control_view_walker()
            .map_err(|e| e.to_string())?;
        let single = node_request(&automation, TreeScope::Element).map_err(|e| e.to_string())?;
        let value = automation
            .create_cache_request()
            .and_then(|request| {
                request.add_property(UIProperty::ValueValue)?;
                // The value and identity must come from the same refresh.
                request.add_property(UIProperty::ControlType)?;
                request.add_property(UIProperty::ProcessId)?;
                // Read with the value: a control can turn into a password
                // field between the walk and this read.
                request.add_property(UIProperty::IsPassword)?;
                request.set_tree_scope(TreeScope::Element)?;
                Ok(request)
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            automation,
            walker,
            single,
            value,
            elements: ElementCache::default(),
            anonymous: 0,
            starts: HashMap::new(),
        })
    }

    /// The identity a handle for `element` is keyed by: its runtime id, or
    /// an identity of its own when it has none.
    ///
    /// `None` when the read failed (a timeout, a disconnect, a malformed
    /// array): not the same as an element without one.
    fn identity(&mut self, element: &UIElement) -> Option<Identity> {
        match crate::os::runtime_id(element.as_ref()) {
            Ok(Some(id)) => Some(Identity::Runtime(id)),
            Ok(None) => Some(self.fresh_identity()),
            Err(_) => None,
        }
    }

    fn fresh_identity(&mut self) -> Identity {
        self.anonymous += 1;
        Identity::Anonymous(self.anonymous)
    }

    /// Issues (or re-issues) the handle for `element` under `key`. UI
    /// Automation may hand a removed element's runtime id to a new one; the
    /// handle already issued for the id is kept only while its own object
    /// still answers to that id, else the id is retired and the new element
    /// gets a fresh handle, so a held handle never retargets to another
    /// control.
    ///
    /// Past `deadline` the held object is not asked again: the id is retired
    /// instead, which only costs the old handle.
    ///
    /// Retiring on doubt (the deadline passed, an identity read failed) is
    /// the safe side, a handle never follows an unverified element, but it
    /// can move a live element to a new handle: `uncertain` is then set, and
    /// the read says it is truncated, so a wait for the old handle to be
    /// gone does not conclude from it.
    fn register(
        &mut self,
        key: Identity,
        element: &UIElement,
        deadline: Instant,
        uncertain: &mut bool,
    ) -> String {
        let pid = cached_pid(element).unwrap_or(0);
        let mut started = *self
            .starts
            .entry(pid)
            .or_insert_with(|| crate::os::process_started(pid));
        // The handle is kept only for the same element ([`Held::same_as`]),
        // and only while its own object still answers to the id: a new
        // element of another control type or process claiming the id is a
        // collision, not the same control read again, and gets its own
        // handle. A provider elsewhere claiming a trusted application's
        // runtime id gets a handle of its own instead of taking over that
        // one.
        let control_type = cached_i32(element, UIProperty::ControlType);
        if let Identity::Runtime(id) = &key
            && let Some(held) = self.elements.by_identity(&key)
        {
            if started.is_none()
                && held.started.is_some()
                && held.pid == pid
                && held.control_type == control_type
            {
                if let Ok(current) = crate::os::process_started_checked(pid) {
                    started = current;
                } else {
                    // An unreadable identity is not a replacement. Keep
                    // the original proxy and binding, but advertise no
                    // capabilities until a later read verifies it.
                    *uncertain = true;
                    return self
                        .elements
                        .handle_of(&key)
                        .expect("BUG: the held identity has an issued handle");
                }
            }
            // `Some(true)` keep, `Some(false)` a different element, `None`
            // not known; each provider call only while time is left.
            let verdict = if Instant::now() >= deadline {
                None
            } else {
                match held.same_as(element, started) {
                    Some(true) if Instant::now() < deadline => {
                        match crate::os::runtime_id(held.element.as_ref()) {
                            Ok(now) => Some(now.as_deref() == Some(id.as_slice())),
                            Err(_) => None,
                        }
                    }
                    Some(true) => None,
                    other => other,
                }
            };
            if verdict != Some(true) {
                *uncertain |= verdict.is_none();
                self.elements.retire(&key);
            }
        }
        let runtime = match &key {
            Identity::Runtime(id) => Some(id.clone()),
            Identity::Anonymous(_) => None,
        };
        self.elements.insert(
            key,
            Held {
                element: element.clone(),
                runtime,
                control_type,
                pid,
                started,
            },
        )
    }

    /// The element behind `handle`, refused as stale once the process it
    /// belonged to when issued is gone.
    ///
    /// An element whose process could not be identified when issued is not
    /// acted on at all: nothing would tell its replacement from it.
    ///
    /// Returns the element as read just now, its cache current.
    fn alive(&mut self, handle: &str) -> ToolResult<UIElement> {
        self.alive_until(handle, None)
    }

    fn alive_until(&mut self, handle: &str, deadline: Option<Instant>) -> ToolResult<UIElement> {
        let result = self.check_alive(handle, deadline);
        if matches!(&result, Err(ToolError::Gone { .. })) {
            self.elements.invalidate(handle);
        }
        result
    }

    fn process_identity(&self, handle: &str) -> Option<(u32, u64)> {
        let held = self.elements.get(handle).ok()?;
        held.started.map(|started| (held.pid, started))
    }

    fn remember_error(&mut self, handle: &str, error: ToolError) -> ToolError {
        if error.code() == "gone" {
            self.elements.invalidate(handle);
        }
        error
    }

    fn check_alive(&self, handle: &str, deadline: Option<Instant>) -> ToolResult<UIElement> {
        let before_call = || before_identity_call(handle, deadline, Instant::now());
        let held = self.elements.get(handle)?;
        let Some(started) = held.started else {
            return Err(ToolError::NotSupported(format!(
                "element `{handle}` came from a process this server could not identify, so it is not acted on; read the tree again"
            )));
        };
        if crate::os::process_started_checked(held.pid)? != Some(started) {
            return Err(ToolError::gone_element(handle, "its process has exited"));
        }
        // The object itself must still answer to the id it was issued
        // under: a control destroyed and replaced inside the same running
        // process (a recycled native window behind a UIA proxy) would
        // otherwise take the action.
        // An element without a runtime id has nothing to re-check it by, so
        // it is read, not acted on: its object could follow a replacement.
        let Some(id) = &held.runtime else {
            return Err(ToolError::NotSupported(format!(
                "element `{handle}` has no runtime id, so it cannot be told from a replacement and is not acted on; use its window's input tools"
            )));
        };
        // Unreadable is not "gone" (a provider that timed out still holds
        // it); a different id is.
        before_call()?;
        match crate::os::runtime_id(held.element.as_ref()) {
            Ok(now) if now.as_deref() == Some(id.as_slice()) => {}
            Ok(_) => {
                return Err(ToolError::gone_element(
                    handle,
                    "the application replaced or removed it",
                ));
            }
            Err(e) => {
                return Err(classify_code(
                    handle,
                    "re-reading it",
                    e.code().0,
                    &e,
                    held.started.map(|started| (held.pid, started)),
                    crate::os::process_started_checked,
                ));
            }
        }
        // Read live: an object that now reports another control type, or
        // another process (a runtime id derived from a recycled native window
        // can come back in another process while the first still runs), is a
        // replacement, whatever its runtime id says. One cross-process call
        // for all of it, bounded by the call timeout.
        if held.control_type.is_none() {
            return Err(ToolError::NotSupported(format!(
                "element `{handle}` could not be told apart when it was read (its control type was unreadable), so it is not acted on; read the tree again"
            )));
        }
        before_call()?;
        let fresh = held
            .element
            .build_updated_cache(&self.single)
            .map_err(|e| {
                classify(
                    handle,
                    "re-reading it",
                    &e,
                    held.started.map(|started| (held.pid, started)),
                )
            })?;
        // Unreadable is not "gone": the element may be there and fine.
        before_call()?;
        match identity_after_refresh(
            started,
            || held.same_as(&fresh, Some(started)),
            || crate::os::process_started_checked(held.pid),
        )? {
            Some(true) => Ok(fresh),
            None => Err(ToolError::platform(
                "re-reading the element",
                "its identity could not all be read, so it is not acted on",
            )),
            Some(false) => Err(ToolError::gone_element(
                handle,
                "another element now answers under its identity",
            )),
        }
    }

    /// `element` and its descendants, fetched one child at a time through
    /// the control-view walker until the walk's budget or deadline runs out.
    /// Children left out by the depth or the budget are counted in
    /// `omitted_children`, up to [`OMITTED_COUNT_CAP`] (a lower bound past
    /// it). `None` for an element this read already emitted: UI Automation
    /// shows an owned window (a dialog) both as a top-level window and under
    /// its owner, and it is reported once.
    fn build(&mut self, element: &UIElement, depth: usize, walk: &mut Walk) -> Option<Node> {
        // The fetch that reached this element can have used up the time:
        // no further provider call starts past the deadline.
        if Instant::now() >= walk.deadline {
            walk.truncated = true;
            return None;
        }
        // Spent before anything else: a repeat or a cycle costs its fetch.
        walk.budget = walk.budget.saturating_sub(1);
        // An unreadable runtime id would mint a handle every action refuses.
        let Some(mut key) = self.identity(element) else {
            walk.truncated = true;
            return None;
        };
        // That call can have run out the time: no further one starts.
        if Instant::now() >= walk.deadline {
            walk.truncated = true;
            return None;
        }
        if let Some(handle) = self.elements.handle_of(&key)
            && walk.seen.contains(&handle)
        {
            // The same runtime id twice in one read. An owned window listed
            // at the top and under its owner is one element; anything else
            // is a different element claiming the id, which gets a handle of
            // its own instead of taking over the first one's.
            // It is the same one only if both have the same native window;
            // otherwise the collision gets its own handle, never hidden.
            if Instant::now() >= walk.deadline {
                walk.truncated = true;
                return None;
            }
            let native = |e: &UIElement| {
                e.get_native_window_handle()
                    .map(Into::<isize>::into)
                    .ok()
                    .filter(|&h| h != 0)
            };
            let held_native = if element.get_cached_control_type().ok() == Some(ControlType::Window)
            {
                self.elements
                    .by_identity(&key)
                    .and_then(|held| native(&held.element))
            } else {
                None
            };
            // Between the two provider calls, as before each.
            if held_native.is_some() && Instant::now() >= walk.deadline {
                walk.truncated = true;
                return None;
            }
            let same_window = held_native.is_some_and(|h| native(element) == Some(h));
            if same_window {
                return None;
            }
            key = self.fresh_identity();
        }
        if Instant::now() >= walk.deadline {
            walk.truncated = true;
            return None;
        }
        let mut uncertain = false;
        let id = self.register(key, element, walk.deadline, &mut uncertain);
        walk.truncated |= uncertain;
        if uncertain {
            // Retaining a handle is not proof that this fresh snapshot is
            // its element: even handle-only state waits must not consume it.
            return None;
        }
        walk.seen.insert(id.clone());
        let mut node = describe(element, id);
        let actionable = self.elements.get(&node.id).is_ok_and(|held| {
            held.runtime.is_some() && held.started.is_some() && held.control_type.is_some()
        });
        constrain_actions(&mut node, actionable);
        // One more call for a text control's value, charged like any other.
        if node.has_text_value {
            if walk.budget == 0 || Instant::now() >= walk.deadline {
                walk.truncated = true;
            } else {
                walk.budget -= 1;
                if let Err(error) = self.read_value(element, &mut node, Some(walk.deadline)) {
                    walk.truncated = true;
                    if error.code() == "gone" {
                        self.elements.invalidate(&node.id);
                        return None;
                    }
                }
            }
        }
        // A cut string is not what a search for the whole one would match,
        // and a property the provider failed to report matches nothing.
        // Only a searched property (name, automation id) cut short keeps the
        // node from matching; a long value or class name does not.
        node.unmatchable |= node.searched_clipped();
        walk.truncated |= node.unmatchable || node.is_clipped();
        walk.bytes = walk.bytes.saturating_sub(node.text_bytes());
        let mut omitted = 0;
        // Past the budget no child is fetched at all, not even the first:
        // `exhausted` marks what is left unread.
        let mut child = if walk.exhausted() {
            // Its children, if any, were not even looked for.
            node.children_unread = true;
            None
        } else {
            walked(
                self.walker
                    .get_first_child_build_cache(element, &self.single),
                walk,
            )
        };
        while let Some(current) = child {
            let exhausted = walk.exhausted();
            if depth < walk.max_depth && !exhausted {
                node.children.extend(self.build(&current, depth + 1, walk));
            } else {
                // Left out by the depth too: what a search did not see.
                walk.truncated = true;
                omitted += 1;
                // Counting what is left out costs a fetch per child too, so
                // it spends the same budget: an exhausted budget stops the
                // count (a lower bound from there).
                if exhausted || omitted >= OMITTED_COUNT_CAP {
                    node.children_unread = true;
                    break;
                }
                walk.budget -= 1;
            }
            // A spent budget fetches no further sibling either; `exhausted`
            // marks the rest unread.
            if walk.exhausted() {
                node.children_unread = true;
                break;
            }
            child = walked(
                self.walker
                    .get_next_sibling_build_cache(&current, &self.single),
                walk,
            );
        }
        if omitted > 0 {
            node.omitted_children = Some(omitted);
        }
        Some(node)
    }

    /// Whether `candidate` is `element` or one of its descendants.
    /// `None` when it cannot be told: a provider call failed or the walk ran
    /// out of time (`until`) or steps.
    fn within(&self, candidate: UIElement, element: &UIElement, until: Instant) -> Option<bool> {
        // Checked before every call, the setup ones included: each can take
        // a whole call timeout, so the walk as a whole keeps to the deadline.
        let late = || Instant::now() >= until;
        if late() {
            return None;
        }
        let walker = self.automation.get_raw_view_walker().ok()?;
        if late() {
            return None;
        }
        let root = self.automation.get_root_element().ok()?;
        let mut current = candidate;
        for _ in 0..ANCESTOR_LIMIT {
            if late() || self.automation.compare_elements(&current, element).ok()? {
                return (!late()).then_some(true);
            }
            if late() || self.automation.compare_elements(&current, &root).ok()? {
                return (!late()).then_some(false);
            }
            if late() {
                return None;
            }
            current = walker.get_parent(&current).ok()?;
        }
        None
    }

    /// Fills in a text control's value with a read of its own, clipped, so
    /// only the clipped string outlives the call. Whether it could be read:
    /// a property the provider does not return, or returns as something
    /// other than a string, is a failed read, not an empty value.
    ///
    /// A password field's value is withheld by design, not unreadable: it
    /// has none to report, and the read is complete.
    ///
    /// An `IsPassword` that is not a readable boolean fails the read rather
    /// than counting as "not a password": a value is fetched only when the
    /// element surely is no password field.
    fn read_value(
        &self,
        element: &UIElement,
        node: &mut Node,
        until: Option<Instant>,
    ) -> ToolResult<()> {
        let before_call = || {
            if until.is_some_and(|until| Instant::now() >= until) {
                Err(ToolError::platform(
                    "reading its value",
                    "the read ran out of time",
                ))
            } else {
                Ok(())
            }
        };
        let unread = || {
            ToolError::platform(
                "reading its value",
                "the value or its password flag could not be read",
            )
        };
        if !node.has_text_value {
            return Ok(());
        }
        match cached_flag(element, UIProperty::IsPassword) {
            Some(true) => return Ok(()),
            None => return Err(unread()),
            Some(false) => {}
        }
        let held = self.elements.get(&node.id)?;
        before_call()?;
        value_while_current(
            || {
                element.build_updated_cache(&self.value).map_err(|error| {
                    classify(
                        &node.id,
                        "reading its value",
                        &error,
                        self.process_identity(&node.id),
                    )
                })
            },
            |fresh| {
                before_call()?;
                let verdict = match held.started {
                    Some(started) => identity_after_refresh(
                        started,
                        || held.same_as(fresh, Some(started)),
                        || crate::os::process_started_checked(held.pid),
                    )?,
                    None => None,
                };
                match verdict {
                    Some(true) => Ok(()),
                    Some(false) => Err(ToolError::gone_element(
                        &node.id,
                        "another element answered while its value was read",
                    )),
                    None => Err(ToolError::platform(
                        "reading its value",
                        "the value's element identity could not be verified",
                    )),
                }
            },
        )
        .and_then(|fresh| {
            consume_value_snapshot(node, cached_flag(&fresh, UIProperty::IsPassword), || {
                cached_str(&fresh, UIProperty::ValueValue)
            })
        })
    }

    /// Whether the element UIA hit-tests at `(x, y)` is `element` or one of
    /// its descendants; an error when that cannot be told, which is not a
    /// miss ("covered") either.
    fn hits_element(
        &mut self,
        handle: &str,
        x: i32,
        y: i32,
        element: &UIElement,
    ) -> ToolResult<bool> {
        hit_while_current(
            self,
            |this| this.read_hit(x, y, element),
            |this| this.alive(handle).map(|_| ()),
        )
    }

    fn read_hit(&self, x: i32, y: i32, element: &UIElement) -> ToolResult<bool> {
        let unknown = || {
            ToolError::platform(
                "hit-testing the element",
                "UI Automation could not tell in time what is at the point",
            )
        };
        let hit = self
            .automation
            .element_from_point(uiautomation::types::Point::new(x, y))
            .map_err(|_| unknown())?;
        self.within(hit, element, Instant::now() + ANCESTOR_DEADLINE)
            .ok_or_else(unknown)
    }

    /// The top-level window `element` belongs to, by the definition the
    /// point check uses (`GA_ROOT` of the window under the point): the root
    /// of the nearest native window at or above it. UI Automation's own
    /// parent chain is no guide there, since it nests an owned dialog under
    /// its owner window. `None` when no native window is found.
    fn top_level_window(&self, element: &UIElement) -> Option<u32> {
        let walker = self.automation.get_raw_view_walker().ok()?;
        let mut current = element.clone();
        let until = Instant::now() + ANCESTOR_DEADLINE;
        for _ in 0..ANCESTOR_LIMIT {
            if Instant::now() >= until {
                return None;
            }
            // Unreadable is not "none": the UIA parent chain would then lead
            // to an owner window instead of an owned dialog's own.
            let handle: isize = current.get_native_window_handle().ok()?.into();
            // An HWND is 32 significant bits, sign-extended by UIA: the low
            // ones are the handle, as `os::windows` reads them.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "an HWND's low 32 bits are the handle"
            )]
            let id = handle as usize as u32;
            if id != 0 {
                return crate::os::root_window(id);
            }
            // Checked again between the two provider calls.
            if Instant::now() >= until {
                return None;
            }
            current = walker.get_parent(&current).ok()?;
        }
        None
    }

    /// Completes `node` from `fresh` and refuses a readback whose state could
    /// not all be read, or that came from another element than the one acted
    /// on (the action closed a window whose handle a new control took): the
    /// action ran, so the answer is `Interrupted`, not a node with defaulted
    /// or borrowed fields.
    fn complete_readback(
        &mut self,
        handle: &str,
        fresh: &UIElement,
        node: &mut Node,
        toggled: bool,
        until: Option<Instant>,
    ) -> ToolResult<()> {
        let held = self.elements.get(handle)?;
        let (pid, started) = (held.pid, held.started);
        let result = readback_while_current(
            handle,
            started,
            || self.readback_state(handle, fresh, node, toggled, until),
            || crate::os::process_started_checked(pid),
        );
        result.map_err(|error| self.remember_error(handle, error))
    }

    fn readback_state(
        &self,
        handle: &str,
        fresh: &UIElement,
        node: &mut Node,
        toggled: bool,
        until: Option<Instant>,
    ) -> ToolResult<()> {
        let held = self.elements.get(handle)?;
        // The same rule as before the action: an element read back must be
        // the one acted on, in the same process.
        let same = match held.started {
            Some(started) => identity_after_refresh(
                started,
                || held.same_as(fresh, Some(started)),
                || crate::os::process_started_checked(held.pid),
            )
            .map_err(|error| {
                error.after(
                    Effect::Ran,
                    "the action succeeded; the readback identity could not be checked",
                )
            })?,
            None => None,
        };
        let range_unread = !node.has_text_value
            && cached_bool(fresh, UIProperty::IsRangeValuePatternAvailable)
            && node.value.is_none();
        if same.is_none() {
            return Err(ToolError::platform(
                "reading back",
                "the element's identity could not be read back",
            )
            .after(
                Effect::Ran,
                "the action itself succeeded; only reading the element afterwards failed, so do not repeat it",
            ));
        }
        if same == Some(false) {
            return Err(ToolError::gone_element(
                handle,
                "another element now answers for this one (its window was reused)",
            )
            .after(
                Effect::Ran,
                "the action itself succeeded; the element it acted on is gone, so do not repeat it",
            ));
        }
        const READBACK: &str = "the action itself succeeded; only reading the element's state afterwards failed, so do not repeat it";
        // The identity read can have used up a caller's deadline: the
        // value read is one more provider call.
        if until.is_some_and(|until| Instant::now() >= until) {
            return Err(
                ToolError::platform("reading back", "the readback ran out of time")
                    .after(Effect::Ran, READBACK),
            );
        }
        self.read_value(fresh, node, until)
            .map_err(|error| error.after(Effect::Ran, READBACK))?;
        if node.unmatchable || range_unread || (toggled && node.checked.is_none()) {
            return Err(ToolError::platform(
                "reading back",
                "the element's state could not all be read",
            )
            .after(Effect::Ran, READBACK));
        }
        Ok(())
    }

    /// `element` once keyboard focus is on it or inside it (a combo box
    /// hands focus to its edit field, a web view host to its content). A
    /// provider can accept `SetFocus` and leave the focus where it was (an
    /// element that is not keyboard-focusable does), so success is read
    /// back, not assumed; the focus may land a moment later, so it is polled
    /// briefly. The request itself went out, so a failed readback says so.
    fn focused(&mut self, handle: &str, element: &UIElement) -> ToolResult<Node> {
        let mut focusable = None;
        let mut unknown = false;
        // One deadline for the whole readback, however slow each poll is.
        let until = Instant::now() + ANCESTOR_DEADLINE;
        for attempt in 0..FOCUS_POLLS {
            if attempt > 0 {
                std::thread::sleep(FOCUS_POLL_INTERVAL);
                // After the pause, so no poll starts past the deadline.
                if Instant::now() >= until {
                    break;
                }
            }
            const REQUESTED: &str = "focus was requested; only reading back where it landed failed";
            let fresh = element.build_updated_cache(&self.single).map_err(|e| {
                classify(
                    handle,
                    "reading back focus",
                    &e,
                    self.process_identity(handle),
                )
                .after(Effect::Ran, REQUESTED)
            })?;
            let mut node = describe(&fresh, handle.to_owned());
            // Checked between the provider calls as well: each can take the
            // whole call timeout.
            let late = || {
                (Instant::now() >= until).then(|| {
                    ToolError::platform("reading back focus", "the readback ran out of time")
                        .after(Effect::Ran, REQUESTED)
                })
            };
            if node.focused {
                if let Some(e) = late() {
                    return Err(e);
                }
                self.complete_readback(handle, &fresh, &mut node, false, Some(until))?;
                return Ok(node);
            }
            // `None`: where focus is could not be read in time.
            let inside = if Instant::now() < until {
                self.automation
                    .get_focused_element()
                    .ok()
                    .and_then(|focused| self.within(focused, element, until))
            } else {
                None
            };
            if inside == Some(true) {
                if let Some(e) = late() {
                    return Err(e);
                }
                self.complete_readback(handle, &fresh, &mut node, false, Some(until))?;
                return Ok(node);
            }
            unknown = inside.is_none();
            focusable = cached_flag(&fresh, UIProperty::IsKeyboardFocusable);
        }
        if unknown {
            return Err(ToolError::platform(
                "reading back focus",
                "where keyboard focus is could not be read",
            )
            .after(
                Effect::Ran,
                "focus was requested; only reading back where it landed failed",
            ));
        }
        Err(ToolError::NotSupported(format!(
            "element `{handle}` accepted focus but keyboard focus is neither on it nor inside it{}; click it or use key tab to move focus",
            if focusable == Some(false) {
                " (it reports is_keyboard_focusable: false)"
            } else {
                ""
            }
        ))
        .after(
            Effect::Incidental,
            "the focus request went out and may have raised or activated its window",
        ))
    }

    /// The element's actions, read live, for error messages: the ones it
    /// offers, and the ones whose availability could not be read. Stopped
    /// after [`ANCESTOR_DEADLINE`] in all, since each read can take a
    /// provider's whole call timeout; the rest then count as unread.
    fn live_actions(&self, element: &UIElement) -> (Vec<&'static str>, Vec<&'static str>) {
        // One read, by the rule a node's own `actions` follows, so the two
        // never disagree.
        match element.build_updated_cache(&self.single) {
            Ok(fresh) if searchable(&fresh) => (
                describe(&fresh, String::new())
                    .actions
                    .iter()
                    .map(|a| a.name())
                    .collect(),
                Vec::new(),
            ),
            _ => (
                Vec::new(),
                ActionName::ALL.iter().map(|a| a.name()).collect(),
            ),
        }
    }

    fn unsupported(&self, handle: &str, element: &UIElement, action: ActionName) -> ToolError {
        let (supported, unread) = self.live_actions(element);
        ToolError::ActionUnsupported {
            element: handle.to_owned(),
            action: action.name(),
            supported,
            unread,
        }
    }

    /// Whether `element` offers the pattern `prop` names, read live. A failed
    /// read is classified, not taken for "no": an element the application
    /// removed must answer as stale, so the caller reads a fresh tree instead
    /// of concluding the action is unsupported.
    fn has_pattern(&self, handle: &str, element: &UIElement, prop: UIProperty) -> ToolResult<bool> {
        let value = element.get_property_value(prop).map_err(|e| {
            classify(
                handle,
                "reading its patterns",
                &e,
                self.process_identity(handle),
            )
        })?;
        // Anything but a boolean is an unreadable answer, not "no": taken
        // for "no", a `set_value` would go to the other pattern.
        of_type(value, VT_BOOL)
            .and_then(|v| TryInto::<bool>::try_into(v).ok())
            .ok_or_else(|| {
                ToolError::platform(
                    "reading its patterns",
                    "the provider answered with something other than a boolean",
                )
            })
    }

    /// Refuses `action` unless its pattern is available, read live.
    fn require(&self, handle: &str, element: &UIElement, action: ActionName) -> ToolResult<()> {
        let Some((pattern, _)) = pattern_of(action) else {
            return Err(self.unsupported(handle, element, action));
        };
        if self.has_pattern(handle, element, pattern)? {
            Ok(())
        } else {
            Err(self.unsupported(handle, element, action))
        }
    }

    fn prepare_action<'a>(
        &self,
        handle: &str,
        element: &UIElement,
        action: &'a Action,
    ) -> ToolResult<PreparedAction<'a>> {
        let pid = self.process_identity(handle);
        let lookup =
            |what: &'static str| move |e: uiautomation::Error| classify(handle, what, &e, pid);
        match action {
            Action::Invoke => {
                self.require(handle, element, ActionName::Invoke)?;
                element
                    .get_pattern::<UIInvokePattern>()
                    .map(PreparedAction::Invoke)
                    .map_err(lookup("invoke"))
            }
            Action::Toggle => {
                self.require(handle, element, ActionName::Toggle)?;
                element
                    .get_pattern::<UITogglePattern>()
                    .map(PreparedAction::Toggle)
                    .map_err(lookup("toggle"))
            }
            Action::Expand | Action::Collapse => {
                let (name, action) = if matches!(action, Action::Expand) {
                    ("expand", ActionName::Expand)
                } else {
                    ("collapse", ActionName::Collapse)
                };
                self.require(handle, element, action)?;
                let pattern = element
                    .get_pattern::<UIExpandCollapsePattern>()
                    .map_err(lookup(name))?;
                perform_expansion(
                    action,
                    || {
                        let value = element
                            .get_property_value(UIProperty::ExpandCollapseExpandCollapseState)
                            .map_err(lookup("reading expansion state"))?;
                        of_type(value, VT_I4)
                            .and_then(|value| TryInto::<i32>::try_into(value).ok())
                            .ok_or_else(|| {
                                ToolError::platform(
                                    "reading expansion state",
                                    "the provider did not return an integer state",
                                )
                            })
                    },
                    || {
                        Ok(if action == ActionName::Expand {
                            PreparedAction::Expand(pattern)
                        } else {
                            PreparedAction::Collapse(pattern)
                        })
                    },
                    || self.unsupported(handle, element, action),
                )
            }
            Action::ScrollIntoView => {
                self.require(handle, element, ActionName::ScrollIntoView)?;
                element
                    .get_pattern::<UIScrollItemPattern>()
                    .map(PreparedAction::ScrollIntoView)
                    .map_err(lookup("scroll_into_view"))
            }
            Action::SetValue(value) => perform_set_value(
                handle,
                value,
                |prop| self.has_pattern(handle, element, prop),
                |value| match value {
                    ValueWrite::Text(value) => element
                        .get_pattern::<UIValuePattern>()
                        .map(|pattern| PreparedAction::Text(pattern, value))
                        .map_err(lookup("set_value")),
                    ValueWrite::Range(number) => element
                        .get_pattern::<UIRangeValuePattern>()
                        .map(|pattern| PreparedAction::Range(pattern, number))
                        .map_err(lookup("set_value")),
                },
                || self.unsupported(handle, element, ActionName::SetValue),
            ),
            Action::Focus => perform_focus(
                || self.has_pattern(handle, element, UIProperty::IsKeyboardFocusable),
                || Ok(PreparedAction::Focus(element.clone())),
                || self.unsupported(handle, element, ActionName::Focus),
            ),
            Action::Select => {
                self.require(handle, element, ActionName::Select)?;
                element
                    .get_pattern::<UISelectionItemPattern>()
                    .map(PreparedAction::Select)
                    .map_err(lookup("select"))
            }
        }
    }

    fn perform(&self, handle: &str, element: &UIElement, action: &Action) -> ToolResult<()> {
        dispatch_while_current(
            || self.prepare_action(handle, element, action),
            || self.check_alive(handle, None).map(|_| ()),
            |prepared| {
                let (what, result) = prepared.dispatch();
                result.map_err(|error| {
                    match classify(handle, what, &error, self.process_identity(handle)) {
                        refused @ ToolError::Disabled { .. } => refused,
                        cause => cause.after(
                            Effect::MayHaveRun,
                            format!("{what} reached the application and then failed or timed out, so it may have run; read the tree before retrying"),
                        ),
                    }
                })
            },
        )
    }
}

/// A value is consumed only after its refresh's identity is verified. Both
/// traversal and action readback use this boundary; no second value is fetched.
fn value_while_current<T>(
    read: impl FnOnce() -> ToolResult<T>,
    validate: impl FnOnce(&T) -> ToolResult<()>,
) -> ToolResult<T> {
    let value = read()?;
    validate(&value)?;
    Ok(value)
}

/// Finish all provider preparation before the final identity check. Failed
/// preparation is checked too: replacement is gone, not unsupported on the old
/// element. Only dispatch can cause effects.
fn dispatch_while_current<T>(
    prepare: impl FnOnce() -> ToolResult<T>,
    validate: impl FnOnce() -> ToolResult<()>,
    dispatch: impl FnOnce(T) -> ToolResult<()>,
) -> ToolResult<()> {
    let prepared = prepare();
    let current = validate();
    // An observed removal is final even when the follow-up identity read
    // itself fails transiently. Keep the evidence that retires this handle.
    if prepared.as_ref().is_err_and(|error| error.code() == "gone") {
        return prepared.map(|_| ());
    }
    current?;
    dispatch(prepared?)
}

/// Acquired capabilities: dispatch performs only the mutation, with no more
/// property reads or pattern lookups after identity validation.
enum PreparedAction<'a> {
    Invoke(UIInvokePattern),
    Toggle(UITogglePattern),
    Expand(UIExpandCollapsePattern),
    Collapse(UIExpandCollapsePattern),
    ScrollIntoView(UIScrollItemPattern),
    Text(UIValuePattern, &'a str),
    Range(UIRangeValuePattern, f64),
    Focus(UIElement),
    Select(UISelectionItemPattern),
}

impl PreparedAction<'_> {
    fn dispatch(self) -> (&'static str, uiautomation::Result<()>) {
        match self {
            Self::Invoke(pattern) => ("invoke", pattern.invoke()),
            Self::Toggle(pattern) => ("toggle", pattern.toggle()),
            Self::Expand(pattern) => ("expand", pattern.expand()),
            Self::Collapse(pattern) => ("collapse", pattern.collapse()),
            Self::ScrollIntoView(pattern) => ("scroll_into_view", pattern.scroll_into_view()),
            Self::Text(pattern, value) => ("set_value", pattern.set_value(value)),
            Self::Range(pattern, value) => ("set_value", pattern.set_value(value)),
            Self::Focus(element) => ("focus", element.set_focus()),
            Self::Select(pattern) => ("select", pattern.select()),
        }
    }
}

fn hit_while_current<S>(
    state: &mut S,
    hit: impl FnOnce(&mut S) -> ToolResult<bool>,
    check: impl FnOnce(&mut S) -> ToolResult<()>,
) -> ToolResult<bool> {
    let result = hit(state);
    check(state)?;
    result
}

fn gone_node(mut node: Node) -> Node {
    node.gone = true;
    node.actions.clear();
    node.value = None;
    node.checked = None;
    node.expanded = None;
    node.selected = None;
    node
}

/// Admit each identity-provider call independently. A call already running
/// remains bounded by UIA's transaction timeout; it cannot be interrupted.
fn before_identity_call(handle: &str, deadline: Option<Instant>, now: Instant) -> ToolResult<()> {
    if deadline.is_some_and(|deadline| now >= deadline) {
        Err(ToolError::Timeout {
            timeout_ms: 0,
            what: format!("revalidating element `{handle}` before reading its subtree"),
            summary: String::new(),
        })
    } else {
        Ok(())
    }
}

/// Final subtree validation is a separate bounded phase: `check_alive`
/// reads the held runtime ID, refreshes the cache, then reads the fresh
/// runtime ID. Reserve one transaction timeout for each of those three
/// calls rather than charging the entire phase to one call's allowance.
/// Calls start only before this deadline; an admitted call can finish up
/// to one transaction timeout later. Local scheduling and OS identity
/// lookups are not a hard wall-clock bound. Failed verification never
/// publishes the partial tree.
fn validate_subtree(
    now: Instant,
    validate: impl FnOnce(Instant) -> ToolResult<()>,
) -> ToolResult<()> {
    let allowance =
        std::time::Duration::from_millis(3 * u64::from(crate::os::UIA_TRANSACTION_TIMEOUT_MS));
    validate(now + allowance)
}

/// Stop traversal early enough that fast final validation commonly fits
/// within the original request budget. Slow final validation has its own
/// allowance in `validate_subtree`; this reserve is not its deadline.
fn subtree_traversal_deadline(now: Instant, deadline: Instant) -> Instant {
    let reserve = (deadline.saturating_duration_since(now) / 2).min(
        std::time::Duration::from_millis(u64::from(crate::os::UIA_TRANSACTION_TIMEOUT_MS)),
    );
    deadline.checked_sub(reserve).unwrap_or(now)
}

/// A definitively replaced root invalidates all handles touched by its walk:
/// some may have been issued from its replacement before the final check.
fn finish_subtree(
    read: Read,
    touched: &HashSet<String>,
    validation: ToolResult<()>,
    mut invalidate: impl FnMut(&str),
) -> ToolResult<Read> {
    if let Err(error) = validation {
        if error.code() == "gone" {
            for handle in touched {
                invalidate(handle);
            }
        }
        return Err(error);
    }
    Ok(read)
}

fn constrain_actions(node: &mut Node, identifiable: bool) {
    if !identifiable {
        node.actions.clear();
    }
}

/// Unknown enabled state is omitted, not represented as a disabled control.
fn enabled_state(node: &mut Node, enabled: Option<bool>) {
    node.disabled = enabled == Some(false);
    if enabled.is_none() {
        node.unread_states.push("disabled");
    }
}

/// Direct element focus is offered only by a readable, live focusable flag.
/// Read failures stay distinct from an unsupported action and emit no focus.
fn perform_focus<T>(
    focusable: impl FnOnce() -> ToolResult<bool>,
    focus: impl FnOnce() -> ToolResult<T>,
    unsupported: impl FnOnce() -> ToolError,
) -> ToolResult<T> {
    if !focusable()? {
        return Err(unsupported());
    }
    focus()
}

/// Readback follows an action that already succeeded. Even its final value
/// cache may block long enough for the process to be replaced; check after
/// all provider work, on failed reads as well as successful ones.
fn readback_while_current(
    handle: &str,
    started: Option<u64>,
    read: impl FnOnce() -> ToolResult<()>,
    process_now: impl FnOnce() -> ToolResult<Option<u64>>,
) -> ToolResult<()> {
    let result = read();
    let current = process_now();
    if result.as_ref().is_err_and(|error| error.code() == "gone") {
        return result;
    }
    let current = current.map_err(|error| {
        error.after(
            Effect::Ran,
            "the action succeeded; its process identity could not be read back",
        )
    })?;
    if started.is_none() || current != started {
        return Err(ToolError::gone_element(
            handle,
            "its process exited or was replaced during readback",
        )
        .after(
            Effect::Ran,
            "the action itself succeeded; its original element is gone, so do not repeat it",
        ));
    }
    result
}

/// Preserve conclusive replacement evidence from either observation. An
/// unreadable process identity alone must not retire an otherwise unchanged
/// element, and is not proof that it is still safe to use.
fn identity_after_refresh(
    started: u64,
    refresh: impl FnOnce() -> Option<bool>,
    process_now: impl FnOnce() -> ToolResult<Option<u64>>,
) -> ToolResult<Option<bool>> {
    let same = refresh();
    let current = process_now();
    if same == Some(false) {
        return Ok(Some(false));
    }
    if current? == Some(started) {
        Ok(same)
    } else {
        Ok(Some(false))
    }
}

/// The shared state rule for advertised and directly requested transitions.
fn expansion_allows(state: Option<i32>, action: ActionName) -> bool {
    let partly = state == Some(ExpandCollapseState::PartiallyExpanded as i32);
    match action {
        ActionName::Expand => partly || state == Some(ExpandCollapseState::Collapsed as i32),
        ActionName::Collapse => partly || state == Some(ExpandCollapseState::Expanded as i32),
        _ => false,
    }
}

fn perform_expansion<T>(
    action: ActionName,
    read: impl FnOnce() -> ToolResult<i32>,
    perform: impl FnOnce() -> ToolResult<T>,
    unsupported: impl FnOnce() -> ToolError,
) -> ToolResult<T> {
    let state = read()?;
    if !(0..=3).contains(&state) {
        return Err(ToolError::platform(
            "reading expansion state",
            "the provider returned an invalid expansion state",
        ));
    }
    if !expansion_allows(Some(state), action) {
        return Err(unsupported());
    }
    perform()
}

/// A write dispatched only after its pattern is known to be writable.
#[derive(Debug, PartialEq)]
enum ValueWrite<'a> {
    Text(&'a str),
    Range(f64),
}

fn perform_set_value<'a, T>(
    handle: &str,
    value: &'a str,
    mut flag: impl FnMut(UIProperty) -> ToolResult<bool>,
    write: impl FnOnce(ValueWrite<'a>) -> ToolResult<T>,
    unsupported: impl FnOnce() -> ToolError,
) -> ToolResult<T> {
    if flag(UIProperty::IsValuePatternAvailable)? && !flag(UIProperty::ValueIsReadOnly)? {
        return write(ValueWrite::Text(value));
    }
    if flag(UIProperty::IsRangeValuePatternAvailable)? && !flag(UIProperty::RangeValueIsReadOnly)? {
        let number = value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| {
                let shown: String = value.chars().take(40).collect();
                let more = if value.chars().nth(40).is_some() { "…" } else { "" };
                ToolError::InvalidArgument(format!(
                    "element `{handle}` takes a finite number (RangeValue pattern); `{shown}{more}` is not one"
                ))
            })?;
        return write(ValueWrite::Range(number));
    }
    Err(unsupported())
}

impl AccessibilityBackend for Uia {
    fn available(&self) -> ToolResult<()> {
        Ok(())
    }

    fn validate_handle(&mut self, element: &str) -> ToolResult<()> {
        self.elements.get(element).map(|_| ())
    }

    fn invalidate_handle(&mut self, element: &str) {
        self.elements.invalidate(element);
    }

    fn tree(
        &mut self,
        windows: &[u32],
        max_depth: usize,
        max_nodes: usize,
        deadline: Instant,
    ) -> ToolResult<Read> {
        self.starts.clear();
        let mut roots = Vec::with_capacity(windows.len());
        let mut walk = Walk {
            max_depth,
            budget: 0,
            bytes: READ_BYTES,
            deadline,
            seen: HashSet::new(),
            truncated: false,
        };
        let mut spare = max_nodes.clamp(1, NODE_BUDGET);
        let mut failed = None;
        for (read, &window) in windows.iter().enumerate() {
            if spare == 0 || walk.bytes == 0 || Instant::now() >= deadline {
                walk.truncated = true;
                break;
            }
            // Each window is held to its share of what is left, so a large
            // first window cannot hide the rest of a process's windows from
            // `find` and `wait_for`; what a small one leaves goes on.
            let share = (spare / (windows.len() - read)).max(1).min(spare);
            walk.budget = share;
            let root = match self
                .automation
                .element_from_handle_build_cache(hwnd(window), &self.single)
            {
                Ok(root) => root,
                // Closed between listing and reading (a splash screen, a
                // popup): the rest of the target is still worth reading.
                // A root fetch that fails still spent a cross-process call,
                // so it is charged like one: a process with thousands of
                // failing windows cannot outrun the budget.
                // Gone only if the window itself is: an element-not-available
                // or a disconnect from a window that still exists is a
                // provider failing, and the read says it is incomplete.
                Err(e) if is_gone(e.code()) => {
                    spare -= 1;
                    if crate::os::window_pid(window).is_some() {
                        walk.truncated = true;
                        failed.get_or_insert((window, e));
                    }
                    continue;
                }
                // Failing (hung, timed out): the rest is still read, and the
                // read says it is incomplete.
                Err(e) => {
                    spare -= 1;
                    walk.truncated = true;
                    failed.get_or_insert((window, e));
                    continue;
                }
            };
            if let Some(mut node) = self.build(&root, 0, &mut walk) {
                node.native_window = Some(window);
                roots.push(node);
            }
            spare = spare.saturating_sub(share - walk.budget);
        }
        if roots.is_empty() {
            if let Some((window, e)) = failed {
                return Err(ToolError::platform(
                    format!("reading the tree of window {window}"),
                    e,
                ));
            }
            if !walk.truncated {
                return Err(ToolError::NotFound(
                    "the target's windows closed while they were being read".into(),
                ));
            }
        }
        Ok(Read {
            roots,
            truncated: walk.truncated,
        })
    }

    fn subtree(
        &mut self,
        element: &str,
        max_depth: usize,
        max_nodes: usize,
        deadline: Instant,
    ) -> ToolResult<Read> {
        self.starts.clear();
        let past = || ToolError::Timeout {
            timeout_ms: 0,
            what: format!("reading the subtree of `{element}` (the read's time ran out first)"),
            summary: String::new(),
        };
        if Instant::now() >= deadline {
            return Err(past());
        }
        // Read fresh (`alive` returns the element as read just now): the
        // held object's cache is from the read that issued it.
        let root = self.alive_until(element, Some(deadline))?;
        if Instant::now() >= deadline {
            return Err(past());
        }
        let mut walk = Walk {
            max_depth,
            budget: max_nodes.clamp(1, NODE_BUDGET),
            bytes: READ_BYTES,
            deadline: subtree_traversal_deadline(Instant::now(), deadline),
            seen: HashSet::new(),
            truncated: false,
        };
        let roots: Vec<Node> = self.build(&root, 0, &mut walk).into_iter().collect();
        // Even an incomplete walk can outlive its root. Never return a new
        // proxy's descendants under the caller's original root handle.
        let validation = validate_subtree(Instant::now(), |validation_deadline| {
            self.alive_until(element, Some(validation_deadline))
                .map(|_| ())
        });
        finish_subtree(
            Read {
                roots,
                truncated: walk.truncated,
            },
            &walk.seen,
            validation,
            |handle| self.elements.invalidate(handle),
        )
    }

    fn act(&mut self, handle: &str, action: &Action) -> ToolResult<Node> {
        let element = self.alive(handle)?;
        if let Err(error) = self.perform(handle, &element, action) {
            if error.code() == "gone" {
                self.elements.invalidate(handle);
            }
            return Err(error);
        }
        if matches!(action, Action::Focus) {
            return self
                .focused(handle, &element)
                .map_err(|error| self.remember_error(handle, error));
        }
        // The reply keeps the caller's handle either way: a readback that
        // minted another one would hide which element this was.
        let result = match element.build_updated_cache(&self.single) {
            Ok(fresh) => {
                let mut node = describe(&fresh, handle.to_owned());
                // A value that cannot be read back is not a control without
                // one: the action ran, and what it left is unknown.
                self.complete_readback(
                    handle,
                    &fresh,
                    &mut node,
                    matches!(action, Action::Toggle),
                    None,
                )?;
                Ok(node)
            }
            // The action succeeded and took its own element away (a Close or
            // Delete button, a navigation): that is the action's result, not a
            // failure. Answer with the node as it was just before, marked.
            // Only the provider saying the element is not available is
            // evidence it went; a disconnect or a failed call (a provider
            // restarting) leaves the outcome unknown, reported below.
            Err(e) if e.code() == E_ELEMENT_NOT_AVAILABLE => {
                self.elements.invalidate(handle);
                // Its identity only: the value and toggle state cached
                // before the action are not what the action left behind.
                Ok(gone_node(describe(&element, handle.to_owned())))
            }
            // The action itself succeeded; only reading the result failed.
            // Say so, or a retry repeats a destructive action.
            Err(e) => Err(classify(handle, "reading back", &e, self.process_identity(handle)).after(
                Effect::Ran,
                "the action itself succeeded; only reading the element afterwards failed, so do not repeat it",
            )),
        };
        result.map_err(|error| self.remember_error(handle, error))
    }

    fn click_point(&mut self, handle: &str) -> ToolResult<ClickPoint> {
        let element = self.alive(handle)?;
        let pid = self.process_identity(handle);
        let point = element.get_clickable_point().map_err(|e| {
            self.remember_error(
                handle,
                classify(handle, "reading its clickable point", &e, pid),
            )
        })?;
        let (x, y) = if let Some(p) = point {
            (p.get_x(), p.get_y())
        } else {
            let r = element.get_bounding_rectangle().map_err(|e| {
                self.remember_error(handle, classify(handle, "reading its bounds", &e, pid))
            })?;
            let rect = Rect::from_ltrb(r.get_left(), r.get_top(), r.get_right(), r.get_bottom());
            if rect.width == 0 || rect.height == 0 {
                return Err(ToolError::NotFound(format!(
                    "element `{handle}` has no on-screen area to click (offscreen or collapsed)"
                )));
            }
            // No clickable point from UIA: the centre is only a guess, so it
            // must hit the element itself (or a descendant) — a sibling
            // covering it would otherwise take the click.
            let (x, y) = rect.center();
            if !self.hits_element(handle, x, y, &element)? {
                return Err(ToolError::NotFound(format!(
                    "element `{handle}` reports no clickable point and its centre ({x}, {y}) is covered by another element; use invoke, or click a point you have verified"
                )));
            }
            (x, y)
        };
        Ok(ClickPoint {
            x,
            y,
            window: self.top_level_window(&element),
        })
    }

    fn hits(&mut self, handle: &str, x: i32, y: i32) -> ToolResult<bool> {
        let element = self.alive(handle)?;
        self.hits_element(handle, x, y, &element)
    }

    fn focus_window(&mut self, window: u32) -> ToolResult<()> {
        self.automation
            .element_from_handle(hwnd(window))
            .and_then(|e| e.set_focus())
            .map_err(platform("focusing the window through UI Automation"))
    }
}

/// A [`Node`] from `element`'s cached properties under handle `id`,
/// children empty.
fn describe(element: &UIElement, id: String) -> Node {
    let offered = |prop| cached_bool(element, prop);
    let has_text_value = offered(UIProperty::IsValuePatternAvailable);
    let has_range = offered(UIProperty::IsRangeValuePatternAvailable);
    // A text control's value, or a slider's number when it exposes only
    // RangeValue — what `set_value` writes, so a caller can read it back.
    // A text control's value is read apart ([`Uia::read_value`]); a
    // slider's number is small and comes with the walk.
    let value = if has_text_value {
        None
    } else if has_range {
        cached_f64(element, UIProperty::RangeValueValue).map(|number| number.to_string())
    } else {
        None
    };
    let checked = offered(UIProperty::IsTogglePatternAvailable)
        .then(|| cached_i32(element, UIProperty::ToggleToggleState).and_then(checked_state))
        .flatten();
    let expansion = offered(UIProperty::IsExpandCollapsePatternAvailable)
        .then(|| cached_i32(element, UIProperty::ExpandCollapseExpandCollapseState))
        .flatten();
    let expanded = expansion.and_then(expanded_state);
    // A value the provider marks read-only is shown, not offered to set.
    let read_only = |prop: UIProperty| match prop {
        UIProperty::IsValuePatternAvailable => {
            cached_flag(element, UIProperty::ValueIsReadOnly) != Some(false)
        }
        UIProperty::IsRangeValuePatternAvailable => {
            cached_flag(element, UIProperty::RangeValueIsReadOnly) != Some(false)
        }
        _ => false,
    };
    let selected = offered(UIProperty::IsSelectionItemPatternAvailable)
        .then(|| cached_flag(element, UIProperty::SelectionItemIsSelected))
        .flatten();
    let focusable = cached_bool(element, UIProperty::IsKeyboardFocusable);
    let mut actions = Vec::new();
    for &(prop, offers) in PATTERNS {
        if !offered(prop) || read_only(prop) {
            continue;
        }
        for &action in offers {
            // Which of expand and collapse applies is the element's state
            // (a partly expanded one takes both); a leaf offers neither.
            let applies = match action {
                ActionName::Expand | ActionName::Collapse => expansion_allows(expansion, action),
                _ => true,
            };
            if applies && !actions.contains(&action) {
                actions.push(action);
            }
        }
    }
    if focusable {
        actions.push(ActionName::Focus);
    }
    let mut unread_states = Vec::new();
    if cached_flag(element, UIProperty::HasKeyboardFocus).is_none() {
        unread_states.push("focused");
    }
    for (state, unread) in [
        ("checked", checked.is_none()),
        ("expanded", expanded.is_none()),
        ("selected", selected.is_none()),
        ("value", value.is_none()),
    ] {
        if unread {
            unread_states.push(state);
        }
    }
    let (role, native_role) = role(element);
    let mut node = Node {
        id,
        role,
        native_role,
        name: non_empty(cached_str(element, UIProperty::Name)),
        value,
        automation_id: non_empty(cached_str(element, UIProperty::AutomationId)),
        class_name: non_empty(cached_str(element, UIProperty::ClassName)),
        rect: element
            .get_cached_bounding_rectangle()
            .ok()
            .map(|r| Rect::from_ltrb(r.get_left(), r.get_top(), r.get_right(), r.get_bottom())),
        disabled: false,
        focused: cached_bool(element, UIProperty::HasKeyboardFocus),
        focusable,
        checked,
        expanded,
        selected,
        actions,
        window: None,
        children: Vec::new(),
        omitted_children: None,
        children_unread: false,
        gone: false,
        unmatchable: !searchable(element),
        unread_states,
        has_text_value,
        native_window: None,
    };
    enabled_state(&mut node, cached_flag(element, UIProperty::IsEnabled));
    refine_role(
        &mut node,
        cached_flag(element, UIProperty::IsPassword),
        cached_flag(element, UIProperty::IsDialog),
    );
    node
}

/// Consume the value and its password discriminant as one observation. The
/// newer flag replaces the earlier role classification before a value can
/// be exposed; an unreadable flag makes the role unmatchable as well.
fn consume_value_snapshot(
    node: &mut Node,
    password: Option<bool>,
    value: impl FnOnce() -> Option<String>,
) -> ToolResult<()> {
    if matches!(node.role, Role::TextInput | Role::PasswordInput) {
        node.role = Role::TextInput;
        refine_role(node, password, None);
    }
    node.value = None;
    if !node.unread_states.contains(&"value") {
        node.unread_states.push("value");
    }
    let unread = || {
        ToolError::platform(
            "reading its value",
            "the value or its password flag could not be read",
        )
    };
    match password {
        Some(true) => return Ok(()),
        None => return Err(unread()),
        Some(false) => {}
    }
    let value = value().ok_or_else(unread)?;
    if value.chars().count() <= MAX_PROPERTY_CHARS {
        node.unread_states.retain(|state| *state != "value");
    }
    node.value = Some(clip(value));
    Ok(())
}

/// Role discriminants must be readable before either alternative can be
/// reported or matched. Unrelated properties do not affect other roles.
fn refine_role(node: &mut Node, password: Option<bool>, dialog: Option<bool>) {
    let (flag, alternative) = match node.role {
        Role::TextInput => (password, Role::PasswordInput),
        Role::Window => (dialog, Role::Dialog),
        _ => return,
    };
    match flag {
        Some(true) => node.role = alternative,
        Some(false) => {}
        None => {
            node.role = Role::Unknown;
            node.unmatchable = true;
        }
    }
}

/// The element's role in the tools' vocabulary, and the UI Automation
/// control type name (`Button`, or `Custom(<id>)` for an id the
/// `uiautomation` crate does not name, rather than a failure that would
/// hide the element from search).
fn role(element: &UIElement) -> (Role, String) {
    let (fallback, native) = match element.get_cached_control_type() {
        Ok(known) => (role_of(known), format!("{known:?}")),
        Err(_) => (
            Role::Unknown,
            cached_i32(element, UIProperty::ControlType)
                .map_or_else(|| "Unknown".to_owned(), |id| format!("Custom({id})")),
        ),
    };
    let precise = cached_str(element, UIProperty::AriaRole)
        .as_deref()
        .and_then(role_from_aria);
    (precise.unwrap_or(fallback), native)
}

/// UI Automation's control types in the tools' vocabulary. What has no
/// counterpart there is `Unknown`, with the native name still reported.
fn role_of(control: ControlType) -> Role {
    match control {
        ControlType::Button | ControlType::SplitButton => Role::Button,
        ControlType::CheckBox => Role::CheckBox,
        ControlType::RadioButton => Role::RadioButton,
        ControlType::ComboBox => Role::ComboBox,
        ControlType::Edit => Role::TextInput,
        ControlType::Hyperlink => Role::Link,
        ControlType::Image => Role::Image,
        ControlType::Text => Role::Label,
        ControlType::List => Role::List,
        ControlType::ListItem => Role::ListItem,
        ControlType::Menu => Role::Menu,
        ControlType::MenuBar => Role::MenuBar,
        ControlType::MenuItem => Role::MenuItem,
        ControlType::ProgressBar => Role::ProgressIndicator,
        ControlType::ScrollBar => Role::ScrollBar,
        ControlType::Slider => Role::Slider,
        ControlType::Spinner => Role::SpinButton,
        ControlType::StatusBar => Role::Status,
        ControlType::Tab => Role::TabList,
        ControlType::TabItem => Role::Tab,
        ControlType::ToolBar | ControlType::AppBar => Role::Toolbar,
        ControlType::ToolTip => Role::Tooltip,
        ControlType::Tree => Role::Tree,
        ControlType::TreeItem => Role::TreeItem,
        ControlType::Group | ControlType::Header => Role::Group,
        ControlType::DataGrid => Role::Grid,
        ControlType::DataItem => Role::Row,
        ControlType::HeaderItem => Role::ColumnHeader,
        ControlType::Table => Role::Table,
        ControlType::Document => Role::Document,
        ControlType::Window => Role::Window,
        ControlType::Pane => Role::Pane,
        ControlType::TitleBar => Role::TitleBar,
        ControlType::Separator => Role::Splitter,
        ControlType::Calendar
        | ControlType::Custom
        | ControlType::Thumb
        | ControlType::SemanticZoom => Role::Unknown,
    }
}

/// The boolean reports only completed expansion states: `true` is fully
/// expanded and `false` fully collapsed. Partial expansion, a leaf, and an
/// unreadable state are neither, so they cannot satisfy either boolean wait.
fn expanded_state(state: i32) -> Option<bool> {
    match state {
        s if s == ExpandCollapseState::Expanded as i32 => Some(true),
        s if s == ExpandCollapseState::Collapsed as i32 => Some(false),
        _ => None,
    }
}

/// Whether the provider reported every property a node shows: what a
/// search matches on (name, control type, automation id), and the states and
/// patterns an agent acts on. A failed read is not the same as an empty
/// value or `false` — a control that reads as disabled because the read
/// failed would be skipped — so it marks the read incomplete.
fn searchable(element: &UIElement) -> bool {
    // The typed reads `describe` and `register` make: a variant of the wrong
    // type is there but reads as missing or false, so a missing name would
    // match `name: ""` and a missing pid would issue a handle every action
    // refuses.
    let offered = |prop| cached_bool(element, prop);
    cached_i32(element, UIProperty::ControlType).is_some()
        && cached_pid(element).is_some()
        && cached_str(element, UIProperty::AriaRole).is_some()
        && cached_str(element, UIProperty::Name).is_some()
        && cached_str(element, UIProperty::AutomationId).is_some()
        && cached_str(element, UIProperty::ClassName).is_some()
        && element.get_cached_bounding_rectangle().is_ok()
        && cached_flag(element, UIProperty::IsEnabled).is_some()
        && cached_flag(element, UIProperty::HasKeyboardFocus).is_some()
        && cached_flag(element, UIProperty::IsKeyboardFocusable).is_some()
        && PATTERNS
            .iter()
            .all(|&(prop, _)| cached_flag(element, prop).is_some())
        && (!offered(UIProperty::IsValuePatternAvailable)
            || cached_flag(element, UIProperty::ValueIsReadOnly).is_some())
        && (!offered(UIProperty::IsRangeValuePatternAvailable)
            || cached_flag(element, UIProperty::RangeValueIsReadOnly).is_some())
        && (!offered(UIProperty::IsTogglePatternAvailable)
            || cached_i32(element, UIProperty::ToggleToggleState)
                .and_then(checked_state)
                .is_some())
        && (!offered(UIProperty::IsExpandCollapsePatternAvailable)
            || cached_i32(element, UIProperty::ExpandCollapseExpandCollapseState)
                .is_some_and(|s| (0..=3).contains(&s)))
        && (!offered(UIProperty::IsSelectionItemPatternAvailable)
            || cached_flag(element, UIProperty::SelectionItemIsSelected).is_some())
        && (!offered(UIProperty::IsRangeValuePatternAvailable)
            || cached_f64(element, UIProperty::RangeValueValue).is_some())
}

/// The element a walker step reached: `None` at the end of the children,
/// and also where the provider failed (it disconnected, the element went),
/// which marks the read truncated rather than passing for the end.
fn walked(step: uiautomation::Result<UIElement>, walk: &mut Walk) -> Option<UIElement> {
    match step {
        Ok(element) => Some(element),
        // A walker with nowhere to go returns a null element, which
        // windows-rs reports as an error with no code.
        Err(e) if e.code() == 0 => None,
        Err(_) => {
            walk.truncated = true;
            None
        }
    }
}

/// What an element handle is keyed by. An element without a runtime id, or
/// a second element claiming one already issued in the same read, gets an
/// identity of its own that no runtime id can equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Identity {
    Runtime(Vec<i32>),
    Anonymous(u64),
}

/// One read's progress: what it may still spend, and what it has emitted.
struct Walk {
    max_depth: usize,
    budget: usize,
    /// String bytes it may still report.
    bytes: usize,
    deadline: Instant,
    seen: HashSet<String>,
    truncated: bool,
}

impl Walk {
    /// Whether the read must stop fetching; once it has, the read is
    /// marked truncated.
    fn exhausted(&mut self) -> bool {
        let out = self.budget == 0 || self.bytes == 0 || Instant::now() >= self.deadline;
        self.truncated |= out;
        out
    }
}

fn node_request(
    automation: &UIAutomation,
    scope: TreeScope,
) -> Result<UICacheRequest, uiautomation::Error> {
    let request = automation.create_cache_request()?;
    for &prop in NODE_PROPERTIES {
        request.add_property(prop)?;
    }
    for &(prop, _) in PATTERNS {
        request.add_property(prop)?;
    }
    // Newer than the rest (Windows 10 1809): without it, Window controls
    // cannot be classified as ordinary windows or dialogs.
    if request.add_property(UIProperty::IsDialog).is_err() {
        tracing::debug!("UI Automation does not know IsDialog; window roles remain unknown");
    }
    request.set_tree_scope(scope)?;
    request.set_tree_filter(automation.get_control_view_condition()?)?;
    Ok(request)
}

/// xcap's window id on Windows is the `HWND` value.
fn hwnd(window: u32) -> Handle {
    Handle::from(window as isize)
}

/// Variant type codes of the properties read here. A value is taken only as
/// the type UI Automation defines for its property: the `uiautomation`
/// conversions coerce any number, bool or numeric string, so a provider's
/// `VT_R8` 0.6 would read as a toggle that is on.
const VT_I4: u16 = 3;
const VT_R8: u16 = 5;
const VT_BSTR: u16 = 8;
const VT_BOOL: u16 = 11;

/// `v` if it has the variant type `vt`.
fn of_type(v: uiautomation::variants::Variant, vt: u16) -> Option<uiautomation::variants::Variant> {
    (v.get_type().0 == vt).then_some(v)
}

fn cached_of(
    element: &UIElement,
    prop: UIProperty,
    vt: u16,
) -> Option<uiautomation::variants::Variant> {
    element
        .get_cached_property_value(prop)
        .ok()
        .and_then(|v| of_type(v, vt))
}

/// A cached boolean, `None` when unreadable or of another type.
fn cached_flag(element: &UIElement, prop: UIProperty) -> Option<bool> {
    flag_value(element.get_cached_property_value(prop).ok())
}

fn flag_value(value: Option<uiautomation::variants::Variant>) -> Option<bool> {
    value
        .and_then(|value| of_type(value, VT_BOOL))
        .and_then(|value| TryInto::<bool>::try_into(value).ok())
}

fn cached_bool(element: &UIElement, prop: UIProperty) -> bool {
    cached_flag(element, prop).unwrap_or(false)
}

fn cached_i32(element: &UIElement, prop: UIProperty) -> Option<i32> {
    cached_of(element, prop, VT_I4).and_then(|v| TryInto::<i32>::try_into(v).ok())
}

/// A cached number, `None` unless it is a finite one (a slider at `NaN` is
/// no position).
fn cached_f64(element: &UIElement, prop: UIProperty) -> Option<f64> {
    cached_of(element, prop, VT_R8)
        .and_then(|v| TryInto::<f64>::try_into(v).ok())
        .filter(|n| n.is_finite())
}

/// The process id an element reports, `None` unless it is a real one.
fn cached_pid(element: &UIElement) -> Option<u32> {
    cached_i32(element, UIProperty::ProcessId)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid != 0)
}

/// A cached string: a `VT_BSTR`, or `VT_EMPTY` as the empty string. The
/// cache read used here substitutes a property's default for one a provider
/// does not supply, so empty never means "unsupported"; some proxies pass
/// `VT_EMPTY` through where the default (an empty string) would be.
fn cached_str(element: &UIElement, prop: UIProperty) -> Option<String> {
    const VT_EMPTY: u16 = 0;
    let v = element.get_cached_property_value(prop).ok()?;
    match v.get_type().0 {
        VT_EMPTY => Some(String::new()),
        VT_BSTR => TryInto::<String>::try_into(v).ok(),
        _ => None,
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty()).map(clip)
}

/// `s` cut to [`MAX_PROPERTY_CHARS`], ending in `…` when it was longer
/// (see [`Node::is_clipped`]).
fn clip(s: String) -> String {
    match s.char_indices().nth(MAX_PROPERTY_CHARS) {
        None => s,
        // A new string: truncating would keep the provider-sized buffer.
        Some((at, _)) => {
            let mut cut = String::with_capacity(at + '…'.len_utf8());
            cut.push_str(&s[..at]);
            cut.push('…');
            cut
        }
    }
}

/// `None` for a value that is no toggle state: an unreadable state, not
/// "mixed".
fn checked_state(state: i32) -> Option<Checked> {
    match state {
        s if s == ToggleState::On as i32 => Some(Checked::True),
        s if s == ToggleState::Off as i32 => Some(Checked::False),
        s if s == ToggleState::Indeterminate as i32 => Some(Checked::Mixed),
        _ => None,
    }
}

/// Whether a UIA failure means the element, its window or its process is
/// gone rather than that the call is wrong.
fn is_gone(code: i32) -> bool {
    matches!(code, E_ELEMENT_NOT_AVAILABLE | E_INVALID_WINDOW) || is_disconnected(code)
}

/// Whether a UIA failure is the provider's process having gone away, or its
/// connection: one while the process still runs is the provider failing,
/// not the element going.
fn is_disconnected(code: i32) -> bool {
    matches!(
        code,
        E_DISCONNECTED | E_SERVER_UNAVAILABLE | E_CALL_FAILED | E_NOT_CONNECTED
    )
}

/// `UIA_E_TIMEOUT`: the provider did not answer within the call timeout.
const E_TIMEOUT: i32 = 0x8013_1505_u32 as i32;

/// Maps a UIA failure on `handle` during `what` to the error the agent can
/// act on. The held process identity includes its start time: a disconnect
/// from that same running process is a provider failure, but a reused pid
/// belongs to a replacement and the original element is gone.
fn classify(
    handle: &str,
    what: &'static str,
    e: &uiautomation::Error,
    identity: Option<(u32, u64)>,
) -> ToolError {
    classify_code(
        handle,
        what,
        e.code(),
        e,
        identity,
        crate::os::process_started_checked,
    )
}

/// [`classify`] for a bare HRESULT and its message.
fn classify_code(
    handle: &str,
    what: &'static str,
    code: i32,
    e: &dyn std::fmt::Display,
    identity: Option<(u32, u64)>,
    process_now: impl FnOnce(u32) -> ToolResult<Option<u64>>,
) -> ToolError {
    let gone = identity
        .is_some_and(|(pid, started)| process_now(pid).is_ok_and(|now| now != Some(started)));
    match code {
        E_ELEMENT_NOT_AVAILABLE => ToolError::gone_element(handle, "the application removed it"),
        E_INVALID_WINDOW => ToolError::gone_element(handle, "its window has closed"),
        _ if is_disconnected(code) && gone => {
            ToolError::gone_element(handle, "its process has exited")
        }
        _ if is_disconnected(code) => ToolError::platform(
            format!("{what} on element `{handle}`"),
            "the accessibility provider disconnected without confirmed process exit",
        ),
        E_ELEMENT_NOT_ENABLED => ToolError::Disabled {
            element: handle.to_owned(),
            action: what,
        },
        E_TIMEOUT => ToolError::Timeout {
            timeout_ms: u64::from(crate::os::UIA_TRANSACTION_TIMEOUT_MS),
            what: format!(
                "{what} on element `{handle}` (the application did not answer; a modal dialog it opened can hold the call)"
            ),
            summary: String::new(),
        },
        _ => ToolError::platform(format!("{what} on element `{handle}`"), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool in the wire vocabulary has a pattern here, so none answers
    /// `action_unsupported` on an element that offers it. `pattern_of` ends in
    /// a wildcard (the vocabulary is `#[non_exhaustive]`), so this is the pin.
    #[test]
    fn every_action_name_has_a_uia_pattern() {
        for &action in ActionName::ALL {
            assert!(
                pattern_of(action).is_some(),
                "`{action}` has no UIA pattern"
            );
        }
    }

    #[test]
    fn fresh_value_password_flag_replaces_the_earlier_text_role() {
        use uiautomation::variants::Variant;

        for (fresh_flag, expected_role, readable) in [
            (Some(Variant::from(true)), Role::PasswordInput, true),
            (None, Role::Unknown, false),
            (Some(Variant::from(1_i32)), Role::Unknown, false),
            (Some(Variant::from(false)), Role::TextInput, true),
        ] {
            let mut node = crate::a11y::tests_node();
            node.role = role_of(ControlType::Edit);
            node.native_role = "Edit".into();
            node.has_text_value = true;
            refine_role(&mut node, Some(false), None);
            assert_eq!(node.role, Role::TextInput);
            let value_read = std::cell::Cell::new(false);
            let result = consume_value_snapshot(&mut node, flag_value(fresh_flag), || {
                value_read.set(true);
                Some("new secret".into())
            });
            assert_eq!(result.is_ok(), readable);
            assert_eq!(node.role, expected_role);
            assert_eq!(node.unmatchable, !readable);
            assert_eq!(value_read.get(), expected_role == Role::TextInput);
            let wire = serde_json::to_value(&node).expect("BUG: node serializes");
            assert_eq!(wire["role"], expected_role.name());
            assert_eq!(
                wire.get("value").is_some(),
                expected_role == Role::TextInput
            );
            for role in [Role::TextInput, Role::PasswordInput] {
                let query = crate::a11y::Query {
                    role: Some(role.name().into()),
                    ..crate::a11y::Query::default()
                }
                .prepared()
                .expect("BUG: valid role query");
                assert_eq!(query.matches(&node), expected_role == role);
            }
        }
    }

    #[test]
    fn semantic_roles_require_readable_discriminants_even_without_a_value_pattern() {
        use uiautomation::variants::Variant;

        for (control, native, ordinary, alternative) in [
            (
                ControlType::Edit,
                "Edit",
                Role::TextInput,
                Role::PasswordInput,
            ),
            (ControlType::Window, "Window", Role::Window, Role::Dialog),
        ] {
            for (value, expected) in [
                (None, None),
                (Some(Variant::from(1_i32)), None),
                (Some(Variant::from(false)), Some(ordinary)),
                (Some(Variant::from(true)), Some(alternative)),
            ] {
                for has_text_value in [false, true] {
                    let mut node = crate::a11y::tests_node();
                    node.role = role_of(control);
                    node.native_role = native.into();
                    node.has_text_value = has_text_value;
                    let flag = flag_value(value.clone());
                    let (password, dialog) = if ordinary == Role::TextInput {
                        (flag, None)
                    } else {
                        (None, flag)
                    };
                    refine_role(&mut node, password, dialog);
                    assert_eq!(
                        node.unmatchable,
                        expected.is_none(),
                        "an incomplete role must mark the tree read truncated"
                    );
                    let wire = serde_json::to_value(&node).expect("BUG: node serializes");
                    assert_eq!(wire["role"], expected.unwrap_or(Role::Unknown).name());
                    assert_eq!(wire["native_role"], native);
                    for role in [ordinary, alternative] {
                        let query = crate::a11y::Query {
                            role: Some(role.name().into()),
                            ..crate::a11y::Query::default()
                        }
                        .prepared()
                        .expect("BUG: valid role query");
                        // Queries also accept native names: a confirmed
                        // dialog still has the native control type Window.
                        let native_window_match =
                            role == Role::Window && expected == Some(Role::Dialog);
                        assert_eq!(
                            query.matches(&node),
                            expected == Some(role) || native_window_match
                        );
                    }
                    let native_query = crate::a11y::Query {
                        role: Some(native.into()),
                        ..crate::a11y::Query::default()
                    }
                    .prepared()
                    .expect("BUG: valid native role query");
                    assert_eq!(native_query.matches(&node), expected.is_some());
                }
            }
        }
    }

    #[test]
    fn replacement_during_preparation_never_reaches_dispatch() {
        for fails in [false, true] {
            let current = std::cell::Cell::new(true);
            let dispatched = std::cell::Cell::new(false);
            let result = dispatch_while_current(
                || {
                    current.set(false);
                    if fails {
                        Err(ToolError::platform(
                            "acquiring a pattern",
                            "provider disconnected",
                        ))
                    } else {
                        Ok(())
                    }
                },
                || {
                    if current.get() {
                        Ok(())
                    } else {
                        Err(ToolError::gone_element("e1", "replaced during preparation"))
                    }
                },
                |()| {
                    dispatched.set(true);
                    Ok(())
                },
            );
            assert!(!dispatched.get());
            let error = result.expect_err("BUG: replacement must prevent dispatch");
            assert_eq!(error.code(), "gone");
            assert!(error.payload()["error"].get("effect").is_none());
        }
    }

    #[test]
    fn conclusive_preparation_removal_survives_unreadable_final_identity() {
        let validated = std::cell::Cell::new(false);
        let result = dispatch_while_current(
            || Err::<(), _>(ToolError::gone_element("e1", "provider reported removal")),
            || {
                validated.set(true);
                Err(ToolError::platform(
                    "re-reading identity",
                    "provider timed out",
                ))
            },
            |()| panic!("an observed removal cannot dispatch an action"),
        );
        assert!(validated.get(), "the final identity check still runs");
        let error = result.expect_err("BUG: observed removal remains final");
        assert_eq!(error.code(), "gone");
        assert!(error.payload()["error"].get("effect").is_none());
    }

    #[test]
    fn unchanged_preparation_dispatches_once_and_preserves_action_effects() {
        let dispatched = std::cell::Cell::new(0);
        let result = dispatch_while_current(
            || Ok(()),
            || Ok(()),
            |()| {
                dispatched.set(dispatched.get() + 1);
                Err(ToolError::platform("invoke", "timed out")
                    .after(Effect::MayHaveRun, "invoke reached the provider"))
            },
        );
        assert_eq!(dispatched.get(), 1);
        assert!(matches!(
            result,
            Err(ToolError::Interrupted {
                effect: Effect::MayHaveRun,
                ..
            })
        ));
    }

    #[test]
    fn value_refresh_replacement_is_rejected_before_consumption() {
        // The original and replacement belong to the same live process;
        // process-only postchecks cannot distinguish these snapshots.
        let original = (42, 10, vec![1, 2], 50004);
        for replacement in [
            (42, 10, vec![1, 3], 50004),
            (42, 10, vec![1, 2], 50000),
            original.clone(),
        ] {
            let consumed = std::cell::Cell::new(false);
            let result = value_while_current(
                || Ok((replacement.clone(), "replacement value")),
                |(identity, _)| {
                    if identity == &original {
                        Ok(())
                    } else {
                        Err(ToolError::gone_element(
                            "e1",
                            "replaced during value refresh",
                        ))
                    }
                },
            )
            .map(|(_, value)| {
                consumed.set(true);
                value
            });
            assert_eq!(consumed.get(), replacement == original);
            if replacement != original {
                assert_eq!(
                    result
                        .expect_err("BUG: replacement value is refused")
                        .code(),
                    "gone"
                );
            }
        }
    }

    #[test]
    fn hit_test_revalidates_after_successful_and_failed_provider_walks() {
        for provider_fails in [false, true] {
            let mut current = true;
            let result = hit_while_current(
                &mut current,
                |current| {
                    *current = false;
                    if provider_fails {
                        Err(ToolError::platform("hit test", "provider disconnected"))
                    } else {
                        Ok(true)
                    }
                },
                |current| {
                    if *current {
                        Ok(())
                    } else {
                        Err(ToolError::gone_element("e1", "replaced during hit test"))
                    }
                },
            );
            assert!(matches!(result, Err(ToolError::Gone { .. })));
        }
        assert!(
            hit_while_current(&mut (), |()| Ok(true), |()| Ok(()))
                .expect("BUG: unchanged hit remains valid")
        );
    }

    #[test]
    fn destructive_action_readback_exposes_no_actions_or_old_value() {
        let mut node = crate::a11y::tests_node();
        node.actions = vec![ActionName::Invoke, ActionName::Focus];
        node.value = Some("old value".into());
        node.checked = Some(Checked::True);
        let wire = serde_json::to_value(gone_node(node)).expect("BUG: node serializes");
        assert_eq!(wire["gone"], true);
        for field in ["actions", "value", "checked", "selected", "expanded"] {
            assert!(wire.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn disconnected_provider_is_gone_when_its_process_id_was_reused() {
        for code in [
            E_DISCONNECTED,
            E_SERVER_UNAVAILABLE,
            E_CALL_FAILED,
            E_NOT_CONNECTED,
        ] {
            for observed in [None, Some(20), Some(10)] {
                let error = classify_code(
                    "e1",
                    "reading",
                    code,
                    &"disconnected",
                    Some((42, 10)),
                    |pid| {
                        assert_eq!(pid, 42);
                        Ok(observed)
                    },
                );
                assert_eq!(
                    error.code(),
                    if observed == Some(10) {
                        "platform"
                    } else {
                        "gone"
                    }
                );
            }
        }
    }

    #[test]
    fn subtree_validation_budget_covers_all_three_provider_calls() {
        let request_start = Instant::now();
        let request_deadline = request_start + std::time::Duration::from_secs(10);
        let traversal = subtree_traversal_deadline(request_start, request_deadline);
        assert!(traversal > request_start && traversal < request_deadline);
        // Traversal's final admitted call consumes the remaining request
        // budget. Final verification still has time for all three calls.
        let clock = std::cell::Cell::new(request_deadline);
        let calls = std::cell::Cell::new(0);
        let duration =
            std::time::Duration::from_millis(u64::from(crate::os::UIA_TRANSACTION_TIMEOUT_MS) - 1);
        let validation = validate_subtree(clock.get(), |deadline| {
            for _ in 0..3 {
                before_identity_call("e1", Some(deadline), clock.get())?;
                calls.set(calls.get() + 1);
                clock.set(clock.get() + duration);
            }
            Ok(())
        });
        let read = finish_subtree(
            Read {
                roots: vec![crate::a11y::tests_node()],
                truncated: true,
            },
            &HashSet::new(),
            validation,
            |_| panic!("verified partial reads preserve their handles"),
        )
        .expect("BUG: all three bounded validation calls fit");
        assert_eq!(calls.get(), 3);
        assert!(clock.get() > request_deadline + duration);
        assert!(read.truncated);
        assert_eq!(read.roots.len(), 1);
    }

    #[test]
    fn subtree_validation_deadline_stops_new_calls_and_never_returns_unverified_tree() {
        let clock = std::cell::Cell::new(Instant::now());
        let calls = std::cell::Cell::new(0);
        let validation = validate_subtree(clock.get(), |deadline| {
            before_identity_call("e1", Some(deadline), clock.get())?;
            calls.set(calls.get() + 1);
            // Scheduling delayed the next call beyond the phase budget.
            clock.set(deadline);
            before_identity_call("e1", Some(deadline), clock.get())?;
            calls.set(calls.get() + 1);
            Ok(())
        });
        let result = finish_subtree(
            Read {
                roots: vec![crate::a11y::tests_node()],
                truncated: true,
            },
            &HashSet::new(),
            validation,
            |_| panic!("a timeout is not proof that an element is gone"),
        );
        assert_eq!(calls.get(), 1);
        assert!(matches!(result, Err(ToolError::Timeout { .. })));
    }

    #[test]
    fn subtree_failure_retires_handles_from_a_replaced_root() {
        let mut cache = ElementCache::default();
        let child = cache.insert("child", ());
        let touched = HashSet::from([child.clone()]);
        let read = Read {
            roots: vec![crate::a11y::tests_node()],
            truncated: true,
        };
        let result = finish_subtree(
            read,
            &touched,
            Err(ToolError::gone_element(
                "e99",
                "root exited during traversal",
            )),
            |handle| cache.invalidate(handle),
        );
        assert!(matches!(result, Err(ToolError::Gone { .. })));
        assert!(matches!(cache.get(&child), Err(ToolError::Gone { .. })));
        let result = finish_subtree(Read::default(), &HashSet::new(), Ok(()), |_| {
            panic!("a valid root retires nothing")
        });
        assert!(result.is_ok());
    }

    #[test]
    fn unidentifiable_nodes_offer_no_actions() {
        for identifiable in [false, true] {
            let mut node = crate::a11y::tests_node();
            node.actions = vec![ActionName::Invoke, ActionName::Focus];
            constrain_actions(&mut node, identifiable);
            let wire = serde_json::to_value(&node).expect("BUG: nodes serialize");
            assert_eq!(wire.get("actions").is_some(), identifiable);
        }
    }

    #[test]
    fn unknown_enabled_state_is_omitted_and_cannot_satisfy_a_wait() {
        for enabled in [None, Some(false), Some(true)] {
            let mut node = crate::a11y::tests_node();
            enabled_state(&mut node, enabled);
            let wire = serde_json::to_value(&node).expect("BUG: nodes serialize");
            assert_eq!(wire.get("disabled").is_some(), enabled == Some(false));
            for disabled in [false, true] {
                let state = crate::params::StateArg {
                    disabled: Some(disabled),
                    ..crate::params::StateArg::default()
                };
                assert_eq!(state.holds(&node), enabled == Some(!disabled));
            }
        }
    }

    #[test]
    fn direct_focus_requires_a_live_readable_focusable_flag() {
        for focusable in [
            Ok(false),
            Ok(true),
            Err(ToolError::platform(
                "reading focusable",
                "missing or malformed boolean",
            )),
        ] {
            let expected_code = match &focusable {
                Ok(true) => None,
                Ok(false) => Some("action_unsupported"),
                Err(_) => Some("platform"),
            };
            let sent = std::cell::Cell::new(false);
            let result = perform_focus(
                || focusable,
                || {
                    sent.set(true);
                    Ok(())
                },
                || ToolError::ActionUnsupported {
                    element: "e1".into(),
                    action: "focus",
                    supported: Vec::new(),
                    unread: Vec::new(),
                },
            );
            assert_eq!(sent.get(), expected_code.is_none());
            assert_eq!(result.as_ref().err().map(ToolError::code), expected_code);
            if let Err(error) = result {
                assert!(
                    error.payload()["error"].get("effect").is_none(),
                    "refusal sent no focus request"
                );
            }
        }
    }

    #[test]
    fn readback_rechecks_process_after_the_last_provider_call() {
        for fails in [false, true] {
            for replacement in [None, Some(20)] {
                let started = std::cell::Cell::new(Some(10));
                let result = readback_while_current(
                    "e1",
                    Some(10),
                    || {
                        // The identity lookup succeeded, then the value
                        // lookup outlived the original process.
                        assert_eq!(started.get(), Some(10));
                        started.set(replacement);
                        if fails {
                            Err(ToolError::platform(
                                "reading value",
                                "provider disconnected",
                            ))
                        } else {
                            Ok(())
                        }
                    },
                    || Ok(started.get()),
                );
                let error = result.expect_err("BUG: replacement state is never returned");
                assert_eq!(error.code(), "gone");
                assert!(matches!(
                    error,
                    ToolError::Interrupted {
                        effect: Effect::Ran,
                        ..
                    }
                ));
            }
        }
        assert!(readback_while_current("e1", Some(10), || Ok(()), || Ok(Some(10))).is_ok());
    }

    #[test]
    fn unreadable_process_identity_does_not_erase_conclusive_element_evidence() {
        let unread = || Err(ToolError::Busy("identity temporarily inaccessible".into()));
        for same in [Some(true), None] {
            let error = identity_after_refresh(10, || same, unread)
                .expect_err("BUG: unreadable is neither current nor gone");
            assert_eq!(error.code(), "busy");
        }
        assert_eq!(
            identity_after_refresh(10, || Some(false), unread)
                .expect("BUG: observed replacement remains conclusive"),
            Some(false)
        );
        assert_eq!(
            identity_after_refresh(10, || None, || Ok(Some(20)))
                .expect("BUG: process replacement is conclusive"),
            Some(false)
        );
        let error = readback_while_current("e1", Some(10), || Ok(()), unread)
            .expect_err("BUG: unreadable readback is refused");
        assert_eq!(error.code(), "busy");
        assert!(matches!(
            error,
            ToolError::Interrupted {
                effect: Effect::Ran,
                ..
            }
        ));
        let error = classify_code(
            "e1",
            "reading",
            E_DISCONNECTED,
            &"disconnected",
            Some((42, 10)),
            |_| unread(),
        );
        assert_ne!(error.code(), "gone");
    }

    #[test]
    fn process_replacement_during_identity_refresh_is_refused() {
        let started = std::cell::Cell::new(Some(10));
        assert_eq!(
            identity_after_refresh(10, || Some(true), || Ok(started.get()))
                .expect("BUG: readable identity"),
            Some(true)
        );
        for replacement in [Some(20), None] {
            started.set(Some(10));
            let verdict = identity_after_refresh(
                10,
                || {
                    started.set(replacement);
                    Some(true)
                },
                || Ok(started.get()),
            );
            assert_eq!(
                verdict.expect("BUG: readable replacement"),
                Some(false),
                "a refreshed proxy cannot outlive its original process"
            );
        }
        started.set(Some(10));
        assert_eq!(
            identity_after_refresh(10, || None, || Ok(started.get()))
                .expect("BUG: readable process"),
            None
        );
    }

    #[test]
    fn expansion_dispatch_requires_a_supported_live_transition() {
        for (state, expand, collapse) in [
            (ExpandCollapseState::Collapsed, true, false),
            (ExpandCollapseState::Expanded, false, true),
            (ExpandCollapseState::PartiallyExpanded, true, true),
            (ExpandCollapseState::LeafNode, false, false),
        ] {
            for (action, allowed) in [
                (ActionName::Expand, expand),
                (ActionName::Collapse, collapse),
            ] {
                let called = std::cell::Cell::new(false);
                let result = perform_expansion(
                    action,
                    || Ok(state as i32),
                    || {
                        called.set(true);
                        Ok(())
                    },
                    || ToolError::ActionUnsupported {
                        element: "e1".into(),
                        action: action.name(),
                        supported: Vec::new(),
                        unread: Vec::new(),
                    },
                );
                assert_eq!(called.get(), allowed, "{state:?} {action:?}");
                assert_eq!(result.is_ok(), allowed);
                assert_eq!(expansion_allows(Some(state as i32), action), allowed);
            }
        }
        for read in [
            Ok(99),
            Err(ToolError::platform("reading expansion", "unreadable")),
        ] {
            let result = perform_expansion(
                ActionName::Expand,
                || read,
                || -> ToolResult<()> { panic!("unknown state must not authorize a transition") },
                unsupported_value,
            );
            assert!(matches!(result, Err(ToolError::Platform { .. })));
        }
    }

    fn unsupported_value() -> ToolError {
        ToolError::ActionUnsupported {
            element: "e1".into(),
            action: "set_value",
            supported: Vec::new(),
            unread: Vec::new(),
        }
    }

    #[test]
    fn read_only_values_never_reach_the_provider_write() {
        for pattern in [
            UIProperty::IsValuePatternAvailable,
            UIProperty::IsRangeValuePatternAvailable,
        ] {
            let result = perform_set_value(
                "e1",
                "42",
                |prop| {
                    Ok(prop == pattern
                        || matches!(
                            prop,
                            UIProperty::ValueIsReadOnly | UIProperty::RangeValueIsReadOnly
                        ))
                },
                |_| -> ToolResult<()> { panic!("a read-only provider must not receive a write") },
                unsupported_value,
            );
            assert!(matches!(result, Err(ToolError::ActionUnsupported { .. })));
        }
    }

    #[test]
    fn unreadable_read_only_flags_never_reach_the_provider_write() {
        for pattern in [
            UIProperty::IsValuePatternAvailable,
            UIProperty::IsRangeValuePatternAvailable,
        ] {
            let result = perform_set_value(
                "e1",
                "42",
                |prop| {
                    if matches!(
                        prop,
                        UIProperty::ValueIsReadOnly | UIProperty::RangeValueIsReadOnly
                    ) {
                        Err(ToolError::platform(
                            "reading properties",
                            "unreadable boolean",
                        ))
                    } else {
                        Ok(prop == pattern)
                    }
                },
                |_| -> ToolResult<()> {
                    panic!("an unreadable property must not authorize a write")
                },
                unsupported_value,
            );
            assert!(matches!(result, Err(ToolError::Platform { .. })));
        }
    }

    #[test]
    fn a_writable_range_is_used_when_the_text_pattern_is_read_only() {
        let mut written = None;
        perform_set_value(
            "e1",
            "42",
            |prop| Ok(prop != UIProperty::RangeValueIsReadOnly),
            |value| {
                written = Some(value);
                Ok(())
            },
            unsupported_value,
        )
        .expect("BUG: the range is writable");
        assert_eq!(written, Some(ValueWrite::Range(42.0)));
    }

    #[test]
    fn writable_text_preserves_its_value_and_invalid_ranges_are_not_sent() {
        let mut written = None;
        perform_set_value(
            "e1",
            " text ",
            |prop| Ok(prop == UIProperty::IsValuePatternAvailable),
            |value| {
                written = Some(value);
                Ok(())
            },
            unsupported_value,
        )
        .expect("BUG: the text is writable");
        assert_eq!(written, Some(ValueWrite::Text(" text ")));
        for invalid in ["NaN", "inf", "-inf", "words"] {
            let result = perform_set_value(
                "e1",
                invalid,
                |prop| Ok(prop == UIProperty::IsRangeValuePatternAvailable),
                |_| -> ToolResult<()> { panic!("an invalid range must not reach the provider") },
                unsupported_value,
            );
            assert!(matches!(result, Err(ToolError::InvalidArgument(_))));
        }
    }

    /// Only the three real toggle states have names; any other value is
    /// unreadable, not "indeterminate".
    #[test]
    fn only_real_toggle_states_are_named() {
        assert_eq!(checked_state(ToggleState::On as i32), Some(Checked::True));
        assert_eq!(checked_state(ToggleState::Off as i32), Some(Checked::False));
        assert_eq!(
            checked_state(ToggleState::Indeterminate as i32),
            Some(Checked::Mixed)
        );
        assert_eq!(checked_state(99), None);
        assert_eq!(expanded_state(ExpandCollapseState::LeafNode as i32), None);
        assert_eq!(
            expanded_state(ExpandCollapseState::PartiallyExpanded as i32),
            None
        );
    }

    #[test]
    fn partial_expansion_satisfies_neither_completed_state_wait() {
        let mut node = crate::a11y::tests_node();
        for (native, expected) in [
            (ExpandCollapseState::Collapsed, Some(false)),
            (ExpandCollapseState::Expanded, Some(true)),
            (ExpandCollapseState::PartiallyExpanded, None),
            (ExpandCollapseState::LeafNode, None),
        ] {
            node.expanded = expanded_state(native as i32);
            for wanted in [false, true] {
                let state = crate::params::StateArg {
                    expanded: Some(wanted),
                    ..crate::params::StateArg::default()
                };
                assert_eq!(state.holds(&node), expected == Some(wanted), "{native:?}");
            }
        }
    }

    /// A clipped string keeps no provider-sized buffer behind it.
    #[test]
    fn a_clipped_string_is_right_sized() {
        let cut = clip("x".repeat(1 << 20));
        assert_eq!(cut.chars().count(), MAX_PROPERTY_CHARS + 1);
        assert!(
            cut.capacity() < 2 * MAX_PROPERTY_CHARS,
            "{}",
            cut.capacity()
        );
    }

    /// A provider's string is cut at a character boundary, marked, and
    /// left alone when it fits.
    #[test]
    fn long_properties_are_cut_on_a_character_boundary() {
        assert_eq!(clip("short".into()), "short");
        let fits = "я".repeat(MAX_PROPERTY_CHARS);
        assert_eq!(clip(fits.clone()), fits);
        let cut = clip("я".repeat(MAX_PROPERTY_CHARS + 10));
        assert_eq!(cut.chars().count(), MAX_PROPERTY_CHARS + 1);
        assert!(cut.ends_with('…'));
    }

    /// Reading a runtime id repeatedly does not grow the process: the
    /// arrays UI Automation hands back are freed.
    #[test]
    fn runtime_ids_are_read_without_leaking() {
        let uia = Uia::new().expect("BUG: UI Automation is available on Windows");
        let root = uia
            .automation
            .get_root_element()
            .expect("BUG: the desktop element exists");
        let id = crate::os::runtime_id(root.as_ref()).expect("BUG: the desktop has a runtime id");
        assert!(id.is_some_and(|id| !id.is_empty()));
    }
}
