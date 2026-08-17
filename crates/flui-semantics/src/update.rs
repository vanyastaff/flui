//! Batched semantics-update payloads, keyed by stable identity.
//!
//! # One id space: [`AccessibilityNodeId`]
//!
//! Every identity in this module — [`SemanticsNodeData::id`], its
//! [`children`](SemanticsNodeData::children), and
//! [`SemanticsTreeUpdate::removed_node_ids`] — is the stable OS-facing
//! [`AccessibilityNodeId`], the same space [`tree_to_update`](crate::tree_to_update)
//! publishes and [`SemanticsOwner::resolve_action`](crate::SemanticsOwner::resolve_action)
//! matches inbound actions against. `SemanticsId` — an arena position in a
//! tree the pipeline rebuilds every pass — never appears here.
//!
//! This mirrors Flutter, whose update payload
//! (`SemanticsUpdateBuilder.updateNode`) carries the node's stable
//! `SemanticsNode.id` and its children (`childrenInTraversalOrder`) in that
//! one id space, with no positional identity anywhere in the payload. FLUI
//! derives the stable id from the boundary's generational render identity
//! instead of a construction-time counter — see
//! [`AccessibilityNodeId`] for that documented
//! divergence — but the payload contract is the same: a node that logically
//! persists keeps its id across rebuilds and sibling reorders, so an adapter
//! can diff two updates and assistive-technology focus stays attached.
//!
//! An earlier shape of this payload carried 0-based arena positions and was
//! doc-labelled as the platform format; publishing those values shifts a
//! control's identity whenever a sibling is inserted or removed, and an
//! action request addressed back with one lands in the wrong number space
//! entirely. The types here now make that unrepresentable: there is no arena
//! value to leak.
//!
//! The constructor that fills a node's payload — including resolving its
//! children into this space — is
//! [`SemanticsTree::node_data`](crate::tree::SemanticsTree::node_data); only
//! the tree can resolve child identities, because a node stores its children
//! as arena ids.

use flui_types::{Matrix4, Rect, geometry::Pixels};
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::identity::AccessibilityNodeId;
use crate::properties::TextDirection;
use crate::role::SemanticsRole;

// ============================================================================
// SemanticsNodeData
// ============================================================================

/// Serialized data for one semantics node, keyed by its stable identity.
///
/// `id` and `children` are [`AccessibilityNodeId`]s — the space the platform
/// tree is published in and actions come back in (see the module doc).
#[derive(Debug, Clone)]
pub struct SemanticsNodeData {
    /// The node's stable OS-facing identity.
    ///
    /// `None` only in hand-built content (fixtures, node-level export of a
    /// node never bound to a render boundary). Such a payload is
    /// unaddressable and never enters an update: the constructor
    /// ([`SemanticsTree::node_data`](crate::tree::SemanticsTree::node_data))
    /// returns `None` for an unaddressable node, and
    /// [`SemanticsTreeUpdateBuilder::add_node`] omits an id-less payload —
    /// the same skip rule as [`tree_to_update`](crate::tree_to_update).
    /// Every node the pipeline assembles carries an identity.
    pub id: Option<AccessibilityNodeId>,
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
    /// Stable identities of this node's addressable children, in child order.
    ///
    /// Same space as [`Self::id`]. An unaddressable child is omitted rather
    /// than exported under a fabricated id. Only
    /// [`SemanticsTree::node_data`](crate::tree::SemanticsTree::node_data)
    /// can fill this — a node alone cannot resolve its children's stable
    /// identities — so a node-level export
    /// ([`SemanticsNode::to_node_data`](crate::SemanticsNode::to_node_data))
    /// leaves it empty.
    pub children: SmallVec<[AccessibilityNodeId; 4]>,
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
            id: None,
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
/// Every id is a stable [`AccessibilityNodeId`] (module doc). In particular a
/// removal notice names exactly the id the node was published under, so an
/// adapter told to drop it is dropping an id it was actually given.
#[derive(Debug, Clone, Default)]
pub struct SemanticsTreeUpdate {
    /// Nodes that have been added or updated.
    pub nodes: Vec<SemanticsNodeData>,

    /// Stable identities of nodes that have been removed — the same space as
    /// [`SemanticsNodeData::id`].
    pub removed_node_ids: SmallVec<[AccessibilityNodeId; 8]>,
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
    removed_node_ids: SmallVec<[AccessibilityNodeId; 8]>,
}

impl SemanticsTreeUpdateBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node to the update.
    ///
    /// A payload without an identity ([`SemanticsNodeData::id`] of `None`) is
    /// **omitted**: an update entry the platform cannot address or diff is
    /// worse than an absent one, and the tree-level constructor
    /// ([`SemanticsTree::node_data`](crate::tree::SemanticsTree::node_data))
    /// never produces one — only a hand-built payload can get here without an
    /// id, and it is dropped with a warning rather than batched.
    pub fn add_node(&mut self, node: SemanticsNodeData) {
        if node.id.is_none() {
            tracing::warn!(
                label = node.label.as_deref(),
                "dropping a semantics update payload without a stable identity"
            );
            return;
        }
        self.nodes.push(node);
    }

    /// Records the removal of the node published under `id`.
    ///
    /// Takes the stable [`AccessibilityNodeId`] — obtained from
    /// [`SemanticsNode::accessibility_id`](crate::SemanticsNode::accessibility_id)
    /// *before* the node is dropped from the tree, since the identity is not
    /// resolvable afterwards. There is deliberately no raw-integer variant:
    /// re-entering this space from an untyped number is how an arena position
    /// leaks back in.
    pub fn add_removed_node(&mut self, id: AccessibilityNodeId) {
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
    use flui_foundation::RenderId;

    use super::*;
    use crate::node::SemanticsNode;
    use crate::tree::SemanticsTree;

    /// A render identity whose packed value is deliberately unequal to any
    /// plausible arena position, so a test cannot pass by coincidence.
    fn render_id(index: u32) -> RenderId {
        RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        )
    }

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
            id: Some(AccessibilityNodeId::from(render_id(1))),
            label: Some(SmolStr::from("Test")),
            ..Default::default()
        });

        let removed = AccessibilityNodeId::from(render_id(5));
        builder.add_removed_node(removed);

        let update = builder.build();

        assert!(!update.is_empty());
        assert_eq!(update.node_count(), 1);
        assert_eq!(update.removed_count(), 1);
        assert!(update.removed_node_ids.contains(&removed));
    }

    #[test]
    fn test_semantics_node_data_default() {
        let data = SemanticsNodeData::default();
        assert!(data.id.is_none());
        assert_eq!(data.flags, 0);
        assert_eq!(data.actions, 0);
        assert!(data.label.is_none());
        assert!(data.children.is_empty());
    }

    /// **The identity-stability contract this payload exists for.** Inserting
    /// a sibling ahead of a control shifts its arena position; its payload
    /// identity must not move, or an adapter diffing two updates sees an
    /// unchanged control as removed-and-replaced and drops focus.
    #[test]
    fn payload_identity_survives_a_sibling_inserted_before_the_node() {
        let source = render_id(42);

        let data_for = |leading_siblings: u32| {
            let mut tree = SemanticsTree::new();
            let mut root_node = SemanticsNode::new().with_source_render_id(render_id(1));
            for index in 0..leading_siblings {
                let sibling =
                    tree.insert(SemanticsNode::new().with_source_render_id(render_id(100 + index)));
                root_node.add_child(sibling);
            }
            let target = tree.insert(SemanticsNode::new().with_source_render_id(source));
            root_node.add_child(target);
            let root = tree.insert(root_node);
            tree.set_root(Some(root));
            tree.node_data(target).expect("target is live")
        };

        let before = data_for(0);
        let after = data_for(3);

        assert_eq!(
            before.id,
            Some(AccessibilityNodeId::from(source)),
            "the payload id is the stable render-boundary identity"
        );
        assert_eq!(
            before.id, after.id,
            "arena positions shifted; the payload identity must not"
        );
    }

    /// Children are exported in the same stable space as the ids they will be
    /// published under — never as arena positions.
    #[test]
    fn payload_children_are_stable_identities_in_child_order() {
        let first = render_id(51);
        let second = render_id(52);

        let mut tree = SemanticsTree::new();
        let first_id = tree.insert(SemanticsNode::new().with_source_render_id(first));
        let second_id = tree.insert(SemanticsNode::new().with_source_render_id(second));
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(50));
        root_node.add_child(first_id);
        root_node.add_child(second_id);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let data = tree.node_data(root).expect("root is live");
        assert_eq!(
            data.children.as_slice(),
            &[
                AccessibilityNodeId::from(first),
                AccessibilityNodeId::from(second),
            ],
        );
        assert_ne!(
            data.children[0].as_u64(),
            (first_id.get() - 1) as u64,
            "the fixture is only meaningful while the two id spaces differ"
        );
    }

    /// An unaddressable child has no identity to export; omitting it mirrors
    /// the publish path's skip rule rather than inventing an id that could
    /// collide with a real control.
    #[test]
    fn an_unaddressable_child_is_omitted_from_the_payload() {
        let mut tree = SemanticsTree::new();
        let unaddressable = tree.insert(SemanticsNode::new());
        let addressable_render = render_id(61);
        let addressable =
            tree.insert(SemanticsNode::new().with_source_render_id(addressable_render));
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(60));
        root_node.add_child(unaddressable);
        root_node.add_child(addressable);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let data = tree.node_data(root).expect("root is live");
        assert_eq!(
            data.children.as_slice(),
            &[AccessibilityNodeId::from(addressable_render)],
        );
    }

    /// A removal notice names exactly the id the node was published under, so
    /// the two ends of the payload cannot drift into different number spaces.
    #[test]
    fn a_removal_notice_names_the_id_the_node_was_published_under() {
        let source = render_id(71);
        let mut tree = SemanticsTree::new();
        let root = tree.insert(SemanticsNode::new().with_source_render_id(source));
        tree.set_root(Some(root));

        let published = crate::tree_to_update(&tree, None)
            .expect("a rooted tree yields an update")
            .nodes
            .first()
            .map(|(id, _)| id.0)
            .expect("one node");

        let identity = tree
            .get(root)
            .and_then(SemanticsNode::accessibility_id)
            .expect("a render-backed node is addressable");

        let mut builder = SemanticsTreeUpdateBuilder::new();
        builder.add_removed_node(identity);
        let update = builder.build();

        assert_eq!(update.removed_node_ids[0].as_u64(), published);
    }

    /// A payload names its subject; a node with no stable identity has no
    /// payload. Returning content under `id: None` would let it into an
    /// update that platform and diff consumers cannot address.
    #[test]
    fn node_data_is_none_for_an_unaddressable_node() {
        let mut tree = SemanticsTree::new();
        let unaddressable = tree.insert(SemanticsNode::new());
        tree.set_root(Some(unaddressable));

        assert!(tree.node_data(unaddressable).is_none());
    }

    /// The builder is the assembly seam for `SemanticsTreeUpdate`, so the
    /// publish path's skip rule must hold there too: a hand-built payload
    /// without identity is omitted, never batched.
    #[test]
    fn the_builder_omits_a_payload_without_identity() {
        let mut builder = SemanticsTreeUpdateBuilder::new();

        builder.add_node(SemanticsNodeData::default());
        builder.add_node(SemanticsNodeData {
            id: Some(AccessibilityNodeId::from(render_id(9))),
            ..Default::default()
        });

        let update = builder.build();
        assert_eq!(update.node_count(), 1);
        assert_eq!(
            update.nodes[0].id,
            Some(AccessibilityNodeId::from(render_id(9))),
            "only the addressable payload survives into the update"
        );
    }

    #[test]
    fn test_smallvec_children() {
        let mut data = SemanticsNodeData::default();

        // Add children up to inline capacity
        for index in 1..=4 {
            data.children
                .push(AccessibilityNodeId::from(render_id(index)));
        }

        assert_eq!(data.children.len(), 4);
        // Should be inline, not heap allocated
    }
}
