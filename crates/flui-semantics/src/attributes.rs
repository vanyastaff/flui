//! What a semantics node publishes (ADR-0206).
//!
//! [`SemanticsAttributes`] is the payload half of a
//! [`SemanticsConfiguration`](crate::SemanticsConfiguration): everything that
//! reaches the platform, without the assembly directives and action handlers
//! that only the framework reads. The tree stores configurations and
//! publication reads their attributes in place.

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::AxisDirection;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::{
    collection::Collection,
    properties::{
        AttributedString, HeadingLevel, SemanticsHintOverrides, SemanticsSortKey, SemanticsTag,
        TextDirection,
    },
    relation::{SemanticsAnchor, SemanticsRelation},
    role::{LiveSetting, SemanticsRole},
    states::States,
};

/// Everything one semantics node publishes.
///
/// Read through [`SemanticsConfiguration`](crate::SemanticsConfiguration)'s
/// accessors; the fields are the crate's, so merging and translation can
/// destructure them exhaustively.
///
/// What most nodes declare (a role, states, a label, a value, a hint, a place
/// in a set) is held inline. Everything else is a [`RareAttributes`] behind
/// one pointer, allocated by the first write to any of its fields and shared
/// by the clones of the configuration until one of them writes: a
/// configuration is held by a widget, its element and its render object and
/// cloned on every assembly, so a node that declares none of them does not
/// carry their room, and one that does copies them only to change them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SemanticsAttributes {
    /// The states this node declares.
    pub(crate) states: States,
    /// How changes are announced, when the node is a live region.
    pub(crate) live: Option<LiveSetting>,
    /// Whether a change announces the whole region (`aria-atomic`) rather
    /// than only what changed.
    pub(crate) live_atomic: Option<bool>,
    /// The label describing this node.
    pub(crate) label: Option<AttributedString>,
    /// The current value of this node.
    pub(crate) value: Option<AttributedString>,
    /// A hint about what will happen when the node is activated.
    pub(crate) hint: Option<AttributedString>,
    /// The widget author's stable identifier for tests and automation
    /// (ADR-0240): published as AccessKit `author_id`, never announced, never
    /// merged into another node.
    pub(crate) identifier: Option<Arc<str>>,
    /// The text direction.
    pub(crate) text_direction: Option<TextDirection>,
    /// A text field's text as runs and its selection, shared because a
    /// configuration is cloned on every assembly.
    pub(crate) text: Option<Arc<crate::TextContent>>,
    /// The outline level of a header node.
    pub(crate) heading_level: Option<HeadingLevel>,
    /// The node's one role; [`SemanticsRole::None`] declares none.
    pub(crate) role: SemanticsRole,
    /// Where this node sits among its siblings and in a table, and how big
    /// the set it contains is. Inline: every child of a lazy list declares
    /// its position.
    pub(crate) collection: Collection,
    /// The attributes few nodes declare.
    pub(crate) rare: Rare,
}

/// The attributes few nodes declare, behind [`SemanticsAttributes::rare`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RareAttributes {
    /// Validated native numeric-range metadata.
    pub(crate) numeric_range: Option<crate::NumericRange>,
    /// The value when increased.
    pub(crate) increased_value: Option<AttributedString>,
    /// The value when decreased.
    pub(crate) decreased_value: Option<AttributedString>,
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
    /// The direction the scroll position grows in; absent, the range grows
    /// from top to bottom.
    pub(crate) scroll_axis_direction: Option<AxisDirection>,
    /// Scroll extent maximum.
    pub(crate) scroll_extent_max: Option<f64>,
    /// Scroll extent minimum.
    pub(crate) scroll_extent_min: Option<f64>,
    /// The scroll index for this node.
    pub(crate) scroll_index: Option<i32>,
    /// Platform view ID if this represents a platform view.
    pub(crate) platform_view_id: Option<i32>,
    /// Maximum character count for text field.
    pub(crate) max_value_length: Option<i32>,
    /// Current character count for text field.
    pub(crate) current_value_length: Option<i32>,
    /// A document's selection across the texts beneath it (ADR-0214),
    /// resolved to their runs when published.
    pub(crate) document_selection: Option<crate::DocumentSelection>,
    /// The runs of the enclosing text this node publishes as its children,
    /// as indices into that text's runs: a link region of a selectable
    /// paragraph holds its own (ADR-0214 §2 as amended).
    pub(crate) hosted_runs: Option<std::ops::Range<usize>>,
    /// Anchors naming this node as a relation target (ADR-0205).
    pub(crate) anchors: SmallVec<[SemanticsAnchor; 1]>,
    /// This node's relations to the nodes other anchors name.
    pub(crate) relations: SmallVec<[(SemanticsRelation, SemanticsAnchor); 1]>,
}

impl RareAttributes {
    /// Every attribute undeclared: what a node that never wrote one reads.
    const EMPTY: &Self = &Self {
        numeric_range: None,
        increased_value: None,
        decreased_value: None,
        tooltip: None,
        placeholder: None,
        keyboard_shortcut: None,
        language: None,
        url: None,
        role_description: None,
        custom_actions: SmallVec::new_const(),
        tags: SmallVec::new_const(),
        sort_key: None,
        hint_overrides: None,
        scroll_position: None,
        scroll_axis_direction: None,
        scroll_extent_max: None,
        scroll_extent_min: None,
        scroll_index: None,
        platform_view_id: None,
        max_value_length: None,
        current_value_length: None,
        document_selection: None,
        hosted_runs: None,
        anchors: SmallVec::new_const(),
        relations: SmallVec::new_const(),
    };
}

/// The rarely declared attributes of one node: absent until the first write,
/// then shared by the node's clones until one of them writes.
///
/// Two are equal when they read the same, so a node that wrote an attribute
/// and cleared it equals one that never wrote it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Rare(Option<Rc<RareAttributes>>);

impl PartialEq for Rare {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (mine, theirs) => {
                mine.as_deref().unwrap_or(RareAttributes::EMPTY)
                    == theirs.as_deref().unwrap_or(RareAttributes::EMPTY)
            }
        }
    }
}

impl From<RareAttributes> for Rare {
    fn from(attributes: RareAttributes) -> Self {
        Self(Some(Rc::new(attributes)))
    }
}

impl SemanticsAttributes {
    /// The rarely declared attributes, all undeclared for a node that never
    /// wrote one.
    pub(crate) fn rare(&self) -> &RareAttributes {
        match &self.rare.0 {
            Some(rare) => rare,
            None => RareAttributes::EMPTY,
        }
    }

    /// The rarely declared attributes of a node that ever wrote one.
    pub(crate) fn rare_declared(&self) -> Option<&RareAttributes> {
        self.rare.0.as_deref()
    }

    /// The rarely declared attributes to write to, allocated by the first
    /// call. A write that only clears goes through
    /// [`rare_if_declared`](Self::rare_if_declared).
    pub(crate) fn rare_mut(&mut self) -> &mut RareAttributes {
        Rc::make_mut(self.rare.0.get_or_insert_default())
    }

    /// The rarely declared attributes, when the node ever wrote one: what a
    /// clearing write goes through, since there is nothing to clear
    /// otherwise.
    pub(crate) fn rare_if_declared(&mut self) -> Option<&mut RareAttributes> {
        self.rare.0.as_mut().map(Rc::make_mut)
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleared_rare_attribute_equals_one_never_declared() {
        let never = SemanticsAttributes::default();
        let mut cleared = SemanticsAttributes::default();
        cleared.rare_mut().tooltip = Some("Archive".into());
        assert_ne!(cleared, never);
        cleared.rare_mut().tooltip = None;
        assert_eq!(cleared, never);
        assert_eq!(never, cleared);
    }

    #[test]
    fn reading_and_clearing_allocate_no_rare_attributes() {
        let mut attributes = SemanticsAttributes::default();
        assert_eq!(attributes.rare().tooltip, None);
        assert!(attributes.rare_if_declared().is_none());
        assert!(attributes.rare_declared().is_none());
    }

    /// A configuration is held by a widget, its element and its render
    /// object: what few nodes declare stays behind one pointer.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_configuration_carries_no_room_for_what_few_nodes_declare() {
        assert!(
            std::mem::size_of::<crate::SemanticsConfiguration>() <= 512,
            "{} bytes",
            std::mem::size_of::<crate::SemanticsConfiguration>()
        );
    }
}
