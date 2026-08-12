//! Batched semantics-update payloads.
//!
//! # These payloads are NOT the platform format — their ids are arena positions
//!
//! Every identity in this module (`SemanticsNodeData::id`, its `children`,
//! `SemanticsTreeUpdate::removed_node_ids`) is a 0-based arena position in a
//! tree the pipeline rebuilds every pass. Inserting or removing a sibling
//! shifts later positions, and a recycled slot hands a retired control's
//! number to an unrelated one — so publishing these values to an OS
//! accessibility adapter drops screen-reader focus on unrelated changes, and
//! an action request addressed back with one of them lands in the wrong
//! number space entirely (`SemanticsOwner::resolve_action` matches the stable
//! [`AccessibilityNodeId`](crate::AccessibilityNodeId) space, so every such
//! action fails as `NodeNotFound`). That is the exact bug the AccessKit
//! translation shipped once and was caught in review.
//!
//! What actually crosses to a platform adapter is [`accesskit::TreeUpdate`],
//! produced by [`tree_to_update`](crate::tree_to_update), which takes node
//! identity from [`SemanticsNode::accessibility_id`](crate::SemanticsNode::accessibility_id)
//! — never from these payloads. Use this module for in-process batching and
//! diagnostics; if a consumer ever needs stable identity here, that change is
//! tracked in issue #680 and is deliberately not made speculatively.

use flui_foundation::SemanticsId;
use flui_types::{Matrix4, Rect, geometry::Pixels};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::properties::TextDirection;
use crate::role::SemanticsRole;

// ============================================================================
// SemanticsNodeData
// ============================================================================

/// Serialized data for one semantics node.
///
/// **Not the platform format**: `id` and `children` are 0-based arena
/// positions, valid only within the pass that produced them — see the module
/// doc for why publishing them to an accessibility adapter is a bug and what
/// the real platform payload is.
#[derive(Debug, Clone)]
pub struct SemanticsNodeData {
    /// The node's 0-based arena position — NOT its stable
    /// [`AccessibilityNodeId`](crate::AccessibilityNodeId); see the module doc.
    pub id: u64,
    /// Flags bitmask.
    pub flags: u64,
    /// Actions bitmask.
    pub actions: u64,
    /// Label text.
    pub label: Option<SmolStr>,
    /// Value text.
    pub value: Option<SmolStr>,
    /// Increased value text.
    pub increased_value: Option<SmolStr>,
    /// Decreased value text.
    pub decreased_value: Option<SmolStr>,
    /// Hint text.
    pub hint: Option<SmolStr>,
    /// Tooltip text.
    pub tooltip: Option<SmolStr>,
    /// Text direction.
    pub text_direction: Option<TextDirection>,
    /// Bounding rectangle.
    pub rect: Rect<Pixels>,
    /// Transform matrix.
    pub transform: Matrix4,
    /// Children as 0-based arena positions — same space and caveat as
    /// [`Self::id`].
    pub children: SmallVec<[u64; 4]>,
    /// Platform view ID.
    pub platform_view_id: Option<i32>,
    /// Maximum value length for text fields.
    pub max_value_length: Option<i32>,
    /// Current value length for text fields.
    pub current_value_length: Option<i32>,
    /// Scroll position.
    pub scroll_position: Option<f64>,
    /// Maximum scroll extent.
    pub scroll_extent_max: Option<f64>,
    /// Minimum scroll extent.
    pub scroll_extent_min: Option<f64>,
    /// Scroll index.
    pub scroll_index: Option<i32>,
    /// Scroll child count.
    pub scroll_child_count: Option<i32>,
    /// The node's explicit accessibility role.
    ///
    /// Carried separately from [`Self::flags`] because the two encode role at
    /// different granularities, exactly as Flutter does. The common controls —
    /// button, link, text field, slider — are identified by a *flag* and leave
    /// this [`SemanticsRole::None`]; the structural roles a screen reader needs
    /// for navigation — `Tab`, `Table`, `ColumnHeader`, `MenuItemRadio` — have
    /// no flag and live only here. A consumer reading one and not the other
    /// sees half the tree's meaning.
    pub role: SemanticsRole,
}

impl Default for SemanticsNodeData {
    fn default() -> Self {
        Self {
            id: 0,
            flags: 0,
            actions: 0,
            label: None,
            value: None,
            increased_value: None,
            decreased_value: None,
            hint: None,
            tooltip: None,
            text_direction: None,
            rect: Rect::ZERO,
            transform: Matrix4::IDENTITY,
            role: SemanticsRole::None,
            children: SmallVec::new(),
            platform_view_id: None,
            max_value_length: None,
            current_value_length: None,
            scroll_position: None,
            scroll_extent_max: None,
            scroll_extent_min: None,
            scroll_index: None,
            scroll_child_count: None,
        }
    }
}

// ============================================================================
// SemanticsTreeUpdate
// ============================================================================

/// A batched semantics-tree update: added/updated nodes plus removed ids.
///
/// **Not the platform format** — every id here is a 0-based arena position
/// (see the module doc). In particular, a removal notice keyed on an arena
/// position would tell an adapter to drop an id it was never given.
///
/// See also [`SemanticsNodeUpdate`](crate::owner::SemanticsNodeUpdate) for
/// individual node updates.
#[derive(Debug, Clone, Default)]
pub struct SemanticsTreeUpdate {
    /// Nodes that have been added or updated.
    pub nodes: Vec<SemanticsNodeData>,

    /// 0-based arena positions of nodes that have been removed — the same
    /// in-process space as [`SemanticsNodeData::id`], with the same caveat.
    pub removed_node_ids: SmallVec<[u64; 8]>,
}

impl SemanticsTreeUpdate {
    /// Creates a new empty update.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether this update is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.removed_node_ids.is_empty()
    }

    /// Returns the number of node updates.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of removed nodes.
    #[inline]
    pub fn removed_count(&self) -> usize {
        self.removed_node_ids.len()
    }
}

// ============================================================================
// SemanticsUpdateBuilder
// ============================================================================

/// Builder for constructing semantics tree updates.
#[derive(Debug, Default)]
pub struct SemanticsTreeUpdateBuilder {
    nodes: Vec<SemanticsNodeData>,
    removed_node_ids: SmallVec<[u64; 8]>,
}

impl SemanticsTreeUpdateBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node to the update.
    pub fn add_node(&mut self, node: SemanticsNodeData) {
        self.nodes.push(node);
    }

    /// Adds a removed node ID.
    pub fn add_removed_node(&mut self, id: SemanticsId) {
        // 0-based arena position — same in-process space as the rest
        // of this payload, NOT a platform-facing identity (module doc).
        self.removed_node_ids.push((id.get() - 1) as u64);
    }

    /// Adds a removed node by raw ID.
    pub fn add_removed_node_raw(&mut self, id: u64) {
        self.removed_node_ids.push(id);
    }

    /// Returns the number of nodes added.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of removed nodes.
    #[inline]
    pub fn removed_count(&self) -> usize {
        self.removed_node_ids.len()
    }

    /// Builds the update.
    pub fn build(self) -> SemanticsTreeUpdate {
        SemanticsTreeUpdate {
            nodes: self.nodes,
            removed_node_ids: self.removed_node_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_tree_update_empty() {
        let update = SemanticsTreeUpdate::new();
        assert!(update.is_empty());
        assert_eq!(update.node_count(), 0);
        assert_eq!(update.removed_count(), 0);
    }

    #[test]
    fn test_semantics_tree_update_builder() {
        let mut builder = SemanticsTreeUpdateBuilder::new();

        builder.add_node(SemanticsNodeData {
            id: 0,
            label: Some(SmolStr::from("Test")),
            ..Default::default()
        });

        builder.add_removed_node_raw(5);

        let update = builder.build();

        assert!(!update.is_empty());
        assert_eq!(update.node_count(), 1);
        assert_eq!(update.removed_count(), 1);
        assert!(update.removed_node_ids.contains(&5));
    }

    #[test]
    fn test_semantics_node_data_default() {
        let data = SemanticsNodeData::default();
        assert_eq!(data.id, 0);
        assert_eq!(data.flags, 0);
        assert_eq!(data.actions, 0);
        assert!(data.label.is_none());
        assert!(data.children.is_empty());
    }

    #[test]
    fn test_smallvec_children() {
        let mut data = SemanticsNodeData::default();

        // Add children up to inline capacity
        data.children.push(1);
        data.children.push(2);
        data.children.push(3);
        data.children.push(4);

        assert_eq!(data.children.len(), 4);
        // Should be inline, not heap allocated
    }
}
