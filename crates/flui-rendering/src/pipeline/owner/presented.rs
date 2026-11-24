//! What each render node shows in the window now: its box mapped into window coordinates, the
//! part of it its ancestors' clips leave, and the opacity it is drawn at (ADR-0321 §1).
//!
//! The walk applies the paint walk's gate and composition step (the `placement` module, which
//! the input-region walk and [`PipelineOwner::presented_placement`] share): a node the paint
//! walk skips, and a child its parent left out of the last paint, are not reported. Opacity is
//! the product of the group opacity of the scopes a parent painted the child in and the
//! [`PaintEffects::opacity`](crate::traits::PaintEffects::opacity) of the node and its ancestors.
//! Every hook it calls is pure in `(self, size)` and runs no user code.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, Rect};

use crate::input_regions::map_rect;
use crate::storage::RenderNode;

use super::PipelineOwner;
use super::placement::PresentedPlacement;
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
    /// The node's laid-out box as drawn, under the transform its own drawing is placed under
    /// (`RenderObject::drawn_transform`), mapped into the window: the bounding box of its four
    /// corners.
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
    placement: PresentedPlacement,
    opacity: f64,
}

impl PipelineOwner {
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
        let Some(placement) = self.root_placement(root) else {
            return nodes;
        };
        let inherited = Inherited {
            parent: None,
            slot: 0,
            placement,
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
        if !self.paint_walk_draws(id, node) {
            return;
        }
        let Some(size) = node.laid_out_size() else {
            return;
        };
        let effects = node.paint_effects();
        // The node's own drawing is placed under its drawn transform (its effect layer, or an
        // offset its paint applies itself); its children reach theirs through
        // `apply_paint_transform` below.
        let mut content = inherited.placement.transform;
        if let Some(own) = drawn_transform(node, size) {
            content *= own;
        }
        let Ok(rect) = map_rect(&content, Rect::from_origin_size(Point::ZERO, size)) else {
            return;
        };
        let own_alpha = effects
            .opacity
            .map_or(1.0, crate::traits::PaintOpacity::alpha);
        let opacity = inherited.opacity * own_alpha;
        let visible = inherited
            .placement
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
            name: node.kind(),
            kind,
            rect,
            visible,
            opacity,
        });

        let children = node.children();
        let painted = self.last_child_paint_orders.get(&id);
        let count = painted.map_or(children.len(), smallvec::SmallVec::len);
        for position in 0..count {
            let (slot, scope_opacity) = painted.map_or((position, 1.0), |order| {
                (order[position].slot, order[position].opacity)
            });
            let Some(&child_id) = children.get(slot) else {
                continue;
            };
            let Some(child) = self.render_tree.get(child_id) else {
                continue;
            };
            let Some(placement) =
                self.child_placement((id, node), slot, (child_id, child), inherited.placement)
            else {
                continue;
            };
            self.collect_presented(
                child_id,
                Inherited {
                    parent: Some(id),
                    slot,
                    placement,
                    opacity: opacity * scope_opacity,
                },
                out,
            );
        }
    }
}

fn drawn_transform(node: &RenderNode, size: flui_foundation::geometry::Size) -> Option<Matrix4> {
    match node {
        RenderNode::Box(entry) => entry.render_object().drawn_transform(size),
        RenderNode::Sliver(entry) => entry.render_object().drawn_transform(size),
    }
}
