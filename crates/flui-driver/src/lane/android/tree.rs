//! The app's accessibility nodes as `uiautomator dump` shows them, matched to the app's own
//! semantics tree and projected onto the wire's nodes (ADR-0095 §4): what TalkBack reads of the
//! app, read without an identifier.
//!
//! `accesskit_android` serves FLUI's nodes under the app's view, the root of the app's tree as
//! that view's own node, and sets no `viewIdResourceName`, so no node of a dump names the
//! identifier (ADR-0240) the app gave it. The lane finds each node by what both trees hold:
//!
//! - **Structure.** The app's root is the outermost node of the app's package that fits it; each
//!   child of the app's node is matched among the children of the dump's node it matched, which
//!   the adapter adds in the order the app's read lists them.
//! - **What fits.** A node of a dump fits an app node when its class is the one
//!   `accesskit_android` gives the app node's AccessKit role ([`roles::class_for`]), its content
//!   description is the app node's name (its label, or a static text's value, as the adapter sets
//!   it), its checked and enabled states are the app node's, and its bounds are the app node's
//!   surface rectangle placed at the surface's origin on the display ([`Mapping`], the lane's
//!   screen mapping) and clipped to it, within [`BOUNDS_TOLERANCE`] on each edge. A node with no
//!   area on the display fits only an app node with none there either: a list's row scrolled out
//!   of view.
//! - **Ambiguity** is refused, never guessed: two nodes of a dump that fit one app node, or two app
//!   nodes that fit one node of a dump, answer [`TreeError::Ambiguous`].
//!
//! Bounds come from the device. They count only when they lie on the display with an area, as
//! UI Automator writes the bounds of a node it shows (clipped to the display); any other bounds
//! place the node nowhere, so the projection gives it no rectangle and no match reads them. The
//! projection keeps to the walk budgets of every lane ([`MAX_NODES`], [`MAX_DEPTH`]), and matching
//! compares each app child with at most [`MAX_NODES`] children of its node.
//!
//! A matched node carries the app node's role and identifier, which the match checked against the
//! node's class and could not read from it: one class stands for many roles. Its name, value and
//! states are the dump's. A node no app node fits carries the coarse role its class stands for
//! ([`roles::wire_role`]) and no identifier, so a projection shows it as a difference. A dump holds
//! no description, no expanded state and no range's value (the adapter hands Android no
//! description and no expanded state, and UI Automator writes no range), so an app node that
//! publishes one projects without it.

use flui_protocol::error::ErrorCode;
use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::Checked;

use super::roles;
use super::uiautomator::{Bounds, UiDump, UiNode};
use crate::coordinates::DevicePoint;
use crate::error::Error;
use crate::lane::walk::{MAX_DEPTH, MAX_NODES};

/// How far, in display pixels, an edge of a node of a dump may sit from the same edge of the app
/// node it fits: the adapter truncates its bounds to whole pixels and the app's read covers them
/// with whole pixels, each from the same box.
pub const BOUNDS_TOLERANCE: i64 = 2;

/// The most candidates an ambiguity names.
const MAX_CANDIDATES: usize = 10;

/// Where the app's surface sits on the display, as the lane maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    /// The surface's top-left corner, in display pixels.
    pub origin: DevicePoint,
    /// The display's width and height in pixels, as it is turned now.
    pub display: (i32, i32),
}

/// Why a dump could not be matched to the app's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeError {
    /// No node of the app's package fits the app's root: the app serves no tree, or another one.
    NoWindow {
        /// The app's package.
        package: String,
    },
    /// More than one candidate fits a node: two nodes of the same role, name, state and bounds.
    Ambiguous {
        /// The node they fit, for people.
        node: String,
        /// The candidates, for people, at most ten.
        candidates: Vec<String>,
    },
}

impl TreeError {
    /// The error the failure answers with.
    #[must_use]
    pub fn into_error(self) -> Error {
        match self {
            Self::NoWindow { package } => Error::NotFound {
                message: format!(
                    "the dump shows no node of {package} that is the app's window as its tree \
                     publishes it: the app serves Android no accessibility tree, or another one"
                ),
                candidates: Vec::new(),
                retry: ErrorCode::NotFound.default_retry(),
            },
            Self::Ambiguous { node, candidates } => Error::Ambiguous {
                message: format!(
                    "{} candidates of the same role, name, state and bounds fit {node}; the lane \
                     does not guess between them",
                    candidates.len()
                ),
                candidates,
                retry: ErrorCode::Ambiguous.default_retry(),
            },
        }
    }
}

/// The app's nodes of `dump`, matched to `app`, the app's tree of the window, and projected: see
/// the [module docs](self). `package` is the app's; nodes of another package are left out.
///
/// # Errors
///
/// [`TreeError::NoWindow`] when no node fits the app's root, [`TreeError::Ambiguous`] when two
/// candidates fit one node.
pub fn read(dump: &UiDump, package: &str, app: &Tree, mapping: Mapping) -> Result<Tree, TreeError> {
    let mut walk = Walk {
        package,
        mapping,
        next_id: 1,
        truncated: false,
    };
    let mut roots = Vec::new();
    for app_root in &app.roots {
        let root = find_root(dump, package, app_root, mapping)?;
        roots.push(walk.project(root, Some(app_root), 0)?);
    }
    if roots.is_empty() {
        return Err(TreeError::NoWindow {
            package: package.to_owned(),
        });
    }
    Ok(Tree::new(roots, walk.truncated))
}

/// The outermost node of `package` in `dump` that fits `app`, the app's root.
fn find_root<'d>(
    dump: &'d UiDump,
    package: &str,
    app: &Node,
    mapping: Mapping,
) -> Result<&'d UiNode, TreeError> {
    let mut found: Vec<&UiNode> = Vec::new();
    let mut pending: Vec<&UiNode> = dump.roots.iter().rev().collect();
    while let Some(node) = pending.pop() {
        if node.package == package && fits(app, node, mapping) {
            // The outermost: what fits inside it is its own content.
            found.push(node);
            continue;
        }
        pending.extend(node.children.iter().rev());
    }
    match found.as_slice() {
        [root] => Ok(root),
        [] => Err(TreeError::NoWindow {
            package: package.to_owned(),
        }),
        several => Err(TreeError::Ambiguous {
            node: describe_app(app),
            candidates: several
                .iter()
                .take(MAX_CANDIDATES)
                .map(|node| describe(node))
                .collect(),
        }),
    }
}

/// Whether the node `os` of a dump fits the app node `app`: see the [module docs](self).
fn fits(app: &Node, os: &UiNode, mapping: Mapping) -> bool {
    os.class == roles::class_for(&app.native_role, app.checked.is_some())
        && os.content_desc == app.name.as_deref().unwrap_or_default()
        && os.enabled != app.disabled
        && checked_agrees(app.checked, os)
        && bounds_agree(app.surface_rect, os.bounds, mapping)
}

/// Whether `os`'s checked state is `checked`, as the adapter sets it: checkable with a toggled
/// state, a mixed one reading as checked.
fn checked_agrees(checked: Option<Checked>, os: &UiNode) -> bool {
    match checked {
        None => !os.checkable,
        Some(Checked::False) => os.checkable && !os.checked,
        Some(Checked::True | Checked::Mixed) => os.checkable && os.checked,
    }
}

/// A rectangle's edges in display pixels, wide enough that no sum of a position and a size
/// overflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edges {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Edges {
    /// The part of these edges on a display of `size`, when it has an area.
    fn on_display(self, (width, height): (i32, i32)) -> Option<Self> {
        let clipped = Self {
            left: self.left.max(0),
            top: self.top.max(0),
            right: self.right.min(i64::from(width)),
            bottom: self.bottom.min(i64::from(height)),
        };
        (clipped.left < clipped.right && clipped.top < clipped.bottom).then_some(clipped)
    }
}

/// `bounds` of a node of a dump, when they lie on a display of `display` with an area: bounds a
/// dump writes for a node it shows. Any other bounds the device sent place the node nowhere.
fn shown(bounds: Option<Bounds>, (width, height): (i32, i32)) -> Option<Bounds> {
    bounds.filter(|bounds| {
        0 <= bounds.left
            && bounds.left < bounds.right
            && bounds.right <= width
            && 0 <= bounds.top
            && bounds.top < bounds.bottom
            && bounds.bottom <= height
    })
}

/// Whether a dump's `bounds` are where the app's `surface` rectangle shows on the display.
fn bounds_agree(surface: Option<Rect>, bounds: Option<Bounds>, mapping: Mapping) -> bool {
    let app = surface.and_then(|rect| {
        let left = i64::from(rect.x) + i64::from(mapping.origin.x);
        let top = i64::from(rect.y) + i64::from(mapping.origin.y);
        Edges {
            left,
            top,
            right: left + i64::from(rect.width),
            bottom: top + i64::from(rect.height),
        }
        .on_display(mapping.display)
    });
    let os = shown(bounds, mapping.display).map(|bounds| Edges {
        left: i64::from(bounds.left),
        top: i64::from(bounds.top),
        right: i64::from(bounds.right),
        bottom: i64::from(bounds.bottom),
    });
    match (app, os) {
        (Some(app), Some(os)) => [
            (app.left, os.left),
            (app.top, os.top),
            (app.right, os.right),
            (app.bottom, os.bottom),
        ]
        .iter()
        .all(|(app, os)| (app - os).abs() <= BOUNDS_TOLERANCE),
        (None, None) => true,
        _ => false,
    }
}

/// A node of a dump, for people.
fn describe(node: &UiNode) -> String {
    let bounds = node.bounds.map_or_else(
        || "no bounds".to_owned(),
        |bounds| {
            format!(
                "[{},{}][{},{}]",
                bounds.left, bounds.top, bounds.right, bounds.bottom
            )
        },
    );
    format!("{} {:?} {bounds}", node.class, node.content_desc)
}

/// An app node, for people.
fn describe_app(node: &Node) -> String {
    let identifier = node
        .identifier
        .as_ref()
        .map_or_default(|identifier| format!(" [identifier={identifier:?}]"));
    format!(
        "the app's {} {:?}{identifier}",
        node.role.name(),
        node.name.as_deref().unwrap_or_default()
    )
}

/// The projection's walk: the next handle, and whether a budget cut it short.
struct Walk<'a> {
    package: &'a str,
    mapping: Mapping,
    next_id: u64,
    truncated: bool,
}

impl Walk<'_> {
    /// `os` and its nodes of the app's package, projected, `app` the app node it matched.
    fn project(
        &mut self,
        os: &UiNode,
        app: Option<&Node>,
        depth: usize,
    ) -> Result<Node, TreeError> {
        let id = ElementId::from_u64(self.next_id)
            .expect("BUG: node numbers start at one and stay under the node budget");
        self.next_id += 1;
        let mut node = self.node(os, app, id);
        let package = self.package;
        let mut children: Vec<&UiNode> = os
            .children
            .iter()
            .filter(|child| child.package == package)
            .take(MAX_NODES + 1)
            .collect();
        if children.is_empty() {
            return Ok(node);
        }
        if depth >= MAX_DEPTH {
            node.children_unread = true;
            self.truncated = true;
            return Ok(node);
        }
        if children.len() > MAX_NODES {
            children.truncate(MAX_NODES);
            node.children_unread = true;
            self.truncated = true;
        }
        let matched = match app {
            Some(app) => self.match_children(&app.children, &children)?,
            None => vec![None; children.len()],
        };
        for (child, app_child) in children.into_iter().zip(matched) {
            if usize::try_from(self.next_id).unwrap_or(usize::MAX) > MAX_NODES {
                node.children_unread = true;
                self.truncated = true;
                break;
            }
            let projected = self.project(child, app_child, depth + 1)?;
            node.children.push(projected);
        }
        Ok(node)
    }

    /// The app node each of `children` fits among `apps`, in their order.
    fn match_children<'n>(
        &self,
        apps: &'n [Node],
        children: &[&UiNode],
    ) -> Result<Vec<Option<&'n Node>>, TreeError> {
        let mut matched: Vec<Option<&'n Node>> = vec![None; children.len()];
        for app in apps {
            let fitting: Vec<usize> = children
                .iter()
                .enumerate()
                .filter(|(_, child)| fits(app, child, self.mapping))
                .map(|(index, _)| index)
                .collect();
            let index = match fitting.as_slice() {
                [] => continue,
                [index] => *index,
                several => {
                    return Err(TreeError::Ambiguous {
                        node: describe_app(app),
                        candidates: several
                            .iter()
                            .take(MAX_CANDIDATES)
                            .map(|index| describe(children[*index]))
                            .collect(),
                    });
                }
            };
            if let Some(other) = matched[index] {
                return Err(TreeError::Ambiguous {
                    node: describe(children[index]),
                    candidates: vec![describe_app(other), describe_app(app)],
                });
            }
            matched[index] = Some(app);
        }
        Ok(matched)
    }

    /// `os` as a wire node with the handle `id`, `app` the app node it matched.
    fn node(&self, os: &UiNode, app: Option<&Node>, id: ElementId) -> Node {
        let role = app.map_or_else(|| roles::wire_role(&os.class), |app| app.role);
        let mut node = Node::new(id, role, os.class.clone());
        let filled = |text: &str| (!text.is_empty()).then(|| text.to_owned());
        node.class_name = filled(&os.class);
        node.name = filled(&os.content_desc);
        // A dump writes an absent text as an empty one: an app node whose value is empty reads
        // the empty value it cannot be told from.
        node.value = filled(&os.text).or_else(|| {
            app.and_then(|app| app.value.clone())
                .filter(String::is_empty)
        });
        node.identifier = app.and_then(|app| app.identifier.clone());
        node.window = app.and_then(|app| app.window);
        node.rect = shown(os.bounds, self.mapping.display).map(|bounds| Rect {
            x: bounds.left,
            y: bounds.top,
            width: bounds.width().unsigned_abs(),
            height: bounds.height().unsigned_abs(),
        });
        node.disabled = !os.enabled;
        node.focused = os.focused;
        node.focusable = os.focusable;
        node.checked = os.checkable.then_some(if os.checked {
            Checked::True
        } else {
            Checked::False
        });
        node.selected = os.selected.then_some(true);
        node
    }
}
