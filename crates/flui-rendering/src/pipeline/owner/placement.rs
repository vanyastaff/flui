//! Where a render node is drawn in the window: the mapping from its local coordinates into the
//! window and the part of the window its ancestors' clips leave it, as the paint walk composes
//! them.
//!
//! The gate ([`PipelineOwner::paint_walk_draws`]) and the step from a parent to a child
//! ([`PipelineOwner::child_placement`]) are the ones the paint walk applies: a node the paint walk
//! skips is not drawn, a transform step is
//! [`RenderObject::apply_paint_transform`](crate::traits::RenderObject::apply_paint_transform)
//! with the follower placement the last composite resolved, and a clip is the parent's
//! [`describe_approximate_paint_clip`](crate::traits::RenderObject::describe_approximate_paint_clip).
//! The input-region walk and the presented-node walk compose through them, and
//! [`PipelineOwner::presented_placement`] answers one node by them, so none of the three
//! disagrees on what is drawn where. Every hook they call is pure in `(self, size)` and runs no
//! user code.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, Rect, Size};

use crate::input_regions::map_rect;
use crate::pipeline::phase::PipelinePhase;
use crate::storage::RenderNode;

use super::PipelineOwner;

/// Where one render node's local space is drawn in the window after the last frame
/// ([`PipelineOwner::presented_placement`]), in window-logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PresentedPlacement {
    /// The mapping from the node's local coordinates into the window, as the paint walk
    /// composes it for the node: every ancestor's paint transform and the follower placement
    /// the last composite resolved, not the node's own drawn transform.
    pub transform: Matrix4,
    /// The part of the window every ancestor's paint clip leaves the node; empty when none
    /// is left.
    pub clip: Rect,
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Where `id`'s local space is drawn in the window after the last frame: `None` when the
    /// paint walk draws nothing of it (it or an ancestor is skipped, offstage or awaiting
    /// layout, or its parent's last paint left it out), when it is not in the tree under the
    /// root, or when its mapping into the window is not finite. Costs the node's depth.
    #[must_use]
    pub fn presented_placement(&self, id: RenderId) -> Option<PresentedPlacement> {
        let root = self.root_id()?;
        let mut parent = self.render_tree.get(root)?;
        let mut path = Vec::new();
        let mut current = id;
        while current != root {
            path.push(current);
            current = self.render_tree.parent(current)?;
        }
        if !self.paint_walk_draws(root, parent) {
            return None;
        }
        let mut placement = self.root_placement(root)?;
        let mut parent_id = root;
        for &child_id in path.iter().rev() {
            let child = self.render_tree.get(child_id)?;
            let slot = parent.children().iter().position(|&id| id == child_id)?;
            let painted = self
                .last_child_paint_orders
                .get(&parent_id)
                .is_none_or(|order| order.iter().any(|entry| entry.slot == slot));
            if !painted || !self.paint_walk_draws(child_id, child) {
                return None;
            }
            placement =
                self.child_placement((parent_id, parent), slot, (child_id, child), placement)?;
            parent_id = child_id;
            parent = child;
        }
        parent.laid_out_size()?;
        Some(placement)
    }

    /// The root's placement: the window, its laid-out box, under the root's follower
    /// placement. `None` before the root's first layout.
    pub(super) fn root_placement(&self, root: RenderId) -> Option<PresentedPlacement> {
        let size = self.render_tree.get(root)?.laid_out_size()?;
        let mut transform = Matrix4::IDENTITY;
        self.compose_follower_placement(root, &mut transform);
        Some(PresentedPlacement {
            transform,
            clip: Rect::from_origin_size(Point::ZERO, size),
        })
    }

    /// Whether the paint walk draws `node` (`id`) and its subtree: not a follower the last
    /// composite resolved to hidden, not skipped, laid out, and not a sliver its geometry
    /// hides.
    pub(super) fn paint_walk_draws(&self, id: RenderId, node: &RenderNode) -> bool {
        !(self.last_hidden_follower_ids.contains(&id)
            || node.skip_paint()
            || node.needs_layout()
            || node
                .geometry_sliver()
                .is_some_and(|geometry| !geometry.visible))
    }

    /// The placement of `child` (`child_id`), which `parent` (`parent_id`) draws in `slot`,
    /// from `parent`'s own `placement`: the parent's clip for the slot mapped through the
    /// parent's transform (an unmappable clip hides the child rather than lifting the
    /// limit), then the parent's paint transform for the slot and the child's follower
    /// placement. `None` when the parent has no laid-out size, did not place the child in
    /// its current layout, or the mapping is not finite.
    pub(super) fn child_placement(
        &self,
        (parent_id, parent): (RenderId, &RenderNode),
        slot: usize,
        (child_id, child): (RenderId, &RenderNode),
        placement: PresentedPlacement,
    ) -> Option<PresentedPlacement> {
        let size = parent.laid_out_size()?;
        if !child.was_placed_by(parent_id, parent.layout_generation()) {
            return None;
        }
        let mut clip = placement.clip;
        if let Some(local) = approximate_paint_clip(parent, slot, size) {
            let mapped = map_rect(&placement.transform, local).unwrap_or(Rect::ZERO);
            clip = clip.intersect(&mapped).unwrap_or(Rect::ZERO);
        }
        let mut transform = placement.transform;
        parent.apply_paint_transform(slot, child.offset(), &mut transform)?;
        self.compose_follower_placement(child_id, &mut transform);
        if transform
            .to_col_major_array()
            .iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        Some(PresentedPlacement { transform, clip })
    }

    /// Composes the transform the last composite resolved for follower `id` (a
    /// `CompositedTransformFollower` placed against its leader), the same placement the
    /// engine paints its subtree with and hit-testing applies.
    fn compose_follower_placement(&self, id: RenderId, transform: &mut Matrix4) {
        if let Some(follower) = self.last_follower_transforms.get(&id) {
            *transform *= *follower;
        }
    }
}

fn approximate_paint_clip(node: &RenderNode, slot: usize, size: Size) -> Option<Rect> {
    match node {
        RenderNode::Box(entry) => entry
            .render_object()
            .describe_approximate_paint_clip(slot, size),
        RenderNode::Sliver(entry) => entry
            .render_object()
            .describe_approximate_paint_clip(slot, size),
    }
}
