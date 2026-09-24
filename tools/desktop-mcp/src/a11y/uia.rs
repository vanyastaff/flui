//! Windows UI Automation backend.
//!
//! Lives on the worker thread: [`uiautomation::UIAutomation::new`] joins that
//! thread to the COM multithreaded apartment, and every `UIElement` this
//! backend holds stays on it.
//!
//! A tree read walks the control view one element at a time: each step
//! fetches the next child or sibling together with every property the
//! [`Node`] shape reports (a cache request with `TreeScope::Element`), so an
//! element costs about two cross-process calls and nothing past the read's
//! budget is ever fetched, however many children a provider claims. The
//! runtime id (the handle's identity) is read live per element.

use std::collections::HashSet;
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

/// The longest a string property (name, value, automation id, class name)
/// is reported, in characters; a longer one is cut and ends in `…`. A
/// provider's strings are otherwise unbounded (a text control's value is the
/// whole document).
const MAX_PROPERTY_CHARS: usize = 4_096;

/// How many bytes of strings one read reports at most, across all its
/// elements; past it the read stops, marked truncated.
const READ_BYTES: usize = 16 << 20;

/// How many parents a walk up to the desktop takes at most: a hostile
/// provider can report an endless or cyclic chain, and the walk runs on the
/// one worker every tool shares.
const ANCESTOR_LIMIT: usize = 256;

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
    UIProperty::ValueValue,
    UIProperty::RangeValueValue,
    UIProperty::ToggleToggleState,
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

/// UI Automation client state for the session.
pub struct Uia {
    automation: UIAutomation,
    /// The control view, walked one sibling at a time so a node's children
    /// are fetched only as far as the budget reaches.
    walker: UITreeWalker,
    single: UICacheRequest,
    elements: ElementCache<Vec<i32>, UIElement>,
    anonymous: i32,
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
    pub fn new() -> Result<Self, uiautomation::Error> {
        let automation = UIAutomation::new()?;
        let walker = automation.get_control_view_walker()?;
        let single = node_request(&automation, TreeScope::Element)?;
        Ok(Self {
            automation,
            walker,
            single,
            elements: ElementCache::default(),
            anonymous: 0,
        })
    }

    /// Issues (or re-issues) the handle for `element`. UI Automation may
    /// hand a removed element's runtime id to a new one; the handle already
    /// issued for the id is kept only while its own object still answers to
    /// that id, else the id is retired and the new element gets a fresh
    /// handle, so a held handle never retargets to another control.
    fn register(&mut self, element: &UIElement) -> String {
        let key = match element.get_runtime_id() {
            Ok(id) if !id.is_empty() => id,
            // No runtime id: a handle that is never shared with another read.
            _ => {
                self.anonymous += 1;
                vec![i32::MIN, self.anonymous]
            }
        };
        if let Some(held) = self.elements.by_identity(&key)
            && !held.get_runtime_id().is_ok_and(|id| id == key)
        {
            self.elements.retire(&key);
        }
        self.elements.insert(key, element.clone())
    }

    fn element(&self, handle: &str) -> ToolResult<UIElement> {
        self.elements.get(handle).cloned()
    }

    /// `element` and its descendants, fetched one child at a time through
    /// the control-view walker until the walk's budget or deadline runs out.
    /// Children left out by the depth or the budget are counted in
    /// `omitted_children`, up to [`OMITTED_COUNT_CAP`] (a lower bound past
    /// it). `None` for an element this read already emitted: UI Automation
    /// shows an owned window (a dialog) both as a top-level window and under
    /// its owner, and it is reported once.
    fn build(&mut self, element: &UIElement, depth: usize, walk: &mut Walk) -> Option<Node> {
        let mut node = self.describe(element);
        if !walk.seen.insert(node.id.clone()) {
            return None;
        }
        walk.budget = walk.budget.saturating_sub(1);
        walk.bytes = walk.bytes.saturating_sub(node.text_bytes());
        let mut omitted = 0;
        let mut child = walked(
            self.walker
                .get_first_child_build_cache(element, &self.single),
            walk,
        );
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

    /// A [`Node`] from `element`'s cached properties, children empty.
    fn describe(&mut self, element: &UIElement) -> Node {
        let id = self.register(element);
        let patterns: Vec<&'static str> = PATTERNS
            .iter()
            .filter(|(prop, _)| cached_bool(element, *prop))
            .map(|&(_, name)| name)
            .collect();
        // A text control's value, or a slider's number when it exposes only
        // RangeValue — what `set_value` writes, so a caller can read it back.
        let value = if patterns.contains(&"Value") {
            element
                .get_cached_property_value(UIProperty::ValueValue)
                .ok()
                .and_then(|v| TryInto::<String>::try_into(v).ok())
                .map(clip)
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
            .then(|| {
                element
                    .get_cached_property_value(UIProperty::ToggleToggleState)
                    .ok()
                    .and_then(|v| TryInto::<i32>::try_into(v).ok())
                    .map(toggle_name)
            })
            .flatten();
        Node {
            id,
            role: element
                .get_cached_control_type()
                .map_or_else(|_| "Unknown".to_owned(), |t| format!("{t:?}")),
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
        }
    }

    /// Whether the element UIA hit-tests at `(x, y)` is `element` or one of
    /// its descendants.
    fn hits_element(&self, x: i32, y: i32, element: &UIElement) -> bool {
        let Ok(hit) = self
            .automation
            .element_from_point(uiautomation::types::Point::new(x, y))
        else {
            return false;
        };
        let (Ok(walker), Ok(root)) = (
            self.automation.get_raw_view_walker(),
            self.automation.get_root_element(),
        ) else {
            return false;
        };
        let mut current = hit;
        for _ in 0..ANCESTOR_LIMIT {
            if self
                .automation
                .compare_elements(&current, element)
                .unwrap_or(false)
            {
                return true;
            }
            if self
                .automation
                .compare_elements(&current, &root)
                .unwrap_or(true)
            {
                return false;
            }
            match walker.get_parent(&current) {
                Ok(parent) => current = parent,
                Err(_) => return false,
            }
        }
        false
    }

    /// The top-level window `element` belongs to, by the definition the
    /// point check uses (`GA_ROOT` of the window under the point): the root
    /// of the nearest native window at or above it. UI Automation's own
    /// parent chain is no guide there, since it nests an owned dialog under
    /// its owner window. `None` when no native window is found.
    fn top_level_window(&self, element: &UIElement) -> Option<u32> {
        let walker = self.automation.get_raw_view_walker().ok()?;
        let mut current = element.clone();
        for _ in 0..ANCESTOR_LIMIT {
            let handle: isize = current.get_native_window_handle().map_or(0, Into::into);
            if let Some(id) = u32::try_from(handle).ok().filter(|&id| id != 0) {
                return crate::os::root_window(id);
            }
            current = walker.get_parent(&current).ok()?;
        }
        None
    }

    /// `element` once it holds keyboard focus. A provider can accept
    /// `SetFocus` and leave the focus where it was (an element that is not
    /// keyboard-focusable does), so success is read back, not assumed; the
    /// focus may land a moment later, so it is polled briefly.
    fn focused(&mut self, handle: &str, element: &UIElement) -> ToolResult<Node> {
        let mut last = None;
        for attempt in 0..FOCUS_POLLS {
            if attempt > 0 {
                std::thread::sleep(FOCUS_POLL_INTERVAL);
            }
            let fresh = element
                .build_updated_cache(&self.single)
                .map_err(|e| classify(handle, "reading back focus", &e))?;
            let node = self.describe(&fresh);
            if node.has_keyboard_focus {
                return Ok(node);
            }
            last = Some(node);
        }
        let focusable = last.is_some_and(|n| n.is_keyboard_focusable);
        Err(ToolError::NotSupported(format!(
            "element `{handle}` accepted focus but did not take keyboard focus{}; click it or use key tab to move focus",
            if focusable {
                ""
            } else {
                " (it reports is_keyboard_focusable: false)"
            }
        )))
    }

    /// The element's patterns, read live, for error messages.
    fn live_patterns(element: &UIElement) -> String {
        PATTERNS
            .iter()
            .filter(|(prop, _)| {
                element
                    .get_property_value(*prop)
                    .ok()
                    .and_then(|v| TryInto::<bool>::try_into(v).ok())
                    .unwrap_or(false)
            })
            .map(|&(_, name)| name)
            .collect::<Vec<_>>()
            .join(", ")
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
        // The action's own call failing because its element went away most
        // often means it ran and closed its window (an OK or Delete button):
        // the caller must look before it retries, not repeat it blindly.
        let lookup = |what: &'static str| move |e: uiautomation::Error| classify(handle, what, &e);
        let fail = |what: &'static str| {
            move |e: uiautomation::Error| match classify(handle, what, &e) {
                stale @ ToolError::StaleElement(_) => ToolError::Interrupted {
                    cause: Box::new(stale),
                    what: format!(
                        "it went away during {what}, which usually means the action ran; read the tree before retrying"
                    ),
                },
                other => other,
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
    fn tree(&mut self, windows: &[u32], max_depth: usize, deadline: Instant) -> ToolResult<Read> {
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
        for (read, &window) in windows.iter().enumerate() {
            if spare == 0 || Instant::now() >= deadline {
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
                Err(e) if is_gone(e.code()) => continue,
                Err(e) => {
                    return Err(ToolError::platform(
                        format!("reading the tree of window {window}"),
                        e,
                    ));
                }
            };
            roots.extend(self.build(&root, 0, &mut walk));
            spare = spare.saturating_sub(share - walk.budget);
        }
        if roots.is_empty() && !walk.truncated {
            return Err(ToolError::NotFound(
                "the target's windows closed while they were being read".into(),
            ));
        }
        Ok(Read {
            roots,
            truncated: walk.truncated,
        })
    }

    fn act(&mut self, handle: &str, action: &Action) -> ToolResult<Node> {
        let element = self.element(handle)?;
        Self::perform(handle, &element, action)?;
        if matches!(action, Action::Focus) {
            return self.focused(handle, &element);
        }
        match element.build_updated_cache(&self.single) {
            Ok(fresh) => Ok(self.describe(&fresh)),
            // The action succeeded and took its own element away (a Close or
            // Delete button, a navigation): that is the action's result, not a
            // failure. Answer with the node as it was just before.
            Err(e)
                if matches!(
                    classify(handle, "reading back", &e),
                    ToolError::StaleElement(_)
                ) =>
            {
                Ok(self.describe(&element))
            }
            Err(e) => Err(classify(handle, "reading back", &e)),
        }
    }

    fn click_point(&mut self, handle: &str) -> ToolResult<ClickPoint> {
        let element = self.element(handle)?;
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
        let element = self.element(handle)?;
        Ok(self.hits_element(x, y, &element))
    }

    fn focus_window(&mut self, window: u32) -> ToolResult<()> {
        self.automation
            .element_from_handle(hwnd(window))
            .and_then(|e| e.set_focus())
            .map_err(platform("focusing the window through UI Automation"))
    }
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

fn non_empty(value: uiautomation::Result<String>) -> Option<String> {
    value.ok().filter(|s| !s.is_empty()).map(clip)
}

/// `s` cut to [`MAX_PROPERTY_CHARS`], ending in `…` when it was longer.
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
}
