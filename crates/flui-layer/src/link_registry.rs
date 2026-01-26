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
//! use flui_layer::{LayerTree, LinkRegistry, LeaderLayer, FollowerLayer, LayerLink, Layer};
//! use flui_types::geometry::{Offset, Size};
//!
//! let mut tree = LayerTree::new();
//! let mut registry = LinkRegistry::new();
//!
//! // Create linked layers
//! let link = LayerLink::new();
//! let leader = LeaderLayer::new(link, Size::new(100.0, 30.0));
//! let follower = FollowerLayer::below(link, 5.0);
//!
//! // Insert into tree
//! let leader_id = tree.insert(Layer::Leader(leader));
//! let follower_id = tree.insert(Layer::Follower(follower));
//!
//! // Register in the link registry
//! registry.register_leader(link, leader_id, Offset::new(50.0, 100.0), Size::new(100.0, 30.0));
//! registry.register_follower(follower_id, link);
//!
//! // Query followers for a leader
//! let followers = registry.followers_for_link(&link);
//! ```

use std::collections::HashMap;

use flui_foundation::LayerId;
use flui_types::geometry::{Offset, Pixels, Size};

use crate::layer::LayerLink;

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
    pub fn get_leader(&self, link: &LayerLink) -> Option<&LeaderInfo> {
        self.leaders.get(link)
    }

    /// Returns mutable leader info for a link.
    pub fn get_leader_mut(&mut self, link: &LayerLink) -> Option<&mut LeaderInfo> {
        self.leaders.get_mut(link)
    }

    /// Returns true if a leader with this link exists.
    pub fn has_leader(&self, link: &LayerLink) -> bool {
        self.leaders.contains_key(link)
    }

    // ========================================================================
    // FOLLOWER REGISTRATION
    // ========================================================================

    /// Registers a follower layer.
    ///
    /// Also adds the follower to the leader's follower list if the leader exists.
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
    pub fn followers_for_link(&self, link: &LayerLink) -> &[LayerId] {
        self.leaders
            .get(link)
            .map_or(&[], |info| info.followers.as_slice())
    }

    /// Returns the leader LayerId for a given link.
    pub fn leader_for_link(&self, link: &LayerLink) -> Option<LayerId> {
        self.leaders.get(link).map(|info| info.layer_id)
    }

    /// Returns the leader info for a follower.
    pub fn leader_for_follower(&self, follower_id: LayerId) -> Option<&LeaderInfo> {
        self.followers
            .get(&follower_id)
            .and_then(|link| self.leaders.get(link))
    }

    /// Returns all registered links.
    pub fn links(&self) -> impl Iterator<Item = &LayerLink> {
        self.leaders.keys()
    }

    /// Returns all registered leader infos.
    pub fn leaders(&self) -> impl Iterator<Item = (&LayerLink, &LeaderInfo)> {
        self.leaders.iter()
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

    /// Rebuilds follower lists in all leaders based on current follower registrations.
    ///
    /// Call this after bulk modifications to ensure consistency.
    pub fn rebuild_follower_lists(&mut self) {
        // Clear all follower lists
        for leader in self.leaders.values_mut() {
            leader.followers.clear();
        }

        // Rebuild from follower map
        for (&follower_id, &link) in &self.followers {
            if let Some(leader) = self.leaders.get_mut(&link) {
                leader.followers.push(follower_id);
            }
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

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

        assert!(registry.has_leader(&link));
        assert_eq!(registry.leader_count(), 1);

        let info = registry.get_leader(&link).unwrap();
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
        registry.register_leader(link, leader_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));

        // Register follower
        registry.register_follower(follower_id, link);

        assert!(registry.has_follower(follower_id));
        assert_eq!(registry.follower_count(), 1);
        assert_eq!(registry.get_follower_link(follower_id), Some(link));

        // Follower should be in leader's list
        let info = registry.get_leader(&link).unwrap();
        assert!(info.followers.contains(&follower_id));
    }

    #[test]
    fn test_unregister_leader() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let layer_id = make_layer_id(1);

        registry.register_leader(link, layer_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        assert!(registry.has_leader(&link));

        let info = registry.unregister_leader(link);
        assert!(info.is_some());
        assert!(!registry.has_leader(&link));
    }

    #[test]
    fn test_unregister_follower() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower_id = make_layer_id(2);

        registry.register_leader(link, leader_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        registry.register_follower(follower_id, link);

        let removed_link = registry.unregister_follower(follower_id);
        assert_eq!(removed_link, Some(link));
        assert!(!registry.has_follower(follower_id));

        // Follower should be removed from leader's list
        let info = registry.get_leader(&link).unwrap();
        assert!(!info.followers.contains(&follower_id));
    }

    #[test]
    fn test_followers_for_link() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower1 = make_layer_id(2);
        let follower2 = make_layer_id(3);

        registry.register_leader(link, leader_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        registry.register_follower(follower1, link);
        registry.register_follower(follower2, link);

        let followers = registry.followers_for_link(&link);
        assert_eq!(followers.len(), 2);
        assert!(followers.contains(&follower1));
        assert!(followers.contains(&follower2));
    }

    #[test]
    fn test_leader_for_follower() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower_id = make_layer_id(2);

        registry.register_leader(
            link,
            leader_id,
            Offset::new(px(50.0), px(100.0)),
            Size::new(px(100.0), px(50.0)),
        );
        registry.register_follower(follower_id, link);

        let leader_info = registry.leader_for_follower(follower_id);
        assert!(leader_info.is_some());
        let info = leader_info.unwrap();
        assert_eq!(info.layer_id, leader_id);
        assert_eq!(info.offset, Offset::new(px(50.0), px(100.0)));
    }

    #[test]
    fn test_update_leader() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let layer_id = make_layer_id(1);

        registry.register_leader(link, layer_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));
        registry.update_leader(link, Offset::new(px(200.0), px(300.0)), Size::new(px(150.0), px(75.0)));

        let info = registry.get_leader(&link).unwrap();
        assert_eq!(info.offset, Offset::new(px(200.0), px(300.0)));
        assert_eq!(info.size, Size::new(px(150.0), px(75.0)));
    }

    #[test]
    fn test_clear() {
        let mut registry = LinkRegistry::new();
        let link = make_link();

        registry.register_leader(link, make_layer_id(1), Offset::ZERO, Size::new(px(100.0), px(50.0)));
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
        registry.register_leader(link1, leader_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));

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
    fn test_rebuild_follower_lists() {
        let mut registry = LinkRegistry::new();
        let link = make_link();
        let leader_id = make_layer_id(1);
        let follower1 = make_layer_id(2);
        let follower2 = make_layer_id(3);

        // Register leader
        registry.register_leader(link, leader_id, Offset::ZERO, Size::new(px(100.0), px(50.0)));

        // Manually add followers to map (simulating deserialization or corruption)
        registry.followers.insert(follower1, link);
        registry.followers.insert(follower2, link);

        // Leader doesn't know about followers yet
        assert!(registry.get_leader(&link).unwrap().followers.is_empty());

        // Rebuild
        registry.rebuild_follower_lists();

        // Now leader knows about followers
        let info = registry.get_leader(&link).unwrap();
        assert_eq!(info.followers.len(), 2);
        assert!(info.followers.contains(&follower1));
        assert!(info.followers.contains(&follower2));
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
        assert_eq!(registry.followers_for_link(&link1).len(), 2);
        assert_eq!(registry.followers_for_link(&link2).len(), 1);
    }
}
