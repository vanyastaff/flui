//! What an action reads back (ADR-0238 §9, R16): nothing, the full tree, or a delta against
//! the tree the action started from.
//!
//! A delta is what makes acting cheap for an agent: the nodes that appeared, the ones gone, and
//! the ones whose name, value, description, state or actions changed, each with its handle.
//! Bounds-only changes are left out unless asked, so a scroll does not flood it.

use crate::tree::{ElementRef, Node, Tree, TreeRevision};

vocabulary! {
    /// Which reading an action answers with.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Snapshot {
        /// No reading: the Rust API's default.
        #[default]
        None => "none",
        /// What changed since the action started: MCP's default.
        Delta => "delta",
        /// The whole tree after the action.
        Full => "full",
    }
}

vocabulary! {
    /// What changed about a node that is in both trees.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ChangedField {
        /// The accessible name.
        Name => "name",
        /// The value.
        Value => "value",
        /// The description.
        Description => "description",
        /// Checked, disabled, focused, expanded or selected.
        State => "state",
        /// The actions it offers.
        Actions => "actions",
        /// Its bounds; reported only when the delta was asked to include them.
        Bounds => "bounds",
    }
}

/// A node in both trees whose description changed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct NodeChange {
    /// The node as it is now, without children.
    pub node: Node,
    /// What differs from the earlier revision.
    pub changed: Vec<ChangedField>,
}

impl NodeChange {
    /// `node`, whose `changed` fields differ.
    #[must_use]
    pub fn new(node: Node, changed: Vec<ChangedField>) -> Self {
        Self { node, changed }
    }
}

/// The difference between two revisions of one window's tree, matched by element id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SnapshotDelta {
    /// The revision the action started on.
    pub from: TreeRevision,
    /// The revision after it.
    pub to: TreeRevision,
    /// Nodes not in `from`, without children.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub appeared: Vec<Node>,
    /// Nodes of `from` no longer in the tree.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub gone: Vec<ElementRef>,
    /// Nodes in both whose description changed.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub changed: Vec<NodeChange>,
    /// Whether the delta was capped like a read: then read the tree.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub truncated: bool,
}

impl SnapshotDelta {
    /// No change from `from` to `to`.
    #[must_use]
    pub fn new(from: TreeRevision, to: TreeRevision) -> Self {
        Self {
            from,
            to,
            appeared: Vec::new(),
            gone: Vec::new(),
            changed: Vec::new(),
            truncated: false,
        }
    }

    /// Whether nothing appeared, went or changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.gone.is_empty() && self.changed.is_empty()
    }
}

/// The reading an action answered with, as its [`Snapshot`] asked.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum SnapshotReading {
    /// What changed.
    Delta(SnapshotDelta),
    /// The whole tree.
    Full(Tree),
}
