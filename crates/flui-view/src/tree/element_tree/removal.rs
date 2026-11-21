//! Removal and unmount of subtrees, including the inherited-dependency and unmount-hook bookkeeping they run.

use flui_foundation::ElementId;

use super::render_children::{RenderRelocation, collect_render_frontier, detach_render_relocation};
use super::{ElementNode, ElementTree};

/// How [`ElementTree::remove_subtree`] treats a `GlobalKey`'d element it finds.
///
/// The two modes exist because "this subtree is going away" means two
/// different things. A parent dropping a child is a within-frame event: a key
/// may be declared again before the frame ends, so the element is deactivated
/// and `finalize_tree` unmounts whatever no key claimed. Tearing a
/// presentation down is not — there is no frame left to run a retake or a
/// finalize, so a deactivated element would sit in the inactive queue with its
/// `dispose` never run, and a later attach could retake stale state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubtreeRemoval {
    /// A keyed element is deactivated for a same-frame retake; the walk stops
    /// there so the retake keeps its children.
    DeactivateKeyed,
    /// Everything goes, keys included. For permanent teardown, where nothing
    /// will run `finalize_tree` afterwards.
    Finalize,
}

impl ElementTree {
    /// Remove `dependent` from the forward maps of the exact providers owned
    /// by the reverse index.
    fn release_inherited_providers(
        &mut self,
        dependent: ElementId,
        providers: impl IntoIterator<Item = ElementId>,
    ) {
        for provider in providers {
            if let Some(node) = self.get_mut(provider)
                && let Some(accessor) = node.element_mut().as_inherited_mut()
            {
                accessor.remove_dependent(crate::context::CrateToken::new(), dependent);
            }
        }
    }

    /// Release active inherited edges at the start of a temporary detach.
    pub(super) fn deactivate_inherited_dependencies(
        &mut self,
        dependent: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let providers = owner.deactivate_inherited_dependent(dependent);
        self.release_inherited_providers(dependent, providers);
    }

    /// Run the unmount hooks `id`'s state registered, right after its
    /// `dispose`; the first panic among them is recorded as the element's
    /// `dispose` failure.
    fn run_unmount_hooks(&self, index: usize, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        let Some(payload) = owner.unmount_hooks.run(id) else {
            return;
        };
        let node = &self.nodes[index];
        let mut panic = crate::owner::RecoveredPanic::from_payload(
            crate::owner::RecoveredAt::Element {
                element: id,
                parent: node.parent(),
            },
            node.element().view_type_id(),
            crate::owner::LifecycleHook::Dispose,
            payload.as_ref(),
            "running an unmount hook",
        )
        .with_site(node.element().creation_site());
        // The element is freed right after its hooks run; read its path while it is a node.
        panic.locate_in(self);
        owner.push_recovered_panic(panic);
        flui_foundation::panic::discard_payload(payload);
    }
    /// Release every inherited edge and lifecycle marker on permanent teardown.
    fn unmount_inherited_dependencies(
        &mut self,
        dependent: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let providers = owner.unmount_inherited_dependent(dependent);
        self.release_inherited_providers(dependent, providers);
        owner.clear_pending_dependency_change(dependent);
        owner.release_reactive(dependent);
        // A keep-alive lease normally releases through its own `Drop` when the
        // holder's state drops. An element can be torn down without that
        // happening in the same step, and a stranded holder would keep
        // resolving to whatever sparse child it last sat under, so the unmount
        // seam clears it too. This is the liveness half of the lease.
        //
        // There is no matching "forget the held child" step: nothing records a
        // held child any more. A hold is resolved from its holder when eviction
        // asks, so an unmounted child simply stops being resolved to.
        owner.keep_alive.forget_holder(dependent);
    }

    /// Snapshot the complete owned child graph without exposing sparse rows to
    /// dense reconciliation. Internal manager guards end before lifecycle work.
    pub(crate) fn owned_children(
        &self,
        id: ElementId,
        registry: &crate::element::child_manager::ChildManagerRegistry,
    ) -> Vec<ElementId> {
        let Some(node) = self.get(id) else {
            return Vec::new();
        };
        let mut children = node.child_ids().to_vec();
        children.extend(self.sparse_children(id, registry));
        children
    }

    /// The lazy children a child manager registered under `id`'s render
    /// object holds, which stay out of `child_ids`. Only children whose
    /// element parent is still `id` count: a child the manager has not yet
    /// forgotten after a GlobalKey move belongs to its new parent.
    pub(super) fn sparse_children(
        &self,
        id: ElementId,
        registry: &crate::element::child_manager::ChildManagerRegistry,
    ) -> Vec<ElementId> {
        let manager = self
            .get(id)
            .and_then(|node| node.element().render_id())
            .and_then(|render_id| registry.borrow().get(&render_id).cloned());
        let Some(manager) = manager else {
            return Vec::new();
        };
        let sparse = manager.borrow().snapshot_children();
        sparse
            .into_iter()
            .filter(|child| {
                self.get(*child)
                    .is_some_and(|node| node.parent() == Some(id))
            })
            .collect::<Vec<ElementId>>()
    }

    pub(crate) fn preflight_unmount(&self, ids: &[ElementId]) {
        for &id in ids {
            if let Some(index) = self.resolve_index(id) {
                self.next_generation(index);
            }
        }
    }

    pub(crate) fn retire_sparse_children(
        &self,
        ids: &[ElementId],
        registry: &crate::element::child_manager::ChildManagerRegistry,
    ) {
        for &id in ids {
            let manager = self
                .get(id)
                .and_then(|node| node.element().render_id())
                .and_then(|render_id| registry.borrow().get(&render_id).cloned());
            if let Some(manager) = manager {
                manager.borrow_mut().retire_children();
            }
        }
    }

    /// Remove an element from the tree.
    ///
    /// # Soft vs eager removal
    ///
    /// - **Soft (keyed):** If the element carries a `GlobalKey` (i.e.
    ///   `ElementNode::registered_global_key` is `Some`), the
    ///   element is deactivated and pushed onto
    ///   `BuildOwner::inactive_elements` — the slab entry stays alive.
    ///   This enables same-frame state migration: a subsequent
    ///   `insert` with the same GlobalKey pulls the element back via
    ///   `try_retake_global_key` (private). End-of-frame
    ///   [`BuildOwner::finalize_tree`](crate::owner::BuildOwner::finalize_tree) drains any stragglers via
    ///   [`Self::remove_finalized`] (full slab-remove + unregister).
    /// - **Eager (un-keyed):** Behaves as before — `Element::unmount`
    ///   then slab-remove. No deferred queue entry.
    ///
    /// Only elements reachable by `GlobalKey` are deferred; ordinary
    /// unmounts are processed inline.
    ///
    /// Does NOT automatically remove children — caller must handle that.
    ///
    /// Returns the `ElementNode` for an eager removal (so `BuildOwner`
    /// gets back ownership) OR `None` for a soft removal (the node
    /// still lives in the slab). Returns `None` if `id` doesn't exist.
    ///
    /// Threads the split-borrow `owner` handle.
    ///
    /// # Panics
    ///
    /// Eager removal refuses an exhausted slot generation before teardown.
    /// The current occupant remains live; keyed soft removal does not free a slot.
    pub fn remove(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        // Staleness-checked: a stale id (slot already freed/reused) resolves
        // to `None` here rather than touching the slot's new occupant.
        let index = self.resolve_index(id)?;

        // Soft-remove for keyed elements: push to inactive queue
        // without slab-removing. State stays intact for same-frame
        // remount.
        if self.nodes[index].registered_global_key.is_some() {
            let depth = self.nodes[index].depth();
            let former_parent = self.nodes[index].parent;
            let mut relocation = RenderRelocation {
                pipeline: self.nodes[index].element().pipeline_owner(),
                frontier: collect_render_frontier(self, id).unwrap_or_default(),
                detached_render_subtrees: None,
            };
            if let Some(parent_id) = former_parent
                && let Some(parent) = self.get_mut(parent_id)
            {
                parent.child_ids.retain(|child_id| *child_id != id);
            }
            detach_render_relocation(&mut relocation);
            self.deactivate_subtree(id, owner);
            if let Some(token) = relocation.detached_render_subtrees {
                owner.push_inactive_with_detached_render_subtrees(id, depth, token);
            } else {
                owner.push_inactive(id, depth);
            }
            // Detach from active tree but keep the slot alive.
            self.nodes[index].parent = None;

            if self.root == Some(id) {
                self.root = None;
            }

            tracing::debug!(
                element_id = ?id,
                key = ?self.nodes[index].registered_global_key,
                "ElementTree::remove soft-removed keyed element into inactive queue"
            );

            // Soft-remove yields no owned node — the caller doesn't
            // get the element back.
            return None;
        }

        // Refuse an exhausted identity before lifecycle, dependency or slot changes.
        let next_generation = self.next_generation(index);

        // Eager path for un-keyed elements. Drop any stale
        // `did_change_dependencies` flag — the dependent
        // leaves the active tree before its rebuild ever runs.
        self.unmount_inherited_dependencies(id, owner);
        self.nodes[index].element_mut().unmount(owner);
        self.run_unmount_hooks(index, id, owner);

        // ADR-0040: `Element::unmount` ran and the slot is about to free —
        // one of the two primitives every unmount path bottoms out in.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_unmounted(&flui_foundation::observe::ElementUnmounted::new(id));
        });

        let node = self.nodes.remove(index);
        // The nonfallible commit follows the preflight above.
        self.generations.commit(next_generation);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }

    /// Remove `id` and its entire descendant subtree.
    ///
    /// Used by [`crate::element::sparse_children::SparseChildren::evict`] to
    /// evict a lazy sliver child together with all of its own descendant
    /// elements and render nodes (e.g. a `Container(Padding(Text))` child
    /// produces three elements; a single-node `remove` would leak the inner
    /// two). Also the removal path a rebuild takes for any un-keyed child
    /// whose element type changed (`ElementTree::update`'s can't-reuse
    /// branch) — e.g. `LaidOut::pump_widget`'s root-swap in the headless
    /// harness.
    ///
    /// The algorithm mirrors `id_reconcile::remove_child` / `collect_subtree_preorder`:
    ///
    /// 1. Peek whether the root is keyed (`registered_global_key`),
    ///    side-effect-free, before touching anything. A keyed root
    ///    soft-removes via `remove` and returns immediately — descendants
    ///    stay untouched, parked for `finalize_tree`'s deferred drain in
    ///    case the root is reclaimed same-frame via `GlobalKey` — so the
    ///    subtree snapshot below is never needed for this branch and would
    ///    be pure wasted work if taken first.
    /// 2. For an un-keyed root, snapshot the subtree in pre-order (parent
    ///    before children) while all `child_ids` lists are intact. Check every
    ///    slot that will be finalized before any detach or teardown occurs.
    /// 3. Free descendants deepest-first via `remove_finalized` BEFORE the
    ///    root's own removal (step 4). Each element's own `unmount` — e.g.
    ///    `RenderView::did_unmount_render_object` for
    ///    `MouseRegion`/`ClipPath`/`Listener`, which unregisters from the
    ///    owner-local interaction lane — needs its OWN render object still
    ///    reachable in the render tree to fire at all
    ///    (`RenderBehavior::on_unmount` looks it up by id and silently skips
    ///    the view-level hook when the lookup misses). Removing the root
    ///    first would cascade-delete every descendant render node in one
    ///    shot (`PipelineOwner::remove_render_object` recurses), so a
    ///    descendant unmounted afterward would find its render object
    ///    already gone and its own unmount hook would silently never run —
    ///    orphaning its interaction-lane registration for the life of the
    ///    owner. Processing descendants first makes each one's own render
    ///    removal a no-op by the time the root's cascade reaches it.
    /// 4. Remove the root: `remove` under `DeactivateKeyed` (so a keyed root
    ///    soft-removes and stays retakeable), `remove_finalized` under
    ///    `Finalize` (nothing survives, including a keyed root).
    ///
    /// Complexity: O(n) time + O(n) peak heap for the work-stack (n = subtree
    /// size), O(h) call-stack for the constant-stack iterative walk — paid
    /// only for an un-keyed root; a keyed root is O(1).
    pub(crate) fn remove_subtree(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
        mode: SubtreeRemoval,
    ) {
        // A keyed root soft-removes into the inactive queue instead of
        // freeing outright (`remove`'s own branch, keyed on
        // `registered_global_key`); descendants stay untouched. Check
        // this FIRST: it needs no subtree walk, and a keyed root never uses
        // the snapshot below, so paying for that walk before checking would
        // be wasted work for every keyed-root removal.
        let root_is_keyed = mode == SubtreeRemoval::DeactivateKeyed
            && self
                .get(id)
                .is_some_and(|node| node.registered_global_key().is_some());

        if root_is_keyed {
            self.remove(id, owner);
            return;
        }

        // Un-keyed root: snapshot the subtree pre-order (parent before
        // children) before touching any node, while every `child_ids` list
        // is still intact.
        //
        // The walk STOPS at a keyed descendant and collects it separately. A
        // `GlobalKey` inside a removed unkeyed subtree can be retaken in the
        // same frame, so that element and everything under it must survive
        // into the inactive queue rather than being freed here — freeing it
        // loses the state the key exists to preserve. Not descending is the
        // other half: a retaken element keeps its own children, so hard-
        // removing them deepest-first would hand the retake an empty subtree.
        let mut subtree: Vec<ElementId> = Vec::new();
        let mut keyed_descendants: Vec<ElementId> = Vec::new();
        {
            let mut work_stack: Vec<ElementId> = vec![id];
            while let Some(node_id) = work_stack.pop() {
                if mode == SubtreeRemoval::DeactivateKeyed
                    && node_id != id
                    && self
                        .get(node_id)
                        .is_some_and(|node| node.registered_global_key().is_some())
                {
                    keyed_descendants.push(node_id);
                    continue;
                }
                subtree.push(node_id);
                work_stack.extend(
                    self.owned_children(node_id, owner.child_manager_registry)
                        .into_iter()
                        .rev(),
                );
            }
        }

        // Admit the whole finalized portion before any keyed detach or teardown.
        // Per-node checks in remove_finalized are too late for a deepest-first
        // walk: an exhausted ancestor would refuse after its children were freed.
        // Keyed boundaries excluded above survive DeactivateKeyed and need no
        // generation advance, including every node under those boundaries.
        self.preflight_unmount(&subtree);

        // Soft-remove the keyed descendants BEFORE their ancestors are freed,
        // so `remove`'s unlink-from-parent and render detach run against a
        // tree that is still intact. No keyed node here is an ancestor of
        // another — the walk never descended past one — so the order among
        // them does not matter.
        for &keyed in &keyed_descendants {
            self.remove(keyed, owner);
        }
        self.retire_sparse_children(&subtree, owner.child_manager_registry);

        // Free descendants deepest-first -- `subtree[0]` is the root itself
        // (removed below, last); iterating the rest in reverse visits each
        // descendant after all of ITS OWN descendants, so a nested unmount
        // hook (e.g. a `MouseRegion` two levels down) always still finds its
        // own render object present.
        for &descendant in subtree[1..].iter().rev() {
            self.remove_finalized(descendant, owner);
        }
        // The root goes out by the same rule as the rest of the subtree.
        // Under `Finalize` nothing survives — a keyed root soft-removed here
        // would sit in the inactive queue with no frame coming to drain it,
        // which is the leak this mode exists to prevent; and a root removed
        // because its own update panicked must never be retakeable, or the
        // half-configured render object resurfaces on the next retake.
        match mode {
            SubtreeRemoval::Finalize => {
                self.remove_finalized(id, owner);
            }
            SubtreeRemoval::DeactivateKeyed => {
                self.remove(id, owner);
            }
        }
    }

    /// Fully remove an element that has already been unmounted (e.g.
    /// from `BuildOwner::finalize_tree`'s end-of-frame drain).
    ///
    /// This bypasses the soft-remove path even for keyed elements:
    /// the slab entry is freed and the `GlobalKey` registration is
    /// cleared via `ElementOwner::unregister_global_key`. The finalization
    /// phase that drains the inactive queue doesn't push back into it.
    ///
    /// # Panics
    ///
    /// An exhausted slot generation refuses removal before teardown or key
    /// unregistration, leaving the current occupant live.
    pub fn remove_finalized(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        self.remove_finalized_inner(id, owner, true)
    }

    /// Retire a node that `insert` minted but never finished mounting.
    ///
    /// Identical teardown to [`remove_finalized`](Self::remove_finalized) with
    /// the unmount observation suppressed: `insert` emits its `Mount` only
    /// *after* `mount` returns, so a node abandoned mid-mount was never
    /// announced, and reporting an unmount for it would hand an observer an
    /// id it never saw appear (ADR-0040's causal ordering).
    ///
    /// The inactive queue only ever holds an element whose `mount` already
    /// returned, so a node abandoned mid-mount never reaches
    /// `remove_finalized`. This teardown is asymmetric with it for that
    /// reason —
    /// `RenderView::did_unmount_render_object` still runs here on a render
    /// object that WAS created and adopted moments earlier (a real seam,
    /// bounded separately).
    pub(crate) fn discard_unannounced(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        self.remove_finalized_inner(id, owner, false)
    }

    fn remove_finalized_inner(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
        announce: bool,
    ) -> Option<ElementNode> {
        // Staleness-checked entry (mirror of `remove`).
        let index = self.resolve_index(id)?;

        // Even keyed finalization must refuse before unregistering or unmounting.
        let next_generation = self.next_generation(index);

        // Unregister the GlobalKey if this element had one. We do it
        // BEFORE `unmount` so the registry doesn't briefly resolve to
        // a partially-unmounted element.
        if let Some(key) = self.nodes[index].registered_global_key.take() {
            owner.unregister_global_key(key.as_ref());
        }

        // Drop any stale `did_change_dependencies` flag —
        // the dependent leaves the tree before its rebuild ever runs.
        self.unmount_inherited_dependencies(id, owner);
        self.nodes[index].element_mut().unmount(owner);
        self.run_unmount_hooks(index, id, owner);

        // ADR-0040: the second of the two unmount primitives (finalize
        // drains, subtree evictions, root detach all bottom out here).
        if announce {
            crate::owner::emit_observation(owner.tree_observer, |o| {
                o.element_unmounted(&flui_foundation::observe::ElementUnmounted::new(id));
            });
        }

        let node = self.nodes.remove(index);
        // The nonfallible commit follows the preflight above.
        self.generations.commit(next_generation);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }
}
