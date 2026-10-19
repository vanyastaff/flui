//! Expanded hit areas: resolving a hit that lands outside every visual target
//! but inside some target's slop.
//!
//! The box walk asks each node for [`RenderObject::hit_slop_area`]. The first
//! walk is purely visual and only *records* the nodes whose expanded area holds
//! the position; they do not take the hit there. When nothing better was hit,
//! the resolution below re-walks for the best candidate, in this order:
//!
//! 1. **Visual first.** A candidate never takes a position that the visual walk
//!    gave to another interactive target (an entry with a pointer target that
//!    is not on the candidate's own path). Shared ancestors — a scroll view
//!    around both — do not count.
//! 2. **Nearest edge.** Among candidates, the one whose own bounds are nearest
//!    to the position wins, measured in that candidate's local space.
//! 3. **Z-order.** Ties go to the candidate the walk reached first, which is
//!    the topmost one.
//!
//! Two more rules keep an area inside its own surface:
//!
//! - **No crossing layers.** A candidate never takes a position that visually
//!   hit content in another composited layer (the nearest repaint boundary at
//!   or above each node): any visual entry off the candidate's path must share
//!   the candidate's layer. A position that hit only the candidate's
//!   ancestors — the gap between list items that are each their own layer —
//!   is free to take.
//! - **No crossing barriers.** A visual hit on a node that blocks expanded
//!   areas (`RenderObject::blocks_hit_slop`: an absorbing `AbsorbPointer`, the
//!   modal barrier) keeps the position, whatever lies above it. Anything that
//!   covers the position and blocks also stops the re-walk before it reaches a
//!   candidate below it.
//!
//! An overlay entry needs no rule of its own: its content that the position
//! hits ends the re-walk of the stack it sits in before the walk reaches an
//! entry below, so a page target's area never takes a position a toast above
//! it covers (`slop_does_not_cross_an_overlay_entry`, `flui-widgets`).
//!
//! [`RenderObject::hit_slop_area`]: crate::traits::RenderObject::hit_slop_area

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::PointerKind;

use crate::hit_testing::{HitTestEntry, HitTestResult};
use crate::pipeline::phase::PipelinePhase;

use super::PipelineOwner;

/// Which nodes take a hit through their expanded area during one walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlopPass {
    /// No node does; nodes whose area holds the position are recorded.
    Visual,
    /// Only this node does: the re-walk that builds its path.
    Expand(RenderId),
}

/// A node whose expanded area held the position in the visual walk.
#[derive(Debug, Clone, Copy)]
struct SlopCandidate {
    id: RenderId,
    /// Distance from the position to the node's own bounds, local space.
    distance: f64,
}

/// Per-walk slop state, threaded through the box and sliver walks.
#[derive(Debug)]
pub(super) struct SlopWalk {
    pass: SlopPass,
    /// In visit order: the topmost candidate first.
    candidates: Vec<SlopCandidate>,
    /// Whether the walk hit a node that blocks expanded areas.
    hit_barrier: bool,
    /// The kind of pointer asking, whose minimum target sizes each area.
    kind: PointerKind,
}

impl SlopWalk {
    pub(super) const fn visual(kind: PointerKind) -> Self {
        Self {
            pass: SlopPass::Visual,
            candidates: Vec::new(),
            hit_barrier: false,
            kind,
        }
    }

    const fn expand(id: RenderId, kind: PointerKind) -> Self {
        Self {
            pass: SlopPass::Expand(id),
            candidates: Vec::new(),
            hit_barrier: false,
            kind,
        }
    }

    /// The kind of pointer the walk tests for.
    pub(super) const fn kind(&self) -> PointerKind {
        self.kind
    }

    /// Whether any expanded area may still take the position.
    pub(super) fn has_candidates(&self) -> bool {
        !self.hit_barrier && !self.candidates.is_empty()
    }

    /// Notes that the walk hit a node that blocks expanded areas.
    pub(super) fn record_barrier(&mut self) {
        self.hit_barrier = true;
    }

    /// Whether `id` takes the position through its expanded area in this walk.
    pub(super) fn expands(&self, id: RenderId) -> bool {
        self.pass == SlopPass::Expand(id)
    }

    /// Records `id` as a candidate when this is the visual walk and its
    /// expanded `area` holds the `position` its own bounds missed.
    pub(super) fn record_miss(
        &mut self,
        id: RenderId,
        area: flui_foundation::geometry::Rect,
        position: Offset,
        size: Size,
    ) {
        if self.pass != SlopPass::Visual || !area.contains(position.to_point()) {
            return;
        }
        self.candidates.push(SlopCandidate {
            id,
            distance: distance_to_bounds(position, size),
        });
    }
}

/// Where the expanded walk probes a node it forces to take the hit: the point
/// of its bounds nearest to the real position, then its centre, for a child
/// whose own shape misses at that nearest point (a circle's bounding-box
/// corner). The probe only selects the path; delivered positions come from the
/// entries' transforms, so the child still sees the real position.
pub(super) fn expanded_probes(position: Offset, size: Size) -> [Offset; 2] {
    let nearest = Offset::new(
        position.dx.clamp(0.0, size.width.max(0.0)),
        position.dy.clamp(0.0, size.height.max(0.0)),
    );
    [nearest, Offset::new(size.width * 0.5, size.height * 0.5)]
}

fn distance_to_bounds(position: Offset, size: Size) -> f64 {
    let dx = (-position.dx).max(position.dx - size.width).max(0.0);
    let dy = (-position.dy).max(position.dy - size.height).max(0.0);
    dx.hypot(dy)
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Re-walks for the best slop candidate the visual walk recorded and, when
    /// one may take the position, replaces the visual path from `base` on with
    /// its path. Returns the re-walk's hit flag, or `None` to keep the visual
    /// result.
    pub(super) fn resolve_hit_slop(
        &self,
        root_id: RenderId,
        position: Offset,
        result: &mut HitTestResult,
        base: usize,
        walk: SlopWalk,
    ) -> Option<bool>
    where
        Phase: Sync,
    {
        let kind = walk.kind;
        let mut candidates = walk.candidates;
        // Stable: equal distances keep visit order, topmost first.
        candidates.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        for candidate in candidates {
            let mut expanded = HitTestResult::new();
            let mut walk = SlopWalk::expand(candidate.id, kind);
            let hit = self.hit_test_subtree(root_id, position, &mut expanded, &mut walk);
            let reached = expanded.iter().any(|entry| entry.target == candidate.id);
            if !reached
                || !self.slop_may_take(&result.path()[base..], expanded.path(), candidate.id)
            {
                continue;
            }
            let path = result.path_mut();
            path.truncate(base);
            path.append(expanded.path_mut());
            return Some(hit);
        }
        None
    }

    /// Whether `candidate`'s expanded path may replace the `visual` one.
    fn slop_may_take(
        &self,
        visual: &[HitTestEntry],
        expanded: &[HitTestEntry],
        candidate: RenderId,
    ) -> bool {
        let on_expanded = |target: RenderId| expanded.iter().any(|entry| entry.target == target);
        let visual_target_elsewhere = visual
            .iter()
            .any(|entry| entry.pointer_target.is_some() && !on_expanded(entry.target));
        if visual_target_elsewhere {
            return false;
        }
        let layer = self.hit_layer(candidate);
        visual
            .iter()
            .filter(|entry| !on_expanded(entry.target))
            .all(|entry| self.hit_layer(entry.target) == layer)
    }

    /// The composited layer `id` paints into: the nearest repaint boundary at
    /// or above it, or the root of its tree.
    fn hit_layer(&self, id: RenderId) -> RenderId {
        let mut current = id;
        loop {
            let Some(node) = self.render_tree.get(current) else {
                return current;
            };
            if node.is_repaint_boundary() {
                return current;
            }
            match node.parent() {
                Some(parent) => current = parent,
                None => return current,
            }
        }
    }
}
