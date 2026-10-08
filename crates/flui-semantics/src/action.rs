//! Semantics actions that can be performed on nodes.
//!
//! This module provides action types for accessibility interactions.
//! [`SemanticsAction`] itself lives in `flui-protocol`, the vocabulary FLUI
//! shares with tests, devtools and agents; it is re-exported here so its path
//! through this crate is unchanged.

use std::sync::Arc;

pub use flui_protocol::SemanticsAction;

use crate::identity::AccessibilityNodeId;

/// The disclosure transition an expandable node offers through its tap handler.
///
/// A node with an expanded state and a [`SemanticsAction::Tap`] handler but no
/// [`SemanticsAction::Expand`] or [`SemanticsAction::Collapse`] handler toggles
/// by tapping, as consumers built before the discrete actions existed do. It
/// offers [`SemanticsAction::Expand`] while collapsed and
/// [`SemanticsAction::Collapse`] while expanded, and the owner routes that one
/// request to the tap handler. A node registering either discrete action offers
/// only what it registers. `actions` are effective action bits and `flags` the
/// node's flag bits.
pub(crate) fn tap_disclosure_transition(actions: u64, flags: u64) -> Option<SemanticsAction> {
    let has = |action: SemanticsAction| actions & action.value() != 0;
    if !has(SemanticsAction::Tap) || has(SemanticsAction::Expand) || has(SemanticsAction::Collapse)
    {
        return None;
    }
    disclosure_transition(flags)
}

/// The one transition a node's expanded state allows: `Expand` while
/// collapsed, `Collapse` while expanded, and none without an expanded state.
pub(crate) fn disclosure_transition(flags: u64) -> Option<SemanticsAction> {
    let flagged = |flag: crate::SemanticsFlag| flags & (flag as u64) != 0;
    if !flagged(crate::SemanticsFlag::HasExpandedState) {
        return None;
    }
    Some(if flagged(crate::SemanticsFlag::IsExpanded) {
        SemanticsAction::Collapse
    } else {
        SemanticsAction::Expand
    })
}

// ============================================================================
// SemanticsActionHandler
// ============================================================================

/// Handler for semantics actions.
pub type SemanticsActionHandler = Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>;

/// Arguments for semantics actions.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ActionArgs {
    /// No arguments.
    #[default]
    None,

    /// Reveal a descendant using current, unclipped root-space logical geometry.
    ShowOnScreen {
        /// The descendant, or the inner viewport already being revealed.
        target_rect: flui_foundation::geometry::Rect<f64>,
        /// The receiving scrollable's viewport.
        viewport_rect: flui_foundation::geometry::Rect<f64>,
        /// The receiving ancestor's position when this geometry was published.
        /// Absent for handlers without scroll-position semantics.
        scroll_position: Option<f64>,
    },

    /// Text selection arguments.
    SetSelection {
        /// Base offset of selection.
        base: i32,
        /// Extent offset of selection.
        extent: i32,
    },

    /// Text content arguments.
    SetText {
        /// The text to set.
        text: String,
    },

    /// An exact numeric value, validated against the current node before dispatch.
    SetNumericValue {
        /// Requested finite value.
        value: f64,
    },

    /// Custom action arguments.
    CustomAction {
        /// The custom action ID.
        action_id: i32,
    },

    /// Move cursor arguments.
    MoveCursor {
        /// Whether to extend selection.
        extend_selection: bool,
    },

    /// Scroll to offset arguments.
    ScrollToOffset {
        /// Target X offset.
        x: f64,
        /// Target Y offset.
        y: f64,
    },
}

// ============================================================================
// SemanticsActionRequest
// ============================================================================

/// An owner-routed action request received from an accessibility adapter.
///
/// The target uses the same stable [`AccessibilityNodeId`] exported in a
/// [`SemanticsSnapshot`](crate::SemanticsSnapshot), never the rebuild-local
/// [`SemanticsId`](crate::SemanticsId). The presentation/realm target is
/// structural: an adapter receives the command capability for the
/// presentation whose snapshot it exposes, so a raw process-global view ID is
/// neither stored here nor resolved through a singleton.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsActionRequest {
    /// Stable target identity from the last platform snapshot.
    pub node_id: AccessibilityNodeId,

    /// The action requested by assistive technology.
    pub action: SemanticsAction,

    /// Optional typed arguments for actions such as text selection or scroll.
    pub arguments: Option<ActionArgs>,
}

impl SemanticsActionRequest {
    /// Creates an argument-free request.
    #[must_use]
    pub const fn new(node_id: AccessibilityNodeId, action: SemanticsAction) -> Self {
        Self {
            node_id,
            action,
            arguments: None,
        }
    }

    /// Creates a request carrying typed action arguments.
    #[must_use]
    pub fn with_arguments(
        node_id: AccessibilityNodeId,
        action: SemanticsAction,
        arguments: ActionArgs,
    ) -> Self {
        Self {
            node_id,
            action,
            arguments: Some(arguments),
        }
    }
}
