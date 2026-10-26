//! The actions assistive technology requests and the typed calls handlers
//! receive.
//!
//! [`SemanticsAction`] is `flui-protocol`'s, the vocabulary FLUI shares with
//! tests, devtools and agents; an [`ActionCall`] is one action fused with
//! exactly the payload it takes.

use std::num::NonZeroU32;
use std::sync::Arc;

use flui_foundation::geometry::{Offset, Rect};
use thiserror::Error;

pub use flui_protocol::SemanticsAction;

use crate::identity::AccessibilityNodeId;

/// The disclosure transition an expandable node offers through its click handler.
///
/// A node with an expanded state and a [`SemanticsAction::Click`] handler but
/// no [`SemanticsAction::Expand`] or [`SemanticsAction::Collapse`] handler
/// toggles by clicking. It offers [`SemanticsAction::Expand`] while collapsed
/// and [`SemanticsAction::Collapse`] while expanded, and the owner routes that
/// one request to the click handler. A node registering either discrete action
/// offers only what it registers. `actions` are effective action bits and
/// `expanded` the node's declared expanded state.
pub(crate) fn tap_disclosure_transition(
    actions: u64,
    expanded: Option<bool>,
) -> Option<SemanticsAction> {
    let has = |action: SemanticsAction| actions & action.value() != 0;
    if !has(SemanticsAction::Click)
        || has(SemanticsAction::Expand)
        || has(SemanticsAction::Collapse)
    {
        return None;
    }
    disclosure_transition(expanded)
}

/// The one transition a node's expanded state allows: `Expand` while
/// collapsed, `Collapse` while expanded, and none without an expanded state.
pub(crate) fn disclosure_transition(expanded: Option<bool>) -> Option<SemanticsAction> {
    expanded.map(|expanded| {
        if expanded {
            SemanticsAction::Collapse
        } else {
            SemanticsAction::Expand
        }
    })
}

// ============================================================================
// ActionCall
// ============================================================================

/// One request for a semantics action, carrying exactly the payload that
/// action takes.
///
/// A handler receives an `ActionCall`, never an action and a payload apart, so
/// a `SetValue` without its text or a `Click` with a scroll offset cannot be
/// expressed. A platform request whose payload does not fit its action is
/// refused while it is decoded ([`ActionDecodeError`]), before any handler
/// runs.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ActionCall {
    /// See [`SemanticsAction::Click`].
    Click,
    /// See [`SemanticsAction::Focus`].
    Focus,
    /// See [`SemanticsAction::Blur`].
    Blur,
    /// See [`SemanticsAction::Expand`].
    Expand,
    /// See [`SemanticsAction::Collapse`].
    Collapse,
    /// See [`SemanticsAction::Increment`].
    Increment,
    /// See [`SemanticsAction::Decrement`].
    Decrement,
    /// Scroll the content up by this unit.
    ScrollUp(ScrollUnit),
    /// Scroll the content down by this unit.
    ScrollDown(ScrollUnit),
    /// Scroll the content left by this unit.
    ScrollLeft(ScrollUnit),
    /// Scroll the content right by this unit.
    ScrollRight(ScrollUnit),
    /// Bring a node into view. `None` asks the receiving node to show itself;
    /// `Some` asks a scrollable ancestor to reveal a descendant, with the
    /// geometry the owner resolved for it.
    ScrollIntoView(Option<Reveal>),
    /// Scroll the node's content to this offset, in the node's coordinates.
    SetScrollOffset(Offset<f64>),
    /// Select this range of the node's text.
    SetTextSelection(TextSelection),
    /// Replace the selected text with this text.
    ReplaceSelectedText(String),
    /// Set the node's value to this text.
    SetValue(String),
    /// Set the node's value to this finite number, admitted by its range.
    SetNumericValue(f64),
    /// See [`SemanticsAction::ShowContextMenu`].
    ShowContextMenu,
    /// See [`SemanticsAction::ShowTooltip`].
    ShowTooltip,
    /// See [`SemanticsAction::HideTooltip`].
    HideTooltip,
    /// Run the custom action with this id.
    CustomAction(CustomActionId),
}

impl ActionCall {
    /// The call for an action that takes no payload, or `None` for one that
    /// needs a payload to be meaningful.
    #[must_use]
    pub const fn without_payload(action: SemanticsAction) -> Option<Self> {
        Some(match action {
            SemanticsAction::Click => Self::Click,
            SemanticsAction::Focus => Self::Focus,
            SemanticsAction::Blur => Self::Blur,
            SemanticsAction::Expand => Self::Expand,
            SemanticsAction::Collapse => Self::Collapse,
            SemanticsAction::Increment => Self::Increment,
            SemanticsAction::Decrement => Self::Decrement,
            SemanticsAction::ScrollUp => Self::ScrollUp(ScrollUnit::Page),
            SemanticsAction::ScrollDown => Self::ScrollDown(ScrollUnit::Page),
            SemanticsAction::ScrollLeft => Self::ScrollLeft(ScrollUnit::Page),
            SemanticsAction::ScrollRight => Self::ScrollRight(ScrollUnit::Page),
            SemanticsAction::ScrollIntoView => Self::ScrollIntoView(None),
            SemanticsAction::ShowContextMenu => Self::ShowContextMenu,
            SemanticsAction::ShowTooltip => Self::ShowTooltip,
            SemanticsAction::HideTooltip => Self::HideTooltip,
            _ => return None,
        })
    }

    /// The action this call requests.
    #[must_use]
    pub const fn action(&self) -> SemanticsAction {
        match self {
            Self::Click => SemanticsAction::Click,
            Self::Focus => SemanticsAction::Focus,
            Self::Blur => SemanticsAction::Blur,
            Self::Expand => SemanticsAction::Expand,
            Self::Collapse => SemanticsAction::Collapse,
            Self::Increment => SemanticsAction::Increment,
            Self::Decrement => SemanticsAction::Decrement,
            Self::ScrollUp(_) => SemanticsAction::ScrollUp,
            Self::ScrollDown(_) => SemanticsAction::ScrollDown,
            Self::ScrollLeft(_) => SemanticsAction::ScrollLeft,
            Self::ScrollRight(_) => SemanticsAction::ScrollRight,
            Self::ScrollIntoView(_) => SemanticsAction::ScrollIntoView,
            Self::SetScrollOffset(_) => SemanticsAction::SetScrollOffset,
            Self::SetTextSelection(_) => SemanticsAction::SetTextSelection,
            Self::ReplaceSelectedText(_) => SemanticsAction::ReplaceSelectedText,
            Self::SetValue(_) => SemanticsAction::SetValue,
            Self::SetNumericValue(_) => SemanticsAction::SetNumericValue,
            Self::ShowContextMenu => SemanticsAction::ShowContextMenu,
            Self::ShowTooltip => SemanticsAction::ShowTooltip,
            Self::HideTooltip => SemanticsAction::HideTooltip,
            Self::CustomAction(_) => SemanticsAction::CustomAction,
        }
    }
}

/// How far one directional scroll moves: AccessKit's `ScrollUnit`, which a
/// request without one leaves at `Page`, as the adapters that send none mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ScrollUnit {
    /// One item, line or character, or the closest the content has to one.
    Item,
    /// About a viewport's worth.
    #[default]
    Page,
}

/// What a scrollable ancestor needs to reveal a descendant: current,
/// unclipped root-space logical geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reveal {
    /// The descendant, or the inner viewport already being revealed.
    pub target_rect: Rect<f64>,
    /// The receiving scrollable's viewport.
    pub viewport_rect: Rect<f64>,
    /// The receiving ancestor's scroll position when this geometry was
    /// published. Absent for a receiver without scroll-position semantics.
    pub scroll_position: Option<f64>,
}

/// A text selection within one node, in characters of its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    /// Where the selection starts; stays put as the selection grows.
    pub anchor: usize,
    /// Where the selection ends; the caret when the selection is collapsed.
    pub focus: usize,
}

// ============================================================================
// SemanticsActionHandler
// ============================================================================

/// Handler for semantics actions: receives the request with its payload.
pub type SemanticsActionHandler = Arc<dyn Fn(ActionCall) + Send + Sync>;

// ============================================================================
// SemanticsActionRequest
// ============================================================================

/// An owner-routed action request received from an accessibility adapter.
///
/// The target uses the same stable [`AccessibilityNodeId`] exported in a
/// published [`TreeUpdate`](crate::TreeUpdate), never the rebuild-local
/// [`SemanticsId`](crate::SemanticsId). The presentation/ui_runtime target is
/// structural: an adapter receives the command capability for the
/// presentation whose tree it exposes, so a raw process-global view ID is
/// neither stored here nor resolved through a singleton.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsActionRequest {
    /// Stable target identity from the last published tree.
    pub node_id: AccessibilityNodeId,

    /// The action requested by assistive technology, with its payload.
    pub call: ActionCall,
}

impl SemanticsActionRequest {
    /// Creates a request.
    #[must_use]
    pub const fn new(node_id: AccessibilityNodeId, call: ActionCall) -> Self {
        Self { node_id, call }
    }

    /// The requested action.
    #[must_use]
    pub const fn action(&self) -> SemanticsAction {
        self.call.action()
    }
}

/// Which of a node's custom actions a request names: its place in the node's
/// list, counted from one.
///
/// Ids are per node and carry no meaning on their own; the configuration
/// assigns them as actions are added and a forced merge renumbers the
/// contributor's after the receiver's. A request whose id no longer names the
/// action the platform was shown is refused by the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomActionId(NonZeroU32);

impl CustomActionId {
    /// The id of the action at `index` (zero-based) of a node's list.
    ///
    /// A widget that offers only some of its custom actions at a time names
    /// each by its place in the full list, so a handler keeps its id while
    /// the others come and go ([`SemanticsConfiguration::add_custom_action_as`]).
    #[must_use]
    pub fn at(index: usize) -> Self {
        let position = u32::try_from(index + 1)
            .ok()
            .and_then(NonZeroU32::new)
            .expect("BUG: a node holds fewer custom actions than u32 counts");
        Self(position)
    }

    /// The zero-based place this id names in the list of the configuration
    /// that gave it.
    #[must_use]
    pub fn index(self) -> usize {
        usize::try_from(self.0.get() - 1).expect("BUG: a u32 position fits in usize")
    }

    /// The id an AccessKit request carries, or `None` for one FLUI never
    /// publishes (zero or negative).
    pub(crate) fn from_published(raw: i32) -> Option<Self> {
        u32::try_from(raw).ok().and_then(NonZeroU32::new).map(Self)
    }

    /// The id as AccessKit publishes it.
    pub(crate) fn published(self) -> i32 {
        i32::try_from(self.0.get()).expect("BUG: a node holds fewer custom actions than i32 counts")
    }
}

/// A platform action FLUI routes to no handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnroutedAction {
    /// A scroll to a point in window pixels, which the semantics layer has
    /// no presentation scale to convert.
    ScrollToPoint,
    /// Where sequential focus navigation starts, which belongs to focus
    /// management rather than to a node.
    SetSequentialFocusNavigationStartingPoint,
}

/// Why a platform action request could not be decoded into an
/// [`ActionCall`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ActionDecodeError {
    /// The raw target id is not one FLUI publishes.
    #[error("the request targets {0}, which is not a FLUI node id")]
    UnknownTarget(u64),
    /// The request addresses a tree other than the one a window publishes.
    #[error("the request targets accessibility tree {0:?}, which this window does not publish")]
    UnknownTree(accesskit::TreeId),
    /// FLUI routes this platform action nowhere.
    #[error("the platform action {0:?} has no FLUI route")]
    Unrouted(UnroutedAction),
    /// The action needs a payload the request did not carry.
    #[error("{action} needs a payload the request did not carry")]
    MissingPayload {
        /// The action requested.
        action: SemanticsAction,
    },
    /// The request carried a payload the action cannot take, or a text
    /// selection whose ends lie on another node.
    #[error("{action} cannot take the payload the request carried")]
    MismatchedPayload {
        /// The action requested.
        action: SemanticsAction,
    },
}
