//! Reads of laid-out geometry and conversion between node-local and global coordinates (ADR-0021).

use flui_foundation::RenderId;
use flui_foundation::geometry::Matrix4;

use crate::pipeline::phase::PipelinePhase;

use super::PipelineOwner;

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// The committed laid-out size of a **box** node, or `None` if `id` is
    /// missing, is a sliver, or has not been laid out yet.
    ///
    /// The production twin of `flui_rendering::testing::box_geometry`, which is
    /// behind the `testing` feature and therefore unreachable from shipped code.
    #[must_use]
    pub fn box_size(&self, id: RenderId) -> Option<flui_foundation::geometry::Size> {
        self.render_tree.get(id)?.as_box()?.state().geometry()
    }

    /// The constraints a **sliver** node was last laid out under and the
    /// geometry it committed, or `None` if `id` is missing, is a box, or has
    /// not been laid out yet. The sliver twin of [`Self::box_size`]: its
    /// painted region is `paint_extent` along `constraints.axis()` by
    /// `cross_axis_extent`, at its own origin.
    #[must_use]
    pub fn sliver_layout(
        &self,
        id: RenderId,
    ) -> Option<(
        crate::constraints::SliverConstraints,
        crate::constraints::SliverGeometry,
    )> {
        let state = self.render_tree.get(id)?.as_sliver()?.state();
        Some((*state.constraints()?, state.geometry()?))
    }

    /// The matrix mapping points in `descendant`'s local coordinate space into
    /// `ancestor`'s.
    ///
    /// This is a **strict descendant → ancestor walk**. Two nodes that merely
    /// share a common ancestor would need the target's half of the path
    /// inverted; nothing in FLUI needs that yet, and the narrow
    /// walk needs no inverse, so it cannot fail on a singular matrix.
    ///
    /// # When this returns `None`
    ///
    /// **Never as "no transform".** A `None` means the question cannot be
    /// answered, for one of two reasons:
    ///
    /// 1. `ancestor` is not an ancestor of `descendant` — including a missing id,
    ///    or two nodes in different trees.
    /// 2. Some node on the path has **not been laid out**. Every step needs its
    ///    parent's committed size, and a size-dependent object — a `FittedBox`, a
    ///    rotation about its centre, a `RenderFractionalTranslation`, a
    ///    `RenderFlow` — yields a different, entirely plausible matrix if that
    ///    size is assumed to be `Size::ZERO`.
    ///
    /// `Some(IDENTITY)` when `descendant == ancestor`: a node's own space maps to
    /// itself whatever its size, and no step runs.
    ///
    /// # Composition
    ///
    /// Walks up from `descendant` collecting the path, then composes downward
    /// from `ancestor`, so the outermost step ends up leftmost:
    /// `T = A · B · … · P`, and a point `v` in `descendant`-local space lands at
    /// `T · v` in `ancestor`-local space. Each step is
    /// [`RenderObject::apply_paint_transform`](crate::traits::RenderObject::apply_paint_transform).
    #[must_use]
    pub fn transform_to(&self, descendant: RenderId, ancestor: RenderId) -> Option<Matrix4> {
        if descendant == ancestor {
            return self.render_tree.get(ancestor).map(|_| Matrix4::IDENTITY);
        }
        self.render_tree.get(descendant)?;

        // `path` ends up as [child-of-ancestor, …, descendant]: the nodes whose
        // local spaces we step *into*, in outermost-first order.
        //
        // Running out of parents is the **only** way to learn that `ancestor` is
        // not an ancestor. Everything below this loop is then guaranteed by
        // the tree's own invariants.
        let mut path = self.transform_paths.checkout(0);
        let mut current = descendant;
        loop {
            path.push(current);
            let parent = self.render_tree.parent(current)?;
            if parent == ancestor {
                break;
            }
            current = parent;
        }
        path.reverse();

        let mut transform = Matrix4::IDENTITY;
        let mut parent = ancestor;
        for &child in path.iter() {
            let parent_node = self
                .render_tree
                .get(parent)
                .expect("BUG: transform_to walked to a parent that is not in the render tree");
            // `child`'s parent *is* `parent` — the walk above established it — so
            // the tree's parent/children links are inconsistent if this misses.
            // Not a `?`: a silent `None` here would masquerade as "not an
            // ancestor" and mask the corruption.
            let child_index = parent_node
                .children()
                .iter()
                .position(|&id| id == child)
                .expect("BUG: render tree parent link has no matching child link");
            let child_offset = self
                .render_tree
                .get(child)
                .expect("BUG: transform_to walked through a node that is not in the render tree")
                .offset();
            // `None` when `parent` has not been laid out. Not "no transform":
            // a size-dependent object produces a different, plausible matrix at
            // `Size::ZERO`. See `RenderNode::apply_paint_transform`.
            parent_node.apply_paint_transform(child_index, child_offset, &mut transform)?;
            parent = child;
        }
        Some(transform)
    }

    /// Converts `point` from `id`'s local coordinate space into `ancestor`'s, or
    /// into the render root's when `ancestor` is `None`.
    ///
    /// The point is transformed by [`transform_to`](Self::transform_to).
    ///
    /// `None` when [`transform_to`](Self::transform_to) is `None`, or when there
    /// is no render root to convert against.
    #[must_use]
    pub fn local_to_global(
        &self,
        id: RenderId,
        point: flui_foundation::geometry::Point,
        ancestor: Option<RenderId>,
    ) -> Option<flui_foundation::geometry::Point> {
        let ancestor = ancestor.or(self.root_id)?;
        let transform = self.transform_to(id, ancestor)?;
        let (x, y) = transform.transform_point(point.x, point.y);
        Some(flui_foundation::geometry::Point::new(x, y))
    }

    /// The inverse of [`local_to_global`](Self::local_to_global): converts
    /// `point` from `ancestor`'s space (the render root's, when `None`) into
    /// `id`'s local space.
    ///
    /// Intersects the point's screen ray with the object's local `z = 0` plane,
    /// including perspective. A parallel ray, an intersection at infinity or
    /// a point behind the camera returns `None`.
    ///
    /// `None` when the transform is missing or **singular** — a zero-scale
    /// `FittedBox`, for instance, which maps every local point to one global
    /// point and cannot be inverted. A `None` says so instead of inventing an
    /// answer.
    #[must_use]
    pub fn global_to_local(
        &self,
        id: RenderId,
        point: flui_foundation::geometry::Point,
        ancestor: Option<RenderId>,
    ) -> Option<flui_foundation::geometry::Point> {
        let ancestor = ancestor.or(self.root_id)?;
        let inverse = self.transform_to(id, ancestor)?.try_inverse()?;
        let (x, y) = inverse.unproject_to_plane(point.x, point.y)?;
        Some(flui_foundation::geometry::Point::new(x, y))
    }
}
