//! Immutable, owned snapshots of the complete rooted semantics result.
//!
//! “Complete” means that the snapshot contains every node reachable from the
//! current semantics root and every annotation or geometry value modeled for
//! platform adapters. It is deliberately not a clone of the transient
//! [`SemanticsConfiguration`](crate::SemanticsConfiguration): boundary and
//! descendant-merge directives decide the assembled tree shape, while
//! `blocks_user_actions` governs which registered actions remain effective.
//! Those inputs are not adapter payload; the snapshot carries the resulting
//! tree and effective action bits.
//!
//! `explicit_child_nodes` is likewise excluded because rendering assembly has
//! already applied it: adapters observe the resulting explicit child nodes and
//! their stable identities, not the structural input that produced them.

use flui_foundation::SemanticsId;
use flui_foundation::geometry::{Matrix4, Rect};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
use smallvec::SmallVec;
use smol_str::SmolStr;
use thiserror::Error;

use crate::{
    identity::AccessibilityNodeId,
    node::SemanticsNode,
    properties::{
        AttributedString, CustomSemanticsAction, SemanticsHintOverrides, SemanticsSortKey,
        SemanticsTag, TextDirection,
    },
    role::SemanticsRole,
    tree::SemanticsTree,
};

/// Failure to build a complete stable-identity snapshot.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SemanticsSnapshotError {
    /// The semantics tree has no root.
    #[error("the semantics tree has no root")]
    MissingRoot,

    /// A tree edge references a missing internal arena node.
    #[error("semantics node {node:?} does not resolve in the semantics arena")]
    MissingNode {
        /// The unresolved internal arena locator.
        node: SemanticsId,
    },

    /// A semantics node was not associated with its source render object.
    #[error("semantics node {node:?} has no stable render identity")]
    MissingAccessibilityIdentity {
        /// The internal arena locator of the malformed node.
        node: SemanticsId,
    },

    /// Two live semantics nodes claim the same external identity.
    #[error(
        "semantics nodes {first_node:?} and {duplicate_node:?} share accessibility identity {id}"
    )]
    DuplicateAccessibilityIdentity {
        /// The duplicated external identity.
        id: AccessibilityNodeId,
        /// The first internal node encountered in preorder.
        first_node: SemanticsId,
        /// The later internal node that reused the identity.
        duplicate_node: SemanticsId,
    },

    /// A node is reachable more than once, which would make the result a graph.
    #[error("semantics node {node:?} is reachable more than once")]
    RepeatedNode {
        /// The repeated internal arena locator.
        node: SemanticsId,
    },
}

/// An owned snapshot of the entire rooted, assembled semantics tree.
///
/// Nodes are stored in deterministic preorder. The snapshot contains no
/// callbacks, arena borrows, or platform-library types, so it can be moved to
/// another thread without exposing the mutable [`SemanticsOwner`](crate::SemanticsOwner).
/// All fields are private and exposed through read-only accessors, allowing
/// later adapter payload additions without permitting partially initialized
/// snapshots.
#[derive(Debug, Clone)]
pub struct SemanticsSnapshot {
    root: AccessibilityNodeId,
    nodes: Box<[SemanticsNodeSnapshot]>,
}

impl SemanticsSnapshot {
    /// Returns the stable identity of the root node.
    #[inline]
    #[must_use]
    pub const fn root(&self) -> AccessibilityNodeId {
        self.root
    }

    /// Returns all nodes in deterministic preorder.
    #[inline]
    #[must_use]
    pub fn nodes(&self) -> &[SemanticsNodeSnapshot] {
        &self.nodes
    }

    /// Looks up a node by its stable accessibility identity.
    ///
    /// This is a linear scan because snapshots are primarily streamed once to
    /// a platform adapter. Callers needing repeated indexed lookup can build an
    /// adapter-owned map without making every snapshot pay for one.
    #[must_use]
    pub fn node(&self, id: AccessibilityNodeId) -> Option<&SemanticsNodeSnapshot> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub(crate) fn from_tree(tree: &SemanticsTree) -> Result<Self, SemanticsSnapshotError> {
        let root_semantics_id = tree.root().ok_or(SemanticsSnapshotError::MissingRoot)?;
        let root = stable_id(tree, root_semantics_id)?;

        let mut nodes = Vec::with_capacity(tree.len());
        let mut pending = Vec::with_capacity(tree.len());
        let mut visited = FxHashSet::with_capacity_and_hasher(tree.len(), FxBuildHasher);
        let mut identity_owners = FxHashMap::with_capacity_and_hasher(tree.len(), FxBuildHasher);
        pending.push((root_semantics_id, None));

        while let Some((semantics_id, parent)) = pending.pop() {
            if !visited.insert(semantics_id) {
                return Err(SemanticsSnapshotError::RepeatedNode { node: semantics_id });
            }

            let node = tree
                .get(semantics_id)
                .ok_or(SemanticsSnapshotError::MissingNode { node: semantics_id })?;
            let id = node.accessibility_id().ok_or(
                SemanticsSnapshotError::MissingAccessibilityIdentity { node: semantics_id },
            )?;

            if let Some(first_node) = identity_owners.insert(id, semantics_id) {
                return Err(SemanticsSnapshotError::DuplicateAccessibilityIdentity {
                    id,
                    first_node,
                    duplicate_node: semantics_id,
                });
            }

            let children = node
                .children()
                .iter()
                .map(|&child| stable_id(tree, child))
                .collect::<Result<SmallVec<[AccessibilityNodeId; 4]>, _>>()?;

            pending.extend(node.children().iter().rev().map(|&child| (child, Some(id))));

            nodes.push(SemanticsNodeSnapshot::from_node(id, parent, children, node));
        }

        Ok(Self {
            root,
            nodes: nodes.into_boxed_slice(),
        })
    }
}

fn stable_id(
    tree: &SemanticsTree,
    semantics_id: SemanticsId,
) -> Result<AccessibilityNodeId, SemanticsSnapshotError> {
    tree.get(semantics_id)
        .ok_or(SemanticsSnapshotError::MissingNode { node: semantics_id })?
        .accessibility_id()
        .ok_or(SemanticsSnapshotError::MissingAccessibilityIdentity { node: semantics_id })
}

/// Owned, callback-free data for one node in a [`SemanticsSnapshot`].
#[derive(Debug, Clone)]
pub struct SemanticsNodeSnapshot {
    id: AccessibilityNodeId,
    parent: Option<AccessibilityNodeId>,
    children: SmallVec<[AccessibilityNodeId; 4]>,
    role: SemanticsRole,
    flags: u64,
    actions: u64,
    label: Option<AttributedString>,
    value: Option<AttributedString>,
    increased_value: Option<AttributedString>,
    decreased_value: Option<AttributedString>,
    hint: Option<AttributedString>,
    tooltip: Option<SmolStr>,
    text_direction: Option<TextDirection>,
    custom_actions: Box<[CustomSemanticsAction]>,
    tags: Box<[SemanticsTag]>,
    sort_key: Option<SemanticsSortKey>,
    hint_overrides: Option<SemanticsHintOverrides>,
    rect: Rect<f64>,
    transform: Matrix4,
    platform_view_id: Option<i32>,
    max_value_length: Option<i32>,
    current_value_length: Option<i32>,
    scroll_position: Option<f64>,
    scroll_extent_max: Option<f64>,
    scroll_extent_min: Option<f64>,
    scroll_index: Option<i32>,
    scroll_child_count: Option<i32>,
    index_in_parent: Option<i32>,
}

impl SemanticsNodeSnapshot {
    fn from_node(
        id: AccessibilityNodeId,
        parent: Option<AccessibilityNodeId>,
        children: SmallVec<[AccessibilityNodeId; 4]>,
        node: &SemanticsNode,
    ) -> Self {
        let config = node.config();
        Self {
            id,
            parent,
            children,
            role: config.role(),
            flags: config.flags().bits(),
            actions: config.effective_actions_as_bits(),
            label: config.label().cloned(),
            value: config.value().cloned(),
            increased_value: config.increased_value().cloned(),
            decreased_value: config.decreased_value().cloned(),
            hint: config.hint().cloned(),
            tooltip: config.tooltip().map(SmolStr::new),
            text_direction: config.text_direction(),
            custom_actions: config
                .effective_custom_actions()
                .to_vec()
                .into_boxed_slice(),
            tags: config.tags().to_vec().into_boxed_slice(),
            sort_key: config.sort_key().cloned(),
            hint_overrides: config.hint_overrides().cloned(),
            rect: node.rect(),
            transform: node.transform().copied().unwrap_or(Matrix4::IDENTITY),
            platform_view_id: config.platform_view_id(),
            max_value_length: config.max_value_length(),
            current_value_length: config.current_value_length(),
            scroll_position: config.scroll_position(),
            scroll_extent_max: config.scroll_extent_max(),
            scroll_extent_min: config.scroll_extent_min(),
            scroll_index: config.scroll_index(),
            scroll_child_count: config.scroll_child_count(),
            index_in_parent: config.index_in_parent(),
        }
    }

    /// Returns this node's stable accessibility identity.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> AccessibilityNodeId {
        self.id
    }

    /// Returns the stable identity of the parent, or `None` for the root.
    #[inline]
    #[must_use]
    pub const fn parent(&self) -> Option<AccessibilityNodeId> {
        self.parent
    }

    /// Returns child identities in traversal order.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[AccessibilityNodeId] {
        &self.children
    }

    /// Returns the structural semantics role.
    #[inline]
    #[must_use]
    pub const fn role(&self) -> SemanticsRole {
        self.role
    }

    /// Returns the raw semantics flag bits.
    #[inline]
    #[must_use]
    pub const fn flags(&self) -> u64 {
        self.flags
    }

    /// Returns the effective action bits exposed to assistive technology.
    #[inline]
    #[must_use]
    pub const fn actions(&self) -> u64 {
        self.actions
    }

    /// Returns the attributed label.
    #[inline]
    #[must_use]
    pub fn label(&self) -> Option<&AttributedString> {
        self.label.as_ref()
    }

    /// Returns the attributed current value.
    #[inline]
    #[must_use]
    pub fn value(&self) -> Option<&AttributedString> {
        self.value.as_ref()
    }

    /// Returns the attributed value announced after increasing.
    #[inline]
    #[must_use]
    pub fn increased_value(&self) -> Option<&AttributedString> {
        self.increased_value.as_ref()
    }

    /// Returns the attributed value announced after decreasing.
    #[inline]
    #[must_use]
    pub fn decreased_value(&self) -> Option<&AttributedString> {
        self.decreased_value.as_ref()
    }

    /// Returns the attributed interaction hint.
    #[inline]
    #[must_use]
    pub fn hint(&self) -> Option<&AttributedString> {
        self.hint.as_ref()
    }

    /// Returns the tooltip text.
    #[inline]
    #[must_use]
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// Returns the text direction.
    #[inline]
    #[must_use]
    pub const fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    /// Returns custom action metadata without their callbacks.
    #[inline]
    #[must_use]
    pub fn custom_actions(&self) -> &[CustomSemanticsAction] {
        &self.custom_actions
    }

    /// Returns semantics tags.
    #[inline]
    #[must_use]
    pub fn tags(&self) -> &[SemanticsTag] {
        &self.tags
    }

    /// Returns the traversal sort key.
    #[inline]
    #[must_use]
    pub fn sort_key(&self) -> Option<&SemanticsSortKey> {
        self.sort_key.as_ref()
    }

    /// Returns platform hint overrides.
    #[inline]
    #[must_use]
    pub fn hint_overrides(&self) -> Option<&SemanticsHintOverrides> {
        self.hint_overrides.as_ref()
    }

    /// Returns the node bounds in the semantics coordinate space.
    #[inline]
    #[must_use]
    pub const fn rect(&self) -> Rect<f64> {
        self.rect
    }

    /// Returns the node transform, using identity for an omitted transform.
    #[inline]
    #[must_use]
    pub const fn transform(&self) -> Matrix4 {
        self.transform
    }

    /// Returns the embedded platform-view identifier.
    #[inline]
    #[must_use]
    pub const fn platform_view_id(&self) -> Option<i32> {
        self.platform_view_id
    }

    /// Returns the maximum text value length.
    #[inline]
    #[must_use]
    pub const fn max_value_length(&self) -> Option<i32> {
        self.max_value_length
    }

    /// Returns the current text value length.
    #[inline]
    #[must_use]
    pub const fn current_value_length(&self) -> Option<i32> {
        self.current_value_length
    }

    /// Returns the current scroll position.
    #[inline]
    #[must_use]
    pub const fn scroll_position(&self) -> Option<f64> {
        self.scroll_position
    }

    /// Returns the maximum scroll extent.
    #[inline]
    #[must_use]
    pub const fn scroll_extent_max(&self) -> Option<f64> {
        self.scroll_extent_max
    }

    /// Returns the minimum scroll extent.
    #[inline]
    #[must_use]
    pub const fn scroll_extent_min(&self) -> Option<f64> {
        self.scroll_extent_min
    }

    /// Returns this node's semantic scroll index.
    #[inline]
    #[must_use]
    pub const fn scroll_index(&self) -> Option<i32> {
        self.scroll_index
    }

    /// Returns the total semantic child count of a scroll container.
    #[inline]
    #[must_use]
    pub const fn scroll_child_count(&self) -> Option<i32> {
        self.scroll_child_count
    }

    /// Returns the node's logical index within its parent.
    #[inline]
    #[must_use]
    pub const fn index_in_parent(&self) -> Option<i32> {
        self.index_in_parent
    }
}

#[cfg(test)]
mod tests {}
