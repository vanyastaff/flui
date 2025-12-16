//! Semantics properties and flags.
//!
//! This module re-exports types from `flui-semantics` for use in the rendering layer.

// Re-export all property types from flui-semantics
pub use flui_semantics::{
    AttributedString, CustomSemanticsAction, SemanticsFlag, SemanticsFlags, SemanticsHintOverrides,
    SemanticsProperties, SemanticsSortKey, SemanticsTag, StringAttribute, TextDirection,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values() {
        assert_eq!(SemanticsFlag::HasCheckedState.value(), 1);
        assert_eq!(SemanticsFlag::IsChecked.value(), 2);
        assert_eq!(SemanticsFlag::IsSelected.value(), 4);
    }

    #[test]
    fn test_flags_operations() {
        let mut flags = SemanticsFlags::new();
        assert!(flags.is_empty());

        flags.set(SemanticsFlag::IsButton);
        assert!(flags.has(SemanticsFlag::IsButton));
        assert!(!flags.has(SemanticsFlag::IsLink));

        flags.set(SemanticsFlag::IsLink);
        assert!(flags.has(SemanticsFlag::IsLink));

        flags.clear(SemanticsFlag::IsButton);
        assert!(!flags.has(SemanticsFlag::IsButton));
    }

    #[test]
    fn test_sort_key_ordering() {
        let key1 = SemanticsSortKey::new(1.0);
        let key2 = SemanticsSortKey::new(2.0);
        assert!(key1 < key2);
    }
}
