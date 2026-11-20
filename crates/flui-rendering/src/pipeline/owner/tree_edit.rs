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
