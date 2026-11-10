//! What each render node shows in the window now: its box mapped into window coordinates, the
//! part of it its ancestors' clips leave, and the opacity it is drawn at (ADR-0321 §1).
//!
//! The walk applies the same gates and the same composition as the paint walk and the input
//! region walk: a node the paint walk skips is not reported, a transform step is
//! [`RenderObject::apply_paint_transform`](crate::traits::RenderObject::apply_paint_transform)
//! with the follower placement the last composite resolved, a clip is the parent's
//! [`describe_approximate_paint_clip`](crate::traits::RenderObject::describe_approximate_paint_clip),
//! and opacity is the product of the
//! [`PaintEffects::opacity`](crate::traits::PaintEffects::opacity) of the node and its ancestors.
//! Every hook it calls is pure in `(self, size)` and runs no user code.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, Rect};

use crate::input_regions::map_rect;
use crate::pipeline::phase::PipelinePhase;
use crate::storage::RenderNode;

use super::PipelineOwner;
use super::subtree_arena::ensure_stack;

/// The protocol a [`PresentedNode`] lays out under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PresentedKind {
    /// A box render object.
    Box,
    /// A sliver render object.
    Sliver,
}

/// One render node as the window shows it after the last frame.
///
/// Geometry is in window-logical coordinates, the space the root lays out in; multiply by the
/// window's scale factor for device pixels (ADR-0098 keeps the two apart).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PresentedNode {
    /// The node.
    pub id: RenderId,
    /// The node's parent, `None` for the root.
    pub parent: Option<RenderId>,
    /// The child slot the node occupies under its parent, `0` for the root.
    pub slot: usize,
    /// The render object's type name, without its module path.
    pub name: &'static str,
    /// The node's protocol.
    pub kind: PresentedKind,
    /// The node's laid-out box mapped into the window: the bounding box of its four corners.
    pub rect: Rect,
    /// The part of [`Self::rect`] inside every ancestor's clip and the window, `None` when no
    /// part of it is.
    pub visible: Option<Rect>,
    /// The opacity the node is drawn at, `0.0..=1.0`: its own group opacity times every
    /// ancestor's.
    pub opacity: f64,
}

/// The state the walk threads from a node to its children.
#[derive(Clone, Copy)]
struct Inherited {
    parent: Option<RenderId>,
    slot: usize,
    transform: Matrix4,
    clip: Rect,
    opacity: f64,
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Every node the last paint drew, in paint order (a node before its descendants), with
    /// the geometry and opacity the window shows it at (ADR-0321 §1). Empty before the root's
    /// first layout.
    ///
    /// A node whose transform to the window is not finite, or whose corners reach the
    /// perspective horizon, is left out with its subtree: no non-finite geometry is published.
    #[must_use]
    pub fn presented_nodes(&self) -> Vec<PresentedNode> {
        let mut nodes = Vec::new();
        let Some(root) = self.root_id() else {
            return nodes;
        };
        let Some(window) = self
            .render_tree
            .get(root)
            .and_then(RenderNode::laid_out_size)
            .map(|size| Rect::from_origin_size(Point::ZERO, size))
        else {
            return nodes;
        };
        let mut transform = Matrix4::IDENTITY;
        self.compose_presented_follower(root, &mut transform);
        let inherited = Inherited {
            parent: None,
            slot: 0,
            transform,
            clip: window,
            opacity: 1.0,
        };
        self.collect_presented(root, inherited, &mut nodes);
        nodes
    }

    fn collect_presented(&self, id: RenderId, inherited: Inherited, out: &mut Vec<PresentedNode>) {
        ensure_stack(|| self.collect_presented_impl(id, inherited, out));
    }

    fn collect_presented_impl(
        &self,
        id: RenderId,
        inherited: Inherited,
        out: &mut Vec<PresentedNode>,
    ) {
        let Some(node) = self.render_tree.get(id) else {
            return;
        };
        if self.last_hidden_follower_ids.contains(&id)
            || node.skip_paint()
            || node.needs_layout()
            || node
                .geometry_sliver()
                .is_some_and(|geometry| !geometry.visible)
        {
            return;
        }
        let Some(size) = node.laid_out_size() else {
            return;
        };
        let Ok(rect) = map_rect(
            &inherited.transform,
            Rect::from_origin_size(Point::ZERO, size),
        ) else {
            return;
        };
        let own_alpha = node
            .paint_effects()
            .opacity
            .map_or(1.0, crate::traits::PaintOpacity::alpha);
        let opacity = inherited.opacity * own_alpha;
        let visible = inherited
            .clip
            .intersect(&rect)
            .filter(|kept| !kept.is_empty());
        let kind = match node {
            RenderNode::Box(_) => PresentedKind::Box,
            RenderNode::Sliver(_) => PresentedKind::Sliver,
        };
        out.push(PresentedNode {
            id,
            parent: inherited.parent,
            slot: inherited.slot,
            name: short_name(node.debug_name()),
            kind,
            rect,
            visible,
            opacity,
        });

        let generation = node.layout_generation();
        let children = node.children();
        let painted = self.last_child_paint_orders.get(&id);
        let count = painted.map_or(children.len(), smallvec::SmallVec::len);
        for position in 0..count {
            let slot = painted.map_or(position, |order| order[position]);
            let Some(&child_id) = children.get(slot) else {
                continue;
            };
            let Some(child) = self.render_tree.get(child_id) else {
                continue;
            };
            if !child.was_placed_by(id, generation) || !visits_presented_child(node, slot, size) {
                continue;
            }
            let mut clip = inherited.clip;
            if let Some(local) = approximate_paint_clip(node, slot, size) {
                // An unmappable clip hides the subtree rather than lifting the limit.
                let mapped = map_rect(&inherited.transform, local).unwrap_or(Rect::ZERO);
                clip = clip.intersect(&mapped).unwrap_or(Rect::ZERO);
            }
            let mut transform = inherited.transform;
            if node
                .apply_paint_transform(slot, child.offset(), &mut transform)
                .is_none()
            {
                continue;
            }
            self.compose_presented_follower(child_id, &mut transform);
            if transform
                .to_col_major_array()
                .iter()
                .any(|value| !value.is_finite())
            {
                continue;
            }
            self.collect_presented(
                child_id,
                Inherited {
                    parent: Some(id),
                    slot,
                    transform,
                    clip,
                    opacity,
                },
                out,
            );
        }
    }

    fn compose_presented_follower(&self, id: RenderId, transform: &mut Matrix4) {
        if let Some(follower) = self.last_follower_transforms.get(&id) {
            *transform *= *follower;
        }
    }
}

/// `std::any::type_name` without its module path or generic arguments' paths.
fn short_name(name: &'static str) -> &'static str {
    let base = name.split('<').next().unwrap_or(name);
    let start = base.rfind("::").map_or(0, |at| at + 2);
    &name[start..]
}

fn visits_presented_child(
    node: &RenderNode,
    slot: usize,
    size: flui_foundation::geometry::Size,
) -> bool {
    match node {
        RenderNode::Box(entry) => entry.render_object().visits_child_for_semantics(slot, size),
        RenderNode::Sliver(entry) => entry.render_object().visits_child_for_semantics(slot, size),
    }
}

fn approximate_paint_clip(
    node: &RenderNode,
    slot: usize,
    size: flui_foundation::geometry::Size,
) -> Option<Rect> {
    match node {
        RenderNode::Box(entry) => entry
            .render_object()
            .describe_approximate_paint_clip(slot, size),
        RenderNode::Sliver(entry) => entry
            .render_object()
            .describe_approximate_paint_clip(slot, size),
    }
}
