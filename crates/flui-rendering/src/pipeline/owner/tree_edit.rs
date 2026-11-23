//! Edits of the render tree that the pipeline owner schedules work for.
//!
//! A caller outside this crate never holds `&mut RenderTree`: it names the
//! edit, and the owner makes it and marks the phases whose output the edit
//! invalidates. Child lists, a node's render object, its parent data and its
//! creation site change only through these operations, so no edit can be made
//! without its dirty marks.

use flui_foundation::{CreationSite, RenderId, SlotSet};

use crate::{
    RenderUpdateImpact,
    parent_data::ParentData,
    pipeline::phase::PipelinePhase,
    storage::{RenderNode, RenderTree},
};

use super::PipelineOwner;

/// The work a change of child membership invalidates on the parent: its
/// layout, which children need compositing, and its semantics children.
const MEMBERSHIP_IMPACT: RenderUpdateImpact = RenderUpdateImpact::LAYOUT
    .union(RenderUpdateImpact::COMPOSITING_BITS)
    .union(RenderUpdateImpact::SEMANTICS);

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Makes `children`, in that order, the whole child list of `parent`, and
    /// returns whether the list changed.
    ///
    /// A listed child held by another parent moves here; a current child left
    /// out becomes a root (see `RenderTree::set_children` for the edge rules).
    /// On a change, `parent` and every parent a child left are marked for
    /// layout, compositing bits and semantics, whether the change was of order
    /// or of membership. [`Self::edit_render_children`] is the batch form,
    /// which marks a pure reorder for layout alone.
    pub fn set_render_children(&mut self, parent: RenderId, children: &[RenderId]) -> bool {
        let update = self.render_tree.set_children(parent, children);
        if !update.changed {
            return false;
        }
        self.apply_render_update_impact(parent, MEMBERSHIP_IMPACT);
        for donor in update.donors {
            self.apply_render_update_impact(donor, MEMBERSHIP_IMPACT);
        }
        true
    }

    /// Runs `edit` over a batch of child-list edits and then marks each parent
    /// the batch changed, once.
    ///
    /// A parent whose membership changed (a child arrived, left, or was
    /// detached) is marked for layout, compositing bits and semantics; one whose
    /// children were only reordered is marked for layout. Marks are made after
    /// `edit` returns, against the committed tree, and a parent removed by then
    /// is skipped. An `edit` that panics leaves its edits made and unmarked.
    pub fn edit_render_children<T>(
        &mut self,
        edit: impl FnOnce(&mut RenderChildrenEdit<'_>) -> T,
    ) -> T {
        let mut batch = RenderChildrenEdit {
            tree: &mut self.render_tree,
            changed: SlotSet::new(),
            membership_changed: SlotSet::new(),
        };
        let output = edit(&mut batch);
        let RenderChildrenEdit {
            changed,
            membership_changed,
            ..
        } = batch;
        for &parent in changed.as_slice() {
            if !self.render_tree.contains(parent) {
                continue;
            }
            let impact = if membership_changed.contains(parent) {
                MEMBERSHIP_IMPACT
            } else {
                RenderUpdateImpact::LAYOUT
            };
            self.apply_render_update_impact(parent, impact);
        }
        output
    }

    /// Lends the render object at `id` to `update` when it is an `R`, and
    /// applies the impact `update` returns to `id`.
    ///
    /// Returns that impact, or `None` when `id` is stale or holds another
    /// type, in which case `update` does not run. An update that changes
    /// nothing the pipeline reads returns [`RenderUpdateImpact::NONE`].
    pub fn update_render_object<R: 'static>(
        &mut self,
        id: RenderId,
        update: impl FnOnce(&mut R) -> RenderUpdateImpact,
    ) -> Option<RenderUpdateImpact> {
        let render_object = self
            .render_tree
            .get_mut(id)?
            .downcast_render_object_mut::<R>()?;
        let impact = update(render_object);
        self.apply_render_update_impact(id, impact);
        Some(impact)
    }

    /// The render object at `id` when it is an `R`; `None` when `id` is stale
    /// or holds another type.
    #[must_use]
    pub fn render_object<R: 'static>(&self, id: RenderId) -> Option<&R> {
        self.render_tree.get(id)?.downcast_render_object::<R>()
    }

    /// Lends the parent data of the node at `id` to `update`, and applies the
    /// impact `update` returns to that node's render parent, the node that
    /// reads it.
    ///
    /// Returns that impact, or `None` when `id` is stale, in which case
    /// `update` does not run. A node with no render parent yet has nothing to
    /// mark. Installing or clearing parent data returns
    /// [`RenderUpdateImpact::NONE`] unless the caller knows a reader to
    /// invalidate.
    pub fn update_parent_data(
        &mut self,
        id: RenderId,
        update: impl FnOnce(&mut ParentDataSlot<'_>) -> RenderUpdateImpact,
    ) -> Option<RenderUpdateImpact> {
        let node = self.render_tree.get_mut(id)?;
        let impact = update(&mut ParentDataSlot { node });
        if let Some(parent) = self.render_tree.parent(id) {
            self.apply_render_update_impact(parent, impact);
        }
        Some(impact)
    }

    /// Records where the view that made the node at `id` was created, which
    /// an error naming the node reports. A stale `id` is a no-op; no phase
    /// reads the site, so nothing is marked.
    pub fn set_creation_site(&mut self, id: RenderId, site: CreationSite) {
        if let Some(node) = self.render_tree.get_mut(id) {
            node.set_creation_site(site);
        }
    }
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Removes the subtree rooted at `id` — THE dispose site.
    ///
    /// Removal is where owner-side state dies (the inversion of the
    /// "Drop will handle it" idea): a `Drop` impl has no
    /// `&PipelineOwner`, so it cannot evict dirty-queue entries — only
    /// this method can. Order:
    ///
    /// 1. collect the subtree's ids;
    /// 2. call [`RenderObject::detach`](crate::traits::RenderObject::detach)
    ///    on every id in the subtree (ADR-0013) — the tree-lifecycle
    ///    counterpart of `attach`, and the only place a render object can
    ///    tear down an `attach`-time subscription;
    /// 3. evict every id from ALL dirty queues (live + mid-phase) so
    ///    no phase walks a freed slot's stale entry;
    /// 4. take the subtree out of the tree, children first (each slot
    ///    advances its generation or retires, so outstanding ids go stale);
    /// 5. clear `root_id` when the root itself was removed, and invalidate
    ///    the former parent;
    /// 6. drop the removed nodes, last: a render object's destructor runs
    ///    against a committed tree and owner, so one that panics loses no
    ///    bookkeeping.
    ///
    /// `Drop` on render objects remains strictly node-local (decoded
    /// images, shaped text, GPU handles).
    ///
    /// Returns the number of nodes removed. Must not run mid-phase —
    /// the layout/paint walks hold raw borrows into the slab.
    pub fn remove_render_object(&mut self, id: RenderId) -> usize {
        debug_assert!(
            !self.scheduler.debug_doing_layout() && !self.scheduler.debug_doing_paint(),
            "remove_render_object during an active layout/paint phase — \
             the walks hold borrows into the slab; defer removal to \
             between-frame work",
        );

        let former_parent = self.render_tree.parent(id);
        let subtree = self
            .render_tree
            .collect_subtree(id, std::mem::take(&mut self.subtree_buffers));
        if subtree.ids().is_empty() {
            self.subtree_buffers = self
                .render_tree
                .take_subtree(subtree, &mut self.removed_nodes);
            return 0;
        }

        // Withdraw the entire subtree before any user-owned parent data or
        // detach hook can submit through a saved peer handle (ADR-0190).
        for &subtree_id in subtree.ids() {
            if let Some(node) = self.render_tree.get_mut(subtree_id) {
                let _ = node.begin_detachment();
            }
        }

        // ADR-0013: detach every node before eviction so attach-time
        // subscriptions can unsubscribe while their storage still exists.
        for &subtree_id in subtree.ids() {
            if let Some(node) = self.render_tree.get_mut(subtree_id) {
                let _ = node.clear_parent_data();
                let _ = node.detach();
            }
        }

        self.scheduler.evict(subtree.members());
        // Layout-poison records for the removed subtree die here too, so
        // the map does not accumulate entries for nodes that no longer
        // exist.
        self.layout_poison.evict(subtree.members());
        // So do boundary records. A stale entry can never be SERVED to a
        // different node — `RenderId` is generational, so a recycled slab slot
        // misses the map — but without this the map grows for the lifetime of
        // the owner as boundaries come and go. Their nodes are retired by the
        // paint that republishes the parent naming them.
        for id in subtree.members() {
            if let Some(record) = self.boundary_records.remove(id) {
                self.follower_boundaries.remove(id);
                self.orphaned_nodes.push(record.node());
            }
        }

        let mut removed_nodes = std::mem::take(&mut self.removed_nodes);
        self.subtree_buffers = self.render_tree.take_subtree(subtree, &mut removed_nodes);
        let count = removed_nodes.len();
        if self.root_id == Some(id) {
            self.root_id = None;
        }
        if let Some(parent_id) = former_parent {
            self.note_render_child_membership_changed(parent_id);
        }
        tracing::debug!(?id, count, "remove_render_object: subtree disposed");
        // Destructors run last, against a committed tree and owner (ADR-0252,
        // ADR-0287): one that panics leaves nothing half-removed.
        // `clear` drops every node even when one panics; the buffer is kept
        // only when none did.
        removed_nodes.clear();
        self.removed_nodes = removed_nodes;
        count
    }

    /// Adopt an existing render node and invalidate every parent phase whose
    /// output depends on child membership.
    pub fn adopt_render_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        let former_parent = self.render_tree.parent(child_id);
        if former_parent == Some(parent_id) {
            return;
        }
        if let Some(former_parent_id) = former_parent {
            self.render_tree.drop_child(former_parent_id, child_id);
        }
        self.render_tree.adopt_child(parent_id, child_id);
        if let Some(former_parent_id) = former_parent.filter(|id| *id != parent_id) {
            self.note_render_child_membership_changed(former_parent_id);
        }
        self.note_render_child_membership_changed(parent_id);
    }

    /// Invalidates every parent phase whose output depends on child membership.
    pub(crate) fn note_render_child_membership_changed(&mut self, parent_id: RenderId) {
        self.apply_render_update_impact(
            parent_id,
            RenderUpdateImpact::LAYOUT
                | RenderUpdateImpact::COMPOSITING_BITS
                | RenderUpdateImpact::SEMANTICS,
        );
    }

    /// Record a pure sibling-order change. Membership and semantics content
    /// are unchanged, but the parent must lay its children out again.
    pub fn note_render_children_reordered(&mut self, parent_id: RenderId) {
        self.apply_render_update_impact(parent_id, RenderUpdateImpact::LAYOUT);
    }
}

/// A batch of child-list edits, lent by
/// [`PipelineOwner::edit_render_children`], which marks the parents it
/// changed once the batch ends.
#[derive(Debug)]
pub struct RenderChildrenEdit<'a> {
    tree: &'a mut RenderTree,
    changed: SlotSet<RenderId>,
    membership_changed: SlotSet<RenderId>,
}

impl RenderChildrenEdit<'_> {
    /// The tree as the edits so far left it.
    #[must_use]
    pub fn tree(&self) -> &RenderTree {
        self.tree
    }

    /// Makes `children`, in that order, the whole child list of `parent`, as
    /// [`PipelineOwner::set_render_children`] does, and returns whether the
    /// list changed. `parent` is marked when the batch ends, and so is every
    /// parent a child left.
    pub fn set_children(&mut self, parent: RenderId, children: &[RenderId]) -> bool {
        let update = self.tree.set_children(parent, children);
        if update.changed {
            self.changed.insert(parent);
        }
        if update.membership_changed {
            self.membership_changed.insert(parent);
        }
        for donor in update.donors {
            self.changed.insert(donor);
            self.membership_changed.insert(donor);
        }
        update.changed
    }

    /// Detaches `child` from `parent`, writing both directions of the edge,
    /// and leaves `child` a root; `parent` is marked when the batch ends.
    ///
    /// A stale `parent` or `child` makes the call a no-op.
    ///
    /// # Panics (debug only)
    ///
    /// If `child`'s current parent is not `parent`.
    pub fn detach_child(&mut self, parent: RenderId, child: RenderId) {
        if !self.tree.contains(parent) || !self.tree.contains(child) {
            return;
        }
        self.tree.drop_child(parent, child);
        self.changed.insert(parent);
        self.membership_changed.insert(parent);
    }
}

/// The parent data of one node, lent by
/// [`PipelineOwner::update_parent_data`].
///
/// Parent data is the record a render parent keeps on each child (a flex
/// factor, a stack position, a sliver index), stored on the child.
#[derive(Debug)]
pub struct ParentDataSlot<'a> {
    node: &'a mut RenderNode,
}

impl ParentDataSlot<'_> {
    /// The node's parent data, `None` before any is installed.
    #[must_use]
    pub fn get(&self) -> Option<&dyn ParentData> {
        self.node.parent_data()
    }

    /// The node's parent data, mutably.
    pub fn get_mut(&mut self) -> Option<&mut dyn ParentData> {
        self.node.parent_data_mut()
    }

    /// The node's parent data when it is a `T`.
    pub fn downcast_mut<T: ParentData>(&mut self) -> Option<&mut T> {
        self.get_mut()?.downcast_mut::<T>()
    }

    /// Installs `data`, replacing what the node held. The replaced box is
    /// dropped without [`ParentData::detach`]: the node stays attached.
    pub fn set(&mut self, data: Box<dyn ParentData>) {
        self.node.set_parent_data(data);
    }

    /// Detaches and removes the node's parent data; returns whether it had
    /// any.
    pub fn clear(&mut self) -> bool {
        self.node.clear_parent_data()
    }
}
