//! Raw edits of a render tree that skip the pipeline owner's dirty marks.
//!
//! Code outside this crate edits an owner's tree only through the owner's
//! operations (`PipelineOwner::set_render_children`, `update_render_object`,
//! `update_parent_data`, …), each of which marks what its edit invalidates.
//! A test reaches for these functions only to build a state no such operation
//! produces (a one-sided child edge, a stale child id, a node whose dirty flags
//! were cleared behind the owner's queues, a node laid out alone), or to
//! exercise the storage of a [`RenderTree`] no owner holds.
//!
//! Each function takes either a bare [`RenderTree`] or a [`PipelineOwner`],
//! whose tree it edits in place ([`RawTree`]).

use flui_foundation::RenderId;

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    error::RenderResult,
    parent_data::ParentData,
    pipeline::{PipelineOwner, PipelinePhase, TextSource},
    protocol::{BoxProtocol, SliverProtocol},
    storage::{ChildrenUpdate, RenderNode, RenderTree},
    traits::RenderObject,
};

mod sealed {
    use crate::storage::RenderTree;

    /// Lends the tree a [`super::RawTree`] function edits; unnameable outside
    /// the crate, so the lend stays inside this module's functions.
    pub trait Sealed {
        fn raw_tree(&mut self) -> &mut RenderTree;
    }
}

/// What a raw edit can be made on: a bare [`RenderTree`], or the tree a
/// [`PipelineOwner`] holds.
pub trait RawTree: sealed::Sealed {}

impl sealed::Sealed for RenderTree {
    fn raw_tree(&mut self) -> &mut RenderTree {
        self
    }
}

impl RawTree for RenderTree {}

impl<Phase: PipelinePhase> sealed::Sealed for PipelineOwner<Phase> {
    fn raw_tree(&mut self) -> &mut RenderTree {
        self.render_tree_mut()
    }
}

impl<Phase: PipelinePhase> RawTree for PipelineOwner<Phase> {}

/// Inserts a Box render object with no parent: no attach, no dirty mark.
pub fn insert_box(
    tree: &mut impl RawTree,
    render_object: Box<dyn RenderObject<BoxProtocol>>,
) -> RenderId {
    tree.raw_tree().insert_box(render_object)
}

/// Inserts a Sliver render object with no parent: no attach, no dirty mark.
pub fn insert_sliver(
    tree: &mut impl RawTree,
    render_object: Box<dyn RenderObject<SliverProtocol>>,
) -> RenderId {
    tree.raw_tree().insert_sliver(render_object)
}

/// Inserts a Box render object as the last child of `parent`, writing both
/// directions of the edge: no attach, no dirty mark. `None` for a stale
/// `parent`.
pub fn insert_box_child(
    tree: &mut impl RawTree,
    parent: RenderId,
    render_object: Box<dyn RenderObject<BoxProtocol>>,
) -> Option<RenderId> {
    tree.raw_tree().insert_box_child(parent, render_object)
}

/// Inserts a Sliver render object as the last child of `parent`, writing both
/// directions of the edge: no attach, no dirty mark. `None` for a stale
/// `parent`.
pub fn insert_sliver_child(
    tree: &mut impl RawTree,
    parent: RenderId,
    render_object: Box<dyn RenderObject<SliverProtocol>>,
) -> Option<RenderId> {
    tree.raw_tree().insert_sliver_child(parent, render_object)
}

/// Makes `children` the whole child list of `parent`, as
/// `PipelineOwner::set_render_children` does, but marks nothing: a test of
/// what the moved nodes' own marks reach. Returns what the edit changed.
pub fn set_children(
    tree: &mut impl RawTree,
    parent: RenderId,
    children: &[RenderId],
) -> ChildrenUpdate {
    tree.raw_tree().set_children(parent, children)
}

/// Appends `child` to `parent`'s child list without writing `child`'s parent
/// link: the one-sided edge a cycle, a duplicate child or a stale child id
/// needs.
pub fn append_unlinked_child(tree: &mut impl RawTree, parent: RenderId, child: RenderId) {
    tree.raw_tree().append_unlinked_child(parent, child);
}

/// Removes the node at `id` alone, unlinking it from its parent's list and
/// leaving its children in the slab; returns it for the caller to drop.
pub fn remove_shallow(tree: &mut impl RawTree, id: RenderId) -> Option<RenderNode> {
    tree.raw_tree().remove_shallow(id)
}

/// Removes the subtree rooted at `id` from the tree alone, with no detach and
/// no owner bookkeeping; returns how many nodes it removed.
pub fn remove_subtree(tree: &mut impl RawTree, id: RenderId) -> usize {
    tree.raw_tree().remove_recursive(id)
}

/// Installs `data` as the parent data of the node at `id`; a stale `id` is a
/// no-op. On an owner's tree, `PipelineOwner::update_parent_data` is the
/// marked form.
pub fn set_parent_data(tree: &mut impl RawTree, id: RenderId, data: Box<dyn ParentData>) {
    if let Some(node) = tree.raw_tree().get_mut(id) {
        node.set_parent_data(data);
    }
}

/// Records `root` as the bare tree's root.
pub fn set_root(tree: &mut RenderTree, root: Option<RenderId>) {
    tree.set_root(root);
}

/// Removes every node; see `RenderTree::clear` for the order it commits in.
pub fn clear(tree: &mut impl RawTree) {
    tree.raw_tree().clear();
}

/// Ages the slot the next insert takes to the last generation an id can
/// carry, so a test reaches generation exhaustion without `u32::MAX`
/// removals. `false` when no slot is free.
pub fn age_next_free_slot(tree: &mut impl RawTree) -> bool {
    tree.raw_tree().age_next_free_slot_for_test()
}

/// Replaces the owner's tree with `tree` and returns the one it held, with
/// none of the owner's records updated.
pub fn replace_tree<Phase: PipelinePhase>(
    owner: &mut PipelineOwner<Phase>,
    tree: RenderTree,
) -> RenderTree {
    std::mem::replace(owner.render_tree_mut(), tree)
}

/// Clears the layout, paint and compositing-bits flags of the node at `id`,
/// leaving the owner's queues as they are, so a test sees which flags the
/// next operation sets. A stale `id` is a no-op.
pub fn clear_dirty_flags(tree: &mut impl RawTree, id: RenderId) {
    if let Some(node) = tree.raw_tree().get(id) {
        node.clear_needs_layout();
        node.clear_needs_paint();
        node.clear_needs_compositing_bits_update();
    }
}

/// Lays out the Sliver leaf at `id` alone, outside any pass, and returns what
/// its layout returned; `None` when `id` is stale or not a Sliver node.
pub fn layout_sliver_leaf(
    tree: &mut impl RawTree,
    id: RenderId,
    constraints: SliverConstraints,
    text: TextSource<'_>,
    has_parent: bool,
) -> Option<RenderResult<SliverGeometry>> {
    let entry = tree.raw_tree().get_mut(id)?.as_sliver_mut()?;
    Some(entry.layout_leaf_only(constraints, text, has_parent))
}
