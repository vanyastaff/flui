//! Semantic actions that can be performed on nodes

/// Actions that can be performed on a semantic node
///
/// Similar to Flutter's `SemanticsAction`. These represent actions
/// that can be requested by accessibility services.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::SemanticsAction;
///
/// let action = SemanticsAction::Tap;
/// assert_eq!(action, SemanticsAction::Tap);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SemanticsAction {
    /// Tap on the node
    #[default]
    Tap,

    /// Long press on the node
    LongPress,

    /// Scroll left
    ScrollLeft,

    /// Scroll right
    ScrollRight,

    /// Scroll up
    ScrollUp,

    /// Scroll down
    ScrollDown,

    /// Increase the value (e.g., slider)
    Increase,

    /// Decrease the value (e.g., slider)
    Decrease,

    /// Show on screen (scroll into view)
    ShowOnScreen,

    /// Move cursor forward in text
    MoveCursorForwardByCharacter,

    /// Move cursor backward in text
    MoveCursorBackwardByCharacter,

    /// Set text selection
    SetSelection,

    /// Copy text to clipboard
    Copy,

    /// Cut text to clipboard
    Cut,

    /// Paste text from clipboard
    Paste,

    /// Dismiss (e.g., close a dialog)
    Dismiss,

    /// Did gain accessibility focus
    DidGainAccessibilityFocus,

    /// Did lose accessibility focus
    DidLoseAccessibilityFocus,

    /// Custom action with an identifier
    CustomAction,
}

impl SemanticsAction {
    /// Returns true if this is a scroll action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::ScrollUp.is_scroll());
    /// assert!(SemanticsAction::ScrollDown.is_scroll());
    /// assert!(!SemanticsAction::Tap.is_scroll());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_scroll(&self) -> bool {
        matches!(
            self,
            Self::ScrollLeft | Self::ScrollRight | Self::ScrollUp | Self::ScrollDown
        )
    }

    /// Returns true if this is a text cursor movement action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::MoveCursorForwardByCharacter.is_cursor_movement());
    /// assert!(SemanticsAction::MoveCursorBackwardByCharacter.is_cursor_movement());
    /// assert!(!SemanticsAction::Tap.is_cursor_movement());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_cursor_movement(&self) -> bool {
        matches!(
            self,
            Self::MoveCursorForwardByCharacter | Self::MoveCursorBackwardByCharacter
        )
    }

    /// Returns true if this is a text editing action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::Copy.is_text_editing());
    /// assert!(SemanticsAction::Cut.is_text_editing());
    /// assert!(SemanticsAction::Paste.is_text_editing());
    /// assert!(!SemanticsAction::Tap.is_text_editing());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_text_editing(&self) -> bool {
        matches!(
            self,
            Self::Copy | Self::Cut | Self::Paste | Self::SetSelection
        )
    }

    /// Returns true if this is a value adjustment action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::Increase.is_value_adjustment());
    /// assert!(SemanticsAction::Decrease.is_value_adjustment());
    /// assert!(!SemanticsAction::Tap.is_value_adjustment());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_value_adjustment(&self) -> bool {
        matches!(self, Self::Increase | Self::Decrease)
    }

    /// Returns true if this is a focus-related action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::DidGainAccessibilityFocus.is_focus_action());
    /// assert!(SemanticsAction::DidLoseAccessibilityFocus.is_focus_action());
    /// assert!(!SemanticsAction::Tap.is_focus_action());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_focus_action(&self) -> bool {
        matches!(
            self,
            Self::DidGainAccessibilityFocus | Self::DidLoseAccessibilityFocus
        )
    }

    /// Returns true if this is an interactive gesture action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert!(SemanticsAction::Tap.is_gesture());
    /// assert!(SemanticsAction::LongPress.is_gesture());
    /// assert!(!SemanticsAction::Copy.is_gesture());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_gesture(&self) -> bool {
        matches!(self, Self::Tap | Self::LongPress)
    }

    /// Returns a human-readable name for this action
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsAction;
    ///
    /// assert_eq!(SemanticsAction::Tap.name(), "tap");
    /// assert_eq!(SemanticsAction::ScrollUp.name(), "scroll_up");
    /// ```
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::LongPress => "long_press",
            Self::ScrollLeft => "scroll_left",
            Self::ScrollRight => "scroll_right",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::Increase => "increase",
            Self::Decrease => "decrease",
            Self::ShowOnScreen => "show_on_screen",
            Self::MoveCursorForwardByCharacter => "move_cursor_forward",
            Self::MoveCursorBackwardByCharacter => "move_cursor_backward",
            Self::SetSelection => "set_selection",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::Dismiss => "dismiss",
            Self::DidGainAccessibilityFocus => "did_gain_focus",
            Self::DidLoseAccessibilityFocus => "did_lose_focus",
            Self::CustomAction => "custom_action",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_action_default() {
        let action = SemanticsAction::default();
        assert_eq!(action, SemanticsAction::Tap);
    }

    #[test]
    fn test_semantics_action_equality() {
        assert_eq!(SemanticsAction::Tap, SemanticsAction::Tap);
        assert_ne!(SemanticsAction::Tap, SemanticsAction::LongPress);
    }

    #[test]
    fn test_semantics_action_variants() {
        let _ = SemanticsAction::ScrollUp;
        let _ = SemanticsAction::ScrollDown;
        let _ = SemanticsAction::Increase;
        let _ = SemanticsAction::Decrease;
    }
}
