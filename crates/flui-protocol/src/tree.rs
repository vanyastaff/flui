//! What a read returns: handles, nodes, the tree around them, the query that
//! scopes a read, and the one-line outline an agent reads in place of JSON
//! (ADR-0080 "Handles", "Vocabulary", "Replies" and "Reads and waits").
//!
//! [`Node`] carries exactly the fields the desktop server serializes, with the
//! same defaults left out, so one type describes an element whichever backend
//! read it.

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

use crate::version::{PROTOCOL_VERSION, ProtocolVersion, canonical_decimal};
use crate::wire::{ActionName, Checked, Role};

/// Why a string is not a handle of the kind asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHandleError {
    text: String,
    prefix: char,
}

impl fmt::Display for ParseHandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a handle of this kind; expected `{}` and a number from 1, such as `{}12`",
            self.text, self.prefix, self.prefix
        )
    }
}

impl std::error::Error for ParseHandleError {}

macro_rules! handle {
    ($(#[$meta:meta])* $name:ident, $prefix:literal, $pattern:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// The handle numbered `n`.
            #[must_use]
            pub const fn new(n: NonZeroU64) -> Self {
                Self(n)
            }

            /// The handle numbered `n`, or `None` for zero, which no handle is.
            #[must_use]
            pub const fn from_u64(n: u64) -> Option<Self> {
                match NonZeroU64::new(n) {
                    Some(n) => Some(Self(n)),
                    None => None,
                }
            }

            /// The handle's number.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = ParseHandleError;

            /// Only the spelling [`Display`](fmt::Display) writes: the
            /// prefix, then a decimal number from 1 with no sign and no
            /// leading zero.
            fn from_str(text: &str) -> Result<Self, Self::Err> {
                text.strip_prefix($prefix)
                    .and_then(canonical_decimal::<u64>)
                    .and_then(Self::from_u64)
                    .ok_or_else(|| ParseHandleError {
                        text: text.to_owned(),
                        prefix: $prefix.chars().next().expect("BUG: a handle prefix is one char"),
                    })
            }
        }

        #[cfg(feature = "serde")]
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": $pattern
                })
            }
        }
    };
}

handle! {
    /// An element's handle, `e12` on the wire.
    ///
    /// A handle names one element for as long as the backend can see it and is
    /// never re-bound to another: an action on an element that has gone
    /// answers `gone`. The desktop server numbers handles per session; the
    /// in-process backend uses the element's generational accessibility id,
    /// which is never reused.
    ElementId, "e", "^e[1-9][0-9]*$"
}

handle! {
    /// A window's handle, `w3` on the wire.
    WindowId, "w", "^w[1-9][0-9]*$"
}

/// An axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// What a tree's rectangles are measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Coordinates {
    /// Physical screen pixels, what the desktop server reports and what
    /// pointer tools take. Left out of a reply, being the default.
    #[default]
    Screen,
    /// Physical pixels from the top-left of the window's drawing surface: the
    /// in-process backend knows no window position.
    Surface,
}

impl Coordinates {
    /// Whether this is [`Self::Screen`], the value a reply leaves out.
    #[must_use]
    pub fn is_screen(&self) -> bool {
        *self == Self::Screen
    }
}

/// One element as a read reports it. A state at its default (enabled, not
/// focused, no children) is left out of the JSON, so a tree costs the reader
/// only what is notable about each node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Node {
    /// The handle later calls pass as `element`.
    pub id: ElementId,
    /// What kind of control it is, in the [`Role`] vocabulary.
    pub role: Role,
    /// What the backend's own API calls it (a UI Automation control type on
    /// the desktop, an AccessKit role in process).
    pub native_role: String,
    /// Accessible name (label).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<String>,
    /// Current value, for elements with a value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
    /// Developer-assigned id.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub automation_id: Option<String>,
    /// Toolkit class name.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub class_name: Option<String>,
    /// Bounds, in the tree's [`Coordinates`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub rect: Option<Rect>,
    /// Whether the element refuses interaction; reported only when it does.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub disabled: bool,
    /// Whether the element has keyboard focus now.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub focused: bool,
    /// Whether the element can take keyboard focus.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub focusable: bool,
    /// The checked state of a checkable element.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub checked: Option<Checked>,
    /// Whether an expandable element is expanded.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub expanded: Option<bool>,
    /// Whether a selectable item is selected.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub selected: Option<bool>,
    /// The tools that act on this element.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub actions: Vec<ActionName>,
    /// The window this element is the root of; on roots only.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
    /// Child elements, in tree order.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub children: Vec<Node>,
    /// Children left out because `max_depth` or the read's budget was
    /// reached.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub omitted_children: Option<usize>,
    /// The read stopped before it had seen all of this element's children:
    /// there may be more than `children` and `omitted_children` show.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub children_unread: bool,
    /// The element is gone: an action removed it, and the other fields are
    /// its state from just before.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub gone: bool,
}

impl Node {
    /// A node with every optional field at its default.
    #[must_use]
    pub fn new(id: ElementId, role: Role, native_role: impl Into<String>) -> Self {
        Self {
            id,
            role,
            native_role: native_role.into(),
            name: None,
            value: None,
            automation_id: None,
            class_name: None,
            rect: None,
            disabled: false,
            focused: false,
            focusable: false,
            checked: None,
            expanded: None,
            selected: None,
            actions: Vec::new(),
            window: None,
            children: Vec::new(),
            omitted_children: None,
            children_unread: false,
            gone: false,
        }
    }
}

/// The result of a read: one tree per window read, and what the read left
/// out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Tree {
    /// The schema version the reply was written in.
    #[cfg_attr(feature = "serde", serde(default = "current_version"))]
    pub protocol: ProtocolVersion,
    /// One tree per window read.
    pub roots: Vec<Node>,
    /// Elements reported.
    pub count: usize,
    /// Whether the read left anything out (its depth, its node budget): then
    /// read a subtree with `root`, or more nodes.
    pub truncated: bool,
    /// What the rectangles are measured from; left out when it is the screen.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Coordinates::is_screen")
    )]
    pub coordinates: Coordinates,
}

#[cfg(feature = "serde")]
fn current_version() -> ProtocolVersion {
    PROTOCOL_VERSION
}

impl Tree {
    /// A tree of `roots` in this crate's [`PROTOCOL_VERSION`], its `count`
    /// taken from the roots.
    #[must_use]
    pub fn new(roots: Vec<Node>, truncated: bool, coordinates: Coordinates) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            count: count(&roots),
            roots,
            truncated,
            coordinates,
        }
    }
}

/// How much of a tree a read returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ReadQuery {
    /// Read under this element instead of from the window.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub root: Option<ElementId>,
    /// Levels below the root to read; the root alone is depth 0.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_depth: Option<usize>,
    /// Elements to report at most.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_nodes: Option<usize>,
}

impl ReadQuery {
    /// The whole tree, unbounded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read under `root`.
    #[must_use]
    pub fn with_root(mut self, root: ElementId) -> Self {
        self.root = Some(root);
        self
    }

    /// Read at most `depth` levels below the root.
    #[must_use]
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Report at most `nodes` elements.
    #[must_use]
    pub fn with_max_nodes(mut self, nodes: usize) -> Self {
        self.max_nodes = Some(nodes);
        self
    }
}

/// The trees as an outline, one line per element, what an agent reads in
/// place of the JSON: `- role "name" [ref=e12] [state] [actions=...]`.
/// States at their default are left out; a root names its window; children
/// left out are counted.
#[must_use]
pub fn outline(roots: &[Node]) -> String {
    fn line(node: &Node, depth: usize, out: &mut String) {
        use std::fmt::Write as _;
        let _ = write!(out, "{}- {}", "  ".repeat(depth), node.role.name());
        if let Some(name) = &node.name {
            let _ = write!(out, " {name:?}");
        }
        let _ = write!(out, " [ref={}]", node.id);
        if let Some(w) = &node.window {
            let _ = write!(out, " [window={w}]");
        }
        if node.role == Role::Unknown {
            let _ = write!(out, " [native={}]", node.native_role);
        }
        if let Some(id) = &node.automation_id {
            let _ = write!(out, " [id={id:?}]");
        }
        if let Some(value) = &node.value {
            let _ = write!(out, " [value={value:?}]");
        }
        if let Some(c) = node.checked {
            let _ = write!(out, " [{c}]");
        }
        if let Some(e) = node.expanded {
            out.push_str(if e { " [expanded]" } else { " [collapsed]" });
        }
        if node.selected == Some(true) {
            out.push_str(" [selected]");
        }
        if node.disabled {
            out.push_str(" [disabled]");
        }
        if node.focused {
            out.push_str(" [focused]");
        }
        if node.gone {
            out.push_str(" [gone]");
        }
        if !node.actions.is_empty() {
            let names: Vec<&str> = node.actions.iter().map(|a| a.name()).collect();
            let _ = write!(out, " [actions={}]", names.join(","));
        }
        out.push('\n');
        for child in &node.children {
            line(child, depth + 1, out);
        }
        if let Some(n) = node.omitted_children {
            let _ = writeln!(
                out,
                "{}- … {n}{} more children not read",
                "  ".repeat(depth + 1),
                if node.children_unread { "+" } else { "" }
            );
        } else if node.children_unread {
            let _ = writeln!(out, "{}- … children not read", "  ".repeat(depth + 1));
        }
    }
    let mut out = String::new();
    for root in roots {
        line(root, 0, &mut out);
    }
    out
}

/// How many elements `roots` hold.
#[must_use]
pub fn count(roots: &[Node]) -> usize {
    roots.iter().map(|n| 1 + count(&n.children)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(n: u64) -> ElementId {
        ElementId::from_u64(n).expect("test handles are non-zero")
    }

    #[test]
    fn handles_display_with_their_prefix_and_parse_only_that_spelling() {
        assert_eq!(e(12).to_string(), "e12");
        assert_eq!("e12".parse(), Ok(e(12)));
        assert_eq!(
            "w3".parse::<WindowId>().map(WindowId::get),
            Ok(3),
            "a window handle is `w`"
        );
        for refused in [
            "", "e", "e0", "e012", "e+1", "E1", "12", "w12", " e1", "e1 ", "e-1",
        ] {
            assert!(
                refused.parse::<ElementId>().is_err(),
                "`{refused}` must not parse as an element handle"
            );
        }
        assert!("e3".parse::<WindowId>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn element_ids_serialize_as_e_handles_and_refuse_other_spellings() {
        assert_eq!(
            serde_json::to_value(e(4_294_967_303)).ok(),
            Some(serde_json::json!("e4294967303"))
        );
        assert_eq!(
            serde_json::from_value::<ElementId>(serde_json::json!("e7")).ok(),
            Some(e(7))
        );
        for refused in [
            serde_json::json!(7),
            serde_json::json!("7"),
            serde_json::json!("w7"),
            serde_json::json!("e0"),
            serde_json::json!("e07"),
        ] {
            assert!(
                serde_json::from_value::<ElementId>(refused.clone()).is_err(),
                "{refused} must not deserialize as an element handle"
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_node_at_its_defaults_serializes_only_role_id_and_native_role() {
        let node = Node::new(e(1), Role::Button, "Button");
        assert_eq!(
            serde_json::to_value(&node).ok(),
            Some(serde_json::json!({"id": "e1", "role": "button", "native_role": "Button"}))
        );
        let back: Node = serde_json::from_value(serde_json::json!(
            {"id": "e1", "role": "button", "native_role": "Button"}
        ))
        .expect("the defaults are optional on the way in");
        assert_eq!(back, node);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_tree_says_its_version_and_leaves_screen_coordinates_out() {
        let tree = Tree::new(
            vec![Node::new(e(1), Role::Window, "Window")],
            false,
            Coordinates::Screen,
        );
        assert_eq!(
            serde_json::to_value(&tree).ok(),
            Some(serde_json::json!({
                "protocol": "0.1",
                "roots": [{"id": "e1", "role": "window", "native_role": "Window"}],
                "count": 1,
                "truncated": false
            }))
        );
        let surface = Tree::new(Vec::new(), true, Coordinates::Surface);
        assert_eq!(
            serde_json::to_value(&surface)
                .ok()
                .and_then(|v| v.get("coordinates").cloned()),
            Some(serde_json::json!("surface"))
        );
        // A reply written before the two fields existed still reads.
        let older: Tree = serde_json::from_value(
            serde_json::json!({"roots": [], "count": 0, "truncated": false}),
        )
        .expect("the version and coordinates are optional on the way in");
        assert_eq!(
            (older.protocol, older.coordinates),
            (PROTOCOL_VERSION, Coordinates::Screen)
        );
    }

    #[test]
    fn outline_is_one_line_per_element() {
        let mut button = Node::new(e(3), Role::Button, "Button");
        button.name = Some("Increment".into());
        button.actions = vec![ActionName::Invoke, ActionName::Focus];
        let mut custom = Node::new(e(4), Role::Unknown, "Custom");
        custom.disabled = true;
        custom.omitted_children = Some(2);
        let mut root = Node::new(e(1), Role::Window, "Window");
        root.name = Some("Counter".into());
        root.window = Some(WindowId::from_u64(3).expect("non-zero"));
        root.children = vec![button, custom];

        assert_eq!(
            outline(&[root]),
            "- window \"Counter\" [ref=e1] [window=w3]\n\
             \x20 - button \"Increment\" [ref=e3] [actions=invoke,focus]\n\
             \x20 - unknown [ref=e4] [native=Custom] [disabled]\n\
             \x20   - … 2 more children not read\n"
        );
    }

    #[test]
    fn count_is_every_node_of_every_root() {
        let mut root = Node::new(e(1), Role::Window, "Window");
        root.children = vec![
            Node::new(e(2), Role::Label, "Text"),
            Node::new(e(3), Role::Button, "Button"),
        ];
        assert_eq!(count(&[root.clone(), root]), 6);
    }
}
