//! [`RenderGeometry`], and the selection of render objects by type that the typed reads share.

use std::any::Any;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Point, Rect, Size};
use flui_rendering::constraints::SliverGeometry;
use flui_rendering::pipeline::PipelineOwner;
use rustc_hash::FxHashSet;

/// Where one render object was laid out in the frame, copied out of it: what
/// [`Frame::only`](super::Frame::only), [`Frame::render_all`](super::Frame::render_all) and
/// [`Frame::geometry`](super::Frame::geometry) read (`getRect`, `getSize` and
/// `renderObject.localToGlobal` in Flutter 3.50.1's `flutter_test`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct RenderGeometry {
    /// The render object, as the frame's render tree names it.
    pub render: RenderId,
    /// A box's committed size; `None` for a sliver, and for a box not yet laid out.
    pub size: Option<Size>,
    /// A sliver's committed geometry; `None` for a box, and for a sliver not yet laid out.
    pub sliver: Option<SliverGeometry>,
    /// The paint offset its parent placed it at, in the parent's space.
    pub offset: Offset,
    /// The matrix from its own space into the render root's, the window's logical pixels:
    /// `None` when a render object on the way has not been laid out, never "no transform".
    pub to_window: Option<Matrix4>,
}

impl RenderGeometry {
    /// The geometry of `id` in `owner`'s tree.
    ///
    /// # Panics
    ///
    /// When `id` is no node of the tree: callers select it from the tree first.
    pub(super) fn of(owner: &PipelineOwner, id: RenderId) -> Self {
        let node = owner
            .render_tree()
            .get(id)
            .expect("BUG: a geometry is read of a node the selection found in the tree");
        let to_window = owner
            .root_id()
            .and_then(|root| owner.transform_to(id, root));
        Self {
            render: id,
            size: owner.box_size(id),
            sliver: owner.sliver_layout(id).map(|(_, geometry)| geometry),
            offset: node.offset(),
            to_window,
        }
    }

    /// A box's bounds in the window's logical pixels: its size from its origin, mapped by
    /// [`to_window`](Self::to_window), the bounding box where the matrix rotates or skews.
    /// `None` for a sliver and for a box whose size or matrix is not known.
    #[must_use]
    pub fn rect(&self) -> Option<Rect> {
        let size = self.size?;
        let to_window = self.to_window?;
        Some(to_window.transform_rect(&Rect::from_origin_size(Point::ZERO, size)))
    }
}

/// `start` and every render object below it, depth first, children in paint order; the whole
/// tree from its root when `start` is `None`. An explicit stack, so a deep tree costs no Rust
/// stack.
fn walk(owner: &PipelineOwner, start: Option<RenderId>) -> Vec<RenderId> {
    let tree = owner.render_tree();
    let mut stack: Vec<RenderId> = start.or_else(|| owner.root_id()).into_iter().collect();
    let mut order = Vec::new();
    while let Some(id) = stack.pop() {
        if tree.get(id).is_none() {
            continue;
        }
        order.push(id);
        stack.extend(tree.children(id).iter().rev());
    }
    order
}

/// Every render object of type `R` at or under `start` (the whole tree when `None`), in tree
/// order.
pub(super) fn ids_of_type<R: Any>(owner: &PipelineOwner, start: Option<RenderId>) -> Vec<RenderId> {
    let tree = owner.render_tree();
    walk(owner, start)
        .into_iter()
        .filter(|id| {
            tree.get(*id)
                .and_then(|node| node.downcast_render_object::<R>())
                .is_some()
        })
        .collect()
}

/// The geometry of every render object of type `R` at or under `start`, in tree order.
pub(super) fn of_type<R: Any>(
    owner: &PipelineOwner,
    start: Option<RenderId>,
) -> Vec<RenderGeometry> {
    ids_of_type::<R>(owner, start)
        .into_iter()
        .map(|id| RenderGeometry::of(owner, id))
        .collect()
}

/// `start` and every render object below it.
pub(super) fn subtree(owner: &PipelineOwner, start: RenderId) -> FxHashSet<RenderId> {
    walk(owner, Some(start)).into_iter().collect()
}
