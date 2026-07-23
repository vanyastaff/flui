//! `RenderSubtreeAnchor` — a transparent proxy that publishes **its own
//! `RenderId`** while it is mounted.
//!
//! # What problem this solves
//!
//! A `RenderId` names a node's coordinate space. Code outside the render tree —
//! a routing layer, an overlay controller — often needs to say *"measure this
//! subtree relative to that one"* (`PipelineOwner::transform_to`,
//! `PipelineOwner::box_size`), and so needs a **stable, addressable render node**
//! at the root of a subtree it owns.
//!
//! Nothing else in the framework can hand that out:
//!
//! * A `RenderId` exists only for a render-object element; a stateful view owns
//!   none.
//! * `BuildContext::find_render_object()` walks strict **ancestors** — it is
//!   Flutter's `findAncestorRenderObjectOfType`, not `context.findRenderObject()`
//!   — so a view can never learn the id of the subtree *below* it.
//! * A `GlobalKey` would work, and is what Flutter uses (`routes.dart:1229` puts
//!   `_subtreeKey` on a `RepaintBoundary`), but it costs a registry lookup and a
//!   keyed element.
//!
//! The first — and only — lifecycle hook where a render object's own id is
//! guaranteed is [`RenderBox::attach`], which receives a [`RepaintHandle`]
//! carrying it. [`RenderBox::detach`] is its exact mirror. So this object
//! publishes on `attach` and clears on `detach`: mount-driven, no key, no element
//! walk, nothing acquired during build / layout / paint.
//!
//! # It is transparent
//!
//! Layout, paint, hit-test, intrinsics and baselines pass straight through to the
//! single child, exactly as [`RenderRepaintBoundary`](super::RenderRepaintBoundary)
//! does — **minus** the boundary: this object is *not* a repaint boundary and does
//! not force compositing. Its only effect is to exist, so that something above it
//! has a `RenderId` to point at.
//!
//! # Flutter equivalence
//!
//! None directly. Flutter reaches the same place with `RepaintBoundary` + a
//! `GlobalKey` (`routes.dart:1229`, `ModalRoute._subtreeKey`), paying a repaint
//! boundary it does not otherwise need. This is the narrower object: identity
//! without the compositing side effect.

use std::sync::Arc;

use flui_rendering::pipeline::RepaintHandle;
use flui_tree::Single;
use flui_types::{Offset, Size};
use parking_lot::Mutex;

use flui_foundation::RenderId;
use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// The cell a [`RenderSubtreeAnchor`] publishes its `RenderId` into.
///
/// Cloneable and `'static`: the owner keeps a clone, the render object keeps a
/// clone, and they name the same slot. `None` before the first `attach` and after
/// `detach`, which is what makes a stale handle **inert rather than wrong** — a
/// caller cannot resolve the id of a subtree that has left the tree.
///
/// The lock is private and never escapes: [`get`](Self::get) copies the id out
/// (`RenderId` is `Copy`), so no guard crosses the API boundary.
#[derive(Clone, Default)]
pub struct SubtreeAnchor {
    published: Arc<Mutex<Option<RenderId>>>,
}

impl SubtreeAnchor {
    /// An anchor naming nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The anchored subtree's root `RenderId`, or `None` when it is not mounted.
    ///
    /// Resolving to `Some` means the render object is in the tree. It does **not**
    /// mean the subtree has been laid out: ask
    /// [`PipelineOwner::box_size`](flui_rendering::pipeline::PipelineOwner::box_size),
    /// which returns `None` before the first layout commits.
    #[must_use]
    pub fn get(&self) -> Option<RenderId> {
        *self.published.lock()
    }

    /// Whether a mounted render object is currently publishing into this anchor.
    #[must_use]
    pub fn is_anchored(&self) -> bool {
        self.get().is_some()
    }

    fn publish(&self, id: RenderId) {
        *self.published.lock() = Some(id);
    }

    fn clear(&self) {
        *self.published.lock() = None;
    }
}

impl std::fmt::Debug for SubtreeAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubtreeAnchor")
            .field("render_id", &self.get())
            .finish()
    }
}

/// A layout/paint/hit-test-transparent proxy that publishes its own `RenderId`
/// into a [`SubtreeAnchor`] for as long as it is mounted.
///
/// See the module docs for why this exists and why `attach`/`detach` are the only
/// hooks that can do it.
#[derive(Debug, Clone, Default)]
pub struct RenderSubtreeAnchor {
    anchor: SubtreeAnchor,
    /// Whether a child was attached at the last layout — gates hit-testing, so a
    /// childless anchor does not absorb hits.
    has_child: bool,
}

impl RenderSubtreeAnchor {
    /// A proxy that publishes into `anchor` while mounted.
    #[must_use]
    pub fn new(anchor: SubtreeAnchor) -> Self {
        Self {
            anchor,
            has_child: false,
        }
    }

    /// The anchor this object publishes into.
    #[must_use]
    pub fn anchor(&self) -> &SubtreeAnchor {
        &self.anchor
    }
}

impl flui_foundation::Diagnosticable for RenderSubtreeAnchor {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "render_id",
            match self.anchor.get() {
                Some(id) => format!("{id:?}"),
                None => "<unmounted>".to_owned(),
            },
        );
    }
}

impl RenderBox for RenderSubtreeAnchor {
    type Arity = Single;
    type ParentData = BoxParentData;

    /// Pass-through: the child is laid out under this object's own constraints and
    /// its size adopted, so inserting an anchor changes no geometry.
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    /// Pure pass-through, as `RenderProxyBox`: `hitTestSelf` is false, so the
    /// anchor is hit iff its child is. Without this the trait default would absorb
    /// the hit and never recurse, blocking the whole subtree from pointer events.
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    /// The publication. `RepaintHandle::id()` is the render object's own id, and
    /// this is the first moment it exists.
    fn attach(&mut self, handle: RepaintHandle) {
        self.anchor.publish(handle.id());
    }

    /// The retraction. A published id must never outlive the mounted node, or a
    /// caller could resolve a stale subtree and measure a disposed route.
    fn detach(&mut self) {
        self.anchor.clear();
    }

    flui_rendering::forward_single_child_box_queries!();

    fn debug_name(&self) -> &'static str {
        "RenderSubtreeAnchor"
    }
}
