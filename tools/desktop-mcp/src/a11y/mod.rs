//! The accessibility seam: one trait, one backend per OS API.
//!
//! [`AccessibilityBackend`] is what the tools call; `uia` implements it with
//! Windows UI Automation. Another OS gets its own module behind the same trait
//! (AX through `objc2-application-services` on macOS, AT-SPI through `atspi`
//! on Linux); until then [`Unsupported`] answers every call with the reason.

#[cfg(target_os = "windows")]
mod uia;

use std::time::Instant;

use serde::Serialize;

use crate::error::{ToolError, ToolResult};
use crate::geometry::Rect;

/// One element as the tools report it.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Session handle (`"e12"`) that later calls pass as `element`.
    pub id: String,
    /// Control type, e.g. `Button`, `Text`, `Window`.
    pub role: String,
    /// Accessible name (label).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Current value, for elements with a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Developer-assigned id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    /// Toolkit class name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Bounds in physical screen pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// Whether the element accepts interaction.
    pub enabled: bool,
    /// Whether the element has keyboard focus now.
    pub has_keyboard_focus: bool,
    /// Whether the element can take keyboard focus.
    pub is_keyboard_focusable: bool,
    /// `on`, `off` or `indeterminate`, for toggleable elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toggle_state: Option<&'static str>,
    /// Control patterns (actions and state interfaces) the element implements.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<&'static str>,
    /// Child elements, in tree order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// Children left out because `max_depth` or the read's budget was
    /// reached (a lower bound past a few hundred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_children: Option<usize>,
    /// The element is gone: an action removed it (a Close or Delete
    /// button), and the other fields are its state from just before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    /// A searched property was cut or could not be read: its shown value
    /// (an empty name, `Unknown`, a trailing `…`) is not the real one, so the
    /// node matches no query rather than a wrong one.
    #[serde(skip)]
    pub unmatchable: bool,
}

/// `s` under Unicode full case folding, so a case-insensitive substring
/// matches the way Unicode defines it: `Straße` contains `STRASSE`, and a
/// final sigma folds like any other. Folding is per character, so a
/// substring folds the same on its own as inside a longer name.
pub fn fold(s: &str) -> String {
    caseless::default_case_fold_str(s)
}

/// The longest a string property is reported, in characters; a longer one
/// is cut and ends in `…`. A provider's strings are otherwise unbounded (a
/// text control's value is the whole document).
pub const CLIPPED_CHARS: usize = 4_096;

/// What one read of a target's windows saw.
#[derive(Debug, Clone, Default)]
pub struct Read {
    /// One tree per window read, front to back.
    pub roots: Vec<Node>,
    /// Whether the read left anything out — at its element, byte or time
    /// budget, below its depth, or where the provider failed partway: then a
    /// search that found nothing is not proof that nothing matches.
    pub truncated: bool,
}

/// An action performed through an element's control pattern.
#[derive(Debug, Clone)]
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "raised by the UIA backend, the only accessibility backend built yet"
    )
)]
pub enum Action {
    /// Invoke (press) it.
    Invoke,
    /// Flip its toggle state.
    Toggle,
    /// Replace its value.
    SetValue(String),
    /// Give it keyboard focus.
    Focus,
    /// Select it within its container.
    Select,
}

/// Criteria for `find` and `wait_for`; every given criterion must hold.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Exact accessible name.
    pub name: Option<String>,
    /// Case-insensitive substring of the name.
    pub name_contains: Option<String>,
    /// Control type, case-insensitive.
    pub role: Option<String>,
    /// Exact automation id.
    pub automation_id: Option<String>,
}

impl Query {
    /// Whether no criterion is given (which would match everything).
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.name_contains.is_none()
            && self.role.is_none()
            && self.automation_id.is_none()
    }

    /// The query ready to run: each criterion at most [`CLIPPED_CHARS`]
    /// characters (no reported string is longer) and none empty but `name`
    /// (an element without a name has the empty one; nothing has an empty
    /// role or automation id, and every name contains ""), and
    /// `name_contains` folded once here rather than for every node visited.
    pub fn prepared(mut self) -> ToolResult<Self> {
        for (field, value) in [
            ("name_contains", &self.name_contains),
            ("role", &self.role),
            ("automation_id", &self.automation_id),
        ] {
            if value.as_deref() == Some("") {
                return Err(ToolError::InvalidArgument(format!(
                    "`{field}` must not be empty; it would match nothing or everything"
                )));
            }
        }
        for (field, value) in [
            ("name", &self.name),
            ("name_contains", &self.name_contains),
            ("role", &self.role),
            ("automation_id", &self.automation_id),
        ] {
            if value
                .as_deref()
                .is_some_and(|v| v.chars().count() > CLIPPED_CHARS)
            {
                return Err(ToolError::InvalidArgument(format!(
                    "`{field}` is longer than {CLIPPED_CHARS} characters, longer than any string a read reports"
                )));
            }
        }
        self.name_contains = self.name_contains.as_deref().map(fold);
        Ok(self)
    }

    /// Whether `node` satisfies every given criterion. Expects a
    /// [`Self::prepared`] query (`name_contains` already lower-cased).
    pub fn matches(&self, node: &Node) -> bool {
        if node.unmatchable {
            return false;
        }
        let name = node.name.as_deref().unwrap_or("");
        self.name.as_deref().is_none_or(|n| name == n)
            && self
                .name_contains
                .as_deref()
                .is_none_or(|n| fold(name).contains(n))
            && self
                .role
                .as_deref()
                .is_none_or(|r| node.role.eq_ignore_ascii_case(r))
            && self
                .automation_id
                .as_deref()
                .is_none_or(|a| node.automation_id.as_deref() == Some(a))
    }

    /// The criteria, formatted for messages.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(n) = &self.name {
            parts.push(format!("name == {n:?}"));
        }
        if let Some(n) = &self.name_contains {
            parts.push(format!("name contains {n:?}"));
        }
        if let Some(r) = &self.role {
            parts.push(format!("role == {r}"));
        }
        if let Some(a) = &self.automation_id {
            parts.push(format!("automation_id == {a:?}"));
        }
        parts.join(" and ")
    }
}

impl Node {
    /// The bytes of the strings this node reports.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "charged by the UIA backend, the only accessibility backend built yet"
        )
    )]
    pub fn text_bytes(&self) -> usize {
        self.id.len()
            + self.role.len()
            + [
                &self.name,
                &self.value,
                &self.automation_id,
                &self.class_name,
            ]
            .into_iter()
            .flatten()
            .map(String::len)
            .sum::<usize>()
    }

    /// Whether a string property was cut to the longest one reported
    /// ([`CLIPPED_CHARS`] characters and a trailing `…`).
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "checked by the UIA backend, the only accessibility backend built yet"
        )
    )]
    pub fn is_clipped(&self) -> bool {
        [
            &self.name,
            &self.value,
            &self.automation_id,
            &self.class_name,
        ]
        .into_iter()
        .flatten()
        .any(|s| s.ends_with('…') && s.chars().count() == CLIPPED_CHARS + 1)
    }

    /// Whether a property a query searches (name, automation id) was cut.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(
            dead_code,
            reason = "checked by the UIA backend, the only accessibility backend built yet"
        )
    )]
    pub fn searched_clipped(&self) -> bool {
        [&self.name, &self.automation_id]
            .into_iter()
            .flatten()
            .any(|s| s.ends_with('…') && s.chars().count() == CLIPPED_CHARS + 1)
    }

    /// This node without its children, copying nothing below it.
    #[must_use]
    pub fn shallow(&self) -> Self {
        Self {
            id: self.id.clone(),
            role: self.role.clone(),
            name: self.name.clone(),
            value: self.value.clone(),
            automation_id: self.automation_id.clone(),
            class_name: self.class_name.clone(),
            rect: self.rect,
            enabled: self.enabled,
            has_keyboard_focus: self.has_keyboard_focus,
            is_keyboard_focusable: self.is_keyboard_focusable,
            toggle_state: self.toggle_state,
            patterns: self.patterns.clone(),
            children: Vec::new(),
            omitted_children: None,
            gone: false,
            unmatchable: self.unmatchable,
        }
    }
}

/// Depth-first matches of `query` in `roots`, children dropped.
pub fn search(roots: &[Node], query: &Query) -> Vec<Node> {
    fn walk(node: &Node, query: &Query, out: &mut Vec<Node>) {
        if query.matches(node) {
            out.push(node.shallow());
        }
        for child in &node.children {
            walk(child, query, out);
        }
    }
    let mut out = Vec::new();
    for root in roots {
        walk(root, query, &mut out);
    }
    out
}

/// A few indented lines naming the tree's elements, for timeout messages.
pub fn summarize(roots: &[Node], max_lines: usize) -> String {
    fn walk(node: &Node, depth: usize, lines: &mut Vec<String>, max: usize) {
        if lines.len() >= max {
            return;
        }
        let name = node.name.as_deref().unwrap_or("");
        lines.push(format!(
            "{}{} {} {name:?}",
            "  ".repeat(depth),
            node.id,
            node.role
        ));
        for child in &node.children {
            walk(child, depth + 1, lines, max);
        }
    }
    let mut lines = Vec::new();
    for root in roots {
        walk(root, 0, &mut lines, max_lines);
    }
    if lines.is_empty() {
        "(empty)".to_owned()
    } else {
        lines.join("\n")
    }
}

/// Where a pointer click on an element lands.
#[derive(Debug, Clone, Copy)]
pub struct ClickPoint {
    /// Physical screen x.
    pub x: i32,
    /// Physical screen y.
    pub y: i32,
    /// The top-level window the element belongs to, when the backend can
    /// tell: a click must land in that window, not a sibling of the process.
    pub window: Option<u32>,
}

/// An OS accessibility API, reached from the one worker thread.
pub trait AccessibilityBackend {
    /// Whether this backend works on this OS; checked first, so an
    /// unsupported OS says so rather than failing on a target's binding.
    fn available(&self) -> ToolResult<()>;

    /// The element trees of the given top-level windows, `max_depth` levels
    /// below each window, read until `deadline` at most. A window that closes
    /// while it is read is left out; none left is `NotFound`.
    fn tree(&mut self, windows: &[u32], max_depth: usize, deadline: Instant) -> ToolResult<Read>;

    /// Performs `action` on a previously issued element and returns its
    /// state afterwards.
    fn act(&mut self, element: &str, action: &Action) -> ToolResult<Node>;

    /// The point a click on `element` should target.
    fn click_point(&mut self, element: &str) -> ToolResult<ClickPoint>;

    /// Whether the element at `(x, y)` is `element` or one of its
    /// descendants: checked before every click on an element, since a
    /// sibling or an overlay inside the same window can appear over it after
    /// its clickable point was read.
    fn hits(&mut self, element: &str, x: i32, y: i32) -> ToolResult<bool>;

    /// Asks the accessibility API to focus a top-level window.
    fn focus_window(&mut self, window: u32) -> ToolResult<()>;
}

/// The backend for an OS without one yet.
#[derive(Debug)]
pub struct Unsupported {
    reason: String,
}

impl Unsupported {
    /// Answers every call with `reason`.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn err<T>(&self) -> ToolResult<T> {
        Err(ToolError::NotSupported(self.reason.clone()))
    }
}

impl AccessibilityBackend for Unsupported {
    fn available(&self) -> ToolResult<()> {
        self.err()
    }
    fn tree(&mut self, _: &[u32], _: usize, _: Instant) -> ToolResult<Read> {
        self.err()
    }
    fn act(&mut self, _: &str, _: &Action) -> ToolResult<Node> {
        self.err()
    }
    fn click_point(&mut self, _: &str) -> ToolResult<ClickPoint> {
        self.err()
    }
    fn hits(&mut self, _: &str, _: i32, _: i32) -> ToolResult<bool> {
        self.err()
    }
    fn focus_window(&mut self, _: u32) -> ToolResult<()> {
        self.err()
    }
}

/// The backend for this OS.
pub fn backend() -> Box<dyn AccessibilityBackend> {
    #[cfg(target_os = "windows")]
    {
        match uia::Uia::new() {
            Ok(b) => Box::new(b),
            Err(e) => Box::new(Unsupported::new(format!(
                "UI Automation failed to initialize: {e}"
            ))),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(Unsupported::new(format!(
            "accessibility tools are not supported on {} yet (UIA only)",
            std::env::consts::OS
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, role: &str, name: &str, children: Vec<Node>) -> Node {
        Node {
            id: id.into(),
            role: role.into(),
            name: (!name.is_empty()).then(|| name.into()),
            value: None,
            automation_id: None,
            class_name: None,
            rect: None,
            enabled: true,
            has_keyboard_focus: false,
            is_keyboard_focusable: false,
            toggle_state: None,
            patterns: Vec::new(),
            children,
            omitted_children: None,
            gone: false,
            unmatchable: false,
        }
    }

    #[test]
    fn only_a_clipped_searched_property_makes_a_node_unmatchable() {
        let cut = format!("{}…", "x".repeat(CLIPPED_CHARS));
        let mut n = node("1", "Edit", "field", Vec::new());
        n.value = Some(cut.clone());
        n.class_name = Some(cut.clone());
        assert!(n.is_clipped());
        assert!(!n.searched_clipped());
        n.automation_id = Some(cut.clone());
        assert!(n.searched_clipped());
        n.automation_id = None;
        n.name = Some(cut);
        assert!(n.searched_clipped());
    }

    fn sample() -> Vec<Node> {
        vec![node(
            "e1",
            "Window",
            "Counter",
            vec![
                node("e2", "Text", "0", vec![]),
                node("e3", "Button", "Increment", vec![]),
                node("e4", "Button", "Reset count", vec![]),
            ],
        )]
    }

    #[test]
    fn criteria_combine_with_and() {
        let q = Query {
            role: Some("button".into()),
            name_contains: Some("INC".into()),
            ..Query::default()
        }
        .prepared()
        .expect("BUG: short criteria are valid");
        let found = search(&sample(), &q);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "e3");
    }

    /// `name_contains` uses full case folding: a German sharp s matches its
    /// capital spelling, a final sigma matches a capital one.
    #[test]
    fn name_contains_folds_unicode_case() {
        let named = |name: &str| node("e1", "Text", name, vec![]);
        let query = |needle: &str| {
            Query {
                name_contains: Some(needle.into()),
                ..Query::default()
            }
            .prepared()
            .expect("BUG: a short criterion is valid")
        };
        assert!(query("STRASSE").matches(&named("Hauptstraße 1")));
        assert!(query("ΟΣ").matches(&named("λόγος και")));
        assert!(!query("strasse").matches(&named("Hauptstrase")));
        // The window-title filter folds the same way.
        assert!(fold("Hauptstraße").contains(&fold("STRASSE")));
    }

    /// A node whose searched properties were not read in full matches no
    /// query, not even `name: ""` against its missing name.
    #[test]
    fn an_unreadable_node_matches_nothing() {
        let mut unread = node("e1", "Unknown", "", vec![]);
        unread.unmatchable = true;
        let query = Query {
            name: Some(String::new()),
            ..Query::default()
        };
        assert!(!query.matches(&unread));
        unread.unmatchable = false;
        assert!(query.matches(&unread));
    }

    #[test]
    fn exact_name_is_case_sensitive() {
        let q = Query {
            name: Some("increment".into()),
            ..Query::default()
        };
        assert!(search(&sample(), &q).is_empty());
    }

    #[test]
    fn search_is_depth_first_and_flat() {
        let q = Query {
            role: Some("Button".into()),
            ..Query::default()
        };
        let ids: Vec<_> = search(&sample(), &q).into_iter().map(|n| n.id).collect();
        assert_eq!(ids, ["e3", "e4"]);
        let q = Query {
            role: Some("window".into()),
            ..Query::default()
        };
        assert!(search(&sample(), &q)[0].children.is_empty());
    }

    #[test]
    fn summary_is_bounded() {
        let text = summarize(&sample(), 2);
        assert_eq!(text.lines().count(), 2);
        assert!(text.starts_with("e1 Window \"Counter\""), "{text}");
        assert_eq!(summarize(&[], 5), "(empty)");
    }
}
