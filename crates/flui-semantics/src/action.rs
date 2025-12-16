//! Semantics actions that can be performed on nodes.
//!
//! This module provides action types for accessibility interactions.

use std::sync::Arc;

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

    /// Unfocus action.
    Unfocus = 1 << 23,
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
            Self::Unfocus => "unfocus",
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
            Self::Unfocus,
        ]
    }
}

// ============================================================================
// SemanticsActionHandler
// ============================================================================

/// Handler for semantics actions.
pub type SemanticsActionHandler = Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>;

/// Arguments for semantics actions.
#[derive(Debug, Clone)]
pub enum ActionArgs {
    /// No arguments.
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
}

impl Default for ActionArgs {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn test_all_actions() {
        let actions = SemanticsAction::values();
        assert!(actions.len() >= 20);
        assert!(actions.contains(&SemanticsAction::Tap));
        assert!(actions.contains(&SemanticsAction::Dismiss));
    }

    #[test]
    fn test_action_bitmask_combination() {
        let combined = SemanticsAction::Tap.value() | SemanticsAction::LongPress.value();
        assert_eq!(combined, 3);
        assert!(combined & SemanticsAction::Tap.value() != 0);
        assert!(combined & SemanticsAction::LongPress.value() != 0);
        assert!(combined & SemanticsAction::ScrollLeft.value() == 0);
    }
}
