//! The vocabulary a widget or render object describes itself to assistive technology with.
//!
//! A render object fills a [`SemanticsConfiguration`] in
//! [`crate::rendering::RenderBox::describe_semantics_configuration`]; a widget sets the same
//! properties through [`crate::widgets::Semantics`]. These are the types its setters take: roles,
//! states, labels with their attributes, actions and their handlers, relations, collections, sort
//! keys and text content. The tree they build, its owner and the platform bridges are the
//! runtime's and are not part of this module.

pub use flui_rendering::semantics::{
    AccessibilityFocusBlockType, AttributedString, HeadingLevel, LiveSetting,
    SemanticsHintOverrides, SemanticsRole, SemanticsSortKey, SemanticsTag, StringAttribute,
    StringAttributeType, Urgency,
};
pub use flui_rendering::semantics::{
    ActionCall, AssistiveFocusHandler, CustomActionId, DeclaredCustomAction, Reveal, ScrollUnit,
    SemanticsAction, SemanticsActionHandler, SemanticsConfiguration, TextSelection,
};
pub use flui_rendering::semantics::{
    AnnounceError, Announcer, AssistiveFocus, AssistiveFocusChange, AssistiveFocusSupport,
};
pub use flui_rendering::semantics::{
    AutoComplete, Current, HasPopup, Invalid, NumericRange, NumericRangeError, States, Toggled,
};
pub use flui_rendering::semantics::{
    CellPosition, SemanticsCollection, SemanticsCollectionKind, SemanticsCollectionOrigin,
    SemanticsIndexOrigin, SetSize, SortDirection, TableShape,
};
pub use flui_rendering::semantics::{DocumentPosition, DocumentSelection};
pub use flui_rendering::semantics::{
    MAX_RUN_CHARACTERS, TextContent, TextRun, TextRunError, TextRunParts, TextRunPosition,
    TextRunSelection,
};
pub use flui_rendering::semantics::{SemanticsAnchor, SemanticsRelation};
