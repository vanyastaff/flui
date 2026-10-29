//! What an agent checks before it points at an element (ADR-0238, R11): is it
//! attached, visible, stable, enabled, and does a pointer at that spot reach
//! it?
//!
//! The facts are read from one committed tree, and the hit-test is the
//! caller's. Two calls make a check: [`SemanticsOwner::actionability_point`]
//! says where the pointer would land, the caller runs the render tree's
//! hit-test there and passes the path it found to
//! [`SemanticsOwner::actionability_wire`]. The owner never calls back into
//! the render tree, so no hit-test (user code) runs under a semantics borrow.
//!
//! # Hit mapping
//!
//! A hit path is the render objects under the point, most specific first. The
//! node that receives the pointer is the first of them the filtered tree shows,
//! because a published node is addressed by its render object's id: the index
//! from render object to semantics node is the node id itself, and cannot go
//! stale. A render object with no node of its own (a decoration, a layout
//! wrapper) passes the pointer to the next one up the path; the element
//! receives the pointer when the node found is it or lies under it.

use accesskit::{NodeId, TreeId};
use accesskit_consumer::{FilterResult, NodeRef, TreeState, common_filter};
use flui_foundation::RenderId;
use flui_foundation::geometry::Rect as Bounds;
use flui_protocol::{Actionability, ElementRef, LogicalPoint, LogicalRect, TreeRevision, outline};

use super::{Placement, WireReadError, element_id, logical_bounds, wire_node};
use crate::owner::SemanticsOwner;

/// Rectangles closer than this, in logical pixels, are the same bounds.
const SAME_BOUNDS: f64 = 1e-3;

impl SemanticsOwner {
    /// Where a pointer aimed at `element` lands: `point` when given, else the
    /// centre of the part of its bounds the window shows. `None` when the
    /// element is not attached, not visible, or in another window.
    ///
    /// # Errors
    ///
    /// [`WireReadError::NoTree`] before the first assembly,
    /// [`WireReadError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn actionability_point(
        &self,
        element: ElementRef,
        point: Option<LogicalPoint>,
        placement: Placement,
    ) -> Result<Option<LogicalPoint>, WireReadError> {
        let read = self.with_published(|state, _| {
            let facts = Facts::read(state, self.device_pixel_ratio(), element, placement);
            facts
                .visible
                .then(|| point.or(facts.default_point))
                .flatten()
        });
        read.map_err(|published| match published {
            super::Published::NoTree => WireReadError::NoTree,
            super::Published::Malformed => WireReadError::Malformed,
        })
    }

    /// The facts about `element` that decide whether input can reach it, from
    /// one committed tree.
    ///
    /// `point` is where the pointer would land, as
    /// [`Self::actionability_point`] answered; `hit` the render objects the
    /// hit-test found there, most specific first. `previous` is the bounds the
    /// element had on the reading before this one: with none to compare, the
    /// element is not yet known to be stable.
    ///
    /// An element the tree does not hold is not attached, and every other
    /// fact is false. An element the filtered tree leaves out (hidden, or
    /// under a hidden node) is attached and not visible.
    ///
    /// # Errors
    ///
    /// [`WireReadError::NoTree`] before the first assembly,
    /// [`WireReadError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn actionability_wire(
        &self,
        element: ElementRef,
        point: Option<LogicalPoint>,
        previous: Option<LogicalRect>,
        hit: &[RenderId],
        placement: Placement,
    ) -> Result<Actionability, WireReadError> {
        let revision = self.revision();
        let read = self.with_published(|state, _| {
            let facts = Facts::read(state, self.device_pixel_ratio(), element, placement);
            facts.into_actionability(state, point, previous, hit, revision)
        });
        read.map_err(|published| match published {
            super::Published::NoTree => WireReadError::NoTree,
            super::Published::Malformed => WireReadError::Malformed,
        })
    }
}

/// What the tree shows of one element, before any hit-test.
struct Facts<'s> {
    /// The element is in the tree.
    node: Option<NodeRef<'s>>,
    /// Its bounds in logical pixels, as published.
    rect: Option<Bounds<f64>>,
    visible: bool,
    default_point: Option<LogicalPoint>,
    enabled: bool,
}

impl<'s> Facts<'s> {
    fn read(
        state: &'s TreeState,
        published_ratio: f64,
        element: ElementRef,
        placement: Placement,
    ) -> Self {
        let absent = Self {
            node: None,
            rect: None,
            visible: false,
            default_point: None,
            enabled: false,
        };
        if element.window != placement.window {
            return absent;
        }
        let Some(node) = state.node_by_tree_local_id(NodeId(element.element.get()), TreeId::ROOT)
        else {
            return absent;
        };
        let rect = logical_bounds(&node, published_ratio);
        let window = logical_bounds(&state.root(), published_ratio);
        // The part of the bounds the window shows. The semantics assembly has
        // already narrowed a node to its clipping ancestors, so what is left
        // is what a pointer can reach.
        let shown = rect.and_then(|rect| match window {
            Some(window) => rect.intersect(&window),
            None => Some(rect),
        });
        let shown = shown.filter(|shown| !shown.is_empty());
        let included = common_filter(&node) == FilterResult::Include;
        let visible = included && shown.is_some();
        let default_point = shown.map(|shown| {
            let centre = shown.center();
            LogicalPoint::new(centre.x, centre.y)
        });
        Self {
            enabled: !disabled_through_ancestors(&node),
            node: Some(node),
            rect,
            visible,
            default_point,
        }
    }

    fn into_actionability(
        self,
        state: &TreeState,
        point: Option<LogicalPoint>,
        previous: Option<LogicalRect>,
        hit: &[RenderId],
        revision: TreeRevision,
    ) -> Actionability {
        let rect = self
            .rect
            .map(|rect| LogicalRect::new(rect.left(), rect.top(), rect.width(), rect.height()));
        let mut out = Actionability::new(self.node.is_some(), revision);
        out.rect = rect;
        out.visible = self.visible;
        out.stable = match (previous, rect) {
            (Some(previous), Some(current)) => same_bounds(previous, current),
            _ => false,
        };
        out.enabled = self.enabled;
        out.point = if self.visible {
            point.or(self.default_point)
        } else {
            None
        };
        if let (Some(target), true) = (&self.node, self.visible) {
            let reached = reached_node(state, hit);
            out.receives = reached
                .as_ref()
                .is_some_and(|reached| is_self_or_under(reached, target));
            if !out.receives {
                out.covered_by = reached.and_then(|reached| {
                    let id = element_id(&reached)?;
                    let wire = wire_node(&reached, id, false);
                    Some(outline(std::slice::from_ref(&wire)).trim_end().to_owned())
                });
            }
        }
        out
    }
}

/// The node the pointer reaches: the first render object of `hit` that the
/// filtered tree shows.
fn reached_node<'s>(state: &'s TreeState, hit: &[RenderId]) -> Option<NodeRef<'s>> {
    hit.iter().find_map(|render| {
        state
            .node_by_tree_local_id(NodeId(render.as_u64()), TreeId::ROOT)
            .filter(|node| common_filter(node) == FilterResult::Include)
    })
}

/// Whether `node` is `ancestor` or lies below it.
fn is_self_or_under(node: &NodeRef<'_>, ancestor: &NodeRef<'_>) -> bool {
    let target = ancestor.locate();
    let mut current = Some(*node);
    while let Some(candidate) = current {
        if candidate.locate() == target {
            return true;
        }
        current = candidate.parent();
    }
    false
}

/// Whether the node or any ancestor refuses interaction.
fn disabled_through_ancestors(node: &NodeRef<'_>) -> bool {
    let mut current = Some(*node);
    while let Some(candidate) = current {
        if candidate.is_disabled() {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn same_bounds(a: LogicalRect, b: LogicalRect) -> bool {
    [
        (a.x, b.x),
        (a.y, b.y),
        (a.width, b.width),
        (a.height, b.height),
    ]
    .into_iter()
    .all(|(a, b)| (a - b).abs() < SAME_BOUNDS)
}
