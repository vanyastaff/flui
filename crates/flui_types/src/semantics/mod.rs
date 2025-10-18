//! Semantics data types for accessibility
//!
//! This module provides types for describing semantic information about UI elements
//! for accessibility purposes (screen readers, assistive technologies, etc.).

pub mod action;
pub mod data;
pub mod events;
pub mod flags;
pub mod sort_key;
pub mod string_attributes;
pub mod tag;

pub use action::SemanticsAction;
pub use data::{SemanticsData, SemanticsHintOverrides, SemanticsProperties, SemanticsRole};
pub use events::{
    AnnounceSemanticsEvent, FocusSemanticEvent, LongPressSemanticsEvent, SemanticsEvent,
    TapSemanticEvent, TooltipSemanticsEvent,
};
pub use flags::SemanticsFlags;
pub use sort_key::{OrdinalSortKey, SemanticsSortKey};
pub use string_attributes::{
    AttributedString, LocaleStringAttribute, SpellOutStringAttribute, StringAttribute,
};
pub use tag::SemanticsTag;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Test that all exports are accessible
        let _ = SemanticsAction::Tap;
        let _ = SemanticsRole::Button;
        let _ = SemanticsFlags::default();
    }
}
