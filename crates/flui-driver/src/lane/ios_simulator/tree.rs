//! The iOS app's elements as the Mac's accessibility API shows them on the Simulator's window,
//! projected onto the wire's nodes and matched to the app's own semantics tree (ADR-0095 §4):
//! what the Simulator shows the Mac of the app, the elements VoiceOver on iOS reads.
//!
//! - **The window.** The device's window is the one window on screen titled for it that names its
//!   simulator's UDID, whatever the others name; else the one that names no UUID, while no other
//!   listed simulator shares the title. A window that names any other UUID is not it, and two
//!   windows that name the UDID, or two that name no UUID when none names it, are refused
//!   ([`choose_window`], which every host's read goes through).
//! - **The screen.** The device's screen is a group of that window with the shape of the app's
//!   window, which fills the screen: in Simulator.app a child of the window or of a group the
//!   bezel adds around it, in DeviceHub (Xcode 27) a group about seven levels down whose subrole
//!   is [`SCREEN_SUBROLE`]. No run has recorded either window's tree yet (`docs/gaps.md`), so the
//!   search holds for both shapes a plain group can take: the screen itself, or a wrapper of a
//!   [`SCREEN_SUBROLE`] group. It reads the window's chrome first, breadth first within
//!   [`MAX_SCREEN_DEPTH`] levels, going into every element but a candidate (a group of the app
//!   window's shape, or a [`SCREEN_SUBROLE`] group); then, with what is left of its budget, it
//!   goes into each plain candidate to look for a [`SCREEN_SUBROLE`] group under it, never into
//!   a [`SCREEN_SUBROLE`] group, whose children are the app's. What cuts the search inside a
//!   plain candidate (the budget, an element gone, the depth) is that candidate's, never the
//!   search's ([`SearchReach`] tells only the chrome's). [`choose_screen`] takes a
//!   [`SCREEN_SUBROLE`] group wherever it was found, else the shallowest group of the window's
//!   shape, unless the chrome was cut, by an element gone or by the budget, since a candidate may
//!   sit in the part not read ([`TreeError::SearchCut`]). The window draws the screen scaled
//!   (DeviceHub at about three quarters), so an element's frame is placed on the app's window by
//!   the screen's scale ([`window_rect`]).
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
//!   (the app's root as a container), its children are read as the screen's, and FLUI's
//!   announcers, which the app's read leaves out, are left out of the projection when that root
//!   list was read whole ([`ANNOUNCERS`]).
//! - **What fits.** An element fits an app node when its native role is the one
//!   [`roles::native_for`] gives the app node's AccessKit role and toggled state, its label is the
//!   app node's name, its enabled and checked states are the app node's, an identifier it carries
//!   is the app node's, and its frame on the window is the app node's surface rectangle within
//!   [`BOUNDS_TOLERANCE_POINTS`] on each edge. An element with no frame fits only an app node with
//!   none either.
//! - **Ambiguity** is refused, never guessed: two elements that fit one app node, two app nodes
//!   that fit one element, or two elements a press could mean, or one on a read that stopped
//!   short, where a second may sit in the part not read ([`locate`]), answer
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

use flui_protocol::error::{ErrorCode, Retry};
use flui_protocol::geometry::LogicalSize;
use flui_protocol::tree::{ElementId, Node, Rect, Tree};
use flui_protocol::wire::{Checked, Role};

use super::accessibility::{
    HostScreen, PointFrame, ScreenElement, ScreenRequest, ScreenSource, ScreenWalk, WindowIdentity,
    list_children, no_window, screen_steady, walk_screen, window_identity,
};
use super::roles::{self, SCREEN_SUBROLE};
use crate::error::{Error, Result};
use crate::lane::ax;
use crate::lane::walk::{Cut, MAX_DEPTH, MAX_NODES, ProviderError};

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

/// How much of the device's window's chrome the screen search read: everything but the insides
/// of the candidates, whose cuts are theirs ([`find_screen`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchReach {
    /// Every element of the chrome.
    Whole,
    /// Every element of the chrome within [`MAX_SCREEN_DEPTH`] levels, the elements on the
    /// deepest level not entered: the levels searched were read whole, so the shallowest group
    /// of the window's shape among them is the shallowest there is within the depth a screen may
    /// sit at, and nothing deeper is a candidate.
    ToDepth,
    /// Part of the chrome: an element of it went away while its children were listed, or after
    /// it was listed and before its facts were read. Reading again may see it whole.
    Gone,
    /// Part of the chrome: the budget of [`MAX_NODES`] elements ran out before all of it was
    /// read. The chrome is the same at every read, so reading again cuts it the same way.
    Budget,
}

impl SearchReach {
    /// Whether the chrome was cut, so a plain candidate may not be the only one.
    #[must_use]
    pub const fn chrome_cut(self) -> bool {
        matches!(self, Self::Gone | Self::Budget)
    }

    /// For people: where the search stopped, after "the window shows no screen".
    fn where_read(self) -> String {
        match self {
            Self::Whole => String::new(),
            Self::ToDepth => format!(" within the {MAX_SCREEN_DEPTH} levels searched"),
            Self::Gone => " in the part of it read before an element went away".to_owned(),
            Self::Budget => {
                format!(
                    " in the part of it read before the search's budget ({MAX_NODES} elements) ran out"
                )
            }
        }
    }

    /// Whether a read again can see more: not when the budget cut it, which it does every time.
    fn retry(self) -> Retry {
        match self {
            Self::Budget => Retry::Never,
            _ => ErrorCode::NotFound.default_retry(),
        }
    }
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
        /// How much of the window's chrome the search read.
        reach: SearchReach,
    },
    /// The search found no [`SCREEN_SUBROLE`] group and read only part of the chrome, above or
    /// beside the plain groups of the window's shape it saw, so none is taken: another may sit in
    /// the part not read, as a press refuses one match on a read that stopped short.
    SearchCut {
        /// The window's title.
        title: String,
        /// What cut the chrome: [`SearchReach::Gone`] or [`SearchReach::Budget`].
        reach: SearchReach,
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
                reach,
            } => Error::NotFound {
                message: format!(
                    "the window {title:?} shows no group with the shape of the app's window, \
                     which is the device's screen{}: the device has not drawn yet, or the window \
                     shows another device",
                    reach.where_read()
                ),
                candidates: groups,
                effect: None,
                retry: reach.retry(),
            },
            Self::SearchCut { title, reach } => Error::NotFound {
                message: format!(
                    "the window {title:?} shows no group of subrole {SCREEN_SUBROLE}, and its \
                     search was cut short above or beside the groups of the app's window's shape \
                     it saw{}: another may sit in the part not read, so none is taken{}",
                    reach.where_read(),
                    if reach == SearchReach::Budget {
                        "; the window's chrome is larger than the search reads, so reading again \
                         cuts it the same way"
                    } else {
                        "; read again"
                    }
                ),
                candidates: Vec::new(),
                effect: None,
                retry: reach.retry(),
            },
            Self::NoAppTree => Error::NotFound {
                message: "the app published no tree of the window to match the Simulator's \
                          elements against"
                    .to_owned(),
                candidates: Vec::new(),
                effect: None,
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

    /// The child of `element` at `index`, in the API's order; `None` past the last. The search
    /// lists children through [`list_children`], which holds the bound.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Gone`] when `element` went away, which cuts its list short; any other
    /// failure fails the search, as [`Self::failure`] answers it.
    fn child_at(
        &mut self,
        element: &Self::Element,
        index: usize,
    ) -> std::result::Result<Option<Self::Element>, ProviderError>;

    /// What a child read that failed with `error` answers.
    fn failure(&mut self, error: ProviderError) -> Error;

    /// `element`'s frame alone, when it has one: what [`read_screen`] reads again once the walk is
    /// done.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Gone`] when `element` went away; any other failure, as
    /// [`Self::failure`] answers it.
    fn frame(
        &mut self,
        element: &Self::Element,
    ) -> std::result::Result<Option<PointFrame>, ProviderError>;

    /// What `element` is and where.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Gone`] when `element` went away after it was listed, which the search
    /// takes as its level cut, as a list a gone parent cuts; any other failure fails the search,
    /// as [`Self::failure`] answers it.
    fn facts(&mut self, element: &Self::Element)
    -> std::result::Result<WindowFacts, ProviderError>;

    /// The texts `window` answers that could name the simulator it shows (its `AXIdentifier`
    /// and `AXDocument`), for [`window_identity`].
    ///
    /// # Errors
    ///
    /// The API's failure.
    fn identity(&mut self, window: &Self::Element) -> Result<Vec<Option<String>>>;
}

/// What an element of the window is to the screen search, for an app window of `size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchRole {
    /// The screen's group as DeviceHub marks it: a candidate never gone into, its children the
    /// app's.
    Marked,
    /// A group of the window's shape ([`screen_scale`]): a candidate, gone into only to look for
    /// a marked group under it.
    Plain,
    /// Anything else: the window's chrome, gone into.
    Chrome,
}

impl SearchRole {
    fn of(facts: &WindowFacts, size: LogicalSize) -> Self {
        if facts.subrole.as_deref() == Some(SCREEN_SUBROLE) {
            Self::Marked
        } else if facts.role == "AXGroup"
            && facts
                .frame
                .is_some_and(|frame| screen_scale(frame, size).is_some())
        {
            Self::Plain
        } else {
            Self::Chrome
        }
    }
}

/// One element's children as the screen search read them.
struct Children<E> {
    /// The chrome among them, to go into next.
    chrome: Vec<E>,
    /// The plain candidates among them, to go into once the chrome is read.
    plain: Vec<E>,
    /// Whether an element went away: the parent while its children were listed, or a child
    /// before its facts were read.
    gone: bool,
    /// Whether the budget ran out before all the parent's children were listed.
    spent: bool,
}

/// The screen search's state across the window: the groups it saw and how many elements it read.
struct Search<E> {
    groups: Vec<(E, ScreenCandidate)>,
    visited: usize,
    size: LogicalSize,
    /// Whether any element went away during the search, in the chrome or in a candidate:
    /// what [`gone_or_refused`] is asked once before the search answers.
    met_gone: bool,
}

/// What an element gone during a read means for `tree`: an element gone, which the read takes
/// as a cut or a busy screen, unless the source answers it as the permission refused (the Mac's
/// `read_failure` while the process is no longer trusted), which no read again serves. The one
/// place a read decides it, asked once for the search and once for the walk and the frame.
///
/// # Errors
///
/// The source's refusal ([`Error::NotSupported`]).
fn gone_or_refused<T: WindowTree>(tree: &mut T) -> Result<()> {
    match tree.failure(ProviderError::Gone) {
        refused @ Error::NotSupported { .. } => Err(refused),
        _ => Ok(()),
    }
}

impl<E: Clone> Search<E> {
    /// The children of `parent`, which sit at `depth`, listed within what is left of the budget
    /// and their facts read; every group with a frame among them recorded.
    fn read_children<T: WindowTree<Element = E>>(
        &mut self,
        tree: &mut T,
        parent: &E,
        depth: usize,
    ) -> Result<Children<E>> {
        let listed = list_children(
            |index| tree.child_at(parent, index),
            MAX_NODES.saturating_sub(self.visited),
        )
        .map_err(|error| tree.failure(error))?;
        self.visited += listed.children.len();
        let mut read = Children {
            chrome: Vec::new(),
            plain: Vec::new(),
            gone: listed.cut == Some(Cut::ListGone),
            spent: listed.cut == Some(Cut::Bound),
        };
        for child in listed.children {
            let facts = match tree.facts(&child) {
                Ok(facts) => facts,
                // Gone between its listing and its facts: its level is cut, as a list a gone
                // parent cuts.
                Err(ProviderError::Gone) => {
                    read.gone = true;
                    continue;
                }
                Err(error) => return Err(tree.failure(error)),
            };
            if facts.role == "AXGroup"
                && let Some(frame) = facts.frame
            {
                self.groups.push((
                    child.clone(),
                    ScreenCandidate {
                        frame,
                        subrole: facts.subrole.clone(),
                        depth,
                    },
                ));
            }
            match SearchRole::of(&facts, self.size) {
                SearchRole::Marked => {}
                SearchRole::Plain => read.plain.push(child),
                SearchRole::Chrome => read.chrome.push(child),
            }
        }
        self.met_gone |= read.gone;
        Ok(read)
    }
}

/// Which of the windows on screen of `screen`'s title shows its device, by what each names
/// (`identities`, in the windows' order, from [`window_identity`]): the one window that names the
/// simulator's UDID, whatever the others name; else the one window that names no UUID, while no
/// other listed simulator shares the title ([`ScreenRequest::others_titled`]), since it could be
/// that simulator's. A window that names any other UUID is never taken. Every host decides here:
/// the Mac's window search ([`find_screen`]) and the scripted window alike.
///
/// # Errors
///
/// [`Error::NotFound`] when no window has the title ([`no_window`]) or every one names another
/// UUID; [`Error::Ambiguous`] when two windows name the simulator, or none does and two name no
/// UUID, or the one that names no UUID shares its title with another listed simulator.
pub fn choose_window(screen: &ScreenRequest, identities: &[WindowIdentity]) -> Result<usize> {
    if identities.is_empty() {
        return Err(no_window(&screen.title));
    }
    let naming = |wanted: WindowIdentity| {
        (0..identities.len())
            .filter(|index| identities[*index] == wanted)
            .collect::<Vec<usize>>()
    };
    let named = naming(WindowIdentity::Named);
    let unknown = naming(WindowIdentity::Unknown);
    let ambiguous = |message: String| Error::Ambiguous {
        message,
        candidates: Vec::new(),
        retry: ErrorCode::Ambiguous.default_retry(),
    };
    match named.as_slice() {
        [] => {}
        [window] => return Ok(*window),
        _ => {
            return Err(ambiguous(format!(
                "{} windows titled {:?} name simulator {}: close all but one",
                named.len(),
                screen.title,
                screen.udid
            )));
        }
    }
    match unknown.as_slice() {
        [] => Err(Error::not_found(format!(
            "the {} windows titled {:?} each name a UUID that is not simulator {}'s: they show \
             another simulator (or name a document), and the lane takes a window only on its \
             simulator's UDID or on no UUID at all",
            identities.len(),
            screen.title,
            screen.udid
        ))),
        [window] if screen.others_titled == 0 => Ok(*window),
        [_] => Err(ambiguous(format!(
            "the window titled {:?} names no simulator, and {} other listed simulators share \
             the title, so it could be theirs: rename all but {} (`xcrun simctl rename`)",
            screen.title, screen.others_titled, screen.udid
        ))),
        _ => Err(ambiguous(format!(
            "{} windows on screen are titled {:?}, and none names its simulator: close all but \
             the window of {}",
            unknown.len(),
            screen.title,
            screen.udid
        ))),
    }
}

/// The window of `screen` among `windows`, the windows on screen of its title, as
/// [`choose_window`] takes it by what each window's attributes name.
///
/// # Errors
///
/// As [`choose_window`]; the API's failure.
fn the_window<T: WindowTree>(
    tree: &mut T,
    screen: &ScreenRequest,
    mut windows: Vec<T::Element>,
) -> Result<T::Element> {
    let identities = windows
        .iter()
        .map(|window| Ok(window_identity(&screen.udid, &tree.identity(window)?)))
        .collect::<Result<Vec<WindowIdentity>>>()?;
    let index = choose_window(screen, &identities)?;
    Ok(windows.swap_remove(index))
}

/// The device's screen among `windows`, the windows on screen titled as `screen` asks: the
/// window [`choose_window`] takes; and in it the group [`choose_screen`] takes among the groups
/// the search sees within [`MAX_SCREEN_DEPTH`] levels and [`MAX_NODES`] elements, each element's
/// children listed by [`list_children`] within what is left of the budget.
///
/// The search reads the window's chrome first, breadth first, going into every element but a
/// candidate: a group of the app window's shape (plain) or a [`SCREEN_SUBROLE`] group (marked).
/// An element of the chrome gone (while its children are listed, or after it was listed and
/// before its facts are read) leaves the chrome cut ([`SearchReach::Gone`]), the budget spent on
/// it too ([`SearchReach::Budget`]); chrome past the deepest level is not entered
/// ([`SearchReach::ToDepth`]). Then, with what is left of the budget, it goes into each plain
/// candidate, breadth first within the same depth, to look for a marked group under it, never
/// into a marked one, whose children are the app's. What cuts that part (the budget, an element
/// gone, the depth) is the candidate's, never the search's: the app under Simulator.app's plain
/// screen never cuts it.
///
/// # Errors
///
/// As [`choose_window`]; as [`choose_screen`] (`TreeError::into_error`); the API's failure; the
/// refusal the source answers an element gone as, when it does ([`WindowTree::failure`]: the
/// permission taken back), asked once when any element went away during the search.
pub fn find_screen<T: WindowTree>(
    tree: &mut T,
    screen: &ScreenRequest,
    windows: Vec<T::Element>,
) -> Result<(T::Element, PointFrame)> {
    let window = the_window(tree, screen, windows)?;
    let mut search = Search {
        groups: Vec::new(),
        visited: 0,
        size: screen.size,
        met_gone: false,
    };
    let mut reach = SearchReach::Whole;
    let mut inside: Vec<(T::Element, usize)> = Vec::new();
    let mut level = vec![window];
    'chrome: for depth in 1..=MAX_SCREEN_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            let read = search.read_children(tree, parent, depth)?;
            next.extend(read.chrome);
            inside.extend(read.plain.into_iter().map(|plain| (plain, depth)));
            if read.gone && reach != SearchReach::Budget {
                reach = SearchReach::Gone;
            }
            if read.spent {
                reach = SearchReach::Budget;
                break 'chrome;
            }
        }
        if next.is_empty() {
            break;
        }
        if depth == MAX_SCREEN_DEPTH && reach == SearchReach::Whole {
            reach = SearchReach::ToDepth;
        }
        level = next;
    }
    // Inside the plain candidates, only to find a marked group: what cuts here is theirs.
    let mut level = inside;
    while !level.is_empty() && search.visited < MAX_NODES {
        let mut next = Vec::new();
        for (parent, depth) in &level {
            if *depth >= MAX_SCREEN_DEPTH {
                continue;
            }
            let read = search.read_children(tree, parent, depth + 1)?;
            next.extend(
                read.chrome
                    .into_iter()
                    .chain(read.plain)
                    .map(|child| (child, depth + 1)),
            );
            if read.spent {
                break;
            }
        }
        level = next;
    }
    let candidates: Vec<ScreenCandidate> = search
        .groups
        .iter()
        .map(|(_, candidate)| candidate.clone())
        .collect();
    if search.met_gone {
        gone_or_refused(tree)?;
    }
    let index = choose_screen(&screen.title, &candidates, screen.size, reach)
        .map_err(TreeError::into_error)?;
    let (group, candidate) = search.groups.swap_remove(index);
    Ok((group, candidate.frame))
}

/// The device's screen read whole over a source that serves both the window and the screen's
/// elements: the screen `find_screen` finds in `windows`, the walk of its elements
/// ([`walk_screen`]), and the screen's frame read again once the walk is done
/// ([`WindowTree::frame`]), which must be the one the search read ([`screen_steady`]): every
/// element is placed on the app's window from it. A screen gone by then, or moved, answers
/// [`Error::Busy`]. The Mac's read is this one, and so is a test's over its own tree; the
/// scripted window (`testing::ScriptedSimulatorScreen`) serves a screen already read, and shares
/// only [`choose_window`] and [`locate`].
///
/// # Errors
///
/// As [`find_screen`]; [`Error::Busy`] when the screen moved, was resized or went away during
/// the read; a failure of the walk or of the frame's read, as `source` answers it
/// ([`WindowTree::failure`]); and, for an element gone anywhere in the read (the search, the
/// walk, the frame read again), the refusal `source` answers it as, when it does (the permission
/// taken back).
pub fn read_screen<E, S>(
    source: &mut S,
    screen: &ScreenRequest,
    windows: Vec<E>,
) -> Result<(HostScreen, ScreenWalk<E>)>
where
    E: Clone,
    S: WindowTree<Element = E> + ScreenSource<Element = E>,
{
    let (group, frame) = find_screen(source, screen, windows)?;
    let walk = walk_screen(source, &group).map_err(|error| WindowTree::failure(source, error))?;
    let (after, frame_gone) = match WindowTree::frame(source, &group) {
        Ok(after) => (after, false),
        Err(ProviderError::Gone) => (None, true),
        Err(error) => return Err(WindowTree::failure(source, error)),
    };
    // An element gone in the walk, or the screen gone since, is a short or busy read, unless the
    // source answers it as the permission refused, which no read again serves.
    if walk.met_gone || frame_gone {
        gone_or_refused(source)?;
    }
    screen_steady(frame, after)?;
    Ok((walk.screen(frame), walk))
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
///
/// `reach` says how much of the window's chrome the search read. A [`SCREEN_SUBROLE`] group is
/// taken wherever it was found and whatever cut the search, since it marks itself as the screen;
/// a plain group is taken only when the chrome was not cut ([`SearchReach::chrome_cut`]), since
/// a marked group, or a second plain one, may sit in the part not read, as a press refuses one
/// match on a read that stopped short. What cut the search inside a plain group is that group's,
/// and does not reach `reach`.
///
/// # Errors
///
/// [`TreeError::NoScreen`] when none has the shape; [`TreeError::SearchCut`] when no
/// [`SCREEN_SUBROLE`] group has it, a plain one does, and the chrome was cut;
/// [`TreeError::Ambiguous`] when [`SCREEN_SUBROLE`] groups of two frames do, or plain groups of
/// two frames at the shallowest depth.
pub fn choose_screen(
    title: &str,
    candidates: &[ScreenCandidate],
    size: LogicalSize,
    reach: SearchReach,
) -> std::result::Result<usize, TreeError> {
    let shaped: Vec<usize> = (0..candidates.len())
        .filter(|index| screen_scale(candidates[*index].frame, size).is_some())
        .collect();
    let marked: Vec<usize> = shaped
        .iter()
        .copied()
        .filter(|index| candidates[*index].subrole.as_deref() == Some(SCREEN_SUBROLE))
        .collect();
    if marked.is_empty() && !shaped.is_empty() && reach.chrome_cut() {
        return Err(TreeError::SearchCut {
            title: title.to_owned(),
            reach,
        });
    }
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
            reach,
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
/// A read that stopped short (`HostScreen::cut`) cannot show that one element is the only one: a
/// second that reads the same may sit in the part not read. An identifier is no proof either,
/// since nothing makes an author's identifier unique. So a press refuses one match on such a
/// read, as it refuses two, with a retry when the cut is one a read again may heal (an element
/// gone) and none when it repeats (a bound).
///
/// # Errors
///
/// [`Error::NotFound`] when none does; [`Error::Ambiguous`] when more than one does, or when the
/// read stopped short: a press does not guess.
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
    match (found.len(), screen.cut) {
        (1, Some(cut)) => Err(short_read_refusal(&what, cut)),
        (1, None) => Ok(found.swap_remove(0)),
        (0, _) => Err(Error::not_found(format!(
            "the Simulator's window shows no element that reads as {what} now"
        ))),
        (count, _) => Err(Error::Ambiguous {
            message: format!(
                "{count} elements of the Simulator's window read as {what}; the lane presses \
                 none of them"
            ),
            candidates: Vec::new(),
            retry: ErrorCode::Ambiguous.default_retry(),
        }),
    }
}

/// What a press answers when the one element that reads as `what` was found on a read that `cut`
/// stopped short: ambiguous, since a second may sit in the part not read, with a retry when a
/// read again may see that part ([`Cut::repeats`] says it cannot).
fn short_read_refusal(what: &str, cut: Cut) -> Error {
    let (why, retry, remedy) = if cut.repeats() {
        (
            format!("its budget of {MAX_NODES} elements or {MAX_DEPTH} levels ran out"),
            Retry::Never,
            "press through the app's own path",
        )
    } else {
        (
            "an element went away while it was read".to_owned(),
            Retry::Soon,
            "read again",
        )
    };
    Error::Ambiguous {
        message: format!(
            "the read of the Simulator's window stopped short ({why}), so the one element that \
             reads as {what} cannot be shown to be the only one; the lane presses none: {remedy}"
        ),
        candidates: Vec::new(),
        retry,
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
        truncated: screen.cut.is_some(),
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
    let root_cut = unread.map_or(screen.children_unread, |cut| {
        Cut::with(screen.children_unread, cut)
    });
    root.children_unread = root_cut.is_some();
    // The announcers the app's read leaves out: the root's last children of their shape that no
    // app node fits, taken only from a root list read whole. A list that ended early ends where
    // the read stopped, not at the announcers; a list with a gap (`Cut::ElementGone`) may have
    // lost an announcer to it, so its last two of that shape need not both be announcers. Either
    // way its last children are kept, and the read is already marked short.
    let mut left_out = 0;
    while root_cut.is_none()
        && left_out < ANNOUNCERS
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

/// The screen's elements, the path they sit at, and why some went unread, if they did: its
/// elements, or its one element's children when that element is the app's root read as a
/// container (an unnamed group without an identifier).
fn lifted(screen: &HostScreen) -> (&[ScreenElement], Vec<usize>, Option<Cut>) {
    if let [only] = screen.elements.as_slice()
        && only.role == "AXGroup"
        && only.subrole.is_none()
        && only.description.is_none()
        && only.title.is_none()
        && only.identifier.is_none()
    {
        return (&only.children, vec![0], only.children_unread);
    }
    (&screen.elements, Vec::new(), None)
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
/// next handle, whether the read or the scope's budget left something unread, the app nodes
/// already matched, and the path each matched.
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
            node.children_unread = element.children_unread.is_some();
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
