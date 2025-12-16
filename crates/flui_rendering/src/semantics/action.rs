//! Semantics actions that can be performed on nodes.
//!
//! This module re-exports types from `flui-semantics` for use in the rendering layer.

// Re-export all action types from flui-semantics
pub use flui_semantics::{ActionArgs, SemanticsAction, SemanticsActionHandler};

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
}
