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

    /// Expand action (for expandable elements).
    Expand = 1 << 24,

    /// Collapse action (for expandable elements).
    Collapse = 1 << 25,

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
            Self::Unfocus => "unfocus",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
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
            Self::Unfocus,
            Self::Expand,
            Self::Collapse,
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
            Self::Focus
                | Self::Unfocus
                | Self::DidGainAccessibilityFocus
                | Self::DidLoseAccessibilityFocus
        )
    }
}

// ============================================================================
// SemanticsActionHandler
// ============================================================================

/// Handler for semantics actions.
pub type SemanticsActionHandler = Arc<dyn Fn(SemanticsAction, Option<ActionArgs>) + Send + Sync>;

/// Arguments for semantics actions.
#[derive(Debug, Clone, Default)]
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
