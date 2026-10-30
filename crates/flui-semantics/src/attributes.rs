//! What a semantics node publishes (ADR-0206).
//!
//! [`SemanticsAttributes`] is the payload half of a
//! [`SemanticsConfiguration`](crate::SemanticsConfiguration): everything that
//! reaches the platform, without the assembly directives and action handlers
//! that only the framework reads. The tree stores configurations and
//! publication reads their attributes in place.

use std::sync::Arc;

use flui_foundation::geometry::Axis;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::{
    collection::Collection,
    properties::{
        AttributedString, HeadingLevel, SemanticsHintOverrides, SemanticsSortKey, SemanticsTag,
        TextDirection,
    },
    relation::{SemanticsAnchor, SemanticsRelation},
    role::{Assertiveness, SemanticsRole},
    states::States,
};

/// Everything one semantics node publishes.
///
/// Read through [`SemanticsConfiguration`](crate::SemanticsConfiguration)'s
/// accessors; the fields are the crate's, so merging and translation can
/// destructure them exhaustively.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SemanticsAttributes {
    /// The states this node declares.
    pub(crate) states: States,
    /// How changes are announced, when the node is a live region.
    pub(crate) live: Option<Assertiveness>,
    /// The label describing this node.
    pub(crate) label: Option<AttributedString>,
    /// The current value of this node.
    pub(crate) value: Option<AttributedString>,
    /// Validated native numeric-range metadata.
    pub(crate) numeric_range: Option<crate::NumericRange>,
    /// The value when increased.
    pub(crate) increased_value: Option<AttributedString>,
    /// The value when decreased.
    pub(crate) decreased_value: Option<AttributedString>,
    /// A hint about what will happen when the node is activated.
    pub(crate) hint: Option<AttributedString>,
    /// The tooltip for this node.
    pub(crate) tooltip: Option<SmolStr>,
    /// What an empty text field shows in place of a value.
    pub(crate) placeholder: Option<SmolStr>,
    /// The keys that activate or focus the node (`aria-keyshortcuts`).
    pub(crate) keyboard_shortcut: Option<SmolStr>,
    /// The BCP 47 language of the node's text.
    pub(crate) language: Option<SmolStr>,
    /// The address a link leads to or an image came from.
    pub(crate) url: Option<SmolStr>,
    /// What a screen reader says for the role (`aria-roledescription`).
    pub(crate) role_description: Option<SmolStr>,
    /// The widget author's stable identifier for tests and automation
    /// (ADR-0240): published as AccessKit `author_id`, never announced, never
    /// merged into another node.
    pub(crate) identifier: Option<Arc<str>>,
    /// The text direction.
    pub(crate) text_direction: Option<TextDirection>,
    /// The labels of the node's custom actions, in id order: a custom
    /// action's `CustomActionId` is its place in this list.
    pub(crate) custom_actions: SmallVec<[SmolStr; 2]>,
    /// Tags for this node.
    pub(crate) tags: SmallVec<[SemanticsTag; 2]>,
    /// Sort key for ordering.
    pub(crate) sort_key: Option<SemanticsSortKey>,
    /// Hint overrides.
    pub(crate) hint_overrides: Option<SemanticsHintOverrides>,
    /// Scroll position.
    pub(crate) scroll_position: Option<f64>,
    /// The axis the scroll range runs along; absent, the range publishes as
    /// vertical.
    pub(crate) scroll_axis: Option<Axis>,
    /// Scroll extent maximum.
    pub(crate) scroll_extent_max: Option<f64>,
    /// Scroll extent minimum.
    pub(crate) scroll_extent_min: Option<f64>,
    /// The scroll index for this node.
    pub(crate) scroll_index: Option<i32>,
    /// Where this node sits among its siblings and in a table, and how big
    /// the set it contains is.
    pub(crate) collection: Collection,
    /// Platform view ID if this represents a platform view.
    pub(crate) platform_view_id: Option<i32>,
    /// Maximum character count for text field.
    pub(crate) max_value_length: Option<i32>,
    /// Current character count for text field.
    pub(crate) current_value_length: Option<i32>,
    /// The outline level of a header node.
    pub(crate) heading_level: Option<HeadingLevel>,
    /// Anchors naming this node as a relation target (ADR-0205).
    pub(crate) anchors: SmallVec<[SemanticsAnchor; 1]>,
    /// This node's relations to the nodes other anchors name.
    pub(crate) relations: SmallVec<[(SemanticsRelation, SemanticsAnchor); 1]>,
    /// The node's one role; [`SemanticsRole::None`] declares none.
    pub(crate) role: SemanticsRole,
}

impl SemanticsAttributes {
    /// The role the node is held to and published as. A node that declares
    /// none and contains a set of declared size is a list: the consumer reads
    /// the size from the nearest ancestor it keeps and keeps no generic
    /// container, so the node is published as the list it stands for, and is
    /// held to what ARIA allows a list.
    pub(crate) fn effective_role(&self) -> SemanticsRole {
        if self.role == SemanticsRole::None && self.collection.set_size.is_some() {
            SemanticsRole::List
        } else {
            self.role
        }
    }
}
