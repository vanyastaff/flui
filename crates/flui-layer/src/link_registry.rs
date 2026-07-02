//! Leader-Follower Link Registry
//!
//! This module provides tracking for leader-follower layer relationships.
//! When building scenes with linked layers (tooltips, dropdowns, etc.),
//! the registry keeps track of which leaders are connected to which followers.
//!
//! # Architecture
//!
//! ```text
//! LinkRegistry
//!   ├─ leaders: HashMap<LayerLink, LeaderInfo>
//!   └─ followers: HashMap<LayerId, LayerLink>
//!
//! LeaderInfo
//!   ├─ layer_id: LayerId
//!   ├─ offset: Offset (computed global position)
//!   ├─ size: Size
//!   └─ followers: Vec<LayerId>
//! ```
//!
//! # Usage
//!
//! ```rust
//! use flui_layer::{FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, LinkRegistry};
//! use flui_types::geometry::{Offset, Size, px};
//!
//! let mut tree = LayerTree::new();
//! let mut registry = LinkRegistry::new();
//!
//! // Create linked layers
//! let link = LayerLink::new();
//! let leader = LeaderLayer::new(link, Size::new(px(100.0), px(30.0)));
//! let follower = FollowerLayer::below(link, 5.0);
//!
//! // Insert into tree
//! let leader_id = tree.insert(Layer::Leader(leader));
//! let follower_id = tree.insert(Layer::Follower(follower));
//!
//! // Register in the link registry
//! registry.register_leader(
//!     link,
//!     leader_id,
//!     Offset::new(px(50.0), px(100.0)),
//!     Size::new(px(100.0), px(30.0)),
//! );
//! registry.register_follower(follower_id, link);
//!
//! // Query followers for a leader
//! let followers = registry.followers_for_link(link);
//! ```

use std::collections::{HashMap, HashSet};

use flui_foundation::LayerId;
use flui_types::geometry::{Offset, Pixels, Size};

use crate::layer::{FollowerLayer, Layer, LayerLink};
use crate::tree::LayerTree;

// ============================================================================
// LEADER INFO
// ============================================================================

/// Information about a registered leader layer.
#[derive(Debug, Clone)]
pub struct LeaderInfo {
    /// The LayerId of the leader in the tree
    pub layer_id: LayerId,

    /// Global offset (computed during traversal)
    pub offset: Offset<Pixels>,

    /// Size of the leader area
    pub size: Size<Pixels>,

    /// List of follower LayerIds linked to this leader
    pub followers: Vec<LayerId>,
}

impl LeaderInfo {
    /// Creates new leader info.
    pub fn new(layer_id: LayerId, offset: Offset<Pixels>, size: Size<Pixels>) -> Self {
        Self {
            layer_id,
            offset,
            size,
            followers: Vec::new(),
        }
    }

    /// Adds a follower to this leader.
    pub fn add_follower(&mut self, follower_id: LayerId) {
        if !self.followers.contains(&follower_id) {
            self.followers.push(follower_id);
        }
    }

    /// Removes a follower from this leader.
    pub fn remove_follower(&mut self, follower_id: LayerId) {
        self.followers.retain(|&id| id != follower_id);
    }

    /// Returns true if this leader has any followers.
    pub fn has_followers(&self) -> bool {
        !self.followers.is_empty()
    }
}

// ============================================================================
// LINK REGISTRY
// ============================================================================

/// Registry for tracking leader-follower layer relationships.
///
/// The LinkRegistry maintains bidirectional mappings between:
/// - LayerLink → LeaderInfo (leader details and follower list)
/// - LayerId → LayerLink (for follower lookup)
///
/// This enables efficient queries in both directions during
/// scene composition and hit testing.
#[derive(Debug, Default)]
pub struct LinkRegistry {
    /// Maps LayerLink to leader information
    leaders: HashMap<LayerLink, LeaderInfo>,

    /// Maps follower LayerId to their LayerLink
    followers: HashMap<LayerId, LayerLink>,
}

impl LinkRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry with pre-allocated capacity.
    pub fn with_capacity(leaders: usize, followers: usize) -> Self {
        Self {
            leaders: HashMap::with_capacity(leaders),
            followers: HashMap::with_capacity(followers),
        }
    }

    // ========================================================================
    // LEADER REGISTRATION
    // ========================================================================

    /// Registers a leader layer.
    ///
    /// If a leader with the same link already exists, it will be updated.
    pub fn register_leader(
        &mut self,
        link: LayerLink,
        layer_id: LayerId,
        offset: Offset<Pixels>,
        size: Size<Pixels>,
    ) {
        let info = self
            .leaders
            .entry(link)
            .or_insert_with(|| LeaderInfo::new(layer_id, offset, size));
        info.layer_id = layer_id;
        info.offset = offset;
        info.size = size;
    }

    /// Updates the offset and size for an existing leader.
    pub fn update_leader(&mut self, link: LayerLink, offset: Offset<Pixels>, size: Size<Pixels>) {
        if let Some(info) = self.leaders.get_mut(&link) {
            info.offset = offset;
            info.size = size;
        }
    }

    /// Removes a leader and returns its info.
    pub fn unregister_leader(&mut self, link: LayerLink) -> Option<LeaderInfo> {
        self.leaders.remove(&link)
    }

    /// Returns leader info for a link.
    ///
    /// Takes `link` by value because [`LayerLink`] is `Copy` and 8 bytes
    /// — passing by reference would force an extra indirection at the
    /// call site for no semantic benefit. The other LinkRegistry
    /// accessors (`register_leader`, `unregister_leader`) already
    /// took `LayerLink` by value; U18 unifies the signature shape.
    pub fn get_leader(&self, link: LayerLink) -> Option<&LeaderInfo> {
        self.leaders.get(&link)
    }

    /// Returns mutable leader info for a link.
    pub fn get_leader_mut(&mut self, link: LayerLink) -> Option<&mut LeaderInfo> {
        self.leaders.get_mut(&link)
    }

    /// Returns true if a leader with this link exists.
    pub fn has_leader(&self, link: LayerLink) -> bool {
        self.leaders.contains_key(&link)
    }

    // ========================================================================
    // FOLLOWER REGISTRATION
    // ========================================================================

    /// Registers a follower layer.
    ///
    /// Also adds the follower to the leader's follower list if the leader
    /// exists.
    pub fn register_follower(&mut self, follower_id: LayerId, link: LayerLink) {
        self.followers.insert(follower_id, link);

        // Add to leader's follower list if leader exists
        if let Some(leader) = self.leaders.get_mut(&link) {
            leader.add_follower(follower_id);
        }
    }

    /// Removes a follower.
    pub fn unregister_follower(&mut self, follower_id: LayerId) -> Option<LayerLink> {
        if let Some(link) = self.followers.remove(&follower_id) {
            // Remove from leader's follower list
            if let Some(leader) = self.leaders.get_mut(&link) {
                leader.remove_follower(follower_id);
            }
            Some(link)
        } else {
            None
        }
    }

    /// Returns the link for a follower.
    pub fn get_follower_link(&self, follower_id: LayerId) -> Option<LayerLink> {
        self.followers.get(&follower_id).copied()
    }

    /// Returns true if a follower with this ID exists.
    pub fn has_follower(&self, follower_id: LayerId) -> bool {
        self.followers.contains_key(&follower_id)
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    /// Returns all follower LayerIds for a given link.
    pub fn followers_for_link(&self, link: LayerLink) -> &[LayerId] {
        self.leaders
            .get(&link)
            .map_or(&[], |info| info.followers.as_slice())
    }

    /// Returns the leader LayerId for a given link.
    pub fn leader_for_link(&self, link: LayerLink) -> Option<LayerId> {
        self.leaders.get(&link).map(|info| info.layer_id)
    }

    /// Returns the number of registered leaders.
    pub fn leader_count(&self) -> usize {
        self.leaders.len()
    }

    /// Returns the number of registered followers.
    pub fn follower_count(&self) -> usize {
        self.followers.len()
    }

    /// Returns true if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.leaders.is_empty() && self.followers.is_empty()
    }

    // ========================================================================
    // MAINTENANCE
    // ========================================================================

    /// Clears all registrations.
    pub fn clear(&mut self) {
        self.leaders.clear();
        self.followers.clear();
    }

    /// Removes orphaned followers (followers whose leader is not registered).
    ///
    /// Returns the number of removed followers.
    pub fn remove_orphaned_followers(&mut self) -> usize {
        let orphans: Vec<LayerId> = self
            .followers
            .iter()
            .filter(|(_, link)| !self.leaders.contains_key(link))
            .map(|(&id, _)| id)
            .collect();

        let count = orphans.len();
        for id in orphans {
            self.followers.remove(&id);
        }
        count
    }
}

// ============================================================================
// FOLLOWER POSITION RESOLUTION
// ============================================================================

/// Resolves the render-time pixel offset a `Layer::Follower` should be
/// translated by, relative to its OWN current position in the tree walk —
/// i.e. the value `render_layer_recursive` should feed straight into
/// `push_offset`/`pop_transform` at the point it visits `follower_layer_id`
/// (see `crates/flui-engine/src/wgpu/renderer.rs`).
///
/// A translation-only analogue of Flutter's
/// `FollowerLayer._pathsToCommonAncestor`/`_collectTransformForLayerChain`
/// (`layer.dart:2722-2765`): walks the leader's and the follower's ancestor
/// chains in the already-fully-built `tree` to their nearest common
/// ancestor, sums the `Layer::Offset` deltas encountered along each side
/// (the only layer kind FLUI's paint composer ever uses to shift the
/// accumulated coordinate space, `pipeline/owner/paint.rs`'s `scope_layer`
/// doc), and feeds the leader's resulting position — as seen from the
/// follower's own local frame — into [`FollowerLayer::calculate_offset`].
///
/// Returns `None` when the follower must not render its subtree at all:
/// unlinked (no leader currently registered under `follower.link()`) AND
/// `follower.show_when_unlinked() == false` — mirroring oracle's early
/// return in `FollowerLayer.addToScene` (`layer.dart:2857-2865`) before
/// `addChildrenToScene` is ever called. When linked-but-disjoint (leader
/// and follower share no common ancestor — should not happen for a single
/// well-formed frame's tree, but is not a panic-worthy invariant), the same
/// unlinked contract is used defensively rather than fabricating a bogus
/// position.
pub fn resolve_follower_offset(
    tree: &LayerTree,
    registry: &LinkRegistry,
    follower_layer_id: LayerId,
    follower: &FollowerLayer,
) -> Option<Offset<Pixels>> {
    let unlinked_fallback = || {
        follower
            .show_when_unlinked()
            .then_some(follower.target_offset())
    };

    let Some(leader_info) = registry.get_leader(follower.link()) else {
        // No leader currently registered — oracle's dual-purpose `offset`
        // field becomes the plain paint-origin-relative fallback position,
        // NOT routed through `calculate_offset` (which requires a resolved
        // leader pose and has no unlinked code path at all).
        return unlinked_fallback();
    };

    let Some(common_ancestor) = find_common_ancestor(tree, leader_info.layer_id, follower_layer_id)
    else {
        tracing::warn!(
            leader_layer_id = ?leader_info.layer_id,
            ?follower_layer_id,
            "resolve_follower_offset: leader and follower share no common \
             ancestor in the layer tree; falling back to the unlinked contract",
        );
        return unlinked_fallback();
    };

    let leader_chain_offset = translation_to_ancestor(tree, leader_info.layer_id, common_ancestor);
    let follower_chain_offset = translation_to_ancestor(tree, follower_layer_id, common_ancestor);
    // The leader's position as seen from the follower's own local frame:
    // both chains are summed only up to the shared ancestor, so the
    // (identical, cancelling) ancestor-to-root portion is never computed.
    let leader_offset_from_follower =
        leader_info.offset + leader_chain_offset - follower_chain_offset;

    Some(follower.calculate_offset(
        leader_offset_from_follower,
        leader_info.size,
        follower.size(),
    ))
}

/// Returns the nearest common ancestor of `a` and `b` (inclusive of `a`/`b`
/// themselves), or `None` if they do not share one.
fn find_common_ancestor(tree: &LayerTree, a: LayerId, b: LayerId) -> Option<LayerId> {
    let mut ancestors_of_a: HashSet<LayerId> = HashSet::new();
    let mut current = Some(a);
    while let Some(id) = current {
        ancestors_of_a.insert(id);
        current = tree.parent(id);
    }

    let mut current = Some(b);
    while let Some(id) = current {
        if ancestors_of_a.contains(&id) {
            return Some(id);
        }
        current = tree.parent(id);
    }
    None
}

/// Accumulates the translation from `start` up to (and excluding) `ancestor`:
/// every `Layer::Offset`'s offset **plus the translation component of every
/// `Layer::Transform`** on the path. The paint composer does push
/// `Layer::Transform` nodes (e.g. for `paint_transform` / `PushTransform`
/// scopes), so a leader or follower inside a `RenderTransform`/`FittedBox`/flow
/// transform would otherwise be resolved as if that transform did not exist.
///
/// FLUI's follower system is offset-only (`FollowerLayer::calculate_offset`
/// takes an `Offset`), so a transform layer contributes only its translation
/// here — a scale or rotation between leader and follower is not representable
/// and is a documented limitation of the offset-only follower, not a silent
/// drop.
fn translation_to_ancestor(tree: &LayerTree, start: LayerId, ancestor: LayerId) -> Offset<Pixels> {
    let mut total = Offset::ZERO;
    let mut current = Some(start);
    while let Some(id) = current {
        if id == ancestor {
            break;
        }
        let Some(node) = tree.get(id) else { break };
        match node.layer() {
            Layer::Offset(offset_layer) => total += offset_layer.offset(),
            Layer::Transform(transform_layer) => {
                let (tx, ty, _tz) = transform_layer.transform().translation_component();
                total += Offset::new(Pixels::new(tx), Pixels::new(ty));
            }
            _ => {}
        }
        current = node.parent();
    }
    total
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    fn make_link() -> LayerLink {
        LayerLink::new()
    }

    fn make_layer_id(n: usize) -> LayerId {
        LayerId::new(n)
    }

    #[test]
    fn test_link_registry_new() {
        let registry = LinkRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.leader_count(), 0);
        assert_eq!(registry.follower_count(), 0);
    }

    #[test]
    fn test_register_leader() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let layer_id = make_layer_id(1);

        registry.register_leader(
            link,
            layer_id,
            Offset::new(px(10.0), px(20.0)),
            Size::new(px(100.0), px(50.0)),
        );

        assert!(registry.has_leader(link));
        assert_eq!(registry.leader_count(), 1);

        let info = registry.get_leader(link).unwrap();
        assert_eq!(info.layer_id, layer_id);
        assert_eq!(info.offset, Offset::new(px(10.0), px(20.0)));
        assert_eq!(info.size, Size::new(px(100.0), px(50.0)));
    }

    #[test]
    fn test_register_follower() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower_id = make_layer_id(2);

        // Register leader first
        registry.register_leader(
            link,
            leader_id,
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );

        // Register follower
        registry.register_follower(follower_id, link);

        assert!(registry.has_follower(follower_id));
        assert_eq!(registry.follower_count(), 1);
        assert_eq!(registry.get_follower_link(follower_id), Some(link));

        // Follower should be in leader's list
        let info = registry.get_leader(link).unwrap();
        assert!(info.followers.contains(&follower_id));
    }

    #[test]
    fn test_unregister_leader() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let layer_id = make_layer_id(1);

        registry.register_leader(link, layer_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        assert!(registry.has_leader(link));

        let info = registry.unregister_leader(link);
        assert!(info.is_some());
        assert!(!registry.has_leader(link));
    }

    #[test]
    fn test_unregister_follower() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower_id = make_layer_id(2);

        registry.register_leader(
            link,
            leader_id,
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );
        registry.register_follower(follower_id, link);

        let removed_link = registry.unregister_follower(follower_id);
        assert_eq!(removed_link, Some(link));
        assert!(!registry.has_follower(follower_id));

        // Follower should be removed from leader's list
        let info = registry.get_leader(link).unwrap();
        assert!(!info.followers.contains(&follower_id));
    }

    #[test]
    fn test_followers_for_link() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower1 = make_layer_id(2);
        let follower2 = make_layer_id(3);

        registry.register_leader(
            link,
            leader_id,
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );
        registry.register_follower(follower1, link);
        registry.register_follower(follower2, link);

        let registered = registry.followers_for_link(link);
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&follower1));
        assert!(registered.contains(&follower2));
    }

    #[test]
    fn test_update_leader() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let layer_id = make_layer_id(1);

        registry.register_leader(link, layer_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        registry.update_leader(
            link,
            Offset::new(px(200.0), px(300.0)),
            Size::new(px(150.0), px(75.0)),
        );

        let info = registry.get_leader(link).unwrap();
        assert_eq!(info.offset, Offset::new(px(200.0), px(300.0)));
        assert_eq!(info.size, Size::new(px(150.0), px(75.0)));
    }

    #[test]
    fn test_clear() {
        let mut registry = LinkRegistry::new();
        let link = make_link();

        registry.register_leader(
            link,
            make_layer_id(1),
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );
        registry.register_follower(make_layer_id(2), link);

        assert!(!registry.is_empty());

        registry.clear();

        assert!(registry.is_empty());
        assert_eq!(registry.leader_count(), 0);
        assert_eq!(registry.follower_count(), 0);
    }

    #[test]
    fn test_remove_orphaned_followers() {
        let mut registry = LinkRegistry::new();
        let link1 = make_link();
        let link2 = make_link();
        let leader_id = make_layer_id(1);
        let follower1 = make_layer_id(2);
        let follower2 = make_layer_id(3);

        // Register leader with link1
        registry.register_leader(
            link1,
            leader_id,
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );

        // Register follower1 with link1 (has leader)
        registry.register_follower(follower1, link1);

        // Register follower2 with link2 (orphan - no leader)
        registry.followers.insert(follower2, link2);

        assert_eq!(registry.follower_count(), 2);

        let removed = registry.remove_orphaned_followers();

        assert_eq!(removed, 1);
        assert_eq!(registry.follower_count(), 1);
        assert!(registry.has_follower(follower1));
        assert!(!registry.has_follower(follower2));
    }

    #[test]
    fn test_multiple_leaders() {
        let mut registry = LinkRegistry::new();
        let link1 = make_link();
        let link2 = make_link();

        registry.register_leader(
            link1,
            make_layer_id(1),
            Offset::ZERO,
            Size::new(px(100.0), px(50.0)),
        );
        registry.register_leader(
            link2,
            make_layer_id(2),
            Offset::new(px(200.0), px(0.0)),
            Size::new(px(100.0), px(50.0)),
        );

        registry.register_follower(make_layer_id(3), link1);
        registry.register_follower(make_layer_id(4), link1);
        registry.register_follower(make_layer_id(5), link2);

        assert_eq!(registry.leader_count(), 2);
        assert_eq!(registry.follower_count(), 3);
        assert_eq!(registry.followers_for_link(link1).len(), 2);
        assert_eq!(registry.followers_for_link(link2).len(), 1);
    }

    // ========================================================================
    // resolve_follower_offset
    // ========================================================================

    use crate::layer::{FollowerLayer, LeaderLayer, OffsetLayer, TransformLayer};

    #[test]
    fn resolve_follower_offset_linked_same_parent_top_left() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root));

        let link = make_link();
        let leader_id = tree.insert(Layer::Leader(LeaderLayer::new(
            link,
            Size::new(px(20.0), px(20.0)),
        )));
        tree.add_child(root, leader_id);

        let follower = FollowerLayer::new(link).with_size(Size::new(px(10.0), px(10.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.add_child(root, follower_id);

        let mut registry = LinkRegistry::new();
        registry.register_leader(
            link,
            leader_id,
            Offset::new(px(30.0), px(40.0)),
            Size::new(px(20.0), px(20.0)),
        );

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        assert_eq!(resolved, Some(Offset::new(px(30.0), px(40.0))));
    }

    /// The cross-repaint-boundary case that motivated the render-time
    /// resolution design (plan §4): leader and follower sit under two
    /// DIFFERENT `Layer::Offset` ancestors. Resolution must sum both
    /// ancestor chains to their common ancestor (the root), not assume a
    /// shared immediate parent.
    #[test]
    fn resolve_follower_offset_linked_across_offset_boundaries() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root));

        let link = make_link();

        let branch_a = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(100.0),
            px(0.0),
        ))));
        tree.add_child(root, branch_a);
        let leader_id = tree.insert(Layer::Leader(LeaderLayer::new(
            link,
            Size::new(px(20.0), px(20.0)),
        )));
        tree.add_child(branch_a, leader_id);

        let branch_b = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(0.0),
            px(200.0),
        ))));
        tree.add_child(root, branch_b);
        let follower = FollowerLayer::new(link).with_size(Size::new(px(10.0), px(10.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.add_child(branch_b, follower_id);

        let mut registry = LinkRegistry::new();
        // Leader's own registered offset — its accumulated position relative
        // to `branch_a` (its nearest `Layer::Offset` ancestor), NOT root-absolute.
        registry.register_leader(
            link,
            leader_id,
            Offset::new(px(5.0), px(5.0)),
            Size::new(px(20.0), px(20.0)),
        );

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        // Leader absolute: branch_a (100,0) + leader local (5,5) = (105,5).
        // Follower's accumulated position at its own tree slot: branch_b (0,200).
        // Push-offset to apply at the follower's walk point: (105,5) - (0,200)
        // = (105,-195) — feeding the child back to (0,200)+(105,-195) = (105,5),
        // matching the leader's absolute position (default TOP_LEFT anchors,
        // zero target offset).
        assert_eq!(resolved, Some(Offset::new(px(105.0), px(-195.0))));
    }

    /// Regression for the Codex PR-review finding: a leader inside a
    /// `Layer::Transform` (which the paint composer pushes for
    /// `RenderTransform`/`FittedBox`/flow scopes) must have that transform's
    /// translation included in follower resolution. Identical geometry to
    /// `resolve_follower_offset_linked_across_offset_boundaries`, but the
    /// leader's `(100, 0)` ancestor is a translation `Layer::Transform` instead
    /// of a `Layer::Offset` — the resolved offset must be the same. Before the
    /// fix, `translation_to_ancestor` skipped transform layers and this
    /// resolved to `(5, -195)` (the 100px translation dropped).
    #[test]
    fn resolve_follower_offset_includes_transform_layer_translation() {
        let mut tree = LayerTree::new();
        let root = tree.insert(Layer::Offset(OffsetLayer::zero()));
        tree.set_root(Some(root));

        let link = make_link();

        // Leader's ancestor is a translation TRANSFORM layer, not an offset.
        let branch_a = tree.insert(Layer::Transform(TransformLayer::translation(100.0, 0.0)));
        tree.add_child(root, branch_a);
        let leader_id = tree.insert(Layer::Leader(LeaderLayer::new(
            link,
            Size::new(px(20.0), px(20.0)),
        )));
        tree.add_child(branch_a, leader_id);

        let branch_b = tree.insert(Layer::Offset(OffsetLayer::new(Offset::new(
            px(0.0),
            px(200.0),
        ))));
        tree.add_child(root, branch_b);
        let follower = FollowerLayer::new(link).with_size(Size::new(px(10.0), px(10.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.add_child(branch_b, follower_id);

        let mut registry = LinkRegistry::new();
        registry.register_leader(
            link,
            leader_id,
            Offset::new(px(5.0), px(5.0)),
            Size::new(px(20.0), px(20.0)),
        );

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        // Leader absolute: transform translation (100,0) + leader local (5,5) =
        // (105,5); follower slot: (0,200); push-offset (105,5) - (0,200) =
        // (105,-195). Dropping the transform's translation would give (5,-195).
        assert_eq!(resolved, Some(Offset::new(px(105.0), px(-195.0))));
    }

    #[test]
    fn resolve_follower_offset_unlinked_show_when_unlinked_true_uses_target_offset() {
        let mut tree = LayerTree::new();
        let link = make_link();
        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(true)
            .with_target_offset(Offset::new(px(7.0), px(9.0)));
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.set_root(Some(follower_id));

        // No leader registered under `link` at all.
        let registry = LinkRegistry::new();

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        assert_eq!(resolved, Some(Offset::new(px(7.0), px(9.0))));
    }

    #[test]
    fn resolve_follower_offset_unlinked_show_when_unlinked_false_hides_subtree() {
        let mut tree = LayerTree::new();
        let link = make_link();
        let follower = FollowerLayer::new(link).with_show_when_unlinked(false);
        let follower_id = tree.insert(Layer::Follower(follower));
        tree.set_root(Some(follower_id));

        let registry = LinkRegistry::new();

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        assert_eq!(
            resolved, None,
            "show_when_unlinked = false must hide the subtree entirely when unlinked"
        );
    }

    #[test]
    fn resolve_follower_offset_no_common_ancestor_falls_back_to_unlinked_contract() {
        let mut tree = LayerTree::new();
        let link = make_link();

        // Leader and follower both inserted but never attached to any
        // parent — no shared ancestor exists in the tree.
        let leader_id = tree.insert(Layer::Leader(LeaderLayer::new(
            link,
            Size::new(px(20.0), px(20.0)),
        )));
        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(true)
            .with_target_offset(Offset::new(px(3.0), px(4.0)));
        let follower_id = tree.insert(Layer::Follower(follower));

        let mut registry = LinkRegistry::new();
        registry.register_leader(link, leader_id, Offset::ZERO, Size::new(px(20.0), px(20.0)));

        let resolved = resolve_follower_offset(&tree, &registry, follower_id, &follower);
        assert_eq!(
            resolved,
            Some(Offset::new(px(3.0), px(4.0))),
            "a disjoint leader/follower pair must fall back to the unlinked contract, \
             not fabricate a bogus resolved position",
        );
    }
}
