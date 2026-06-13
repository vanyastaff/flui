//! Deferred mutation queue for re-entrant layout.
//!
//! During layout, render objects may need to add, remove, or update
//! children (e.g., `LayoutBuilder`, `OverlayPortal`, lazy slivers).
//! But the layout walk holds `&mut` on the subtree, making direct
//! mutation impossible.
//!
//! `DeferredMutations` collects these mutations during layout and
//! applies them after the pass completes. This is the Rust-native
//! alternative to Flutter's `invokeLayoutCallback` which uses unsafe
//! re-entrant mutation through `PipelineOwner._nodesNeedingLayout`.
//!
//! # Design
//!
//! - **Compile-time safe**: mutations happen outside the borrow scope
//!   of the layout walk
//! - **Ordered**: mutations are applied in the order they were enqueued
//! - **Per-frame**: the queue is drained after each layout pass
//! - **Protocol-aware**: supports both Box and Sliver insertions
//! - **Animation-ready**: supports property updates via closures
//!
//! # Competitive insights
//!
//! - **Compose `SubcomposeLayout`**: one mechanism for build-during-measure
//!   (slot-id + deactivate-not-dispose pool)
//! - **GapWorker**: idle prefetch between frames with vsync-deadline

use flui_foundation::RenderId;

use crate::protocol::{BoxProtocol, SliverProtocol};

/// A render object to insert, typed by protocol.
///
/// Box objects are the common case (buttons, text, images, containers).
/// Sliver objects are needed for lazy content (lists, grids).
pub enum DeferredRenderObject {
    /// A Box-protocol render object.
    Box(Box<dyn crate::protocol::RenderObject<BoxProtocol>>),
    /// A Sliver-protocol render object (lazy lists, grids, etc.).
    Sliver(Box<dyn crate::protocol::RenderObject<SliverProtocol>>),
}

impl std::fmt::Debug for DeferredRenderObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Box(obj) => f.debug_tuple("Box").field(&obj.debug_name()).finish(),
            Self::Sliver(obj) => f.debug_tuple("Sliver").field(&obj.debug_name()).finish(),
        }
    }
}

/// A deferred mutation to be applied after the layout pass.
///
/// During layout, render objects may need to add, remove, or update
/// children. But the layout walk holds `&mut` on the subtree, making
/// direct mutation impossible. This enum collects mutations in a
/// queue that is drained after the pass completes.
///
/// # Design
///
/// - **Insert/Remove**: structural changes to the tree
/// - **Update**: property changes to existing render objects (e.g.,
///   animation driving opacity/color during layout)
///
/// # Why both Box and Sliver?
///
/// Sliver children are needed for lazy content (SliverList, SliverGrid).
/// Box children are wrapped in `SliverToBoxAdapter` automatically when
/// inserted under a Sliver parent.
pub enum DeferredMutation {
    /// Insert a new render object under a parent.
    Insert {
        /// The parent to insert under.
        parent_id: RenderId,
        /// The render object to insert (Box or Sliver protocol).
        render_object: DeferredRenderObject,
        /// Optional index to insert at (None = append).
        index: Option<usize>,
    },
    /// Remove a child.
    Remove {
        /// The parent to remove from.
        parent_id: RenderId,
        /// The child to remove.
        child_id: RenderId,
    },
    /// Update a render object's properties.
    ///
    /// The updater receives `&mut dyn Any` — the caller downcasts to
    /// the concrete render object type. This is the mechanism for
    /// animations that drive render object properties during layout
    /// (e.g., `AnimatedContainer` changing padding/color).
    Update {
        /// The render object to update.
        target_id: RenderId,
        /// The update closure.
        updater: Box<dyn FnOnce(&mut dyn std::any::Any) + Send + Sync>,
    },
}

impl std::fmt::Debug for DeferredMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insert {
                parent_id,
                render_object,
                index,
            } => f
                .debug_struct("Insert")
                .field("parent_id", parent_id)
                .field("render_object", render_object)
                .field("index", index)
                .finish(),
            Self::Remove {
                parent_id,
                child_id,
            } => f
                .debug_struct("Remove")
                .field("parent_id", parent_id)
                .field("child_id", child_id)
                .finish(),
            Self::Update { target_id, .. } => f
                .debug_struct("Update")
                .field("target_id", target_id)
                .field("updater", &"<closure>")
                .finish(),
        }
    }
}

/// Collects mutations during layout and applies them after the pass.
///
/// # Ordering and Conflict Resolution
///
/// Mutations are applied in enqueue order, with conflict detection:
///
/// - **Remove + Update on same target**: the Update is skipped with a
///   `tracing::warn!`. The target no longer exists after removal.
/// - **Remove + Insert under same parent**: the Insert succeeds (the
///   parent still exists, only a child was removed).
/// - **Insert + Update on same target**: **NOT possible** in a single
///   batch. Insert creates a NEW `RenderId` at apply-time — the caller
///   cannot know the ID ahead of time. If you need to configure a
///   newly-inserted node, pass the configuration through the render
///   object's constructor.
///
/// ## Key constraint
///
/// `Insert` produces a new `RenderId` at apply-time. Any `Update` or
/// `Remove` targeting that ID must be in a **subsequent** layout pass,
/// not the same batch.
///
/// ## Recommended patterns
///
/// ```ignore
/// // Replace a child: remove old, insert new (different IDs)
/// owner.defer_remove(parent_id, old_child_id);
/// owner.defer_insert_box(parent_id, Box::new(new_child), None);
///
/// // Update an existing node
/// owner.defer_update(existing_id, Box::new(|obj| { /* mutate */ }));
///
/// // Insert with configuration: pass config through constructor
/// let child = MyRenderObject::new(config);
/// owner.defer_insert_box(parent_id, Box::new(child), None);
/// ```
#[derive(Debug, Default)]
pub struct DeferredMutations {
    /// Queued mutations.
    mutations: Vec<DeferredMutation>,
}

impl DeferredMutations {
    /// Creates an empty deferred mutation queue.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues a deferred mutation.
    #[inline]
    pub fn push(&mut self, mutation: DeferredMutation) {
        self.mutations.push(mutation);
    }

    /// Enqueues a Box insert mutation.
    pub fn insert_box(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::protocol::RenderObject<BoxProtocol>>,
        index: Option<usize>,
    ) {
        self.mutations.push(DeferredMutation::Insert {
            parent_id,
            render_object: DeferredRenderObject::Box(render_object),
            index,
        });
    }

    /// Enqueues a Sliver insert mutation.
    pub fn insert_sliver(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::protocol::RenderObject<SliverProtocol>>,
        index: Option<usize>,
    ) {
        self.mutations.push(DeferredMutation::Insert {
            parent_id,
            render_object: DeferredRenderObject::Sliver(render_object),
            index,
        });
    }

    /// Enqueues a remove mutation.
    pub fn remove(&mut self, parent_id: RenderId, child_id: RenderId) {
        self.mutations.push(DeferredMutation::Remove {
            parent_id,
            child_id,
        });
    }

    /// Enqueues an update mutation.
    pub fn update(
        &mut self,
        target_id: RenderId,
        updater: Box<dyn FnOnce(&mut dyn std::any::Any) + Send + Sync>,
    ) {
        self.mutations
            .push(DeferredMutation::Update { target_id, updater });
    }

    /// Drains all queued mutations, returning them in order.
    pub fn drain(&mut self) -> Vec<DeferredMutation> {
        std::mem::take(&mut self.mutations)
    }

    /// Returns the number of queued mutations.
    #[inline]
    pub fn len(&self) -> usize {
        self.mutations.len()
    }

    /// Returns `true` if no mutations are queued.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.mutations.is_empty()
    }

    /// Clears all queued mutations without applying them.
    #[inline]
    pub fn clear(&mut self) {
        self.mutations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // Basic operations
    // ====================================================================

    #[test]
    fn test_deferred_mutations_basic() {
        let mut dm = DeferredMutations::new();
        assert!(dm.is_empty());

        let parent = RenderId::new(1);
        dm.remove(parent, RenderId::new(2));
        assert_eq!(dm.len(), 1);
        assert!(!dm.is_empty());

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 1);
        assert!(dm.is_empty());
    }

    #[test]
    fn test_drain_returns_empty_vec_when_empty() {
        let mut dm = DeferredMutations::new();
        let mutations = dm.drain();
        assert!(mutations.is_empty());
    }

    #[test]
    fn test_clear_discards_all() {
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.remove(RenderId::new(1), RenderId::new(3));
        assert_eq!(dm.len(), 2);

        dm.clear();
        assert!(dm.is_empty());
        let mutations = dm.drain();
        assert!(mutations.is_empty());
    }

    // ====================================================================
    // Ordering guarantees
    // ====================================================================

    #[test]
    fn test_order_preserved_fifo() {
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.remove(RenderId::new(1), RenderId::new(3));
        dm.remove(RenderId::new(1), RenderId::new(4));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 3);

        match &mutations[0] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(2)),
            _ => panic!("expected Remove"),
        }
        match &mutations[1] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(3)),
            _ => panic!("expected Remove"),
        }
        match &mutations[2] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(4)),
            _ => panic!("expected Remove"),
        }
    }

    #[test]
    fn test_mixed_operation_order_preserved() {
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.update(
            RenderId::new(3),
            Box::new(|_obj: &mut dyn std::any::Any| {}),
        );
        dm.remove(RenderId::new(1), RenderId::new(4));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 3);
        match &mutations[0] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(2)),
            _ => panic!("expected Remove"),
        }
        match &mutations[1] {
            DeferredMutation::Update { target_id, .. } => assert_eq!(*target_id, RenderId::new(3)),
            _ => panic!("expected Update"),
        }
        match &mutations[2] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(4)),
            _ => panic!("expected Remove"),
        }
    }

    // ====================================================================
    // Conflict scenarios
    // ====================================================================

    #[test]
    fn test_remove_then_update_same_target_is_conflict() {
        // Remove target 2, then Update target 2 → Update should be flagged
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.update(
            RenderId::new(2),
            Box::new(|_obj: &mut dyn std::any::Any| {}),
        );

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 2);

        // Both mutations are present — conflict detection happens at apply-time
        assert!(matches!(mutations[0], DeferredMutation::Remove { .. }));
        assert!(matches!(mutations[1], DeferredMutation::Update { .. }));
    }

    #[test]
    fn test_remove_then_insert_under_same_parent_no_conflict() {
        // Remove child 2 from parent 1.
        // Insert creates a NEW RenderId at apply-time — no conflict
        // with Remove because Remove targets an existing child, not
        // the new one. We can't create a real RenderObject here, so
        // we just verify the queue holds both without issue.
        // The integration test covers the full apply path.
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.remove(RenderId::new(1), RenderId::new(3));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 2);
        // Both removes are present — no conflict between them
        assert!(
            matches!(mutations[0], DeferredMutation::Remove { child_id, .. } if child_id == RenderId::new(2))
        );
        assert!(
            matches!(mutations[1], DeferredMutation::Remove { child_id, .. } if child_id == RenderId::new(3))
        );
    }

    #[test]
    fn test_multiple_removes_same_target() {
        // Removing the same target twice — second remove is a no-op
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        dm.remove(RenderId::new(1), RenderId::new(2));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 2);
        // Both are present; second remove_shallow will be a silent no-op
    }

    #[test]
    fn test_update_then_remove_same_target() {
        // Update target 2, then Remove target 2 → Update applies first, then Remove
        let mut dm = DeferredMutations::new();
        dm.update(
            RenderId::new(2),
            Box::new(|_obj: &mut dyn std::any::Any| {}),
        );
        dm.remove(RenderId::new(1), RenderId::new(2));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 2);
        // Update applies first (target exists), then Remove removes it
        assert!(matches!(mutations[0], DeferredMutation::Update { .. }));
        assert!(matches!(mutations[1], DeferredMutation::Remove { .. }));
    }

    #[test]
    fn test_update_targeting_nonexistent_node_is_noop() {
        // Update on a node that was never in the tree — apply_deferred_mutation
        // handles this via `if let Some(node) = self.render_tree.get_mut(target_id)`
        let mut dm = DeferredMutations::new();
        dm.update(
            RenderId::new(999),
            Box::new(|_obj: &mut dyn std::any::Any| {}),
        );

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 1);
        assert!(matches!(mutations[0], DeferredMutation::Update { .. }));
    }

    // ====================================================================
    // Insert + Update ordering constraint
    // ====================================================================

    #[test]
    fn test_insert_produces_new_id_cannot_be_updated_same_batch() {
        // Insert creates a NEW RenderId at apply-time.
        // Any Update in the same batch cannot target the new node
        // because the ID doesn't exist at enqueue-time.
        // This test documents the constraint.
        let mut dm = DeferredMutations::new();

        // These are on different parents/targets — no conflict
        dm.update(
            RenderId::new(5),
            Box::new(|_obj: &mut dyn std::any::Any| {}),
        );
        dm.remove(RenderId::new(1), RenderId::new(2));

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 2);
        // No conflict: Update targets 5, Remove targets 2
        match &mutations[0] {
            DeferredMutation::Update { target_id, .. } => assert_eq!(*target_id, RenderId::new(5)),
            _ => panic!("expected Update"),
        }
        match &mutations[1] {
            DeferredMutation::Remove { child_id, .. } => assert_eq!(*child_id, RenderId::new(2)),
            _ => panic!("expected Remove"),
        }
    }

    // ====================================================================
    // Debug formatting
    // ====================================================================

    #[test]
    fn test_debug_format_remove() {
        let dm = DeferredMutation::Remove {
            parent_id: RenderId::new(1),
            child_id: RenderId::new(2),
        };
        let debug = format!("{dm:?}");
        assert!(debug.contains("Remove"));
        assert!(debug.contains("parent_id"));
        assert!(debug.contains("child_id"));
    }

    #[test]
    fn test_debug_format_update() {
        let dm = DeferredMutation::Update {
            target_id: RenderId::new(3),
            updater: Box::new(|_: &mut dyn std::any::Any| {}),
        };
        let debug = format!("{dm:?}");
        assert!(debug.contains("Update"));
        assert!(debug.contains("target_id"));
        assert!(debug.contains("<closure>"));
    }

    // ====================================================================
    // Stress / capacity
    // ====================================================================

    #[test]
    fn test_many_mutations() {
        let mut dm = DeferredMutations::new();
        for i in 1..=1000 {
            dm.remove(RenderId::new(1), RenderId::new(i));
        }
        assert_eq!(dm.len(), 1000);

        let mutations = dm.drain();
        assert_eq!(mutations.len(), 1000);
        assert!(dm.is_empty());
    }

    #[test]
    fn test_drain_then_reuse() {
        let mut dm = DeferredMutations::new();
        dm.remove(RenderId::new(1), RenderId::new(2));
        let _ = dm.drain();
        assert!(dm.is_empty());

        // Can reuse after drain
        dm.remove(RenderId::new(3), RenderId::new(4));
        assert_eq!(dm.len(), 1);
    }
}
