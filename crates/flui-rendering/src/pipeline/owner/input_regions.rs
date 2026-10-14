//! Collecting a window's input regions from the laid-out tree (see
//! [`crate::input_regions`] for the mapping rules).
//!
//! The walk visits nodes without composing matrices: a node's transform and clip are
//! composed along its path from the root only when it reported a region, so a tree with
//! a handful of reporters pays a plain traversal plus `depth` steps per reporter.

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Point, Rect, Size};

use crate::input_regions::{InputRegion, InputRegionSink, UnmappableRegion, map_rect};
use crate::pipeline::phase::PipelinePhase;
use crate::storage::RenderNode;

use super::PipelineOwner;
use super::subtree_arena::ensure_stack;

const TARGET: &str = "flui_rendering::input_regions";

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// This window's input regions in window-logical coordinates, as the laid-out tree
    /// reports them now: what a presentation hands
    /// `PlatformWindow::set_input_regions` on every composited frame.
    ///
    /// The window is the root's laid-out box; nothing outside it is reported. Empty
    /// before the root's first layout.
    #[must_use]
    pub fn input_regions(&self) -> Vec<InputRegion> {
        let mut regions = Vec::new();
        let Some(root) = self.root_id() else {
            return regions;
        };
        let Some(window) = self
            .render_tree
            .get(root)
            .and_then(RenderNode::laid_out_size)
            .map(|size| Rect::from_origin_size(Point::ZERO, size))
        else {
            return regions;
        };
        let mut walk = RegionWalk {
            path: Vec::new(),
            sink: InputRegionSink::default(),
            window,
            regions: &mut regions,
        };
        self.collect_input_regions(root, &mut walk);
        regions
    }

    fn collect_input_regions(&self, id: RenderId, walk: &mut RegionWalk<'_>) {
        ensure_stack(|| self.collect_input_regions_impl(id, walk));
    }

    fn collect_input_regions_impl(&self, id: RenderId, walk: &mut RegionWalk<'_>) {
        let Some(node) = self.render_tree.get(id) else {
            return;
        };
        // The paint walk's own gates: nothing of this subtree is drawn. A
        // follower the last composite resolved to hidden draws nothing either.
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
        match node {
            RenderNode::Box(entry) => entry
                .render_object()
                .describe_input_regions(size, &mut walk.sink),
            RenderNode::Sliver(entry) => entry
                .render_object()
                .describe_input_regions(size, &mut walk.sink),
        }
        if !walk.sink.is_empty() {
            self.map_reported_regions(id, walk);
        }

        let generation = node.layout_generation();
        for (slot, &child_id) in node.children().iter().enumerate() {
            let Some(child) = self.render_tree.get(child_id) else {
                continue;
            };
            if !child.was_placed_by(id, generation) || !visits_child(node, slot) {
                continue;
            }
            walk.path.push((id, slot));
            self.collect_input_regions(child_id, walk);
            walk.path.pop();
        }
    }

    /// Maps the regions `id` just reported into window coordinates.
    fn map_reported_regions(&self, id: RenderId, walk: &mut RegionWalk<'_>) {
        let reporter = self.render_tree.get(id).map_or("?", RenderNode::debug_name);
        let Some((transform, clip)) = self.transform_and_clip_to_root(id, walk) else {
            for (kind, rect) in walk.sink.drain() {
                tracing::warn!(
                    target: TARGET,
                    render_object = reporter,
                    ?kind,
                    ?rect,
                    "input region dropped: the transform to the window is not finite"
                );
            }
            return;
        };
        for (kind, rect) in walk.sink.drain() {
            let finite = [rect.left(), rect.top(), rect.right(), rect.bottom()]
                .iter()
                .all(|edge| edge.is_finite());
            if finite && rect.is_empty() {
                continue;
            }
            match map_rect(&transform, rect) {
                Ok(mapped) => {
                    if let Some(kept) = clip.intersect(&mapped).filter(|kept| !kept.is_empty()) {
                        walk.regions.push(InputRegion::new(kind, kept));
                    }
                }
                Err(UnmappableRegion::NonFinite) => tracing::warn!(
                    target: TARGET,
                    render_object = reporter,
                    ?kind,
                    ?rect,
                    "input region dropped: its corners in window coordinates are not finite"
                ),
                Err(UnmappableRegion::BeyondHorizon) => tracing::warn!(
                    target: TARGET,
                    render_object = reporter,
                    ?kind,
                    ?rect,
                    "input region dropped: it reaches the perspective horizon"
                ),
            }
        }
    }

    /// The mapping from `id`'s local coordinates into the window, and the clip every
    /// ancestor (and the window) imposes on it, in window coordinates. `None` when the
    /// mapping is not finite.
    fn transform_and_clip_to_root(
        &self,
        id: RenderId,
        walk: &RegionWalk<'_>,
    ) -> Option<(Matrix4, Rect)> {
        let mut transform = Matrix4::IDENTITY;
        let mut clip = walk.window;
        let root = walk.path.first().map_or(id, |&(root, _)| root);
        self.shift_by_follower_offset(root, &mut transform);
        let children = walk
            .path
            .iter()
            .skip(1)
            .map(|&(ancestor, _)| ancestor)
            .chain(std::iter::once(id));
        for (&(ancestor_id, slot), child_id) in walk.path.iter().zip(children) {
            let ancestor = self.render_tree.get(ancestor_id)?;
            let child = self.render_tree.get(child_id)?;
            let size = ancestor.laid_out_size()?;
            if let Some(local) = approximate_paint_clip(ancestor, slot, size) {
                // An unmappable clip refuses coverage rather than lifting the limit.
                let mapped = map_rect(&transform, local).unwrap_or(Rect::ZERO);
                clip = clip.intersect(&mapped).unwrap_or(Rect::ZERO);
            }
            ancestor.apply_paint_transform(slot, child.offset(), &mut transform)?;
            self.shift_by_follower_offset(child_id, &mut transform);
            if transform
                .to_col_major_array()
                .iter()
                .any(|value| !value.is_finite())
            {
                return None;
            }
        }
        Some((transform, clip))
    }

    /// Composes the offset the last composite resolved for follower `id` (a
    /// `CompositedTransformFollower` placed against its leader), the same shift the
    /// engine paints its subtree with and hit-testing applies.
    fn shift_by_follower_offset(&self, id: RenderId, transform: &mut Matrix4) {
        if let Some(offset) = self.last_follower_offsets.get(&id) {
            *transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
        }
    }
}

/// The state one collection threads through the walk.
struct RegionWalk<'a> {
    /// `(ancestor, child slot)` from the root down to the node being visited.
    path: Vec<(RenderId, usize)>,
    sink: InputRegionSink,
    /// The root's box: the window in window-logical coordinates.
    window: Rect,
    regions: &'a mut Vec<InputRegion>,
}

fn visits_child(node: &RenderNode, slot: usize) -> bool {
    match node {
        RenderNode::Box(entry) => entry.render_object().visits_child_for_semantics(slot),
        RenderNode::Sliver(entry) => entry.render_object().visits_child_for_semantics(slot),
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
