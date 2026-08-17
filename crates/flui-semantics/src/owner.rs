//! SemanticsOwner - Manages the semantics tree lifecycle
//!
//! The SemanticsOwner coordinates updates to the semantics tree and
//! sends updates to the platform accessibility services.

use std::sync::Arc;

use flui_foundation::SemanticsId;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use thiserror::Error;

use crate::{
    action::{ActionArgs, SemanticsAction, SemanticsActionHandler, SemanticsActionRequest},
    identity::AccessibilityNodeId,
    node::SemanticsNode,
    snapshot::{SemanticsSnapshot, SemanticsSnapshotError},
    tree::SemanticsTree,
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
/// - The payload must be keyed by the stable `AccessibilityNodeId` an adapter
///   publishes and routes actions back through — never by `SemanticsId`,
///   which is an arena position in a tree the pipeline rebuilds every pass.
///   (A per-node payload keyed on `SemanticsId` used to live here and was
///   removed for exactly that reason.) See [`crate::tree_to_update`].
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
/// let callback = Arc::new(|update: &flui_semantics::TreeUpdate| {
///     for (id, _node) in &update.nodes {
///         tracing::debug!(?id, "semantics update");
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

    /// What the last update told the platform, keyed by stable identity.
    ///
    /// `None` until the first publish (or after
    /// [`Self::schedule_full_publish`] deliberately forgets it): the next
    /// flush is then a self-contained full update. While `Some`, a flush
    /// diffs each dirty node's translation against this mirror and publishes
    /// only what actually changed — which is what keeps a rebuild-everything
    /// assembly pass (ADR-0014) from republishing an entire tree because one
    /// checkbox toggled.
    published: Option<PublishedState>,

    /// Forces the next flush to publish the complete tree even if the diff
    /// would be empty. Set by [`Self::send_full_tree`] and
    /// [`Self::schedule_full_publish`]; cleared once a full update is
    /// actually delivered (an unrooted tree leaves it pending, exactly like
    /// the dirty bits it travels with).
    full_publish_pending: bool,
}

/// Mirror of the adapter-visible tree as of the last delivered update.
///
/// The map holds the translated [`accesskit::Node`] per published
/// [`accesskit::NodeId`] — equality against a fresh translation is the diff.
/// Entries are pruned when their node leaves the arena, so a node that is
/// removed and later returns with identical content is correctly republished
/// (the adapter dropped it in between; the mirror must forget it too).
struct PublishedState {
    nodes: FxHashMap<accesskit::NodeId, accesskit::Node>,
    root: accesskit::NodeId,
    focus: accesskit::NodeId,
}

impl PublishedState {
    /// Mirror of a self-contained full update the adapter was just given.
    ///
    /// Consumes the update (callers are done delivering it by reference) so
    /// the mirror is built by moving the translated nodes, not re-cloning
    /// every label and property the adapter just received — on the full
    /// publish path that clone was the single largest added cost.
    fn mirror_of(update: crate::TreeUpdate) -> Self {
        Self {
            root: update
                .tree
                .as_ref()
                .map(|tree| tree.root)
                .expect("BUG: a full update always carries tree metadata"),
            focus: update.focus,
            nodes: update.nodes.into_iter().collect(),
        }
    }
}

impl std::fmt::Debug for SemanticsOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsOwner")
            .field("tree", &self.tree)
            .field("callback", &self.callback.as_ref().map(|_| "<callback>"))
            .field("enabled", &self.enabled)
            .field(
                "published",
                &self.published.as_ref().map(|state| state.nodes.len()),
            )
            .field("full_publish_pending", &self.full_publish_pending)
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
            published: None,
            full_publish_pending: false,
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
            published: None,
            full_publish_pending: false,
        }
    }

    /// Creates a SemanticsOwner with pre-allocated capacity.
    pub fn with_capacity(capacity: usize, callback: SemanticsUpdateCallback) -> Self {
        Self {
            tree: SemanticsTree::with_capacity(capacity),
            callback: Some(callback),
            enabled: true,
            published: None,
            full_publish_pending: false,
        }
    }

    /// Replaces the platform callback on a live owner.
    ///
    /// For the composition root that learns its platform bridge *after* this
    /// owner was created — an owner lazily constructed by the pipeline on
    /// enablement, wired to the real adapter once one exists.
    ///
    /// A live owner may already be **clean**: [`Self::flush`] gates on dirty
    /// nodes, so without help the new callback would first hear from us only
    /// on the next tree change — a late-wired adapter would present nothing
    /// until the user did something. So a rooted current tree is published
    /// through the new callback immediately; translation is snapshot-shaped,
    /// so this says exactly what a flush would say.
    ///
    /// The swap also resets the published-state mirror to exactly what the
    /// new callback was just told: the previous adapter's knowledge is
    /// irrelevant to this one, and diffing future flushes against it would
    /// withhold nodes the new adapter has never seen.
    pub fn set_callback(&mut self, callback: SemanticsUpdateCallback) {
        self.published = None;
        if self.enabled
            && let Some(update) = crate::tree_to_update(&self.tree, None)
        {
            callback(&update);
            self.published = Some(PublishedState::mirror_of(update));
        }
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
        self.published = None;
        self.full_publish_pending = false;
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

    /// Publishes what changed — and only what changed — to the platform.
    ///
    /// The observable contract is Flutter's (`SemanticsOwner
    /// .sendSemanticsUpdate`): an idle frame publishes nothing and returns in
    /// O(1); only nodes whose content actually changed serialize into the
    /// update. The *mechanism* diverges because FLUI rebuilds the semantics
    /// arena every assembly pass (ADR-0014) where Flutter mutates persistent
    /// nodes: a rebuild marks every node dirty, so the dirty bit alone cannot
    /// say what changed. The diff therefore compares each dirty node's
    /// translation, keyed by its stable [`AccessibilityNodeId`], against a
    /// private mirror of the last delivered update — payload equality is
    /// authoritative, dirty bits only bound how much is re-examined.
    ///
    /// Three shapes come out of one call:
    ///
    /// - **Clean tree** — O(1) early return, no translation, no callback.
    /// - **First publish / [`Self::schedule_full_publish`] pending / root
    ///   identity changed** — a self-contained full update carrying
    ///   [`accesskit::TreeUpdate::tree`] metadata. `tree: Some` is this
    ///   owner's promise that the update stands alone (the Linux bridge
    ///   retains exactly these to answer a late-activating screen reader).
    /// - **Otherwise** — an incremental update: changed nodes only,
    ///   `tree: None`. Structure changes ride along for free because a
    ///   node's children list is part of its payload — the parent whose
    ///   children changed is itself a changed node, which is also how an
    ///   adapter learns a removed child is gone. If nothing survives the
    ///   diff and focus is unchanged, no update is delivered at all.
    ///
    /// An unrooted tree publishes nothing and stays dirty, so the first
    /// rooted flush retries (unchanged from the pre-diff behavior; see the
    /// comment inside the private full-publish path).
    pub fn flush(&mut self) {
        if !self.enabled || !(self.tree.has_dirty_nodes() || self.full_publish_pending) {
            return;
        }

        let needs_full = self.full_publish_pending
            || match (&self.published, self.current_root_id()) {
                // A changed root identity is a different tree to the
                // adapter; a diff cannot express that transition, so it
                // escalates to a self-contained full update.
                (Some(state), Some(root)) => state.root != root,
                // Nothing published yet — or no addressable root (both
                // paths publish nothing and stay dirty; route through the
                // full path for one exit).
                _ => true,
            };

        if needs_full {
            self.publish_full();
        } else {
            self.publish_incremental();
        }
    }

    /// The stable identity the current root would publish under.
    fn current_root_id(&self) -> Option<accesskit::NodeId> {
        self.tree
            .root()
            .and_then(|root| self.tree.get(root))
            .and_then(SemanticsNode::accessibility_id)
            .map(|id| accesskit::NodeId(id.as_u64()))
    }

    /// Publishes the complete rooted tree and resets the mirror to it.
    fn publish_full(&mut self) {
        // Translate before touching the callback so the borrow of `self.tree`
        // ends first.
        let Some(update) = crate::tree_to_update(&self.tree, None) else {
            // No root yet — nothing an adapter could apply. Leave the tree
            // dirty (and any full publish pending) so the next flush retries
            // once assembly has rooted it, rather than silently swallowing
            // the first real update.
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

        self.published = Some(PublishedState::mirror_of(update));
        self.full_publish_pending = false;
        self.tree.mark_all_clean();
    }

    /// Diffs dirty nodes against the mirror and publishes only the changes.
    ///
    /// One pass over the arena does all four jobs: collect the live id set
    /// (to prune mirror entries for removed nodes), re-translate dirty nodes
    /// (clean ones are unchanged by the dirty-bit contract and are skipped),
    /// compare against the mirror, and derive focus. The pass is O(arena)
    /// with translation cost O(dirty) — the arena walk itself is the same
    /// order as the assembly pass that produced the dirt.
    fn publish_incremental(&mut self) {
        let state = self
            .published
            .as_mut()
            .expect("BUG: incremental publish requires a prior published state");

        let mut live: FxHashSet<accesskit::NodeId> =
            FxHashSet::with_capacity_and_hasher(self.tree.len(), rustc_hash::FxBuildHasher);
        let mut changed: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();
        // Focus derivation, folded into the same pass `focused_node` would
        // otherwise repeat: exactly one claimant wins, ambiguity falls back
        // to the root (matching `tree_to_update`).
        let mut focus_claimant: Option<accesskit::NodeId> = None;
        let mut focus_ambiguous = false;

        for (_, node) in self.tree.iter() {
            let Some(identity) = node.accessibility_id() else {
                // Unaddressable: never published, nothing to diff. Same
                // skip rule as `tree_to_update`.
                continue;
            };
            let id = accesskit::NodeId(identity.as_u64());
            live.insert(id);

            if node.config().is_focused() {
                if focus_claimant.is_some() {
                    focus_ambiguous = true;
                } else {
                    focus_claimant = Some(id);
                }
            }

            if !node.is_dirty() {
                continue;
            }
            let Some(data) = self.tree.node_data_of(node) else {
                continue;
            };
            let translated = crate::accesskit_translation::to_node(&data);
            if state.nodes.get(&id) != Some(&translated) {
                changed.push((id, translated));
            }
        }

        // Prune mirror entries whose nodes left the arena. Without this, a
        // node that is removed and later returns with identical content
        // would diff as "unchanged" against a mirror the adapter no longer
        // agrees with — the adapter dropped it when its parent was
        // republished without it — and never be re-sent.
        state.nodes.retain(|id, _| live.contains(id));

        if focus_ambiguous {
            tracing::warn!("semantics tree has more than one focused node; publishing the root");
        }
        let focus = focus_claimant
            .filter(|_| !focus_ambiguous)
            .filter(|claimant| live.contains(claimant))
            .unwrap_or(state.root);

        if changed.is_empty() && focus == state.focus {
            // Everything the dirty bits pointed at translated identically —
            // a rebuild that reproduced the same tree. The adapter hears
            // nothing; the dirt is simply consumed.
            self.tree.mark_all_clean();
            return;
        }

        for (id, node) in &changed {
            state.nodes.insert(*id, node.clone());
        }
        state.focus = focus;

        let update = crate::TreeUpdate {
            nodes: changed,
            tree: None,
            tree_id: accesskit::TreeId::ROOT,
            focus,
        };

        let callback = self.callback.as_ref().map(Arc::clone);
        if let Some(callback) = callback {
            callback(&update);
        }

        self.tree.mark_all_clean();
    }

    /// Forces the next flush to publish a self-contained full update, even
    /// if no node is dirty.
    ///
    /// The reconnect primitive: when assistive technology (re)activates, the
    /// adapter's state is unknown — it may have forgotten everything — so the
    /// mirror is forgotten with it. The composition root pairs this with
    /// re-seeding the assembly pass so a flush actually runs.
    pub fn schedule_full_publish(&mut self) {
        self.published = None;
        self.full_publish_pending = true;
    }

    /// Forces a full tree update, now.
    ///
    /// Marks all nodes dirty and publishes the complete tree, bypassing the
    /// incremental diff. Use when accessibility services reconnect or
    /// request the full tree.
    pub fn send_full_tree(&mut self) {
        if !self.enabled {
            return;
        }

        self.tree.mark_all_dirty();
        self.full_publish_pending = true;
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

    /// A callback swapped onto a live, already-clean owner immediately
    /// receives the current rooted tree. `flush` gates on dirty nodes, so
    /// without this a late-wired platform adapter would present nothing
    /// until the user's next interaction happened to dirty the tree.
    #[test]
    fn a_swapped_in_callback_immediately_receives_the_current_rooted_tree() {
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        owner.set_root(Some(root));

        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        owner.set_callback(Arc::new(move |update: &crate::TreeUpdate| {
            sink.lock().push(update.nodes.len());
        }));

        assert_eq!(
            received.lock().as_slice(),
            &[1],
            "the swap itself must publish the full rooted tree once"
        );
    }

    /// The swap publishes nothing while the tree is unrooted — there is
    /// nothing an adapter could apply, and the first real flush will carry
    /// the rooted tree anyway.
    #[test]
    fn a_swapped_in_callback_on_an_unrooted_tree_hears_nothing() {
        let mut owner = SemanticsOwner::new_without_callback();
        let calls = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&calls);
        owner.set_callback(Arc::new(move |_update: &crate::TreeUpdate| {
            sink.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
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
