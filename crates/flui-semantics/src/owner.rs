//! SemanticsOwner - Manages the semantics tree lifecycle
//!
//! The SemanticsOwner coordinates updates to the semantics tree and
//! sends updates to the platform accessibility services.

use std::sync::Arc;

use flui_foundation::SemanticsId;
use flui_types::semantics::SemanticsData;

use crate::node::SemanticsNode;
use crate::tree::SemanticsTree;

// ============================================================================
// CALLBACK TYPE
// ============================================================================

/// Callback for semantics updates.
///
/// Called when the semantics tree changes and needs to be sent to the platform.
/// The callback receives a list of changed semantics nodes with their data.
pub type SemanticsUpdateCallback = Arc<dyn Fn(&[SemanticsUpdate]) + Send + Sync>;

// ============================================================================
// SEMANTICS UPDATE
// ============================================================================

/// A single semantics update to send to the platform.
#[derive(Debug, Clone)]
pub struct SemanticsUpdate {
    /// The semantics node ID.
    pub id: SemanticsId,

    /// The semantics data for this node.
    pub data: SemanticsData,

    /// Parent node ID (None for root).
    pub parent: Option<SemanticsId>,

    /// Child node IDs.
    pub children: Vec<SemanticsId>,
}

impl SemanticsUpdate {
    /// Creates a new semantics update.
    pub fn new(id: SemanticsId, data: SemanticsData) -> Self {
        Self {
            id,
            data,
            parent: None,
            children: Vec::new(),
        }
    }

    /// Sets the parent node ID.
    pub fn with_parent(mut self, parent: Option<SemanticsId>) -> Self {
        self.parent = parent;
        self
    }

    /// Sets the child node IDs.
    pub fn with_children(mut self, children: Vec<SemanticsId>) -> Self {
        self.children = children;
        self
    }
}

// ============================================================================
// SEMANTICS OWNER
// ============================================================================

/// Manages the semantics tree lifecycle and platform updates.
///
/// SemanticsOwner is responsible for:
/// 1. Managing the semantics tree
/// 2. Tracking dirty nodes that need updates
/// 3. Flushing updates to the platform accessibility services
///
/// # Flutter Protocol
///
/// Similar to Flutter's `SemanticsOwner`:
/// - Owns the semantics tree for a render tree
/// - Manages update lifecycle (mark dirty → flush)
/// - Sends updates to platform channel
///
/// # Example
///
/// ```rust,ignore
/// use flui_semantics::{SemanticsOwner, SemanticsNode, SemanticsProperties, SemanticsRole};
/// use std::sync::Arc;
///
/// // Create owner with platform callback
/// let callback = Arc::new(|updates: &[SemanticsUpdate]| {
///     for update in updates {
///         println!("Semantics update: {:?}", update.id);
///     }
/// });
/// let mut owner = SemanticsOwner::new(callback);
///
/// // Build semantics tree
/// let node = SemanticsNode::new()
///     .with_properties(
///         SemanticsProperties::new()
///             .with_role(SemanticsRole::Button)
///             .with_label("Submit")
///     );
/// let id = owner.insert(node);
/// owner.set_root(Some(id));
///
/// // Flush updates to platform
/// owner.flush();
/// ```
pub struct SemanticsOwner {
    /// The semantics tree.
    tree: SemanticsTree,

    /// Platform callback for sending updates.
    callback: Option<SemanticsUpdateCallback>,

    /// Whether semantics is enabled.
    enabled: bool,
}

impl std::fmt::Debug for SemanticsOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsOwner")
            .field("tree", &self.tree)
            .field("callback", &self.callback.as_ref().map(|_| "<callback>"))
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl SemanticsOwner {
    /// Creates a new SemanticsOwner with a platform callback.
    pub fn new(callback: SemanticsUpdateCallback) -> Self {
        Self {
            tree: SemanticsTree::new(),
            callback: Some(callback),
            enabled: true,
        }
    }

    /// Creates a new SemanticsOwner without a callback (for testing).
    pub fn new_without_callback() -> Self {
        Self {
            tree: SemanticsTree::new(),
            callback: None,
            enabled: true,
        }
    }

    /// Creates a SemanticsOwner with pre-allocated capacity.
    pub fn with_capacity(capacity: usize, callback: SemanticsUpdateCallback) -> Self {
        Self {
            tree: SemanticsTree::with_capacity(capacity),
            callback: Some(callback),
            enabled: true,
        }
    }

    // ========== Enabled State ==========

    /// Returns whether semantics is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables semantics.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disables semantics.
    ///
    /// When disabled, no updates are sent to the platform.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    // ========== Tree Access ==========

    /// Returns a reference to the semantics tree.
    #[inline]
    pub fn tree(&self) -> &SemanticsTree {
        &self.tree
    }

    /// Returns a mutable reference to the semantics tree.
    #[inline]
    pub fn tree_mut(&mut self) -> &mut SemanticsTree {
        &mut self.tree
    }

    // ========== Root Management ==========

    /// Get the root SemanticsNode ID.
    #[inline]
    pub fn root(&self) -> Option<SemanticsId> {
        self.tree.root()
    }

    /// Set the root SemanticsNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<SemanticsId>) {
        self.tree.set_root(root);
    }

    // ========== Node Management ==========

    /// Inserts a SemanticsNode into the tree.
    pub fn insert(&mut self, node: SemanticsNode) -> SemanticsId {
        self.tree.insert(node)
    }

    /// Returns a reference to a SemanticsNode.
    #[inline]
    pub fn get(&self, id: SemanticsId) -> Option<&SemanticsNode> {
        self.tree.get(id)
    }

    /// Returns a mutable reference to a SemanticsNode.
    #[inline]
    pub fn get_mut(&mut self, id: SemanticsId) -> Option<&mut SemanticsNode> {
        self.tree.get_mut(id)
    }

    /// Removes a SemanticsNode from the tree.
    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        self.tree.remove(id)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.tree.clear();
    }

    /// Disposes of the SemanticsOwner.
    ///
    /// This clears all nodes, removes the callback, and disables semantics.
    /// After calling dispose, the owner should not be used.
    ///
    /// # Flutter Protocol
    ///
    /// Similar to Flutter's `SemanticsOwner.dispose()`:
    /// - Clears the semantics tree
    /// - Removes all listeners
    /// - Releases resources
    pub fn dispose(&mut self) {
        self.tree.clear();
        self.callback = None;
        self.enabled = false;
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent SemanticsNode.
    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        self.tree.add_child(parent_id, child_id);
    }

    /// Removes a child from a parent SemanticsNode.
    pub fn remove_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        self.tree.remove_child(parent_id, child_id);
    }

    // ========== Dirty Tracking ==========

    /// Returns true if any node needs to be sent to the platform.
    pub fn needs_flush(&self) -> bool {
        self.enabled && self.tree.has_dirty_nodes()
    }

    /// Marks a specific node as dirty.
    pub fn mark_dirty(&mut self, id: SemanticsId) {
        if let Some(node) = self.tree.get_mut(id) {
            node.mark_dirty();
        }
    }

    // ========== Flush to Platform ==========

    /// Flushes dirty nodes to the platform.
    ///
    /// This collects all dirty nodes, creates update payloads,
    /// and sends them to the platform callback.
    ///
    /// After flushing, all nodes are marked clean.
    pub fn flush(&mut self) {
        if !self.enabled {
            return;
        }

        // Collect dirty node IDs first
        let dirty_ids: Vec<SemanticsId> = self.tree.dirty_nodes().collect();

        if dirty_ids.is_empty() {
            return;
        }

        // Build updates
        let updates: Vec<SemanticsUpdate> = dirty_ids
            .iter()
            .filter_map(|&id| self.build_update(id))
            .collect();

        // Send to platform
        if let Some(ref callback) = self.callback {
            callback(&updates);
        }

        // Mark all nodes clean
        self.tree.mark_all_clean();
    }

    /// Builds an update for a single node.
    fn build_update(&self, id: SemanticsId) -> Option<SemanticsUpdate> {
        let node = self.tree.get(id)?;

        Some(
            SemanticsUpdate::new(id, node.to_data())
                .with_parent(node.parent())
                .with_children(node.children().to_vec()),
        )
    }

    /// Forces a full tree update.
    ///
    /// Marks all nodes dirty and flushes to platform.
    /// Use when accessibility services reconnect or request full tree.
    pub fn send_full_tree(&mut self) {
        if !self.enabled {
            return;
        }

        // Mark all nodes dirty
        for (_, node) in self.tree.iter_mut() {
            node.mark_dirty();
        }

        // Flush
        self.flush();
    }
}

impl Default for SemanticsOwner {
    fn default() -> Self {
        Self::new_without_callback()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::semantics::{SemanticsProperties, SemanticsRole};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_semantics_owner_new() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            counter_clone.fetch_add(updates.len(), Ordering::SeqCst);
        });

        let owner = SemanticsOwner::new(callback);
        assert!(owner.is_enabled());
        assert!(owner.tree().is_empty());
    }

    #[test]
    fn test_semantics_owner_without_callback() {
        let owner = SemanticsOwner::new_without_callback();
        assert!(owner.is_enabled());
    }

    #[test]
    fn test_semantics_owner_enable_disable() {
        let mut owner = SemanticsOwner::new_without_callback();

        assert!(owner.is_enabled());
        owner.disable();
        assert!(!owner.is_enabled());
        owner.enable();
        assert!(owner.is_enabled());
    }

    #[test]
    fn test_semantics_owner_insert_and_get() {
        let mut owner = SemanticsOwner::new_without_callback();

        let node =
            SemanticsNode::new().with_properties(SemanticsProperties::new().with_label("Test"));
        let id = owner.insert(node);

        let retrieved = owner.get(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().label(), Some("Test"));
    }

    #[test]
    fn test_semantics_owner_tree_operations() {
        let mut owner = SemanticsOwner::new_without_callback();

        let parent_id = owner.insert(SemanticsNode::new());
        let child_id = owner.insert(SemanticsNode::new());

        owner.add_child(parent_id, child_id);

        let parent = owner.get(parent_id).unwrap();
        assert_eq!(parent.children().len(), 1);
        assert_eq!(parent.children()[0], child_id);

        let child = owner.get(child_id).unwrap();
        assert_eq!(child.parent(), Some(parent_id));
    }

    #[test]
    fn test_semantics_owner_root() {
        let mut owner = SemanticsOwner::new_without_callback();

        assert!(owner.root().is_none());

        let id = owner.insert(SemanticsNode::new());
        owner.set_root(Some(id));

        assert_eq!(owner.root(), Some(id));
    }

    #[test]
    fn test_semantics_owner_flush() {
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = Arc::clone(&update_count);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            update_count_clone.fetch_add(updates.len(), Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);

        // Insert some nodes (they start dirty)
        let id1 = owner.insert(
            SemanticsNode::new()
                .with_properties(SemanticsProperties::new().with_role(SemanticsRole::Button)),
        );
        let id2 = owner.insert(
            SemanticsNode::new().with_properties(SemanticsProperties::new().with_label("Child")),
        );
        owner.add_child(id1, id2);
        owner.set_root(Some(id1));

        assert!(owner.needs_flush());

        // Flush should send 2 updates
        owner.flush();

        assert_eq!(update_count.load(Ordering::SeqCst), 2);
        assert!(!owner.needs_flush());
    }

    #[test]
    fn test_semantics_owner_flush_when_disabled() {
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = Arc::clone(&update_count);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            update_count_clone.fetch_add(updates.len(), Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);

        let _ = owner.insert(SemanticsNode::new());
        owner.disable();

        // Should not flush when disabled
        assert!(!owner.needs_flush()); // needs_flush returns false when disabled
        owner.flush();

        assert_eq!(update_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_semantics_owner_send_full_tree() {
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = Arc::clone(&update_count);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            update_count_clone.fetch_add(updates.len(), Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);

        // Insert and flush
        let _ = owner.insert(SemanticsNode::new());
        let _ = owner.insert(SemanticsNode::new());
        owner.flush();
        assert_eq!(update_count.load(Ordering::SeqCst), 2);

        // Send full tree should send all nodes again
        owner.send_full_tree();
        assert_eq!(update_count.load(Ordering::SeqCst), 4); // 2 + 2
    }

    #[test]
    fn test_semantics_owner_mark_dirty() {
        let mut owner = SemanticsOwner::new_without_callback();

        let id = owner.insert(SemanticsNode::new());

        // Initially dirty
        assert!(owner.get(id).unwrap().is_dirty());

        // Flush marks clean
        owner.flush();
        assert!(!owner.get(id).unwrap().is_dirty());

        // Mark dirty again
        owner.mark_dirty(id);
        assert!(owner.get(id).unwrap().is_dirty());
    }

    #[test]
    fn test_semantics_owner_remove() {
        let mut owner = SemanticsOwner::new_without_callback();

        let id = owner.insert(SemanticsNode::new());
        assert!(owner.get(id).is_some());

        let removed = owner.remove(id);
        assert!(removed.is_some());
        assert!(owner.get(id).is_none());
    }

    #[test]
    fn test_semantics_owner_clear() {
        let mut owner = SemanticsOwner::new_without_callback();

        let id = owner.insert(SemanticsNode::new());
        owner.set_root(Some(id));

        assert!(!owner.tree().is_empty());
        assert!(owner.root().is_some());

        owner.clear();

        assert!(owner.tree().is_empty());
        assert!(owner.root().is_none());
    }

    #[test]
    fn test_semantics_update() {
        let data = SemanticsData::new(
            SemanticsProperties::new().with_label("Test"),
            flui_types::geometry::Rect::ZERO,
        );

        let update = SemanticsUpdate::new(SemanticsId::new(1), data)
            .with_parent(Some(SemanticsId::new(2)))
            .with_children(vec![SemanticsId::new(3), SemanticsId::new(4)]);

        assert_eq!(update.id, SemanticsId::new(1));
        assert_eq!(update.parent, Some(SemanticsId::new(2)));
        assert_eq!(update.children.len(), 2);
    }
}
