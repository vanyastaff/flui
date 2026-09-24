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
    UIInvokePattern, UIRangeValuePattern, UISelectionItemPattern, UITogglePattern, UIValuePattern,
};
use uiautomation::types::{Handle, ToggleState, TreeScope, UIProperty};
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use super::{AccessibilityBackend, Action, ClickPoint, Node, Read};
use crate::cache::ElementCache;
use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;

/// Pattern-availability properties, and the names the tools report.
const PATTERNS: &[(UIProperty, &str)] = &[
    (UIProperty::IsInvokePatternAvailable, "Invoke"),
    (UIProperty::IsTogglePatternAvailable, "Toggle"),
    (UIProperty::IsValuePatternAvailable, "Value"),
    (UIProperty::IsRangeValuePatternAvailable, "RangeValue"),
    (UIProperty::IsSelectionItemPatternAvailable, "SelectionItem"),
    (UIProperty::IsSelectionPatternAvailable, "Selection"),
    (
        UIProperty::IsExpandCollapsePatternAvailable,
        "ExpandCollapse",
    ),
    (UIProperty::IsScrollPatternAvailable, "Scroll"),
    (UIProperty::IsScrollItemPatternAvailable, "ScrollItem"),
    (UIProperty::IsTextPatternAvailable, "Text"),
    (UIProperty::IsGridPatternAvailable, "Grid"),
    (UIProperty::IsTablePatternAvailable, "Table"),
    (UIProperty::IsWindowPatternAvailable, "Window"),
];

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
    UIProperty::AutomationId,
    UIProperty::ClassName,
    UIProperty::BoundingRectangle,
    UIProperty::IsEnabled,
    UIProperty::HasKeyboardFocus,
    UIProperty::IsKeyboardFocusable,
    UIProperty::RangeValueValue,
    UIProperty::ToggleToggleState,
    UIProperty::ProcessId,
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
    /// What kind of element it was when issued: control type, automation id
    /// and class name. A runtime id derived from a recycled native window can
    /// repeat for a replacement in the same process; one of another kind is
    /// told apart by these.
    kind: Kind,
    pid: u32,
    started: Option<u64>,
}

/// An element's kind, the properties that do not change over its life.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kind {
    control_type: Option<i32>,
    automation_id: Option<String>,
    class_name: Option<String>,
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
    fn identity(&mut self, element: &UIElement) -> Identity {
        match crate::os::runtime_id(element.as_ref()) {
            Ok(Some(id)) => Identity::Runtime(id),
            _ => self.fresh_identity(),
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
    fn register(&mut self, key: Identity, element: &UIElement) -> String {
        let pid = cached_i32(element, UIProperty::ProcessId)
            .and_then(|pid| u32::try_from(pid).ok())
            .unwrap_or(0);
        let started = *self
            .starts
            .entry(pid)
            .or_insert_with(|| crate::os::process_started(pid));
        // The handle is kept only for the same element: its object still
        // answers to the id, and the new element comes from the same process.
        // A provider elsewhere claiming a trusted application's runtime id
        // gets a handle of its own instead of taking over that one.
        let kind = Kind {
            control_type: cached_i32(element, UIProperty::ControlType),
            automation_id: element.get_cached_automation_id().ok(),
            class_name: element.get_cached_classname().ok(),
        };
        // A new element of another kind claiming the id is a collision, not
        // the same control read again: it gets its own handle.
        if let Identity::Runtime(id) = &key
            && let Some(held) = self.elements.by_identity(&key)
            && (held.pid != pid
                || held.started != started
                || held.kind != kind
                || !crate::os::runtime_id(held.element.as_ref())
                    .is_ok_and(|now| now.as_deref() == Some(id.as_slice())))
        {
            self.elements.retire(&key);
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
                kind,
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
    fn alive(&self, handle: &str) -> ToolResult<UIElement> {
        let held = self.elements.get(handle)?;
        let Some(started) = held.started else {
            return Err(ToolError::NotSupported(format!(
                "element `{handle}` came from a process this server could not identify, so it is not acted on; read the tree again"
            )));
        };
        if crate::os::process_started(held.pid) != Some(started) {
            return Err(ToolError::StaleElement(handle.to_owned()));
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
        if !crate::os::runtime_id(held.element.as_ref())
            .is_ok_and(|now| now.as_deref() == Some(id.as_slice()))
        {
            return Err(ToolError::StaleElement(handle.to_owned()));
        }
        // Read live: an object that now reports another kind of element is a
        // replacement, whatever its runtime id says.
        // One cross-process call for all three, bounded by the call timeout.
        let fresh = held
            .element
            .build_updated_cache(&self.single)
            .map_err(|_| ToolError::StaleElement(handle.to_owned()))?;
        let live = Kind {
            control_type: cached_i32(&fresh, UIProperty::ControlType),
            automation_id: fresh.get_cached_automation_id().ok(),
            class_name: fresh.get_cached_classname().ok(),
        };
        if live != held.kind {
            return Err(ToolError::StaleElement(handle.to_owned()));
        }
        Ok(held.element.clone())
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
        let mut key = self.identity(element);
        if let Some(handle) = self.elements.handle_of(&key)
            && walk.seen.contains(&handle)
        {
            // The same runtime id twice in one read. An owned window listed
            // at the top and under its owner is one element; anything else
            // is a different element claiming the id, which gets a handle of
            // its own instead of taking over the first one's.
            // It is the same one only if both have the same native window;
            // otherwise the collision gets its own handle, never hidden.
            let same_window = role(element) == "Window"
                && self.elements.by_identity(&key).is_some_and(|held| {
                    let native = |e: &UIElement| {
                        e.get_native_window_handle()
                            .map(Into::<isize>::into)
                            .ok()
                            .filter(|&h| h != 0)
                    };
                    native(&held.element).is_some_and(|h| native(element) == Some(h))
                });
            if same_window {
                return None;
            }
            key = self.fresh_identity();
        }
        let id = self.register(key, element);
        walk.seen.insert(id.clone());
        let mut node = describe(element, id);
        // One more call for a text control's value, charged like any other.
        if node.patterns.contains(&"Value") {
            walk.budget = walk.budget.saturating_sub(1);
            walk.truncated |=
                Instant::now() >= walk.deadline || !self.read_value(element, &mut node);
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
                    break;
                }
                walk.budget -= 1;
            }
            // A spent budget fetches no further sibling either; `exhausted`
            // marks the rest unread.
            if walk.exhausted() {
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
    fn is_within(&self, candidate: UIElement, element: &UIElement) -> bool {
        let (Ok(walker), Ok(root)) = (
            self.automation.get_raw_view_walker(),
            self.automation.get_root_element(),
        ) else {
            return false;
        };
        let mut current = candidate;
        let until = Instant::now() + ANCESTOR_DEADLINE;
        // Checked before every provider call, each of which can take the
        // whole call timeout, so the walk as a whole keeps to the deadline.
        let late = || Instant::now() >= until;
        for _ in 0..ANCESTOR_LIMIT {
            if late() {
                return false;
            }
            if self
                .automation
                .compare_elements(&current, element)
                .unwrap_or(false)
            {
                return true;
            }
            if late()
                || self
                    .automation
                    .compare_elements(&current, &root)
                    .unwrap_or(true)
            {
                return false;
            }
            if late() {
                return false;
            }
            match walker.get_parent(&current) {
                Ok(parent) => current = parent,
                Err(_) => return false,
            }
        }
        false
    }

    /// Fills in a text control's value with a read of its own, clipped, so
    /// only the clipped string outlives the call. Whether it could be read:
    /// a property the provider does not return, or returns as something
    /// other than a string, is a failed read, not an empty value.
    fn read_value(&self, element: &UIElement, node: &mut Node) -> bool {
        if !node.patterns.contains(&"Value") {
            return true;
        }
        let Ok(fresh) = element.build_updated_cache(&self.value) else {
            return false;
        };
        let Some(value) = fresh
            .get_cached_property_value(UIProperty::ValueValue)
            .ok()
            .and_then(|v| TryInto::<String>::try_into(v).ok())
        else {
            return false;
        };
        node.value = Some(clip(value));
        true
    }

    /// Whether the element UIA hit-tests at `(x, y)` is `element` or one of
    /// its descendants.
    fn hits_element(&self, x: i32, y: i32, element: &UIElement) -> bool {
        self.automation
            .element_from_point(uiautomation::types::Point::new(x, y))
            .is_ok_and(|hit| self.is_within(hit, element))
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
            let handle: isize = current.get_native_window_handle().map_or(0, Into::into);
            if let Some(id) = u32::try_from(handle).ok().filter(|&id| id != 0) {
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

    /// `element` once keyboard focus is on it or inside it (a combo box
    /// hands focus to its edit field, a web view host to its content). A
    /// provider can accept `SetFocus` and leave the focus where it was (an
    /// element that is not keyboard-focusable does), so success is read
    /// back, not assumed; the focus may land a moment later, so it is polled
    /// briefly. The request itself went out, so a failed readback says so.
    fn focused(&mut self, handle: &str, element: &UIElement) -> ToolResult<Node> {
        let mut last = None;
        // One deadline for the whole readback, however slow each poll is.
        let until = Instant::now() + ANCESTOR_DEADLINE;
        for attempt in 0..FOCUS_POLLS {
            if attempt > 0 {
                if Instant::now() >= until {
                    break;
                }
                std::thread::sleep(FOCUS_POLL_INTERVAL);
            }
            let fresh =
                element
                    .build_updated_cache(&self.single)
                    .map_err(|e| ToolError::Interrupted {
                        cause: Box::new(classify(handle, "reading back focus", &e)),
                        what: "focus was requested; only reading back where it landed failed"
                            .into(),
                    })?;
            let node = describe(&fresh, handle.to_owned());
            // Checked between the provider calls as well: each can take the
            // whole call timeout.
            let inside = !node.has_keyboard_focus
                && Instant::now() < until
                && self.automation.get_focused_element().is_ok_and(|focused| {
                    Instant::now() < until && self.is_within(focused, element)
                });
            if node.has_keyboard_focus || inside {
                return Ok(node);
            }
            last = Some(node);
        }
        let focusable = last.is_some_and(|n| n.is_keyboard_focusable);
        Err(ToolError::NotSupported(format!(
            "element `{handle}` accepted focus but keyboard focus is neither on it nor inside it{}; click it or use key tab to move focus",
            if focusable {
                ""
            } else {
                " (it reports is_keyboard_focusable: false)"
            }
        )))
    }

    /// The element's patterns, read live, for error messages: stopped after
    /// [`ANCESTOR_DEADLINE`] in all, since each read can take a provider's
    /// whole call timeout, and marked when cut short.
    fn live_patterns(element: &UIElement) -> String {
        let until = Instant::now() + ANCESTOR_DEADLINE;
        let mut names = Vec::new();
        for &(prop, name) in PATTERNS {
            if Instant::now() >= until {
                names.push("… (not all read)");
                break;
            }
            let offered = element
                .get_property_value(prop)
                .ok()
                .and_then(|v| TryInto::<bool>::try_into(v).ok())
                .unwrap_or(false);
            if offered {
                names.push(name);
            }
        }
        names.join(", ")
    }

    /// Whether `element` offers the pattern `prop` names, read live. A failed
    /// read is classified, not taken for "no": an element the application
    /// removed must answer as stale, so the caller reads a fresh tree instead
    /// of concluding the action is unsupported.
    fn has_pattern(handle: &str, element: &UIElement, prop: UIProperty) -> ToolResult<bool> {
        let value = element
            .get_property_value(prop)
            .map_err(|e| classify(handle, "reading its patterns", &e))?;
        Ok(TryInto::<bool>::try_into(value).unwrap_or(false))
    }

    fn require(
        handle: &str,
        element: &UIElement,
        prop: UIProperty,
        pattern: &'static str,
    ) -> ToolResult<()> {
        if Self::has_pattern(handle, element, prop)? {
            Ok(())
        } else {
            Err(ToolError::PatternUnsupported {
                element: handle.to_owned(),
                pattern,
                supported: Self::live_patterns(element),
            })
        }
    }

    fn perform(handle: &str, element: &UIElement, action: &Action) -> ToolResult<()> {
        let lookup = |what: &'static str| move |e: uiautomation::Error| classify(handle, what, &e);
        // The action's own call reached the application, so a failure there
        // does not mean it did not run: its element going away most often
        // means it ran and closed its window (an OK or Delete button), and a
        // timeout that it is still running (a modal dialog it opened keeps
        // the call from returning). Only a disabled element is a clean
        // refusal. Anything else must be looked at before a retry.
        let fail = |what: &'static str| {
            move |e: uiautomation::Error| match classify(handle, what, &e) {
                refused @ ToolError::InvalidArgument(_) => refused,
                cause => {
                    let what = if matches!(cause, ToolError::StaleElement(_)) {
                        format!(
                            "it went away during {what}, which usually means the action ran; read the tree before retrying"
                        )
                    } else {
                        format!(
                            "{what} reached the application and then failed or timed out, so it may have run; read the tree before retrying"
                        )
                    };
                    ToolError::Interrupted {
                        cause: Box::new(cause),
                        what,
                    }
                }
            }
        };
        match action {
            Action::Invoke => {
                Self::require(
                    handle,
                    element,
                    UIProperty::IsInvokePatternAvailable,
                    "Invoke",
                )?;
                element
                    .get_pattern::<UIInvokePattern>()
                    .map_err(lookup("invoke"))?
                    .invoke()
                    .map_err(fail("invoke"))
            }
            Action::Toggle => {
                Self::require(
                    handle,
                    element,
                    UIProperty::IsTogglePatternAvailable,
                    "Toggle",
                )?;
                element
                    .get_pattern::<UITogglePattern>()
                    .map_err(lookup("toggle"))?
                    .toggle()
                    .map_err(fail("toggle"))
            }
            Action::SetValue(value) => {
                if Self::has_pattern(handle, element, UIProperty::IsValuePatternAvailable)? {
                    return element
                        .get_pattern::<UIValuePattern>()
                        .map_err(lookup("set_value"))?
                        .set_value(value)
                        .map_err(fail("set_value"));
                }
                if Self::has_pattern(handle, element, UIProperty::IsRangeValuePatternAvailable)? {
                    let number: f64 = value.trim().parse().map_err(|_| {
                        ToolError::InvalidArgument(format!(
                            "element `{handle}` takes a number (RangeValue pattern); `{value}` is not one"
                        ))
                    })?;
                    return element
                        .get_pattern::<UIRangeValuePattern>()
                        .map_err(lookup("set_value"))?
                        .set_value(number)
                        .map_err(fail("set_value"));
                }
                Err(ToolError::PatternUnsupported {
                    element: handle.to_owned(),
                    pattern: "Value (or RangeValue)",
                    supported: Self::live_patterns(element),
                })
            }
            Action::Focus => element.set_focus().map_err(fail("focus")),
            Action::Select => {
                Self::require(
                    handle,
                    element,
                    UIProperty::IsSelectionItemPatternAvailable,
                    "SelectionItem",
                )?;
                element
                    .get_pattern::<UISelectionItemPattern>()
                    .map_err(lookup("select"))?
                    .select()
                    .map_err(fail("select"))
            }
        }
    }
}

impl AccessibilityBackend for Uia {
    fn available(&self) -> ToolResult<()> {
        Ok(())
    }

    fn tree(&mut self, windows: &[u32], max_depth: usize, deadline: Instant) -> ToolResult<Read> {
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
        let mut spare = NODE_BUDGET;
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
            roots.extend(self.build(&root, 0, &mut walk));
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

    fn act(&mut self, handle: &str, action: &Action) -> ToolResult<Node> {
        let element = self.alive(handle)?;
        Self::perform(handle, &element, action)?;
        if matches!(action, Action::Focus) {
            return self.focused(handle, &element);
        }
        // The reply keeps the caller's handle either way: a readback that
        // minted another one would hide which element this was.
        match element.build_updated_cache(&self.single) {
            Ok(fresh) => {
                let mut node = describe(&fresh, handle.to_owned());
                // A value that cannot be read back is not a control without
                // one: the action ran, and what it left is unknown.
                if !self.read_value(&fresh, &mut node) {
                    return Err(ToolError::Interrupted {
                        cause: Box::new(ToolError::platform(
                            "reading back",
                            "the element's value could not be read",
                        )),
                        what: "the action itself succeeded; only reading the element's value afterwards failed, so do not repeat it".into(),
                    });
                }
                Ok(node)
            }
            // The action succeeded and took its own element away (a Close or
            // Delete button, a navigation): that is the action's result, not a
            // failure. Answer with the node as it was just before, marked.
            // Only the provider saying the element is not available is
            // evidence it went; a disconnect or a failed call (a provider
            // restarting) leaves the outcome unknown, reported below.
            Err(e) if e.code() == E_ELEMENT_NOT_AVAILABLE => {
                // Its identity only: the value and toggle state cached
                // before the action are not what the action left behind.
                Ok(Node {
                    gone: true,
                    value: None,
                    toggle_state: None,
                    ..describe(&element, handle.to_owned())
                })
            }
            // The action itself succeeded; only reading the result failed.
            // Say so, or a retry repeats a destructive action.
            Err(e) => Err(ToolError::Interrupted {
                cause: Box::new(classify(handle, "reading back", &e)),
                what: "the action itself succeeded; only reading the element afterwards failed, so do not repeat it".into(),
            }),
        }
    }

    fn click_point(&mut self, handle: &str) -> ToolResult<ClickPoint> {
        let element = self.alive(handle)?;
        let (x, y) = if let Ok(Some(p)) = element.get_clickable_point() {
            (p.get_x(), p.get_y())
        } else {
            let r = element
                .get_bounding_rectangle()
                .map_err(|e| classify(handle, "reading its bounds", &e))?;
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
            if !self.hits_element(x, y, &element) {
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
        Ok(self.hits_element(x, y, &element))
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
    let patterns: Vec<&'static str> = PATTERNS
        .iter()
        .filter(|(prop, _)| cached_bool(element, *prop))
        .map(|&(_, name)| name)
        .collect();
    // A text control's value, or a slider's number when it exposes only
    // RangeValue — what `set_value` writes, so a caller can read it back.
    // A text control's value is read apart ([`Uia::read_value`]); a
    // slider's number is small and comes with the walk.
    let value = if patterns.contains(&"Value") {
        None
    } else if patterns.contains(&"RangeValue") {
        element
            .get_cached_property_value(UIProperty::RangeValueValue)
            .ok()
            .and_then(|v| TryInto::<f64>::try_into(v).ok())
            .map(|number| number.to_string())
    } else {
        None
    };
    let toggle_state = patterns
        .contains(&"Toggle")
        .then(|| cached_i32(element, UIProperty::ToggleToggleState).map(toggle_name))
        .flatten();
    Node {
        id,
        role: role(element),
        name: non_empty(element.get_cached_name()),
        value,
        automation_id: non_empty(element.get_cached_automation_id()),
        class_name: non_empty(element.get_cached_classname()),
        rect: element
            .get_cached_bounding_rectangle()
            .ok()
            .map(|r| Rect::from_ltrb(r.get_left(), r.get_top(), r.get_right(), r.get_bottom())),
        enabled: element.is_cached_enabled().unwrap_or(false),
        has_keyboard_focus: element.has_cached_keyboard_focus().unwrap_or(false),
        is_keyboard_focusable: element.is_cached_keyboard_focusable().unwrap_or(false),
        toggle_state,
        patterns,
        children: Vec::new(),
        omitted_children: None,
        gone: false,
        unmatchable: !searchable(element),
    }
}

/// The element's control type, as `find`'s `role` matches it: the UIA name
/// (`Button`), or `Custom(<id>)` for an id the `uiautomation` crate does
/// not name, rather than a failure that would hide the element from search.
fn role(element: &UIElement) -> String {
    match element.get_cached_control_type() {
        Ok(known) => format!("{known:?}"),
        Err(_) => cached_i32(element, UIProperty::ControlType)
            .map_or_else(|| "Unknown".to_owned(), |id| format!("Custom({id})")),
    }
}

/// Whether the provider reported every property a node shows: what a
/// search matches on (name, control type, automation id), and the states and
/// patterns an agent acts on. A failed read is not the same as an empty
/// value or `false` — a control that reads as disabled because the read
/// failed would be skipped — so it marks the read incomplete.
fn searchable(element: &UIElement) -> bool {
    cached_i32(element, UIProperty::ControlType).is_some()
        && NODE_PROPERTIES
            .iter()
            .chain(PATTERNS.iter().map(|(prop, _)| prop))
            .all(|&prop| element.get_cached_property_value(prop).is_ok())
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
    request.set_tree_scope(scope)?;
    request.set_tree_filter(automation.get_control_view_condition()?)?;
    Ok(request)
}

/// xcap's window id on Windows is the `HWND` value.
fn hwnd(window: u32) -> Handle {
    Handle::from(window as isize)
}

fn cached_bool(element: &UIElement, prop: UIProperty) -> bool {
    element
        .get_cached_property_value(prop)
        .ok()
        .and_then(|v| TryInto::<bool>::try_into(v).ok())
        .unwrap_or(false)
}

fn cached_i32(element: &UIElement, prop: UIProperty) -> Option<i32> {
    element
        .get_cached_property_value(prop)
        .ok()
        .and_then(|v| TryInto::<i32>::try_into(v).ok())
}

fn non_empty(value: uiautomation::Result<String>) -> Option<String> {
    value.ok().filter(|s| !s.is_empty()).map(clip)
}

/// `s` cut to [`MAX_PROPERTY_CHARS`], ending in `…` when it was longer
/// (see [`Node::is_clipped`]).
fn clip(s: String) -> String {
    match s.char_indices().nth(MAX_PROPERTY_CHARS) {
        None => s,
        Some((at, _)) => {
            let mut cut = s;
            cut.truncate(at);
            cut.push('…');
            cut
        }
    }
}

fn toggle_name(state: i32) -> &'static str {
    match state {
        s if s == ToggleState::On as i32 => "on",
        s if s == ToggleState::Off as i32 => "off",
        _ => "indeterminate",
    }
}

/// Whether a UIA failure means the element, its window or its process is
/// gone rather than that the call is wrong.
fn is_gone(code: i32) -> bool {
    matches!(
        code,
        E_ELEMENT_NOT_AVAILABLE
            | E_DISCONNECTED
            | E_SERVER_UNAVAILABLE
            | E_CALL_FAILED
            | E_NOT_CONNECTED
            | E_INVALID_WINDOW
    )
}

/// Maps a UIA failure on `handle` to the error the agent can act on.
fn classify(handle: &str, what: &str, e: &uiautomation::Error) -> ToolError {
    match e.code() {
        code if is_gone(code) => ToolError::StaleElement(handle.to_owned()),
        E_ELEMENT_NOT_ENABLED => ToolError::InvalidArgument(format!(
            "element `{handle}` is disabled; {what} was not performed"
        )),
        _ => ToolError::platform(format!("{what} on element `{handle}`"), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
