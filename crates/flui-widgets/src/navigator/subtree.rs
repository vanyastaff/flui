//! Route subtree identity — the FLUI shape of `ModalRoute._subtreeKey`.
//!
//! ADR-0021 U2, seam 4. **Private**: nothing here is exported.
//!
//! # What Flutter names, and how
//!
//! `ModalRoute` hangs a `GlobalKey` on the `RepaintBoundary` that wraps *only*
//! `buildPage(...)` — not the transitions around it (`routes.dart:1229-1231`,
//! `:2268`). `Route.subtreeContext => _subtreeKey.currentContext` (`:1966`) is
//! what `HeroController` then measures against:
//!
//! ```dart
//! final fromRouteRenderBox = toRoute.subtreeContext?.findRenderObject() as RenderBox?;  // heroes.dart:952
//! Hero._allHeroesFor(from.subtreeContext!, …)                                            // heroes.dart:1014
//! ```
//!
//! So a route must publish two things: a **`BuildContext`** to walk its page
//! subtree from, and a **`RenderBox`** to resolve coordinates against.
//!
//! # Why FLUI cannot spell it the same way
//!
//! `BuildContext::find_render_object()` walks strict **ancestors** — it is
//! Flutter's `findAncestorRenderObjectOfType`, not `context.findRenderObject()`.
//! A `BuildContext` therefore cannot yield the `RenderId` *below* it, and a
//! `GlobalKey` would not change that. The two ids have to come from two different
//! lifecycle hooks:
//!
//! | Flutter | FLUI | Published at | Cleared at |
//! |---|---|---|---|
//! | `_subtreeKey.currentContext` | [`RouteSubtree::element_id`] | `ViewState::init_state` | `ViewState::dispose` |
//! | `…currentContext.findRenderObject()` | [`RouteSubtree::render_id`] | `RenderBox::attach` | `RenderBox::detach` |
//!
//! [`RouteSubtreeAnchor`] is the view that owns both hooks. Its element is the
//! route's `subtreeContext`; the [`RenderSubtreeAnchor`] it builds is the route's
//! render coordinate space.
//!
//! **The one-element offset.** Flutter's key sits *on* the `RepaintBoundary`, so
//! its element and its render object are the same node. Here they are parent and
//! child: `element_id` names the stateful anchor view, `render_id` names the
//! `RenderSubtreeAnchor` immediately below it. Both bracket exactly the page
//! subtree, which is what `_allHeroesFor` and `_boundingBoxFor` need, so no
//! observable behaviour depends on the offset. It is recorded, not claimed away.
//!
//! **Not a repaint boundary.** Flutter's `_subtreeKey` rides on a
//! `RepaintBoundary` that exists for its own reasons. [`RenderSubtreeAnchor`] is
//! identity without the compositing side effect (`flui_objects`, module docs).
//!
//! # Resolution is two-stage, and the second stage is not here
//!
//! [`RouteSubtreeCell::resolve`] answers *"is this route's page mounted and
//! attached?"* — nothing more. A render object exists from `attach`, which happens
//! during **build**, long before layout commits. Asking a `RouteSubtree` for
//! geometry means asking [`PipelineOwner::box_size`], which returns `None` until
//! the first layout commits (ADR-0021 U1). `SubtreeAnchor::get()` alone is *not*
//! layout-readiness, and `route_subtree_ids_are_published_before_layout_commits`
//! pins that.
//!
//! [`PipelineOwner::box_size`]: flui_rendering::pipeline::PipelineOwner::box_size

use std::fmt;
use std::sync::Arc;

use flui_foundation::{ElementId, RenderId};
use flui_objects::{RenderSubtreeAnchor, SubtreeAnchor};
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{Child, RenderView, impl_render_view};
use parking_lot::Mutex;

/// Where a route's page subtree lives, once it is both mounted and attached.
///
/// Owned data, never a borrow into the trees: the caller that reads this is
/// outside any tree borrow by construction (it came from a `NavigatorHandle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouteSubtree {
    /// The route's `subtreeContext` — the element to walk its page subtree from.
    pub(crate) element_id: ElementId,
    /// The root of the route's page **coordinate space**. Measurable only once
    /// layout has committed; see the module docs.
    pub(crate) render_id: RenderId,
}

/// The element half and the render half of a [`RouteSubtreeCell`], unjoined.
#[cfg(test)]
pub(crate) type SubtreeParts = (Option<ElementId>, Option<RenderId>);

/// The two cells a mounted [`RouteSubtreeAnchor`] publishes into.
///
/// Cloneable and `'static`: the route keeps one, the view keeps one, the
/// navigator's registry keeps one, and all three name the same slots. Both cells
/// are `None` before mount and after unmount, which is what makes a *disposed*
/// route unresolvable rather than stale — `resolve()` cannot resurrect it.
#[derive(Clone, Default)]
pub(crate) struct RouteSubtreeCell {
    anchor: SubtreeAnchor,
    element: Arc<Mutex<Option<ElementId>>>,
}

impl RouteSubtreeCell {
    /// A cell naming nothing yet. A `ModalRoute` creates one in its constructor.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Both ids, or `None` unless the page is mounted **and** its render object
    /// is attached.
    ///
    /// Not a layout-readiness check. See the module docs.
    pub(crate) fn resolve(&self) -> Option<RouteSubtree> {
        Some(RouteSubtree {
            element_id: (*self.element.lock())?,
            render_id: self.anchor.get()?,
        })
    }

    fn publish_element(&self, element_id: ElementId) {
        *self.element.lock() = Some(element_id);
    }

    fn clear_element(&self) {
        *self.element.lock() = None;
    }

    /// The two halves, separately. Test-facing: [`resolve`](Self::resolve) is an
    /// `AND`, so it cannot tell which half a bug left behind — a test that only
    /// checks `resolve() == None` passes when *either* retraction works, and would
    /// stay green with one of them deleted.
    #[cfg(test)]
    pub(crate) fn parts(&self) -> SubtreeParts {
        (*self.element.lock(), self.anchor.get())
    }
}

impl fmt::Debug for RouteSubtreeCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteSubtreeCell")
            .field("resolved", &self.resolve())
            .finish()
    }
}

/// Wraps a route's page and publishes its identity into a [`RouteSubtreeCell`]
/// for as long as it is mounted.
///
/// Stateful, because `element_id` can only be read from a lifecycle hook that
/// receives a `BuildContext` and `dispose()` is the only mirror of `init_state`.
#[derive(Debug, Clone)]
pub(crate) struct RouteSubtreeAnchor {
    cell: RouteSubtreeCell,
    child: Child,
}

impl RouteSubtreeAnchor {
    /// Anchor `child` — a route's page — into `cell`.
    pub(crate) fn new(cell: RouteSubtreeCell, child: impl IntoView) -> Self {
        Self {
            cell,
            child: Child::some(child.into_view()),
        }
    }
}

impl View for RouteSubtreeAnchor {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for RouteSubtreeAnchor {
    type State = RouteSubtreeAnchorState;

    fn create_state(&self) -> Self::State {
        RouteSubtreeAnchorState {
            cell: self.cell.clone(),
        }
    }
}

/// Publishes the element half of the identity. The render half is published by
/// `RenderSubtreeAnchor::attach`, which is the only hook where its id exists.
pub(crate) struct RouteSubtreeAnchorState {
    cell: RouteSubtreeCell,
}

impl ViewState<RouteSubtreeAnchor> for RouteSubtreeAnchorState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.cell.publish_element(ctx.element_id());
    }

    /// The retraction. A published element id must never outlive its element, or
    /// `HeroController` would walk a disposed route's subtree. A route unmounted
    /// because it was covered with `maintain_state == false` comes back through
    /// `init_state` on a fresh element and republishes.
    fn dispose(&mut self) {
        self.cell.clear_element();
    }

    fn build(&self, view: &RouteSubtreeAnchor, _ctx: &dyn BuildContext) -> impl IntoView {
        AnchoredBox {
            anchor: view.cell.anchor.clone(),
            child: view.child.clone(),
        }
    }
}

/// The render half: a transparent proxy whose only job is to have a `RenderId`.
#[derive(Debug, Clone)]
struct AnchoredBox {
    anchor: SubtreeAnchor,
    child: Child,
}

impl RenderView for AnchoredBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSubtreeAnchor;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSubtreeAnchor::new(self.anchor.clone())
    }

    /// Nothing to update: a render object's anchor is its identity, fixed for the
    /// life of the node. Reconciliation only ever hands this the same cell.
    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(AnchoredBox);
