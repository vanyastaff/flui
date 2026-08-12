//! SemanticsOwner - Manages the semantics tree lifecycle
//!
//! The SemanticsOwner coordinates updates to the semantics tree and
//! sends updates to the platform accessibility services.

use std::sync::Arc;

use flui_foundation::SemanticsId;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use thiserror::Error;

use crate::{
    action::{ActionArgs, SemanticsAction, SemanticsActionHandler, SemanticsActionRequest},
    identity::AccessibilityNodeId,
    node::SemanticsNode,
    snapshot::{SemanticsSnapshot, SemanticsSnapshotError},
    tree::SemanticsTree,
    update::SemanticsNodeData,
};

// ============================================================================
// CALLBACK TYPE
// ============================================================================

/// Callback for semantics updates.
///
/// Called when the semantics tree changes and needs to be sent to the platform.
///
/// The payload is a [`TreeUpdate`](crate::TreeUpdate) — the shape a platform
/// accessibility adapter consumes directly — rather than FLUI's own node type.
/// Two reasons, and the second is structural:
///
/// - The platform capability speaks accesskit types only, because
///   `flui-platform` (layer 2) cannot depend on `flui-semantics` (layer 3).
///   Translating on the producing side is what keeps that edge absent.
/// - [`SemanticsNodeUpdate`] carries `SemanticsId`s, which are arena positions
///   in a tree the pipeline rebuilds every pass. It cannot express the stable
///   `AccessibilityNodeId` an adapter must publish and route actions back
///   through — see [`crate::tree_to_update`].
pub type SemanticsUpdateCallback = Arc<dyn Fn(&crate::TreeUpdate) + Send + Sync>;

// ============================================================================
// ACTION RESOLUTION
// ============================================================================

/// Why an accessibility action could not be resolved against the current tree.
///
/// Platform routers intentionally treat these outcomes as graceful drops:
/// assistive technologies may act on an older snapshot after a node has been
/// removed or its actions changed. Keeping the reason typed makes that
/// forgiving behavior observable without turning stale platform input into a
/// panic.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SemanticsActionError {
    /// No rooted semantics result is currently available.
    #[error("no rooted semantics tree is available")]
    SemanticsUnavailable,

    /// The stable identity from the platform snapshot is no longer live.
    #[error("accessibility node {node_id} is no longer present")]
    NodeNotFound {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// A malformed tree exposes the same stable identity more than once.
    #[error("accessibility node identity {node_id} resolves to multiple live nodes")]
    AmbiguousNode {
        /// Duplicated platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// The current node no longer exposes the requested action.
    #[error("accessibility node {node_id} does not expose action {action:?}")]
    UnsupportedAction {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
        /// Action absent from the node's effective action mask.
        action: SemanticsAction,
    },

    /// The owning presentation has begun or completed teardown; actions are
    /// refused regardless of whether the node itself still resolves.
    #[error("presentation is closing or closed; accessibility action refused")]
    PresentationClosed,
}

/// A resolved action whose handler has been cloned out of the semantics tree.
///
/// Resolution and invocation are deliberately separate. A caller may resolve
/// this value while holding an outer `PipelineOwner` lock, release that lock,
/// and only then call [`Self::invoke`]. Reentrant handlers therefore cannot
/// deadlock by reaching back into the render pipeline.
#[must_use = "resolved semantics actions must be invoked or intentionally dropped"]
pub struct SemanticsActionInvocation {
    node_id: AccessibilityNodeId,
    action: SemanticsAction,
    arguments: Option<ActionArgs>,
    handler: SemanticsActionHandler,
}

impl std::fmt::Debug for SemanticsActionInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SemanticsActionInvocation")
            .field("node_id", &self.node_id)
            .field("action", &self.action)
            .field("arguments", &self.arguments)
            .field("handler", &"<callback>")
            .finish()
    }
}

impl SemanticsActionInvocation {
    /// Stable identity of the node whose handler was resolved.
    #[inline]
    #[must_use]
    pub const fn node_id(&self) -> AccessibilityNodeId {
        self.node_id
    }

    /// Action passed to the handler.
    #[inline]
    #[must_use]
    pub const fn action(&self) -> SemanticsAction {
        self.action
    }

    /// Invoke the cloned handler.
    ///
    /// No semantics-tree borrow is held while user code runs.
    pub fn invoke(self) {
        (self.handler)(self.action, self.arguments);
    }
}

// ============================================================================
// SEMANTICS NODE UPDATE
// ============================================================================

/// A single semantics node update to send to the platform.
///
/// This represents an update for one node in the semantics tree,
/// including its data, parent reference, and children.
///
/// See also [`SemanticsTreeUpdate`](crate::update::SemanticsTreeUpdate) for
/// batched tree-level updates.
#[derive(Debug, Clone)]
pub struct SemanticsNodeUpdate {
    /// The semantics node ID.
    pub id: SemanticsId,

    /// The semantics data for this node.
    pub data: SemanticsNodeData,

    /// Parent node ID (None for root).
    pub parent: Option<SemanticsId>,

    /// Child node IDs.
    pub children: Vec<SemanticsId>,
}

impl SemanticsNodeUpdate {
    /// Creates a new semantics node update.
    pub fn new(id: SemanticsId, data: SemanticsNodeData) -> Self {
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
/// 4. Producing immutable full-tree snapshots for adapter handoff
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
/// let callback = Arc::new(|updates: &[SemanticsNodeUpdate]| {
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
    ///
    /// **Testing only** — gated on `#[cfg(any(test, feature = "testing"))]`.
    /// Production code constructs through [`Self::new`] which requires a
    /// platform callback; a no-callback owner is a scaffolding-only
    /// convenience.
    #[cfg(any(test, feature = "testing"))]
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

    /// Replaces the platform callback on a live owner.
    ///
    /// For the composition root that learns its platform bridge *after* this
    /// owner was created — an owner lazily constructed by the pipeline on
    /// enablement, wired to the real adapter once one exists. Updates flushed
    /// from here on reach the new callback; nothing already published is
    /// re-sent (the next [`Self::flush`] carries the full rooted tree
    /// anyway, since translation is snapshot-shaped, not delta-shaped).
    pub fn set_callback(&mut self, callback: SemanticsUpdateCallback) {
        self.callback = Some(callback);
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

    /// The current tree as an AccessKit `TreeUpdate`.
    ///
    /// This is the shape both an OS accessibility adapter and a query-by-role
    /// test harness consume, so publishing and asserting cannot drift apart:
    /// what a screen reader is told is exactly what a test can inspect.
    ///
    /// `focus` names the node the platform should treat as focused; a `None`,
    /// or an id not present in the tree, falls back to the root, because
    /// AccessKit requires a valid focus target.
    ///
    /// Returns `None` before the first assembly pass, when the tree has no
    /// root and no applicable update exists.
    #[must_use]
    pub fn to_accesskit_tree_update(
        &self,
        focus: Option<SemanticsId>,
    ) -> Option<accesskit::TreeUpdate> {
        crate::accesskit_translation::tree_to_update(&self.tree, focus)
    }

    /// Returns a mutable reference to the semantics tree.
    #[inline]
    pub fn tree_mut(&mut self) -> &mut SemanticsTree {
        &mut self.tree
    }

    /// Builds an owned, callback-free snapshot of the complete rooted tree.
    ///
    /// Unlike the legacy dirty-node callback, this path never derives an
    /// external identifier from the rebuild-local [`SemanticsId`]. Every node
    /// must carry the generational render identity of the boundary that formed
    /// it, otherwise a typed [`SemanticsSnapshotError`] is returned.
    pub fn snapshot(&self) -> Result<SemanticsSnapshot, SemanticsSnapshotError> {
        SemanticsSnapshot::from_tree(&self.tree)
    }

    /// Resolves a platform request against the current rooted semantics tree.
    ///
    /// The lookup uses the stable accessibility identity exported in the
    /// latest snapshot. It never interprets that value as a rebuild-local
    /// [`SemanticsId`]. Only effective actions are routable, so
    /// `blocks_user_actions` applies identically to snapshot export and input
    /// dispatch.
    ///
    /// The returned invocation owns an `Arc` clone of the handler and may be
    /// invoked after any outer owner lock has been released.
    pub fn resolve_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<SemanticsActionInvocation, SemanticsActionError> {
        let root = self
            .tree
            .root()
            .ok_or(SemanticsActionError::SemanticsUnavailable)?;

        // Traverse only the rooted result: orphaned arena entries were never
        // exported and must not remain actionable. The visited set also makes
        // malformed repeated edges/cycles finite; snapshot validation reports
        // those structural errors separately.
        let mut pending = SmallVec::<[SemanticsId; 32]>::new();
        let mut visited = FxHashSet::default();
        let mut resolved = None;
        pending.push(root);

        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.tree.get(id) else {
                continue;
            };
            pending.extend(node.children().iter().rev().copied());

            if node.accessibility_id() != Some(request.node_id) {
                continue;
            }
            if resolved.is_some() {
                return Err(SemanticsActionError::AmbiguousNode {
                    node_id: request.node_id,
                });
            }
            resolved = Some(node);
        }

        let node = resolved.ok_or(SemanticsActionError::NodeNotFound {
            node_id: request.node_id,
        })?;
        let action_is_effective =
            node.config().effective_actions_as_bits() & request.action.value() != 0;
        let Some(handler) = action_is_effective
            .then(|| node.config().action_handler(request.action))
            .flatten()
            .map(Arc::clone)
        else {
            return Err(SemanticsActionError::UnsupportedAction {
                node_id: request.node_id,
                action: request.action,
            });
        };

        Ok(SemanticsActionInvocation {
            node_id: request.node_id,
            action: request.action,
            arguments: request.arguments,
            handler,
        })
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

    /// Removes a SemanticsNode from the tree (cascades to all descendants).
    ///
    /// Routes through the unified [`TreeWrite::remove`](flui_tree::TreeWrite::remove)
    /// contract (cascade by default). For non-cascading removal,
    /// reach into [`SemanticsTree::remove_shallow`](crate::tree::SemanticsTree::remove_shallow) via
    /// [`Self::tree`] / [`Self::tree_mut`].
    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        use flui_tree::TreeWrite;
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

    /// Publishes the tree to the platform when anything has changed.
    ///
    /// Assembly is a classic full rebuild (ADR-0014), so the published
    /// [`accesskit::TreeUpdate`] carries every node rather than a diff. That is
    /// what `TreeUpdate` permits but does not require; adapters suppress
    /// extraneous events, so this is correct-but-chatty. Incremental diffing
    /// needs the identity to be carried per node and gets its own oracle.
    ///
    /// A clean tree publishes nothing and performs no translation. The trigger
    /// is [`SemanticsTree::has_dirty_nodes`], which scans until it finds dirt:
    /// O(1) average on a dirty tree (it stops at the first dirty node), O(n)
    /// worst case — and the worst case is the *idle* frame, since a clean tree
    /// is the one that must be scanned to the end. `n` is the number of
    /// semantics boundaries, not widgets. A cached dirty count would make this
    /// O(1) unconditionally; that is a separate change, with a benchmark, not
    /// an assertion made in a doc comment.
    pub fn flush(&mut self) {
        if !self.enabled || !self.tree.has_dirty_nodes() {
            return;
        }

        // Translate before touching the callback so the borrow of `self.tree`
        // ends first.
        let Some(update) = crate::tree_to_update(&self.tree, None) else {
            // No root yet — nothing an adapter could apply. Leave the tree
            // dirty so the next flush retries once assembly has rooted it,
            // rather than silently swallowing the first real update.
            //
            // A tree that *loses* its root after publishing takes this path
            // too, and nothing withdraws the tree already sent: the adapter
            // keeps presenting it. Withdrawal is the adapter's deactivation
            // signal rather than an empty update — `TreeUpdate` has no
            // representation for "no tree" — so it is defined with the
            // adapter lifecycle rather than invented here without one. See
            // the teardown item in the Linux bridge issue.
            return;
        };

        // Clone-and-release: cloning the `Arc` out of `self.callback`
        // decouples the invocation from any lock the owner may hold, so the
        // callback never runs while one is held.
        let callback = self.callback.as_ref().map(Arc::clone);
        if let Some(callback) = callback {
            callback(&update);
        }

        self.tree.mark_all_clean();
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

#[cfg(any(test, feature = "testing"))]
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::RenderId;
    use parking_lot::Mutex;

    use super::*;
    use crate::{AccessibilityNodeId, SemanticsActionRequest};

    #[test]
    fn test_semantics_owner_new() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            counter_clone.fetch_add(updates.nodes.len(), Ordering::SeqCst);
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

        let mut node = SemanticsNode::new();
        node.config_mut().set_label("Test");
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

    /// A production-shaped node. Assembly always attaches the boundary's render
    /// object (`rebuild_semantics_owner`), and that is where the OS-facing
    /// identity comes from — a node without one is not publishable at all.
    fn addressable(index: u32) -> SemanticsNode {
        SemanticsNode::new().with_source_render_id(flui_foundation::RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(1).expect("fixture generation is non-zero"),
        ))
    }

    #[test]
    fn test_semantics_owner_flush() {
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = Arc::clone(&update_count);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            update_count_clone.fetch_add(updates.nodes.len(), Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);

        // Insert some nodes (they start dirty)
        let mut node1 = addressable(1);
        node1.config_mut().set_button(true);
        let id1 = owner.insert(node1);

        let mut node2 = addressable(2);
        node2.config_mut().set_label("Child");
        let id2 = owner.insert(node2);

        owner.add_child(id1, id2);
        owner.set_root(Some(id1));

        assert!(owner.needs_flush());

        // The published update carries the whole tree, not a per-node diff.
        owner.flush();

        assert_eq!(update_count.load(Ordering::SeqCst), 2);
        assert!(!owner.needs_flush());
    }

    #[test]
    fn test_semantics_owner_flush_when_disabled() {
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = Arc::clone(&update_count);

        let callback: SemanticsUpdateCallback = Arc::new(move |updates| {
            update_count_clone.fetch_add(updates.nodes.len(), Ordering::SeqCst);
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
            update_count_clone.fetch_add(updates.nodes.len(), Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);

        // Insert, root, and flush. A rooted tree is required: the payload is a
        // `TreeUpdate`, and an unrooted tree has nothing an adapter can apply.
        let root = owner.insert(addressable(1));
        let child = owner.insert(addressable(2));
        owner.add_child(root, child);
        owner.set_root(Some(root));
        owner.flush();
        assert_eq!(update_count.load(Ordering::SeqCst), 2);

        // Send full tree should send all nodes again
        owner.send_full_tree();
        assert_eq!(update_count.load(Ordering::SeqCst), 4); // 2 + 2
    }

    /// An unrooted tree has nothing an adapter could apply, and inventing a root
    /// would publish a tree the application does not have. The dirt is
    /// deliberately **retained** so the first real update is not swallowed once
    /// assembly roots the tree — dropping it here would lose a frame's content
    /// permanently, with no later event to recover it.
    #[test]
    fn an_unrooted_tree_publishes_nothing_and_stays_dirty() {
        let published = Arc::new(AtomicUsize::new(0));
        let published_clone = Arc::clone(&published);
        let callback: SemanticsUpdateCallback = Arc::new(move |_| {
            published_clone.fetch_add(1, Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);
        let id = owner.insert(addressable(1));

        owner.flush();

        assert_eq!(published.load(Ordering::SeqCst), 0);
        assert!(
            owner.get(id).expect("node is live").is_dirty(),
            "the update must survive to the flush that follows rooting"
        );

        owner.set_root(Some(id));
        owner.flush();
        assert_eq!(published.load(Ordering::SeqCst), 1);
    }

    /// A clean tree costs one flag test: no translation, no callback.
    #[test]
    fn a_clean_tree_publishes_nothing() {
        let published = Arc::new(AtomicUsize::new(0));
        let published_clone = Arc::clone(&published);
        let callback: SemanticsUpdateCallback = Arc::new(move |_| {
            published_clone.fetch_add(1, Ordering::SeqCst);
        });

        let mut owner = SemanticsOwner::new(callback);
        let id = owner.insert(addressable(1));
        owner.set_root(Some(id));

        owner.flush();
        assert_eq!(published.load(Ordering::SeqCst), 1);

        // Nothing changed since.
        owner.flush();
        assert_eq!(
            published.load(Ordering::SeqCst),
            1,
            "an idle frame must not republish"
        );
    }

    /// The published ids are the stable space actions come back in, all the way
    /// through the owner's own publish path — not just the raw translation.
    #[test]
    fn published_ids_are_stable_accessibility_ids() {
        let seen: Arc<parking_lot::Mutex<Vec<u64>>> = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen_clone = Arc::clone(&seen);
        let callback: SemanticsUpdateCallback = Arc::new(move |update| {
            *seen_clone.lock() = update.nodes.iter().map(|(id, _)| id.0).collect();
        });

        let mut owner = SemanticsOwner::new(callback);
        let id = owner.insert(addressable(7));
        owner.set_root(Some(id));
        owner.flush();

        let expected = owner
            .get(id)
            .and_then(SemanticsNode::accessibility_id)
            .expect("a render-backed node is addressable")
            .as_u64();
        assert_eq!(*seen.lock(), vec![expected]);
    }

    #[test]
    fn test_semantics_owner_mark_dirty() {
        let mut owner = SemanticsOwner::new_without_callback();

        let id = owner.insert(addressable(1));
        owner.set_root(Some(id));

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
    fn test_semantics_node_update() {
        let data = SemanticsNodeData {
            label: Some("Test".into()),
            ..Default::default()
        };

        let update = SemanticsNodeUpdate::new(SemanticsId::new(1), data)
            .with_parent(Some(SemanticsId::new(2)))
            .with_children(vec![SemanticsId::new(3), SemanticsId::new(4)]);

        assert_eq!(update.id, SemanticsId::new(1));
        assert_eq!(update.parent, Some(SemanticsId::new(2)));
        assert_eq!(update.children.len(), 2);
    }

    #[test]
    fn snapshot_rejects_a_node_without_stable_render_identity() {
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new());
        owner.set_root(Some(root));

        assert_eq!(
            owner.snapshot().expect_err("identity is required"),
            SemanticsSnapshotError::MissingAccessibilityIdentity { node: root },
        );
    }

    #[test]
    fn snapshot_rejects_a_missing_root() {
        let owner = SemanticsOwner::new_without_callback();

        assert_eq!(
            owner.snapshot().expect_err("a rooted result is required"),
            SemanticsSnapshotError::MissingRoot,
        );
    }

    #[test]
    fn snapshot_rejects_an_edge_to_a_missing_node() {
        let mut owner = SemanticsOwner::new_without_callback();
        let mut root_node = SemanticsNode::new().with_source_render_id(RenderId::new(1));
        let missing = SemanticsId::new(99);
        root_node.add_child(missing);
        let root = owner.insert(root_node);
        owner.set_root(Some(root));

        assert_eq!(
            owner.snapshot().expect_err("every child edge must resolve"),
            SemanticsSnapshotError::MissingNode { node: missing },
        );
    }

    #[test]
    fn snapshot_rejects_duplicate_accessibility_identity() {
        let render_id = RenderId::new(7);
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(render_id));
        let duplicate = owner.insert(SemanticsNode::new().with_source_render_id(render_id));
        owner.add_child(root, duplicate);
        owner.set_root(Some(root));

        assert_eq!(
            owner
                .snapshot()
                .expect_err("one stable identity cannot name two live nodes"),
            SemanticsSnapshotError::DuplicateAccessibilityIdentity {
                id: render_id.into(),
                first_node: root,
                duplicate_node: duplicate,
            },
        );
    }

    #[test]
    fn snapshot_rejects_a_node_reached_by_two_paths() {
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        let left = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(2)));
        let right = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(3)));
        let repeated = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(4)));
        owner.add_child(root, left);
        owner.add_child(root, right);
        owner.add_child(left, repeated);
        owner
            .get_mut(right)
            .expect("right node must remain live")
            .add_child(repeated);
        owner.set_root(Some(root));

        assert_eq!(
            owner
                .snapshot()
                .expect_err("a semantics result must be a tree"),
            SemanticsSnapshotError::RepeatedNode { node: repeated },
        );
    }

    #[test]
    fn snapshot_rejects_a_cycle() {
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        let child = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(2)));
        owner.add_child(root, child);
        owner
            .get_mut(child)
            .expect("child must remain live")
            .add_child(root);
        owner.set_root(Some(root));

        assert_eq!(
            owner.snapshot().expect_err("cycles cannot be snapshotted"),
            SemanticsSnapshotError::RepeatedNode { node: root },
        );
    }

    #[test]
    fn snapshot_is_owned_preorder_data_and_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SemanticsSnapshot>();
        assert_send_sync::<crate::SemanticsNodeSnapshot>();

        let mut owner = SemanticsOwner::new_without_callback();
        let mut root_node = SemanticsNode::new().with_source_render_id(RenderId::new(1));
        root_node.config_mut().set_label("Root");
        let root = owner.insert(root_node);

        let mut child_node = SemanticsNode::new().with_source_render_id(RenderId::new(2));
        child_node.config_mut().set_label("Child");
        let child = owner.insert(child_node);
        owner.add_child(root, child);
        owner.set_root(Some(root));

        let snapshot = owner.snapshot().expect("all nodes have render identities");
        owner.clear();

        assert_eq!(snapshot.root(), AccessibilityNodeId::from(RenderId::new(1)));
        assert_eq!(
            snapshot
                .nodes()
                .iter()
                .map(crate::SemanticsNodeSnapshot::id)
                .collect::<Vec<_>>(),
            vec![
                AccessibilityNodeId::from(RenderId::new(1)),
                AccessibilityNodeId::from(RenderId::new(2)),
            ],
        );
        assert_eq!(
            snapshot
                .node(AccessibilityNodeId::from(RenderId::new(2)))
                .and_then(|node| node.label())
                .map(crate::AttributedString::as_str),
            Some("Child"),
            "clearing the owner must not invalidate owned snapshot strings",
        );
    }

    #[test]
    fn action_resolution_clones_the_handler_without_invoking_it() {
        let target = AccessibilityNodeId::from(RenderId::new(7));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_in_handler = Arc::clone(&calls);
        let mut node = SemanticsNode::new().with_source_render_id(RenderId::new(7));
        node.config_mut().add_action(
            SemanticsAction::SetText,
            Arc::new(move |action, arguments| {
                calls_in_handler.lock().push((action, arguments));
            }),
        );

        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(node);
        owner.set_root(Some(root));

        let invocation = owner
            .resolve_action(SemanticsActionRequest::with_arguments(
                target,
                SemanticsAction::SetText,
                ActionArgs::SetText {
                    text: "updated".to_owned(),
                },
            ))
            .expect("the exported action must resolve");
        assert!(
            calls.lock().is_empty(),
            "resolution must not call user code while the owner may be borrowed"
        );

        invocation.invoke();
        assert_eq!(
            calls.lock().as_slice(),
            &[(
                SemanticsAction::SetText,
                Some(ActionArgs::SetText {
                    text: "updated".to_owned(),
                }),
            )],
        );
    }

    #[test]
    fn action_resolution_applies_the_effective_action_mask() {
        let render_id = RenderId::new(3);
        let target = AccessibilityNodeId::from(render_id);
        let mut node = SemanticsNode::new().with_source_render_id(render_id);
        node.config_mut()
            .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        node.config_mut().set_blocks_user_actions(true);

        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(node);
        owner.set_root(Some(root));

        assert_eq!(
            owner
                .resolve_action(SemanticsActionRequest::new(target, SemanticsAction::Tap,))
                .expect_err("blocked pointer actions must not remain routable"),
            SemanticsActionError::UnsupportedAction {
                node_id: target,
                action: SemanticsAction::Tap,
            },
        );
    }

    #[test]
    fn action_resolution_ignores_orphaned_and_stale_snapshot_nodes() {
        let root_render_id = RenderId::new(1);
        let orphan_render_id = RenderId::new(2);
        let stale_render_id = RenderId::new(99);
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(root_render_id));
        let mut orphan = SemanticsNode::new().with_source_render_id(orphan_render_id);
        orphan
            .config_mut()
            .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        let _orphan = owner.insert(orphan);
        owner.set_root(Some(root));

        for target in [
            AccessibilityNodeId::from(orphan_render_id),
            AccessibilityNodeId::from(stale_render_id),
        ] {
            assert_eq!(
                owner
                    .resolve_action(SemanticsActionRequest::new(target, SemanticsAction::Tap,))
                    .expect_err("nodes absent from the rooted snapshot are stale"),
                SemanticsActionError::NodeNotFound { node_id: target },
            );
        }
    }

    #[test]
    fn action_resolution_rejects_duplicate_platform_identity() {
        let render_id = RenderId::new(5);
        let target = AccessibilityNodeId::from(render_id);
        let mut first = SemanticsNode::new().with_source_render_id(render_id);
        first
            .config_mut()
            .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        let mut duplicate = SemanticsNode::new().with_source_render_id(render_id);
        duplicate
            .config_mut()
            .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));

        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(first);
        let child = owner.insert(duplicate);
        owner.add_child(root, child);
        owner.set_root(Some(root));

        assert_eq!(
            owner
                .resolve_action(SemanticsActionRequest::new(target, SemanticsAction::Tap,))
                .expect_err("ambiguous identity must never choose a handler arbitrarily"),
            SemanticsActionError::AmbiguousNode { node_id: target },
        );
    }
}
