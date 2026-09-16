//! Semantics actions that can be performed on nodes.
//!
//! This module provides action types for accessibility interactions.

use std::sync::Arc;

use crate::identity::AccessibilityNodeId;

// ============================================================================
// SemanticsAction
// ============================================================================

/// Actions that can be performed on a semantics node.
///
/// These correspond to actions that assistive technologies can request,
/// such as screen readers activating a button.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsAction` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum SemanticsAction {
    /// Tap action (like clicking a button).
    Tap = 1 << 0,

    /// Long press action.
    LongPress = 1 << 1,

    /// Scroll left action.
    ScrollLeft = 1 << 2,

    /// Scroll right action.
    ScrollRight = 1 << 3,

    /// Scroll up action.
    ScrollUp = 1 << 4,

    /// Scroll down action.
    ScrollDown = 1 << 5,

    /// Increase action (for sliders, steppers).
    Increase = 1 << 6,

    /// Decrease action (for sliders, steppers).
    Decrease = 1 << 7,

    /// Show on-screen keyboard.
    ShowOnScreen = 1 << 8,

    /// Move cursor forward by character.
    MoveCursorForwardByCharacter = 1 << 9,

    /// Move cursor backward by character.
    MoveCursorBackwardByCharacter = 1 << 10,

    /// Set selection in text field.
    SetSelection = 1 << 11,

    /// Copy text.
    Copy = 1 << 12,

    /// Cut text.
    Cut = 1 << 13,

    /// Paste text.
    Paste = 1 << 14,

    /// Did gain accessibility focus.
    DidGainAccessibilityFocus = 1 << 15,

    /// Did lose accessibility focus.
    DidLoseAccessibilityFocus = 1 << 16,

    /// Custom action.
    CustomAction = 1 << 17,

    /// Dismiss action (for dialogs, drawers).
    Dismiss = 1 << 18,

    /// Move cursor forward by word.
    MoveCursorForwardByWord = 1 << 19,

    /// Move cursor backward by word.
    MoveCursorBackwardByWord = 1 << 20,

    /// Set text content.
    SetText = 1 << 21,

    /// Focus action.
    Focus = 1 << 22,

    // NOTE: variants `Unfocus`, `Expand`, `Collapse` were removed
    // from this enum to match Flutter's `dart:ui.SemanticsAction` wire
    // format. The expand/collapse state lives on `SemanticsFlag` as
    // `HasExpandedState` + `IsExpanded`; un-focus is folded into the
    // platform's focus-management API rather than a discrete semantics
    // action. Their former bit slots (1 << 23 / 24 / 25) are RESERVED
    // and must not be reused for a new action without first realigning
    // with the engine.
    /// Scroll to a specific offset.
    ScrollToOffset = 1 << 26,
}

impl SemanticsAction {
    /// Returns the bitmask value for this action.
    #[inline]
    pub fn value(self) -> u64 {
        self as u64
    }

    /// Returns the name of this action.
    pub fn name(self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::LongPress => "longPress",
            Self::ScrollLeft => "scrollLeft",
            Self::ScrollRight => "scrollRight",
            Self::ScrollUp => "scrollUp",
            Self::ScrollDown => "scrollDown",
            Self::Increase => "increase",
            Self::Decrease => "decrease",
            Self::ShowOnScreen => "showOnScreen",
            Self::MoveCursorForwardByCharacter => "moveCursorForwardByCharacter",
            Self::MoveCursorBackwardByCharacter => "moveCursorBackwardByCharacter",
            Self::SetSelection => "setSelection",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::DidGainAccessibilityFocus => "didGainAccessibilityFocus",
            Self::DidLoseAccessibilityFocus => "didLoseAccessibilityFocus",
            Self::CustomAction => "customAction",
            Self::Dismiss => "dismiss",
            Self::MoveCursorForwardByWord => "moveCursorForwardByWord",
            Self::MoveCursorBackwardByWord => "moveCursorBackwardByWord",
            Self::SetText => "setText",
            Self::Focus => "focus",
            Self::ScrollToOffset => "scrollToOffset",
        }
    }

    /// Returns all semantics actions.
    pub fn values() -> &'static [SemanticsAction] {
        &[
            Self::Tap,
            Self::LongPress,
            Self::ScrollLeft,
            Self::ScrollRight,
            Self::ScrollUp,
            Self::ScrollDown,
            Self::Increase,
            Self::Decrease,
            Self::ShowOnScreen,
            Self::MoveCursorForwardByCharacter,
            Self::MoveCursorBackwardByCharacter,
            Self::SetSelection,
            Self::Copy,
            Self::Cut,
            Self::Paste,
            Self::DidGainAccessibilityFocus,
            Self::DidLoseAccessibilityFocus,
            Self::CustomAction,
            Self::Dismiss,
            Self::MoveCursorForwardByWord,
            Self::MoveCursorBackwardByWord,
            Self::SetText,
            Self::Focus,
            Self::ScrollToOffset,
        ]
    }

    /// Returns whether this action is a scroll action.
    pub fn is_scroll_action(self) -> bool {
        matches!(
            self,
            Self::ScrollLeft
                | Self::ScrollRight
                | Self::ScrollUp
                | Self::ScrollDown
                | Self::ScrollToOffset
        )
    }

    /// Returns whether this action is a cursor movement action.
    pub fn is_cursor_action(self) -> bool {
        matches!(
            self,
            Self::MoveCursorForwardByCharacter
                | Self::MoveCursorBackwardByCharacter
                | Self::MoveCursorForwardByWord
                | Self::MoveCursorBackwardByWord
        )
    }

    /// Returns whether this action is a text editing action.
    pub fn is_text_action(self) -> bool {
        matches!(
            self,
            Self::SetSelection | Self::SetText | Self::Copy | Self::Cut | Self::Paste
        )
    }

    /// Returns whether this action is a focus-related action.
    pub fn is_focus_action(self) -> bool {
        matches!(
            self,
            Self::Focus | Self::DidGainAccessibilityFocus | Self::DidLoseAccessibilityFocus
        )
    }
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
    use std::collections::BTreeSet;

    use flui_foundation::RenderId;

    use super::*;

    #[test]
    fn test_action_values() {
        assert_eq!(SemanticsAction::Tap.value(), 1);
        assert_eq!(SemanticsAction::LongPress.value(), 2);
        assert_eq!(SemanticsAction::ScrollLeft.value(), 4);
    }

    #[test]
    fn test_action_names() {
        assert_eq!(SemanticsAction::Tap.name(), "tap");
        assert_eq!(SemanticsAction::LongPress.name(), "longPress");
    }

    /// Every [`SemanticsAction`], listed once.
    ///
    /// A list is only exhaustive if something checks it. The array's length is
    /// not that check: it is a literal nothing compares against the enum, so on
    /// its own a variant could be added to the enum and to `values()` without
    /// ever appearing here, or appear here twice while another went missing.
    ///
    /// What is checked, in two links that are not one chain.
    /// [`declared_variant`] is a `match` with no wildcard arm, so a variant
    /// added to the enum stops compiling there and must be *named* in its
    /// alternation. Naming it is all that link obliges: the assertion the arm
    /// calls, [`declared`], runs only for an action the loop in
    /// [`test_all_actions`] visits, and that loop iterates
    /// [`SemanticsAction::values`]. The second link is that test's set
    /// comparison over the same `values()`. Together they catch the drift that
    /// happens in practice — a variant added to the enum and to `values()`
    /// without being listed here, or listed here without being added to
    /// `values()`, fails that comparison.
    ///
    /// **The residual hole, named rather than left to be inferred:** a variant
    /// added to the enum and named in the `match` but never added to
    /// `values()` compiles, is never visited by that loop and so never reaches
    /// [`declared`], and leaves the two sides equal because both lack it. No
    /// part of this gate sees that variant, and a consumer that enumerates the
    /// vocabulary has [`SemanticsAction::values`] to walk — which does not
    /// carry it.
    const EVERY_ACTION: [SemanticsAction; 24] = [
        SemanticsAction::Tap,
        SemanticsAction::LongPress,
        SemanticsAction::ScrollLeft,
        SemanticsAction::ScrollRight,
        SemanticsAction::ScrollUp,
        SemanticsAction::ScrollDown,
        SemanticsAction::Increase,
        SemanticsAction::Decrease,
        SemanticsAction::ShowOnScreen,
        SemanticsAction::MoveCursorForwardByCharacter,
        SemanticsAction::MoveCursorBackwardByCharacter,
        SemanticsAction::SetSelection,
        SemanticsAction::Copy,
        SemanticsAction::Cut,
        SemanticsAction::Paste,
        SemanticsAction::DidGainAccessibilityFocus,
        SemanticsAction::DidLoseAccessibilityFocus,
        SemanticsAction::CustomAction,
        SemanticsAction::Dismiss,
        SemanticsAction::MoveCursorForwardByWord,
        SemanticsAction::MoveCursorBackwardByWord,
        SemanticsAction::SetText,
        SemanticsAction::Focus,
        SemanticsAction::ScrollToOffset,
    ];

    /// Returns `action` if [`EVERY_ACTION`] lists it.
    ///
    /// Panics naming the variant, because the reader of a failure needs to know
    /// which action is missing rather than only that the counts disagree.
    fn declared(action: SemanticsAction) -> SemanticsAction {
        assert!(
            EVERY_ACTION.contains(&action),
            "`{}` is not in EVERY_ACTION: a new variant belongs in that list, \
             not only in the enum, or the list stops describing the enum",
            action.name(),
        );
        action
    }

    /// Names every variant, with no wildcard arm.
    ///
    /// The compile-time link. Adding a variant to the enum makes this `match`
    /// non-exhaustive, so the variant has to be named in the alternation below
    /// before the crate compiles again — and naming it is the whole
    /// obligation. This function runs only for the actions [`test_all_actions`]
    /// iterates, and that loop is driven by [`SemanticsAction::values`], so
    /// agreeing to compile does not by itself put the variant in
    /// [`EVERY_ACTION`]; the run-time link is that test's set comparison, and
    /// the list's own doc states what the pair does and does not catch. The
    /// arms are written as one alternation because every one of them does the
    /// same thing; the list that gives them meaning is [`EVERY_ACTION`].
    fn declared_variant(action: SemanticsAction) -> SemanticsAction {
        match action {
            SemanticsAction::Tap
            | SemanticsAction::LongPress
            | SemanticsAction::ScrollLeft
            | SemanticsAction::ScrollRight
            | SemanticsAction::ScrollUp
            | SemanticsAction::ScrollDown
            | SemanticsAction::Increase
            | SemanticsAction::Decrease
            | SemanticsAction::ShowOnScreen
            | SemanticsAction::MoveCursorForwardByCharacter
            | SemanticsAction::MoveCursorBackwardByCharacter
            | SemanticsAction::SetSelection
            | SemanticsAction::Copy
            | SemanticsAction::Cut
            | SemanticsAction::Paste
            | SemanticsAction::DidGainAccessibilityFocus
            | SemanticsAction::DidLoseAccessibilityFocus
            | SemanticsAction::CustomAction
            | SemanticsAction::Dismiss
            | SemanticsAction::MoveCursorForwardByWord
            | SemanticsAction::MoveCursorBackwardByWord
            | SemanticsAction::SetText
            | SemanticsAction::Focus
            | SemanticsAction::ScrollToOffset => declared(action),
        }
    }

    #[test]
    fn test_all_actions() {
        for action in SemanticsAction::values() {
            declared_variant(*action);
        }

        // Compared as bitmasks rather than as slices: the order an action is
        // published in is not part of the contract, the set of actions is.
        let published: BTreeSet<u64> = SemanticsAction::values()
            .iter()
            .map(|action| action.value())
            .collect();
        let listed: BTreeSet<u64> = EVERY_ACTION.iter().map(|action| action.value()).collect();

        let missing: Vec<_> = EVERY_ACTION
            .iter()
            .filter(|action| !published.contains(&action.value()))
            .map(|action| action.name())
            .collect();
        let unexpected: Vec<_> = SemanticsAction::values()
            .iter()
            .filter(|action| !listed.contains(&action.value()))
            .map(|action| action.name())
            .collect();

        // A duplicate is invisible to a set comparison — both entries carry the
        // same mask — so each side is checked for one-entry-per-action as well.
        // Distinct count equal to total count is exactly that, and these two
        // assertions plus the set equality below are multiset equality.
        assert_eq!(
            published.len(),
            SemanticsAction::values().len(),
            "values() lists an action twice: the duplicate hides whichever entry \
             it displaced, because both publish the same mask",
        );
        assert_eq!(
            listed.len(),
            EVERY_ACTION.len(),
            "EVERY_ACTION lists an action twice; the pinned array length hides \
             it, which is why that length is not the gate",
        );
        assert_eq!(
            published, listed,
            "values() and EVERY_ACTION must be the same set of actions; listed \
             but not published: {missing:?}, published but not listed: \
             {unexpected:?}",
        );
    }

    #[test]
    fn test_action_bitmask_combination() {
        let combined = SemanticsAction::Tap.value() | SemanticsAction::LongPress.value();
        assert_eq!(combined, 3);
        assert!(combined & SemanticsAction::Tap.value() != 0);
        assert!(combined & SemanticsAction::LongPress.value() != 0);
        assert_eq!(combined & SemanticsAction::ScrollLeft.value(), 0);
    }

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
