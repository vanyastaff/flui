//! The iOS app's elements as the Mac's accessibility API shows them on the Simulator's window,
//! matched to the app's own semantics tree and projected onto the wire's nodes (ADR-0095 §4):
//! what VoiceOver on iOS reads of the app.
//!
//! - **The screen.** The device's screen is a group in the device's window: in Simulator.app a
//!   child of the window, in DeviceHub (Xcode 27) a group deeper down whose subrole is
//!   [`SCREEN_SUBROLE`] ([`is_screen_candidate`]). Of those, the one whose frame has the shape of
//!   the app's window, which fills the screen, is the screen; none or two are refused
//!   ([`choose_screen`]). The window draws the screen scaled (DeviceHub at about three quarters),
//!   so an element's frame is placed on the app's window by the screen's scale
//!   ([`window_rect`]).
//! - **What VoiceOver reads.** `accesskit_ios` makes a node with an action, a value or a toggled
//!   state an element of its own, folding its descendants into its label, and a node with
//!   focusable children a container that is no element; the Simulator's bridge serves the
//!   elements VoiceOver reads, so a container the app publishes may be absent and its elements
//!   children of an outer one. The match follows that: each element is matched among the
//!   descendants of the app node its nearest matched ancestor fits, not among that node's children
//!   alone. When the screen's one child is an unnamed group (the app's root as a container), its
//!   children are read as the screen's.
//! - **What fits.** An element fits an app node when its native role is the one
//!   [`roles::native_for`] gives the app node's AccessKit role and toggled state, its label is the
//!   app node's name (its label, or a static text's value, as the adapter sets it), its enabled
//!   and checked states are the app node's, an identifier it carries is the app node's, and its
//!   frame on the window is the app node's surface rectangle within [`BOUNDS_TOLERANCE_POINTS`]
//!   on each edge. An element with no frame fits only an app node with none either.
//! - **Ambiguity** is refused, never guessed: two elements that fit one app node, or two app
//!   nodes that fit one element, answer [`TreeError::Ambiguous`].
//!
//! A matched element carries the app node's role and identifier, which the match checked against
//! its native role and could not read from it: a generic element stands for many roles. Its label,
//! value, hint and states are the OS's, and an empty value the app publishes reads as the empty
//! value it cannot be told from. An element no app node fits carries the coarse role its native
//! role stands for ([`roles::wire_role`]) and no identifier, so a projection shows it as a
//! difference.

use flui_protocol::error::ErrorCode;
use flui_protocol::geometry::LogicalSize;
use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::Checked;

use super::accessibility::PointFrame;
use super::roles::{self, SCREEN_SUBROLE};
use crate::error::Error;
use crate::lane::walk::MAX_NODES;

/// How far, in the window's points, an edge of an element's frame may sit from the same edge of
/// the app node it fits: the bridge places an iOS frame in the Mac's points and the window draws
/// the screen scaled down, so a frame rounded to whole Mac points moves by up to a point of the
/// device's.
pub const BOUNDS_TOLERANCE_POINTS: f64 = 1.5;

/// How far apart the screen's scale across and down may be for a group to have the window's
/// shape, as a fraction of the scale.
pub const SHAPE_TOLERANCE: f64 = 0.02;

/// The deepest a group may sit in the device's window and still be taken for its screen:
/// DeviceHub's sat at depth six to eight (Xcode 27.1, as `mgcrea/cupertino` measured it).
pub const MAX_SCREEN_DEPTH: usize = 10;

/// The most candidates an ambiguity names.
const MAX_CANDIDATES: usize = 10;

/// A group of the device's window that may be its screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenCandidate {
    /// Its frame in the Mac's global points.
    pub frame: PointFrame,
    /// Its subrole, if any.
    pub subrole: Option<String>,
    /// How deep it sits under the window: 1 for the window's child.
    pub depth: usize,
}

/// Why the Simulator's elements could not be read as the app's.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeError {
    /// The device's window shows no group of the app's window's shape.
    NoScreen {
        /// The window's title.
        title: String,
        /// The groups it shows, for people, at most ten.
        groups: Vec<String>,
    },
    /// The app published no tree of the window to match the elements against.
    NoAppTree,
    /// More than one candidate fits: two screens, or two elements or app nodes alike.
    Ambiguous {
        /// What they fit, for people.
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
            Self::NoScreen { title, groups } => Error::NotFound {
                message: format!(
                    "the window {title:?} shows no group with the shape of the app's window, \
                     which is the device's screen: the device has not drawn yet, or the window \
                     shows another device"
                ),
                candidates: groups,
                retry: ErrorCode::NotFound.default_retry(),
            },
            Self::NoAppTree => Error::NotFound {
                message: "the app published no tree of the window to match the Simulator's \
                          elements against"
                    .to_owned(),
                candidates: Vec::new(),
                retry: ErrorCode::NotFound.default_retry(),
            },
            Self::Ambiguous { node, candidates } => Error::Ambiguous {
                message: format!(
                    "{} candidates fit {node}; the lane does not guess between them",
                    candidates.len()
                ),
                candidates,
                retry: ErrorCode::Ambiguous.default_retry(),
            },
        }
    }
}

/// Whether an element of `ax_role` and `subrole`, `depth` levels under the device's window, may be
/// its screen: a group that is the window's child (Simulator.app) or carries [`SCREEN_SUBROLE`]
/// (DeviceHub).
#[must_use]
pub fn is_screen_candidate(ax_role: &str, subrole: Option<&str>, depth: usize) -> bool {
    ax_role == "AXGroup" && (depth == 1 || subrole == Some(SCREEN_SUBROLE))
}

/// The scale the window draws a screen of `frame` at for an app window of `size`, when `frame`
/// has the window's shape: its scale across and down agree within [`SHAPE_TOLERANCE`].
#[must_use]
pub fn screen_scale(frame: PointFrame, size: LogicalSize) -> Option<f64> {
    if !(frame.is_addressable() && size.width > 0.0 && size.height > 0.0) {
        return None;
    }
    let across = frame.width / size.width;
    let down = frame.height / size.height;
    let scale = across.max(down);
    (scale.is_finite() && (across - down).abs() <= SHAPE_TOLERANCE * scale).then_some(scale)
}

/// Which of `candidates`, the groups of the window titled `title`, is the device's screen for an
/// app window of `size`: the one with the window's shape ([`screen_scale`]). Two groups of one
/// frame are one screen, reached along two paths.
///
/// # Errors
///
/// [`TreeError::NoScreen`] when none has the shape, [`TreeError::Ambiguous`] when groups of two
/// frames do.
pub fn choose_screen(
    title: &str,
    candidates: &[ScreenCandidate],
    size: LogicalSize,
) -> Result<usize, TreeError> {
    let mut found: Vec<usize> = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let shaped = screen_scale(candidate.frame, size).is_some();
        let seen = found
            .iter()
            .any(|other| candidates[*other].frame == candidate.frame);
        if shaped && !seen {
            found.push(index);
        }
    }
    let describe = |candidate: &ScreenCandidate| {
        let frame = candidate.frame;
        format!(
            "a group at ({}, {}) of {} x {} points, depth {}{}",
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            candidate.depth,
            candidate
                .subrole
                .as_ref()
                .map_or_default(|subrole| format!(", subrole {subrole}"))
        )
    };
    match found.as_slice() {
        [index] => Ok(*index),
        [] => Err(TreeError::NoScreen {
            title: title.to_owned(),
            groups: candidates
                .iter()
                .take(MAX_CANDIDATES)
                .map(describe)
                .collect(),
        }),
        several => Err(TreeError::Ambiguous {
            node: format!("the screen of the window {title:?}"),
            candidates: several
                .iter()
                .take(MAX_CANDIDATES)
                .map(|index| describe(&candidates[*index]))
                .collect(),
        }),
    }
}

/// `frame`, an element's frame in the Mac's points, placed on the app's window of `size` at
/// `scale` whose screen the window draws at `screen`: in the window's device pixels, from its
/// top-left corner. `None` for a frame that is not addressable, or a screen without the window's
/// shape.
#[must_use]
pub fn window_rect(
    frame: PointFrame,
    screen: PointFrame,
    size: LogicalSize,
    scale: f64,
) -> Option<Rect> {
    if !(frame.is_addressable() && scale.is_finite() && scale > 0.0) {
        return None;
    }
    let drawn = screen_scale(screen, size)?;
    let device = |mac: f64, origin: f64| -> Option<i64> {
        let pixels = ((mac - origin) / drawn * scale).round();
        // Within i32's range, so the cast keeps the whole number.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number checked inside i32's range"
        )]
        let pixels =
            (pixels.is_finite() && pixels >= f64::from(i32::MIN) && pixels <= f64::from(i32::MAX))
                .then_some(pixels as i64);
        pixels
    };
    let left = device(frame.x, screen.x)?;
    let top = device(frame.y, screen.y)?;
    let right = device(frame.x + frame.width, screen.x)?;
    let bottom = device(frame.y + frame.height, screen.y)?;
    Some(Rect {
        x: i32::try_from(left).ok()?,
        y: i32::try_from(top).ok()?,
        width: u32::try_from(right - left)
            .ok()
            .filter(|width| *width > 0)?,
        height: u32::try_from(bottom - top)
            .ok()
            .filter(|height| *height > 0)?,
    })
}

/// The elements of `os`, the screen as `HostAccessibility::read` gives it, matched to `app`, the
/// app's tree of the window at `scale`, and projected: see the [module docs](self).
///
/// # Errors
///
/// [`TreeError::NoAppTree`] when `app` has no root, [`TreeError::Ambiguous`] when two candidates
/// fit one node.
pub fn read(os: &Tree, app: &Tree, scale: f64) -> Result<Tree, TreeError> {
    let Some(app_root) = app.roots.first() else {
        return Err(TreeError::NoAppTree);
    };
    let mut walk = Walk {
        next_id: 1,
        tolerance: bounds_tolerance(scale),
    };
    let mut roots = Vec::new();
    for screen in &os.roots {
        let (elements, unread) = lifted(screen);
        let mut root = walk.root(screen, app_root);
        root.children = walk.children(elements, Some(app_root))?;
        root.children_unread = unread;
        roots.push(root);
    }
    Ok(Tree::new(roots, os.truncated))
}

/// The projected node whose identifier is `identifier`, in `projection`, a [`read`]'s answer.
#[must_use]
pub fn element_of<'a>(projection: &'a [Node], identifier: &str) -> Option<&'a Node> {
    projection.iter().find_map(|node| {
        if node.identifier.as_deref() == Some(identifier) {
            Some(node)
        } else {
            element_of(&node.children, identifier)
        }
    })
}

/// The tolerance of [`BOUNDS_TOLERANCE_POINTS`] in device pixels at `scale`.
fn bounds_tolerance(scale: f64) -> i64 {
    let pixels = (BOUNDS_TOLERANCE_POINTS * scale).ceil();
    // A finite scale of a display is a few units, so the cast keeps the whole number.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked to be small"
    )]
    let pixels = if pixels.is_finite() && (0.0..=1024.0).contains(&pixels) {
        pixels as i64
    } else {
        0
    };
    pixels
}

/// The screen's elements, and whether some went unread: its children, or its one child's when
/// that child is the app's root read as a container (an unnamed group without an identifier).
fn lifted(screen: &Node) -> (&[Node], bool) {
    if let [only] = screen.children.as_slice()
        && only.native_role == "AXGroup"
        && only.name.is_none()
        && only.automation_id.is_none()
    {
        return (
            &only.children,
            screen.children_unread || only.children_unread,
        );
    }
    (&screen.children, screen.children_unread)
}

/// Whether the element `os` fits the app node `app`: see the [module docs](self).
fn fits(app: &Node, os: &Node, tolerance: i64) -> bool {
    os.native_role == roles::native_for(&app.native_role, app.checked.is_some())
        && os.name.as_deref().unwrap_or_default() == app.name.as_deref().unwrap_or_default()
        && os.disabled == app.disabled
        && checked_agrees(app.checked, os.checked)
        && os
            .automation_id
            .as_ref()
            .is_none_or(|identifier| app.identifier.as_ref() == Some(identifier))
        && bounds_agree(app.surface_rect, os.rect, tolerance)
}

/// Whether an element's checked state is the app node's `checked`, as the adapter reports a
/// toggled state: on or off, a mixed one reading as on.
fn checked_agrees(app: Option<Checked>, os: Option<Checked>) -> bool {
    match app {
        None => os.is_none(),
        Some(Checked::False) => os == Some(Checked::False),
        Some(Checked::True | Checked::Mixed) => os == Some(Checked::True),
    }
}

/// Whether an element's `os` rectangle on the window is the app's `surface` rectangle, each edge
/// within `tolerance` device pixels.
fn bounds_agree(surface: Option<Rect>, os: Option<Rect>, tolerance: i64) -> bool {
    let edges = |rect: Rect| {
        let left = i64::from(rect.x);
        let top = i64::from(rect.y);
        [
            left,
            top,
            left + i64::from(rect.width),
            top + i64::from(rect.height),
        ]
    };
    match (surface, os) {
        (Some(app), Some(os)) => edges(app)
            .iter()
            .zip(edges(os))
            .all(|(app, os)| (app - os).abs() <= tolerance),
        (None, None) => true,
        _ => false,
    }
}

/// An element, for people.
fn describe(node: &Node) -> String {
    let rect = node.rect.map_or_else(
        || "no frame".to_owned(),
        |rect| format!("[{},{} {}x{}]", rect.x, rect.y, rect.width, rect.height),
    );
    format!(
        "{} {:?} {rect}",
        node.native_role,
        node.name.as_deref().unwrap_or_default()
    )
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

/// The descendants of `node`, in document order, at most [`MAX_NODES`].
fn descendants(node: &Node) -> Vec<&Node> {
    let mut found = Vec::new();
    let mut pending: Vec<&Node> = node.children.iter().rev().collect();
    while let Some(next) = pending.pop() {
        if found.len() >= MAX_NODES {
            break;
        }
        found.push(next);
        pending.extend(next.children.iter().rev());
    }
    found
}

/// The projection's walk: the next handle and the bounds tolerance in device pixels.
struct Walk {
    next_id: u64,
    tolerance: i64,
}

impl Walk {
    fn next(&mut self) -> ElementId {
        let id = ElementId::from_u64(self.next_id)
            .expect("BUG: node numbers start at one and stay under the walk's node budget");
        self.next_id += 1;
        id
    }

    /// The screen as the root of the projection: the app's root, which the screen shows.
    fn root(&mut self, screen: &Node, app: &Node) -> Node {
        let mut root = Node::new(self.next(), app.role, screen.native_role.clone());
        root.rect = screen.rect;
        root.identifier.clone_from(&app.identifier);
        root.window = app.window;
        root
    }

    /// `elements` projected, each matched among the descendants of `scope`, the app node their
    /// nearest matched ancestor fits.
    fn children(
        &mut self,
        elements: &[Node],
        scope: Option<&Node>,
    ) -> Result<Vec<Node>, TreeError> {
        let matched = match scope {
            Some(scope) => self.matched(&descendants(scope), elements)?,
            None => vec![None; elements.len()],
        };
        let mut projected = Vec::with_capacity(elements.len());
        for (element, app) in elements.iter().zip(matched) {
            let mut node = self.node(element, app);
            node.children = self.children(&element.children, app.or(scope))?;
            projected.push(node);
        }
        Ok(projected)
    }

    /// The app node each of `elements` fits among `apps`, in their order.
    fn matched<'n>(
        &self,
        apps: &[&'n Node],
        elements: &[Node],
    ) -> Result<Vec<Option<&'n Node>>, TreeError> {
        let mut matched: Vec<Option<&'n Node>> = vec![None; elements.len()];
        for app in apps {
            let fitting: Vec<usize> = elements
                .iter()
                .enumerate()
                .filter(|(_, element)| fits(app, element, self.tolerance))
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
                            .map(|index| describe(&elements[*index]))
                            .collect(),
                    });
                }
            };
            if let Some(other) = matched[index] {
                return Err(TreeError::Ambiguous {
                    node: describe(&elements[index]),
                    candidates: vec![describe_app(other), describe_app(app)],
                });
            }
            matched[index] = Some(app);
        }
        Ok(matched)
    }

    /// `element` with a handle of the projection's, `app` the app node it matched.
    fn node(&mut self, element: &Node, app: Option<&Node>) -> Node {
        let mut node = Node::new(self.next(), element.role, element.native_role.clone());
        node.name.clone_from(&element.name);
        node.value.clone_from(&element.value);
        node.description.clone_from(&element.description);
        node.automation_id.clone_from(&element.automation_id);
        node.rect = element.rect;
        node.disabled = element.disabled;
        node.focused = element.focused;
        node.focusable = element.focusable;
        node.checked = element.checked;
        node.selected = element.selected;
        node.children_unread = element.children_unread;
        if let Some(app) = app {
            node.role = app.role;
            node.identifier.clone_from(&app.identifier);
            node.window = app.window;
            // An element reads an empty value as none: an app node whose value is empty reads
            // the empty value it cannot be told from.
            if node.value.is_none() && app.value.as_deref() == Some("") {
                node.value = Some(String::new());
            }
        }
        node
    }
}
