//! Semantics actions that can be performed on nodes.
//!
//! This module provides action types for accessibility interactions.
//! [`SemanticsAction`] itself lives in `flui-protocol`, the vocabulary FLUI
//! shares with tests, devtools and agents; it is re-exported here so its path
//! through this crate is unchanged.

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
    /// See [`SemanticsAction::ScrollUp`].
    ScrollUp,
    /// See [`SemanticsAction::ScrollDown`].
    ScrollDown,
    /// See [`SemanticsAction::ScrollLeft`].
    ScrollLeft,
    /// See [`SemanticsAction::ScrollRight`].
    ScrollRight,
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
    CustomAction(i32),
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
            SemanticsAction::ScrollUp => Self::ScrollUp,
            SemanticsAction::ScrollDown => Self::ScrollDown,
            SemanticsAction::ScrollLeft => Self::ScrollLeft,
            SemanticsAction::ScrollRight => Self::ScrollRight,
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
            Self::ScrollUp => SemanticsAction::ScrollUp,
            Self::ScrollDown => SemanticsAction::ScrollDown,
            Self::ScrollLeft => SemanticsAction::ScrollLeft,
            Self::ScrollRight => SemanticsAction::ScrollRight,
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

/// Why a platform action request could not be decoded into an
/// [`ActionCall`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ActionDecodeError {
    /// The target is not an id FLUI publishes.
    #[error("the request targets {0:?}, which is not a FLUI node id")]
    UnknownTarget(accesskit::NodeId),
    /// FLUI routes this AccessKit action nowhere.
    #[error("AccessKit action {0:?} has no FLUI route")]
    Unrouted(accesskit::Action),
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
