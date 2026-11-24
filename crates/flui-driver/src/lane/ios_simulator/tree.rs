//! The iOS app's elements as the Mac's accessibility API shows them on the Simulator's window,
//! projected onto the wire's nodes and matched to the app's own semantics tree (ADR-0095 §4):
//! what the Simulator shows the Mac of the app, the elements VoiceOver on iOS reads.
//!
//! - **The window.** The device's window is the one window on screen titled for it; a window
//!   whose attributes name another simulator is not it, and two windows of the title are refused
//!   ([`find_screen`]).
//! - **The screen.** The device's screen is a group of that window with the shape of the app's
//!   window, which fills the screen: in Simulator.app a child of the window or of a group the
//!   bezel adds around it, in DeviceHub (Xcode 27) a group about seven levels down whose subrole
//!   is [`SCREEN_SUBROLE`]. The search walks every group within [`MAX_SCREEN_DEPTH`] levels,
//!   going into each but a [`SCREEN_SUBROLE`] one, whose children are the app's, and
//!   [`choose_screen`] takes a [`SCREEN_SUBROLE`] group when there is one and the shallowest of
//!   the window's shape otherwise. The window draws the screen scaled (DeviceHub at about three
//!   quarters), so an element's frame is placed on the app's window by the screen's scale
//!   ([`window_rect`]).
//! - **What an element reads as** ([`describe`]): its native role (`AXRole` or
//!   `AXRole/AXSubrole`), its label (`AXDescription`, else `AXTitle`, else a static text's
//!   value, the rule the macOS lane shares, `lane::ax::label`), its value, hint (`AXHelp`),
//!   identifier, enabled, focused, selected and checked states, and its frame on the window.
//! - **What VoiceOver reads.** `accesskit_ios` makes a node with an action, a value or a toggled
//!   state an element of its own, folding its descendants into its label, and a node with
//!   focusable children a container that is no element; the bridge serves the elements VoiceOver
//!   reads, so a container the app publishes may be absent and its elements children of an outer
//!   one. The match follows that: each element is matched among the descendants of the app node
//!   its nearest matched ancestor fits, not among that node's children alone, and an app node
//!   matched once is matched to nothing else. When the screen's one child is an unnamed group
//!   (the app's root as a container), its children are read as the screen's.
//! - **What fits.** An element fits an app node when its native role is the one
//!   [`roles::native_for`] gives the app node's AccessKit role and toggled state, its label is the
//!   app node's name, its enabled and checked states are the app node's, an identifier it carries
//!   is the app node's, and its frame on the window is the app node's surface rectangle within
//!   [`BOUNDS_TOLERANCE_POINTS`] on each edge. An element with no frame fits only an app node with
//!   none either.
//! - **Ambiguity** is refused, never guessed: two elements that fit one app node, two app nodes
//!   that fit one element, or two elements a press could mean ([`locate`]) answer
//!   [`TreeError::Ambiguous`] or [`Error::Ambiguous`].
//!
//! A matched element carries the app node's role and identifier, which the match checked against
//! its native role and could not read from it: a generic element stands for many roles. Its label,
//! value, hint and states are the OS's, and an empty value the app publishes reads as the empty
//! value it cannot be told from. An element no app node fits carries the coarse role its native
//! role stands for ([`roles::wire_role`]) and no identifier, so a projection shows it as a
//! difference. The match keeps to the walk's budgets: an app subtree past [`MAX_NODES`] nodes is
//! compared in part, and the projection says so (`Tree::truncated`).

use std::collections::BTreeSet;

use flui_protocol::error::ErrorCode;
use flui_protocol::geometry::LogicalSize;
use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::{Checked, Role};

use super::accessibility::{
    HostScreen, PointFrame, ScreenElement, ScreenRequest, WindowIdentity, no_window,
    window_identity,
};
use super::roles::{self, SCREEN_SUBROLE};
use crate::error::{Error, Result};
use crate::lane::ax;
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

/// How many announcer nodes the app's root holds last: FLUI's polite and assertive live regions
/// (`SemanticsOwner::adopt_announcers`), static text with no area that `accesskit_ios` serves
/// and the app's read leaves out, since they are no element of the interface. The root's last
/// elements of that shape that no app node fits are left out of the projection, as the Android
/// lane leaves them out of its.
pub const ANNOUNCERS: usize = 2;

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
        /// Whether the search stopped at its budget before it saw every group.
        cut_short: bool,
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
            Self::NoScreen {
                title,
                groups,
                cut_short,
            } => Error::NotFound {
                message: format!(
                    "the window {title:?} shows no group with the shape of the app's window, \
                     which is the device's screen{}: the device has not drawn yet, or the window \
                     shows another device",
                    if cut_short {
                        format!(
                            " within the first {MAX_NODES} elements and {MAX_SCREEN_DEPTH} levels"
                        )
                    } else {
                        String::new()
                    }
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

/// What the screen search reads of an element of the device's window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowFacts {
    /// `AXRole`.
    pub role: String,
    /// `AXSubrole`, if any.
    pub subrole: Option<String>,
    /// Its frame, when it has one.
    pub frame: Option<PointFrame>,
}

/// The windows on screen as the screen search walks them: the Mac's accessibility API, or a
/// test's window.
pub trait WindowTree {
    /// One element of a window, the window itself included.
    type Element: Clone;

    /// The children of `element`, in the API's order.
    ///
    /// # Errors
    ///
    /// The API's failure.
    fn children(&mut self, element: &Self::Element) -> Result<Vec<Self::Element>>;

    /// What `element` is and where.
    ///
    /// # Errors
    ///
    /// The API's failure.
    fn facts(&mut self, element: &Self::Element) -> Result<WindowFacts>;

    /// The texts `window` answers that could name the simulator it shows (its `AXIdentifier`
    /// and `AXDocument`), for [`window_identity`].
    ///
    /// # Errors
    ///
    /// The API's failure.
    fn identity(&mut self, window: &Self::Element) -> Result<Vec<Option<String>>>;
}

/// Whether the search goes into an element of the window: into every one but the screen's group
/// as DeviceHub marks it, whose children are the iOS app's.
fn descends(facts: &WindowFacts) -> bool {
    facts.subrole.as_deref() != Some(SCREEN_SUBROLE)
}

/// The device's screen among `windows`, the windows on screen titled as `screen` asks: the
/// window, of those whose attributes name no other simulator, the one; and in it the group
/// [`choose_screen`] takes among every group within [`MAX_SCREEN_DEPTH`] levels and
/// [`MAX_NODES`] elements, the search going into each but a [`SCREEN_SUBROLE`] group.
///
/// # Errors
///
/// [`Error::NotFound`] when no window is left ([`no_window`]) or it shows no screen;
/// [`Error::Ambiguous`] when two windows are left, or two screens fit; the API's failure.
pub fn find_screen<T: WindowTree>(
    tree: &mut T,
    screen: &ScreenRequest,
    windows: Vec<T::Element>,
) -> Result<(T::Element, PointFrame)> {
    let mut left = Vec::new();
    for window in windows {
        if window_identity(&screen.udid, &tree.identity(&window)?) != WindowIdentity::Other {
            left.push(window);
        }
    }
    let window = match left.len() {
        0 => return Err(no_window(&screen.title)),
        1 => left.swap_remove(0),
        count => {
            return Err(Error::Ambiguous {
                message: format!(
                    "{count} windows on screen are titled {:?}, and none names its simulator: \
                     close all but the window of {}",
                    screen.title, screen.udid
                ),
                candidates: Vec::new(),
                retry: ErrorCode::Ambiguous.default_retry(),
            });
        }
    };
    let mut groups: Vec<(T::Element, ScreenCandidate)> = Vec::new();
    let mut level = vec![window];
    let mut visited = 0_usize;
    let mut cut_short = false;
    'search: for depth in 1..=MAX_SCREEN_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            for child in tree.children(parent)? {
                visited += 1;
                if visited > MAX_NODES {
                    cut_short = true;
                    break 'search;
                }
                let facts = tree.facts(&child)?;
                if facts.role == "AXGroup"
                    && let Some(frame) = facts.frame
                {
                    groups.push((
                        child.clone(),
                        ScreenCandidate {
                            frame,
                            subrole: facts.subrole.clone(),
                            depth,
                        },
                    ));
                }
                if descends(&facts) {
                    next.push(child);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        if depth == MAX_SCREEN_DEPTH {
            cut_short = true;
        }
        level = next;
    }
    let candidates: Vec<ScreenCandidate> = groups
        .iter()
        .map(|(_, candidate)| candidate.clone())
        .collect();
    let index = choose_screen(&screen.title, &candidates, screen.size, cut_short)
        .map_err(TreeError::into_error)?;
    let (group, candidate) = groups.swap_remove(index);
    Ok((group, candidate.frame))
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
/// app window of `size`: of the groups with the window's shape ([`screen_scale`]), the
/// [`SCREEN_SUBROLE`] one when there is one, and otherwise the shallowest. Two groups of one
/// frame are one screen, reached along two paths or wrapped by a group of its own frame.
/// `cut_short` says the search stopped at its budget, which a failure names.
///
/// # Errors
///
/// [`TreeError::NoScreen`] when none has the shape, [`TreeError::Ambiguous`] when
/// [`SCREEN_SUBROLE`] groups of two frames do, or plain groups of two frames at the shallowest
/// depth.
pub fn choose_screen(
    title: &str,
    candidates: &[ScreenCandidate],
    size: LogicalSize,
    cut_short: bool,
) -> std::result::Result<usize, TreeError> {
    let shaped: Vec<usize> = (0..candidates.len())
        .filter(|index| screen_scale(candidates[*index].frame, size).is_some())
        .collect();
    let marked: Vec<usize> = shaped
        .iter()
        .copied()
        .filter(|index| candidates[*index].subrole.as_deref() == Some(SCREEN_SUBROLE))
        .collect();
    let pool: Vec<usize> = if marked.is_empty() {
        let shallowest = shaped.iter().map(|index| candidates[*index].depth).min();
        shaped
            .into_iter()
            .filter(|index| Some(candidates[*index].depth) == shallowest)
            .collect()
    } else {
        marked
    };
    let mut found: Vec<usize> = Vec::new();
    for index in pool {
        if !found
            .iter()
            .any(|other| candidates[*other].frame == candidates[index].frame)
        {
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
            cut_short,
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

/// Where a projection places what it reads: the screen's frame and the app's window it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placing {
    /// The screen's group's frame in the Mac's points.
    pub screen: PointFrame,
    /// The app's window in points.
    pub size: LogicalSize,
    /// The window's scale.
    pub scale: f64,
}

/// `element` as a wire node with the handle `id`, its children left out: see the
/// [module docs](self).
#[must_use]
pub fn describe(element: &ScreenElement, id: ElementId, placing: Placing) -> Node {
    let subrole = element.subrole.as_deref();
    let role = roles::wire_role(&element.role, subrole);
    let mut node = Node::new(id, role, roles::native_role(&element.role, subrole));
    let named = element
        .description
        .clone()
        .or_else(|| element.title.clone());
    let reads_value = role != Role::Label || named.is_some();
    node.name = ax::label(role, named, element.value.clone());
    if element.role == "AXCheckBox" {
        // A toggle's value is its state: `accesskit_ios` hands UIKit "1" or "0".
        node.checked = element.value.as_deref().and_then(|value| match value {
            "1" | "true" => Some(Checked::True),
            "0" | "false" => Some(Checked::False),
            _ => None,
        });
    } else if reads_value {
        // A static text whose value is its label reads no value besides.
        node.value.clone_from(&element.value);
    }
    node.description.clone_from(&element.help);
    node.automation_id.clone_from(&element.identifier);
    node.disabled = element.enabled == Some(false);
    node.focused = element.focused;
    node.selected = element.selected.filter(|selected| *selected);
    node.rect = element
        .frame
        .and_then(|frame| window_rect(frame, placing.screen, placing.size, placing.scale));
    node
}

/// The elements of `screen`, matched to `app`, the app's tree of the window `placing` places, and
/// projected: see the [module docs](self).
///
/// # Errors
///
/// [`TreeError::NoAppTree`] when `app` has no root, [`TreeError::Ambiguous`] when two candidates
/// fit one node.
pub fn read(
    screen: &HostScreen,
    app: &Tree,
    placing: Placing,
) -> std::result::Result<Tree, TreeError> {
    project(screen, app, placing).map(|(tree, _)| tree)
}

/// The element of `screen` the app's node of identifier `identifier` matched, when one did.
///
/// # Errors
///
/// As [`read`].
pub fn matched_element<'s>(
    screen: &'s HostScreen,
    app: &Tree,
    placing: Placing,
    identifier: &str,
) -> std::result::Result<Option<&'s ScreenElement>, TreeError> {
    let (_, matched) = project(screen, app, placing)?;
    let Some(node) = element_of(&app.roots, identifier) else {
        return Ok(None);
    };
    Ok(matched
        .iter()
        .find(|(id, _)| *id == node.id)
        .and_then(|(_, path)| element_at(&screen.elements, path)))
}

/// The element at `path`, the indexes from `elements` down.
#[must_use]
pub fn element_at<'s>(elements: &'s [ScreenElement], path: &[usize]) -> Option<&'s ScreenElement> {
    let (first, rest) = path.split_first()?;
    let element = elements.get(*first)?;
    if rest.is_empty() {
        Some(element)
    } else {
        element_at(&element.children, rest)
    }
}

/// The path of the one element of `screen` that reads as `wanted`: the same role, subrole,
/// label attributes, identifier and frame, the facts a read gave and a press finds an element by
/// again.
///
/// # Errors
///
/// [`Error::NotFound`] when none does, [`Error::Ambiguous`] when more than one does: a press
/// does not guess.
pub fn locate(screen: &HostScreen, wanted: &ScreenElement) -> Result<Vec<usize>> {
    fn search(
        elements: &[ScreenElement],
        wanted: &ScreenElement,
        path: &mut Vec<usize>,
        found: &mut Vec<Vec<usize>>,
    ) {
        for (index, element) in elements.iter().enumerate() {
            path.push(index);
            if same_element(element, wanted) {
                found.push(path.clone());
            }
            search(&element.children, wanted, path, found);
            path.pop();
        }
    }
    let mut found = Vec::new();
    search(&screen.elements, wanted, &mut Vec::new(), &mut found);
    let what = format!(
        "{} {:?}",
        wanted.role,
        wanted
            .description
            .as_deref()
            .or(wanted.title.as_deref())
            .unwrap_or_default()
    );
    match found.len() {
        1 => Ok(found.swap_remove(0)),
        0 => Err(Error::not_found(format!(
            "the Simulator's window shows no element that reads as {what} now"
        ))),
        count => Err(Error::Ambiguous {
            message: format!(
                "{count} elements of the Simulator's window read as {what}; the lane presses \
                 none of them"
            ),
            candidates: Vec::new(),
            retry: ErrorCode::Ambiguous.default_retry(),
        }),
    }
}

/// Whether `found` is the element `wanted` describes: see [`locate`].
fn same_element(found: &ScreenElement, wanted: &ScreenElement) -> bool {
    found.role == wanted.role
        && found.subrole == wanted.subrole
        && found.description == wanted.description
        && found.title == wanted.title
        && found.identifier == wanted.identifier
        && found.frame == wanted.frame
}

/// The node of `nodes` whose identifier is `identifier`.
#[must_use]
pub fn element_of<'a>(nodes: &'a [Node], identifier: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        if node.identifier.as_deref() == Some(identifier) {
            Some(node)
        } else {
            element_of(&node.children, identifier)
        }
    })
}

/// [`read`]'s projection, and the path of the element each matched app node (by its handle)
/// matched.
#[expect(
    clippy::type_complexity,
    reason = "the projection and the paths it matched, read by two callers"
)]
fn project(
    screen: &HostScreen,
    app: &Tree,
    placing: Placing,
) -> std::result::Result<(Tree, Vec<(ElementId, Vec<usize>)>), TreeError> {
    let Some(app_root) = app.roots.first() else {
        return Err(TreeError::NoAppTree);
    };
    let mut walk = Walk {
        placing,
        tolerance: bounds_tolerance(placing.scale),
        next_id: 1,
        truncated: screen.truncated,
        used: BTreeSet::new(),
        matched: Vec::new(),
    };
    let (elements, mut path, unread) = lifted(screen);
    let mut root = Node::new(walk.next(), app_root.role, "AXGroup".to_owned());
    root.rect = window_rect(placing.screen, placing.screen, placing.size, placing.scale);
    root.identifier.clone_from(&app_root.identifier);
    root.window = app_root.window;
    let scope = walk.scope(app_root);
    root.children = walk.children(elements, &mut path, &scope)?;
    root.children_unread = unread;
    // The announcers the app's read leaves out: the root's last children of their shape that no
    // app node fits.
    let mut left_out = 0;
    while left_out < ANNOUNCERS
        && root
            .children
            .last()
            .is_some_and(|last| last.identifier.is_none() && announcer_shaped(last))
    {
        root.children.pop();
        left_out += 1;
    }
    Ok((Tree::new(vec![root], walk.truncated), walk.matched))
}

/// Whether `node`, an element as [`describe`] reads it, has the shape of an announcer: static
/// text with no frame on the window.
fn announcer_shaped(node: &Node) -> bool {
    node.native_role == "AXStaticText" && node.rect.is_none() && node.children.is_empty()
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

/// The screen's elements, the path they sit at, and whether some went unread: its elements, or
/// its one element's children when that element is the app's root read as a container (an
/// unnamed group without an identifier).
fn lifted(screen: &HostScreen) -> (&[ScreenElement], Vec<usize>, bool) {
    if let [only] = screen.elements.as_slice()
        && only.role == "AXGroup"
        && only.subrole.is_none()
        && only.description.is_none()
        && only.title.is_none()
        && only.identifier.is_none()
    {
        return (&only.children, vec![0], only.children_unread);
    }
    (&screen.elements, Vec::new(), false)
}

/// Whether the element `os` fits the app node of `candidate`: see the [module docs](self).
fn fits(candidate: &Candidate<'_>, os: &Node, tolerance: i64) -> bool {
    let app = candidate.node;
    os.native_role == candidate.native
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
/// within `tolerance` device pixels. An app rectangle with no area places its node nowhere, as an
/// element's frame with none does ([`window_rect`]).
fn bounds_agree(surface: Option<Rect>, os: Option<Rect>, tolerance: i64) -> bool {
    let surface = surface.filter(|rect| rect.width > 0 && rect.height > 0);
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
fn describe_os(node: &Node) -> String {
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

/// An app node an element may be matched to, with the native role it would read as, computed
/// once for the scope.
struct Candidate<'n> {
    node: &'n Node,
    native: String,
}

/// The projection's walk: where it places elements, the bounds tolerance in device pixels, the
/// next handle, whether a budget cut it short, the app nodes already matched, and the path each
/// matched.
struct Walk {
    placing: Placing,
    tolerance: i64,
    next_id: u64,
    truncated: bool,
    used: BTreeSet<ElementId>,
    matched: Vec<(ElementId, Vec<usize>)>,
}

impl Walk {
    fn next(&mut self) -> ElementId {
        let id = ElementId::from_u64(self.next_id)
            .expect("BUG: node numbers start at one and stay under the walk's node budget");
        self.next_id += 1;
        id
    }

    /// The descendants of `node`, in document order, as candidates, at most [`MAX_NODES`]: a
    /// subtree past that is compared in part, which the projection says.
    fn scope<'n>(&mut self, node: &'n Node) -> Vec<Candidate<'n>> {
        let mut found = Vec::new();
        let mut pending: Vec<&'n Node> = node.children.iter().rev().collect();
        while let Some(next) = pending.pop() {
            if found.len() >= MAX_NODES {
                self.truncated = true;
                break;
            }
            found.push(Candidate {
                node: next,
                native: roles::native_for(&next.native_role, next.checked.is_some()),
            });
            pending.extend(next.children.iter().rev());
        }
        found
    }

    /// `elements`, at `path`, projected, each matched among `scope`, the candidates under the app
    /// node their nearest matched ancestor fits.
    fn children(
        &mut self,
        elements: &[ScreenElement],
        path: &mut Vec<usize>,
        scope: &[Candidate<'_>],
    ) -> std::result::Result<Vec<Node>, TreeError> {
        let described: Vec<Node> = elements
            .iter()
            .map(|element| {
                describe(
                    element,
                    ElementId::from_u64(1).expect("BUG: one is a handle"),
                    self.placing,
                )
            })
            .collect();
        let matched = self.matched_among(scope, &described)?;
        let mut projected = Vec::with_capacity(elements.len());
        for (index, ((element, mut node), app)) in
            elements.iter().zip(described).zip(matched).enumerate()
        {
            node.id = self.next();
            node.children_unread = element.children_unread;
            path.push(index);
            if let Some(app) = app {
                self.matched.push((app.id, path.clone()));
                node.role = app.role;
                node.identifier.clone_from(&app.identifier);
                node.window = app.window;
                // An element reads an empty value as none: an app node whose value is empty
                // reads the empty value it cannot be told from.
                if node.value.is_none() && app.value.as_deref() == Some("") {
                    node.value = Some(String::new());
                }
                let inner = self.scope(app);
                node.children = self.children(&element.children, path, &inner)?;
            } else {
                node.children = self.children(&element.children, path, scope)?;
            }
            path.pop();
            projected.push(node);
        }
        Ok(projected)
    }

    /// The app node each of `elements` fits among `scope`, in their order; an app node matched
    /// before is matched to nothing else.
    fn matched_among<'n>(
        &mut self,
        scope: &[Candidate<'n>],
        elements: &[Node],
    ) -> std::result::Result<Vec<Option<&'n Node>>, TreeError> {
        let mut matched: Vec<Option<&'n Node>> = vec![None; elements.len()];
        for candidate in scope {
            if self.used.contains(&candidate.node.id) {
                continue;
            }
            let fitting: Vec<usize> = elements
                .iter()
                .enumerate()
                .filter(|(_, element)| fits(candidate, element, self.tolerance))
                .map(|(index, _)| index)
                .collect();
            let index = match fitting.as_slice() {
                [] => continue,
                [index] => *index,
                several => {
                    return Err(TreeError::Ambiguous {
                        node: describe_app(candidate.node),
                        candidates: several
                            .iter()
                            .take(MAX_CANDIDATES)
                            .map(|index| describe_os(&elements[*index]))
                            .collect(),
                    });
                }
            };
            if let Some(other) = matched[index] {
                return Err(TreeError::Ambiguous {
                    node: describe_os(&elements[index]),
                    candidates: vec![describe_app(other), describe_app(candidate.node)],
                });
            }
            matched[index] = Some(candidate.node);
        }
        for app in matched.iter().flatten() {
            self.used.insert(app.id);
        }
        Ok(matched)
    }
}
