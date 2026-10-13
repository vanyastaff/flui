//! What a semantics node publishes (ADR-0202).
//!
//! [`SemanticsAttributes`] is the payload half of a
//! [`SemanticsConfiguration`](crate::SemanticsConfiguration): everything that
//! reaches the platform, without the assembly directives and action handlers
//! that only the framework reads. The tree stores configurations and
//! publication reads their attributes in place.

use flui_foundation::geometry::Axis;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::{
    properties::{
        AttributedString, CustomSemanticsAction, HeadingLevel, SemanticsHintOverrides,
        SemanticsSortKey, SemanticsTag, TextDirection,
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
pub struct SemanticsAttributes {
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
    /// The text direction.
    pub(crate) text_direction: Option<TextDirection>,
    /// Custom semantic actions.
    pub(crate) custom_actions: SmallVec<[CustomSemanticsAction; 2]>,
    /// Tags for this node.
    pub(crate) tags: SmallVec<[SemanticsTag; 2]>,
    /// Sort key for ordering.
    pub(crate) sort_key: Option<SemanticsSortKey>,
    /// Hint overrides.
    pub(crate) hint_overrides: Option<SemanticsHintOverrides>,
    /// Scroll position.
    pub(crate) scroll_position: Option<f64>,
    /// Actual range axis; absent metadata preserves vertical scalar callers.
    pub(crate) scroll_axis: Option<Axis>,
    /// Scroll extent maximum.
    pub(crate) scroll_extent_max: Option<f64>,
    /// Scroll extent minimum.
    pub(crate) scroll_extent_min: Option<f64>,
    /// This node's zero-based index among its parent's semantic children.
    pub(crate) index_in_parent: Option<i32>,
    /// The scroll index for this node.
    pub(crate) scroll_index: Option<i32>,
    /// The total scroll child count.
    pub(crate) scroll_child_count: Option<i32>,
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
