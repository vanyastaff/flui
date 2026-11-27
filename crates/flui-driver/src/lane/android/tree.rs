//! The app's accessibility nodes as `uiautomator dump` shows them, matched to the app's own
//! semantics tree and projected onto the wire's nodes (ADR-0095 §4): what TalkBack reads of the
//! app, read without an identifier.
//!
//! `accesskit_android` serves FLUI's nodes under the app's view, the root of the app's tree as
//! that view's own node, and sets no `viewIdResourceName`, so no node of a dump names the
//! identifier (ADR-0240) the app gave it. The lane finds each node by what both trees hold:
//!
//! - **The root.** `FluiView.install` adds the app's view to the decor view, as its first child
//!   over the whole window, so the app's root is a child of a window's top node of the dump that
//!   fits the app's root. Its class tells it from nothing: the adapter names the root's class
//!   from its role (an `android.view.View` for a window), and NativeActivity's own content view,
//!   a plain `android.view.View` inside the content frame, has the same bounds and states in an
//!   edge-to-edge window; a dump writes no importance for accessibility. Its place does.
//! - **Children.** The children of a matched app node are matched among the children of the
//!   dump's node it matched, in order: the adapter adds a node's children in the order the app's
//!   read lists them, leaving out those `accesskit_consumer`'s filter drops (`published`). A
//!   tie is settled by that order; two candidates for one node that the order does not tell apart
//!   answer [`TreeError::Ambiguous`].
//! - **What fits.** A node of a dump fits an app node when its class is the one
//!   `accesskit_android` gives the app node's AccessKit role ([`roles::class_for`]), its content
//!   description is the app node's name (its label, or a static text's value, as the adapter sets
//!   it), its toggled state is the app node's where the app's read reports it (`Toggle`:
//!   checkable and checked, a radio button's selection included; another selection role's is
//!   not reported), it is enabled as the app node is, and its bounds are the app
//!   node's surface rectangle placed at the surface's origin on the display ([`Mapping`], the
//!   lane's screen mapping) and clipped to it, within [`BOUNDS_TOLERANCE`] on each edge. A node
//!   with no area on the display fits only an app node with none there either: a list's row
//!   scrolled out of view.
//!
//! Bounds come from the device. They count only when they lie on the display with an area, as
//! UI Automator writes the bounds of a node it shows (clipped to the display); any other bounds
//! place the node nowhere, so the projection gives it no rectangle and no match reads them. The
//! projection keeps to the walk budgets of every lane ([`MAX_NODES`], [`MAX_DEPTH`]), and a node's
//! children are cut to the room the node budget has left before they are matched.
//!
//! A matched node carries the app node's role and identifier, which the match checked against the
//! node's class and could not read from it: one class stands for many roles. Its name, value and
//! states are the dump's, but for what the adapter serves and a dump cannot show
//! ([`Unobservable`]), which projects as the app published it, never as a difference. A node no
//! app node fits carries the coarse role its class stands for ([`roles::wire_role`]) and no
//! identifier, so a projection shows it as a difference. The adapter hands Android no description
//! and no expanded state, so an app node that publishes one projects without it: TalkBack does
//! not read it either.

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

/// How many announcer nodes the app's root holds last: FLUI's polite and assertive live regions
/// (`SemanticsOwner::adopt_announcers`).
pub const ANNOUNCERS: usize = 2;

/// What `accesskit_android` serves Android and a dump cannot show, or what the app's read leaves
/// out and the adapter serves: the lane cannot observe it, so it is never a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unobservable {
    /// A range's value: the adapter hands it to Android as the node's `RangeInfo`, which
    /// UI Automator does not write. A matched slider, spin button, progress bar, meter or scroll
    /// bar whose value the app's read gives as a number, and whose dump node has no text, projects
    /// the app's value.
    RangeValue,
    /// The announcer nodes ([`ANNOUNCERS`]): static text with no area that the app's root holds
    /// last, carrying what TalkBack speaks. The adapter serves them; the app's read leaves them
    /// out, since they are no element of the interface. The root's last children of that shape
    /// that no app node fits are left out of the projection.
    Announcers,
    /// A toggled state the app's read does not report: the read follows UI Automation, which
    /// offers a node of a selection role other than a radio button (a grid cell, a list item
    /// that publishes a selection) as `SelectionItem`, reading its `selected` and no `checked`,
    /// while the adapter publishes its toggled state as checkable and checked. The match does not
    /// read the dump's checked state of such a node, and the projection reports its selection as
    /// the read does (the `Toggle::Unread` case).
    UnreportedToggle,
}

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
    /// No child of a window's top node of the app's package fits the app's root: the app serves
    /// no tree, or another one.
    NoWindow {
        /// The app's package.
        package: String,
    },
    /// More than one candidate fits a node, and the adapter's order does not tell them apart.
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
                    "the dump shows no view of {package} over its window that is the app's window \
                     as its tree publishes it: the app serves Android no accessibility tree, or \
                     another one"
                ),
                candidates: Vec::new(),
                effect: None,
                retry: ErrorCode::NotFound.default_retry(),
            },
            Self::Ambiguous { node, candidates } => Error::Ambiguous {
                message: format!(
                    "{} candidates of the same role, name, state and bounds fit {node}, and the \
                     order the adapter adds them in does not tell them apart; the lane does not \
                     guess between them",
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

/// The child of a window's top node, of `package`, that fits `app`, the app's root: where
/// `FluiView.install` puts the app's view.
fn find_root<'d>(
    dump: &'d UiDump,
    package: &str,
    app: &Node,
    mapping: Mapping,
) -> Result<&'d UiNode, TreeError> {
    let found: Vec<&UiNode> = dump
        .roots
        .iter()
        .flat_map(|top| &top.children)
        .filter(|node| node.package == package && fits(app, node, mapping))
        .collect();
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
    let toggle = Toggle::of(app);
    os.class == roles::class_for(&app.native_role, toggle.is_toggled())
        && os.content_desc == app.name.as_deref().unwrap_or_default()
        && os.enabled != app.disabled
        && toggle.agrees(os)
        && bounds_agree(app.surface_rect, os.bounds, mapping)
}

/// What the app's read says of `app`'s toggled state, which the adapter publishes for any node
/// with one as checkable and checked.
///
/// The read follows UI Automation, which offers a node of a selection role as `SelectionItem`
/// (`agent::offers_selection_item` in `flui-semantics` decides which): such a node reads
/// `selected` and no `checked`, and no other node reads `selected`. A radio button's selection is
/// its toggled state; any other's toggled state the read does not report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    /// The read reports it as `checked`: none, or the state.
    Checked(Option<Checked>),
    /// A radio button's selection, which is its toggled state.
    RadioSelection(bool),
    /// A node of a selection role whose toggled state the read does not report: the dump's
    /// checkable and checked are unobservable from the app's side
    /// ([`Unobservable::UnreportedToggle`]).
    Unread,
}

impl Toggle {
    fn of(app: &Node) -> Self {
        // The read reports `selected` on a node it offers as `SelectionItem` and on no other, and
        // then no `checked`: the selection roles are its decision, which the driver does not copy.
        let selection = app.checked.is_none() && app.selected.is_some();
        match app.native_role.as_str() {
            "RadioButton" | "MenuItemRadio" if selection => {
                Self::RadioSelection(app.selected == Some(true))
            }
            _ if selection => Self::Unread,
            _ => Self::Checked(app.checked),
        }
    }

    /// Whether the node has a toggled state, which turns a button into a toggle button.
    fn is_toggled(self) -> bool {
        matches!(self, Self::Checked(Some(_)) | Self::RadioSelection(_))
    }

    /// Whether `os`'s checked state is this one, as the adapter sets it: checkable with a toggled
    /// state, a mixed one reading as checked.
    fn agrees(self, os: &UiNode) -> bool {
        match self {
            Self::Checked(None) => !os.checkable,
            Self::Checked(Some(Checked::False)) | Self::RadioSelection(false) => {
                os.checkable && !os.checked
            }
            Self::Checked(Some(Checked::True | Checked::Mixed)) | Self::RadioSelection(true) => {
                os.checkable && os.checked
            }
            Self::Unread => true,
        }
    }
}

/// A rectangle's edges, wide enough that no sum of a position and a size overflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edges {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Edges {
    fn of(rect: Rect) -> Self {
        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        Self {
            left,
            top,
            right: left + i64::from(rect.width),
            bottom: top + i64::from(rect.height),
        }
    }

    /// These edges moved by `origin`.
    fn at(self, origin: DevicePoint) -> Self {
        let (x, y) = (i64::from(origin.x), i64::from(origin.y));
        Self {
            left: self.left + x,
            top: self.top + y,
            right: self.right + x,
            bottom: self.bottom + y,
        }
    }

    /// Whether these edges and `other` share an area, as AccessKit's `Rect::intersect` reads it.
    fn meets(self, other: Self) -> bool {
        self.left.max(other.left) < self.right.min(other.right)
            && self.top.max(other.top) < self.bottom.min(other.bottom)
    }

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
        Edges::of(rect)
            .at(mapping.origin)
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

/// The children of `parent` the adapter hands Android, in order: `accesskit_consumer`'s
/// `common_filter`, which the app's read keeps without its clipping rule. A child whose box shares
/// no area with its parent's is left out, with everything under it, unless the sibling before or
/// after it shares an area with the parent's box: a list hands Android one row past its viewport
/// on each side and keeps the rest of the rows it built to itself.
///
/// AccessKit applies the rule under a parent that clips its children, which the wire does not
/// say; the rule is applied under every parent, since a child laid out wholly outside its parent
/// is a row a scroll view keeps past its viewport, which clips it. A child with no box, or under
/// a parent with none, is kept.
fn published(parent: &Node) -> Vec<&Node> {
    let Some(parent_box) = parent.surface_rect.map(Edges::of) else {
        return parent.children.iter().collect();
    };
    let meets = |node: &Node| {
        node.surface_rect
            .is_none_or(|rect| Edges::of(rect).meets(parent_box))
    };
    let children = &parent.children;
    children
        .iter()
        .enumerate()
        .filter(|(index, child)| {
            meets(child)
                || index
                    .checked_sub(1)
                    .and_then(|before| children.get(before))
                    .is_some_and(meets)
                || children.get(index + 1).is_some_and(meets)
        })
        .map(|(_, child)| child)
        .collect()
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

/// Whether `os` has the shape of an announcer node ([`Unobservable::Announcers`]): static text
/// with no area.
fn announcer_shaped(os: &UiNode) -> bool {
    os.class == roles::class_for("Label", false)
        && os
            .bounds
            .is_none_or(|bounds| bounds.width() == 0 || bounds.height() == 0)
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
        // The children the node budget leaves room for, cut before they are aligned: a dump wider
        // than the budget costs no more than the budget.
        let room =
            (MAX_NODES + 1).saturating_sub(usize::try_from(self.next_id).unwrap_or(usize::MAX));
        let cut = children.len() > room;
        if cut {
            children.truncate(room);
            node.children_unread = true;
            self.truncated = true;
        }
        let mut matched = match app {
            Some(app) => self.match_children(app, &children)?,
            None => vec![None; children.len()],
        };
        if depth == 0 && app.is_some() && !cut {
            // The announcers the app's read leaves out: the root's last children of their shape
            // that no app node fits.
            let mut left_out = 0;
            while left_out < ANNOUNCERS
                && children.last().is_some_and(|last| announcer_shaped(last))
                && matched.last().is_some_and(Option::is_none)
            {
                children.pop();
                matched.pop();
                left_out += 1;
            }
        }
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

    /// The app node each of `children` fits among the children of `parent`, in their order. The
    /// children the adapter hands Android under a parent that clips them ([`published`]) are
    /// matched first; under a parent that does not, the adapter hands it every child, so every
    /// child is matched too, and the alignment that holds is taken: the one without an
    /// ambiguity, of the two without one the one that pairs more nodes, the first on a tie.
    ///
    /// # Errors
    ///
    /// [`TreeError::Ambiguous`] when neither alignment holds, the first's ambiguity.
    fn match_children<'n>(
        &self,
        parent: &'n Node,
        children: &[&UiNode],
    ) -> Result<Vec<Option<&'n Node>>, TreeError> {
        let clipped = published(parent);
        let first = self.align(&clipped, children);
        if clipped.len() == parent.children.len() {
            return first;
        }
        let every: Vec<&'n Node> = parent.children.iter().collect();
        match (first, self.align(&every, children)) {
            (Ok(first), Ok(second))
                if second.iter().flatten().count() > first.iter().flatten().count() =>
            {
                Ok(second)
            }
            (Ok(first), _) => Ok(first),
            (Err(_), Ok(second)) => Ok(second),
            (Err(error), Err(_)) => Err(error),
        }
    }

    /// The app node of `apps` each of `children` fits, in their order: each child of the dump
    /// takes the first app node past the one the child before it took that it fits.
    ///
    /// # Errors
    ///
    /// [`TreeError::Ambiguous`] when a pair has a rival the order does not rule out: another app
    /// node between the pairs before and after it that fits the same node of the dump, or another
    /// node of the dump between them that fits the same app node.
    fn align<'n>(
        &self,
        apps: &[&'n Node],
        children: &[&UiNode],
    ) -> Result<Vec<Option<&'n Node>>, TreeError> {
        let fit = |app: usize, os: usize| fits(apps[app], children[os], self.mapping);
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut next_app = 0;
        for os in 0..children.len() {
            if let Some(app) = (next_app..apps.len()).find(|&app| fit(app, os)) {
                pairs.push((os, app));
                next_app = app + 1;
            }
        }
        for (index, &(os, app)) in pairs.iter().enumerate() {
            let before = index.checked_sub(1).map(|before| pairs[before]);
            let after = pairs.get(index + 1).copied();
            let app_range =
                before.map_or(0, |(_, app)| app + 1)..after.map_or(apps.len(), |(_, app)| app);
            let rival_apps: Vec<usize> = app_range
                .filter(|&rival| rival != app && fit(rival, os))
                .collect();
            if !rival_apps.is_empty() {
                return Err(TreeError::Ambiguous {
                    node: describe(children[os]),
                    candidates: std::iter::once(app)
                        .chain(rival_apps)
                        .take(MAX_CANDIDATES)
                        .map(|candidate| describe_app(apps[candidate]))
                        .collect(),
                });
            }
            let os_range =
                before.map_or(0, |(os, _)| os + 1)..after.map_or(children.len(), |(os, _)| os);
            let rival_nodes: Vec<usize> = os_range
                .filter(|&rival| rival != os && fit(app, rival))
                .collect();
            if !rival_nodes.is_empty() {
                return Err(TreeError::Ambiguous {
                    node: describe_app(apps[app]),
                    candidates: std::iter::once(os)
                        .chain(rival_nodes)
                        .take(MAX_CANDIDATES)
                        .map(|candidate| describe(children[candidate]))
                        .collect(),
                });
            }
        }
        let mut matched: Vec<Option<&'n Node>> = vec![None; children.len()];
        for (os, app) in pairs {
            matched[os] = Some(apps[app]);
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
        node.value = filled(&os.text).or_else(|| {
            let value = app.and_then(|app| app.value.clone())?;
            // A dump writes an absent text as an empty one: an app node whose value is empty
            // reads the empty value it cannot be told from, and a range's value is
            // unobservable.
            (value.is_empty() || app.is_some_and(|app| range_valued(app, &value))).then_some(value)
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
        let checked = os.checkable.then_some(os.checked);
        // In the vocabulary of the app's read (`Toggle`): a radio button's toggled state is its
        // selection, and another selection role's toggled state is not reported.
        match app.map(Toggle::of) {
            Some(Toggle::RadioSelection(_)) => node.selected = checked,
            Some(Toggle::Unread) => node.selected = os.selected.then_some(true),
            Some(Toggle::Checked(_)) | None => {
                node.checked = checked.map(|on| if on { Checked::True } else { Checked::False });
                node.selected = os.selected.then_some(true);
            }
        }
        node
    }
}

/// Whether `value`, `app`'s value, is a range's ([`Unobservable::RangeValue`]): a number on a
/// node of a role FLUI publishes a numeric range for.
fn range_valued(app: &Node, value: &str) -> bool {
    matches!(
        app.native_role.as_str(),
        "Slider" | "SpinButton" | "ProgressIndicator" | "Meter" | "ScrollBar"
    ) && value.parse::<f64>().is_ok_and(f64::is_finite)
}
