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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SemanticsAction {
    /// Tap on the node
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

impl Default for SemanticsAction {
    fn default() -> Self {
        Self::Tap
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
