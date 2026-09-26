//! Semantics actions that can be performed on nodes.
//!
//! This module provides action types for accessibility interactions.
//! [`SemanticsAction`] itself lives in `flui-protocol`, the vocabulary FLUI
//! shares with tests, devtools and agents; it is re-exported here so its path
//! through this crate is unchanged.

use std::sync::Arc;

pub use flui_protocol::SemanticsAction;

use crate::identity::AccessibilityNodeId;

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

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;

    use super::*;

    #[test]
    fn action_request_uses_the_snapshot_identity_and_typed_arguments() {
        let node_id = AccessibilityNodeId::from(RenderId::new(7));
        let request = SemanticsActionRequest::with_arguments(
            node_id,
            SemanticsAction::SetText,
            ActionArgs::SetText {
                text: "Hello".to_owned(),
            },
        );

        assert_eq!(request.node_id, node_id);
        assert_eq!(request.action, SemanticsAction::SetText);
        assert_eq!(
            request.arguments,
            Some(ActionArgs::SetText {
                text: "Hello".to_owned(),
            }),
        );
    }
}
