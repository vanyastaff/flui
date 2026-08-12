//! `SliverPersistentHeader` render views + element — the element half of the
//! persistent-header build-during-layout seam.
//!
//! # What this wires together
//!
//! - `flui_objects::sliver::sliver_persistent_header`: all four render objects
//!   of the pinned × floating matrix, whose shared `PersistentHeaderCore`
//!   publishes `(shrink_offset, overlaps_content)` into a shared
//!   [`HeaderShrinkCell`] on every layout pass — inside its own change gate, so
//!   an unchanged pair raises nothing.
//! - `owner/layout_builder.rs`: the payload-blind build-during-layout registry
//!   and `BuildOwner::service_layout_builders`, whose entry doc names this
//!   header as its second consumer.
//! - This module: the element that mints the cell, installs it on the render
//!   object at mount, registers `RenderId -> (ElementId, cell)`, and — between
//!   layout passes — rebuilds its child by handing the *published* shrink state
//!   to a [`SliverPersistentHeaderDelegate`].
//!
//! # Same-frame settling
//!
//! Identical to [`LayoutBuilder`](super::layout_builder::LayoutBuilder)'s
//! fixpoint, with shrink state in place of box constraints:
//!
//! ```text
//! run_layout      → PersistentHeaderCore publishes (shrink, overlaps),
//!                   raises needs_build iff the pair changed
//! service_*       → this element rebuilds; delegate.build(shrink, overlaps)
//!                   produces the child; reconcile mounts it; cell.commit()
//! run_layout      → the fresh child is laid out at the new extent
//! service_*       → pair republished == committed ⇒ clean ⇒ converges
//! ```
//!
//! # The first build has no shrink state, and does not invent any
//!
//! Before the first layout pass nothing has published — the shrink offset
//! depends on a scroll offset that does not exist yet. The element builds **no
//! child** for that one pass rather than guessing `0.0`: the render object
//! sizes from its extents alone, publishes, and the fixpoint builds the real
//! child in the same frame. Same rule, same reason, as `LayoutBuilder`'s
//! constraints (ADR-0017).
//!
//! # Public surface
//!
//! [`SliverPersistentHeaderDelegate`] and the four concrete views are public:
//! `flui-widgets`' `SliverPersistentHeader` facade composes over them, picking
//! the view type from its `pinned`/`floating` flags so that flipping a flag is
//! a view-type change the reconciler answers by replacement — never an
//! in-place update against a render object of the wrong variant.
//!
//! Cross-checked against `.flutter/packages/flutter/lib/src/widgets/sliver_persistent_header.dart`
//! (delegate contract, `shouldRebuild` semantics) — the servicing *timing*
//! deliberately follows FLUI's between-passes fixpoint rather than Flutter's
//! inside-layout `updateChild`, because FLUI's layout walk holds
//! `&mut RenderTree` (ADR-0017's rejected alternatives).

use std::{marker::PhantomData, rc::Rc, sync::Arc};

use flui_objects::{
    HeaderShrinkCell, RenderSliverFloatingPersistentHeader,
    RenderSliverFloatingPinnedPersistentHeader, RenderSliverPinnedPersistentHeader,
    RenderSliverScrollingPersistentHeader,
};
use flui_rendering::protocol::SliverProtocol;

use super::{
    Variable,
    behavior::{ElementBehavior, RenderBehavior, make_build_ctx},
    behavior_commons::{build_or_recover, should_build_with_trace, single_child_views},
    generic::ElementCore,
    unified::Element,
};
use crate::{
    BoxedView, ElementOwner,
    context::BuildContext,
    view::{RenderView, View},
};

// ============================================================================
// DELEGATE
// ============================================================================

/// Configuration and child factory for a persistent header.
///
/// The header's *box* is owned by the render layer (extent interpolation,
/// pinning, floating, paint clamping); the delegate owns what the header
/// **contains** at a given collapse state. `build` runs between layout passes
/// with the shrink state the render object actually published — never a
/// guessed or stale pair.
///
/// # `should_rebuild` contract
///
/// When a rebuilt widget carries a *different* delegate instance, the new
/// delegate is asked whether the swap is observable. Return `true` whenever
/// the new delegate could produce different content **or different
/// `min_extent`/`max_extent`** — the extents are pushed to the render object
/// on the same update path, so answering `false` freezes both the child and
/// the geometry. (Flutter's `SliverPersistentHeaderDelegate.shouldRebuild`
/// states the same obligation.)
pub trait SliverPersistentHeaderDelegate {
    /// Build the header's content for the given collapse state.
    ///
    /// `shrink_offset` grows from `0.0` (fully expanded) toward
    /// [`max_extent`](Self::max_extent) as the header scrolls away;
    /// `overlaps_content` is `true` while the header overlaps the sliver
    /// after it (the pinned-and-scrolled-under state a shadow usually keys
    /// on).
    fn build(
        &self,
        ctx: &dyn BuildContext,
        shrink_offset: f32,
        overlaps_content: bool,
    ) -> BoxedView;

    /// The extent the header collapses to.
    fn min_extent(&self) -> f32;

    /// The extent the header expands to.
    fn max_extent(&self) -> f32;

    /// Whether replacing `old` with `self` is observable. See the trait doc
    /// for the obligation this carries.
    fn should_rebuild(
        &self,
        old: &dyn SliverPersistentHeaderDelegate, // PORT-CHECK-OK-DYN: the old delegate is inherently type-erased on the view (see SharedHeaderDelegate); a generic parameter here would make the trait non-object-safe and the view un-erasable
    ) -> bool {
        let _ = old;
        true
    }
}

/// The shared delegate handle carried by every persistent-header view.
///
/// `Rc` (not a generic parameter) for the same reason as
/// `LayoutWidgetBuilder`: the view stays cheaply cloneable and object-safe as
/// a `dyn View`, and the delegate stays UI-owner-local under ADR-0027.
pub type SharedHeaderDelegate = Rc<dyn SliverPersistentHeaderDelegate>; // PORT-CHECK-OK-DYN: same erasure and ownership shape as LayoutWidgetBuilder — the view must stay cheaply cloneable and object-safe as a dyn View, and the delegate stays UI-owner-local under ADR-0027

// ============================================================================
// RENDER-OBJECT ABSTRACTION (one behavior, four variants)
// ============================================================================

/// The slice of the four persistent-header render objects this element needs:
/// construction from extents, cell installation, and extent refresh.
///
/// Sealed by location — implemented here for exactly the four variants; the
/// pinned/floating *behavior* differences live entirely in the render layer.
pub trait PersistentHeaderRenderObject:
    flui_rendering::traits::RenderObject<SliverProtocol> + Send + Sync + 'static
{
    /// Construct with the delegate's extents. No cell yet — the element mints
    /// and installs it at mount, because only the element knows the registry
    /// entry the cell must also land in.
    fn create(min_extent: f32, max_extent: f32) -> Self;

    /// Install the shared mailbox this render object publishes shrink state
    /// into on every layout pass.
    fn install_shrink_cell(&mut self, cell: Arc<HeaderShrinkCell>);

    /// Refresh both extents from a rebuilt delegate.
    fn update_extents(
        &mut self,
        min_extent: f32,
        max_extent: f32,
    ) -> flui_rendering::RenderUpdateImpact;
}

impl PersistentHeaderRenderObject for RenderSliverScrollingPersistentHeader {
    fn create(min_extent: f32, max_extent: f32) -> Self {
        Self::new(min_extent, max_extent)
    }

    fn install_shrink_cell(&mut self, cell: Arc<HeaderShrinkCell>) {
        self.set_shrink_cell(cell);
    }

    fn update_extents(
        &mut self,
        min_extent: f32,
        max_extent: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        self.set_min_extent(min_extent) | self.set_max_extent(max_extent)
    }
}

impl PersistentHeaderRenderObject for RenderSliverPinnedPersistentHeader {
    fn create(min_extent: f32, max_extent: f32) -> Self {
        Self::new(min_extent, max_extent)
    }

    fn install_shrink_cell(&mut self, cell: Arc<HeaderShrinkCell>) {
        self.set_shrink_cell(cell);
    }

    fn update_extents(
        &mut self,
        min_extent: f32,
        max_extent: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        self.set_min_extent(min_extent) | self.set_max_extent(max_extent)
    }
}

impl PersistentHeaderRenderObject for RenderSliverFloatingPersistentHeader {
    fn create(min_extent: f32, max_extent: f32) -> Self {
        // No controller: snap and programmatic expansion are deferred until a
        // snap configuration reaches this seam (the facade documents this).
        Self::new(min_extent, max_extent, None)
    }

    fn install_shrink_cell(&mut self, cell: Arc<HeaderShrinkCell>) {
        self.set_shrink_cell(cell);
    }

    fn update_extents(
        &mut self,
        min_extent: f32,
        max_extent: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        self.set_min_extent(min_extent) | self.set_max_extent(max_extent)
    }
}

impl PersistentHeaderRenderObject for RenderSliverFloatingPinnedPersistentHeader {
    fn create(min_extent: f32, max_extent: f32) -> Self {
        Self::new(min_extent, max_extent, None)
    }

    fn install_shrink_cell(&mut self, cell: Arc<HeaderShrinkCell>) {
        self.set_shrink_cell(cell);
    }

    fn update_extents(
        &mut self,
        min_extent: f32,
        max_extent: f32,
    ) -> flui_rendering::RenderUpdateImpact {
        self.set_min_extent(min_extent) | self.set_max_extent(max_extent)
    }
}

// ============================================================================
// VIEW (generic over the four variants)
// ============================================================================

/// One persistent-header view, parameterised by which render variant carries
/// it.
///
/// Not used directly by applications — `flui-widgets`' `SliverPersistentHeader`
/// facade picks the concrete alias from its `pinned`/`floating` flags. The
/// aliases are distinct *types*, so flipping a flag reconciles as
/// teardown-and-replace: an in-place update could never migrate a pinned
/// render object into a floating one.
pub struct PersistentHeaderView<R> {
    delegate: SharedHeaderDelegate,
    _variant: PhantomData<fn() -> R>,
}

/// The scrolling (neither pinned nor floating) variant.
pub type ScrollingPersistentHeaderView =
    PersistentHeaderView<RenderSliverScrollingPersistentHeader>;
/// The pinned variant.
pub type PinnedPersistentHeaderView = PersistentHeaderView<RenderSliverPinnedPersistentHeader>;
/// The floating variant.
pub type FloatingPersistentHeaderView = PersistentHeaderView<RenderSliverFloatingPersistentHeader>;
/// The floating + pinned variant.
pub type FloatingPinnedPersistentHeaderView =
    PersistentHeaderView<RenderSliverFloatingPinnedPersistentHeader>;

impl<R> PersistentHeaderView<R> {
    /// Wrap a delegate in this variant.
    #[must_use]
    pub fn new(delegate: SharedHeaderDelegate) -> Self {
        Self {
            delegate,
            _variant: PhantomData,
        }
    }
}

impl<R> Clone for PersistentHeaderView<R> {
    fn clone(&self) -> Self {
        Self {
            delegate: Rc::clone(&self.delegate),
            _variant: PhantomData,
        }
    }
}

impl<R> std::fmt::Debug for PersistentHeaderView<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentHeaderView")
            .field("min_extent", &self.delegate.min_extent())
            .field("max_extent", &self.delegate.max_extent())
            .finish_non_exhaustive()
    }
}

impl<R: PersistentHeaderRenderObject> RenderView for PersistentHeaderView<R> {
    type Protocol = SliverProtocol;
    type RenderObject = R;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        R::create(self.delegate.min_extent(), self.delegate.max_extent())
    }

    /// Refresh the extents; the child is rebuilt by `build_into_views`, and
    /// the cell survives rebuilds untouched (same invariant as
    /// `LayoutBuilder`).
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.update_extents(self.delegate.min_extent(), self.delegate.max_extent())
    }

    /// The child is produced by `build_into_views` from the published shrink
    /// state, not carried on the view. Same invariant as `LayoutBuilder`.
    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {}
}

impl<R: PersistentHeaderRenderObject> View for PersistentHeaderView<R> {
    fn create_element(&self) -> crate::element::ElementKind {
        // Custom behavior (not the generic `RenderBehavior`) so `on_mount`
        // mints + installs + registers the shrink cell and `build_into_views`
        // builds from the published shrink state.
        crate::element::ElementKind::RenderVariable(Box::new(PersistentHeaderElement::new(
            self,
            PersistentHeaderBehavior::new(),
        )))
    }

    /// Skip when the delegate is the same instance, or when a distinct
    /// delegate declares the swap unobservable — `should_rebuild`'s contract
    /// obliges it to answer `true` if content *or extents* could differ, so
    /// skipping on `false` also safely skips the extent refresh.
    fn should_skip_rebuild(&self, previous: &Self) -> bool {
        Rc::ptr_eq(&self.delegate, &previous.delegate)
            || !self.delegate.should_rebuild(previous.delegate.as_ref())
    }
}

impl<R: PersistentHeaderRenderObject> crate::element::RenderElementBase<Variable>
    for PersistentHeaderElement<R>
{
}

/// The concrete element type for one persistent-header variant.
pub(crate) type PersistentHeaderElement<R> =
    Element<PersistentHeaderView<R>, Variable, PersistentHeaderBehavior<R>>;

// ============================================================================
// BEHAVIOR
// ============================================================================

/// Element behavior for [`PersistentHeaderView`].
///
/// Wraps the generic [`RenderBehavior`] for render-object creation / disposal
/// and adds the seam: cell mint + install + registry lifecycle, and a
/// `build_into_views` that reads the published shrink state.
pub(crate) struct PersistentHeaderBehavior<R: PersistentHeaderRenderObject> {
    /// Handles render-object creation, attachment, and removal.
    inner: RenderBehavior<PersistentHeaderView<R>>,
    /// The mailbox shared with the render object. `None` until `on_mount`
    /// mints it and installs it on the freshly created render object.
    cell: Option<Arc<HeaderShrinkCell>>,
}

impl<R: PersistentHeaderRenderObject> std::fmt::Debug for PersistentHeaderBehavior<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentHeaderBehavior")
            .field("render_id", &self.inner.render_id)
            .field("published", &self.cell.as_ref().and_then(|c| c.shrink()))
            .finish_non_exhaustive()
    }
}

impl<R: PersistentHeaderRenderObject> PersistentHeaderBehavior<R> {
    pub(crate) fn new() -> Self {
        Self {
            inner: RenderBehavior::new(),
            cell: None,
        }
    }
}

impl<R: PersistentHeaderRenderObject> ElementBehavior<PersistentHeaderView<R>, Variable>
    for PersistentHeaderBehavior<R>
{
    fn debug_kind(&self) -> &'static str {
        "PersistentHeaderElement"
    }

    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.inner.render_id
    }

    /// Build the child from the shrink state the render object published.
    ///
    /// Runs inside `BuildOwner::service_layout_builders`' `build_scope`, i.e.
    /// *between* layout passes, with no pipeline lock and no arena borrow
    /// held.
    fn build_into_views(
        &mut self,
        core: &mut ElementCore<PersistentHeaderView<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        if !should_build_with_trace(core, "PersistentHeaderBehavior") {
            return Vec::new();
        }

        // No layout pass has run yet, so no shrink state exists. Build no
        // child rather than guessing `0.0`: the render object sizes from its
        // extents alone for this single pass, publishes, and the fixpoint
        // rebuilds us with the real pair before the frame paints.
        let Some(shrink) = self.cell.as_ref().and_then(|cell| cell.shrink()) else {
            tracing::debug!(
                "PersistentHeaderBehavior: no shrink state published yet — deferring the \
                 child to this frame's next fixpoint pass"
            );
            core.clear_dirty();
            return Vec::new();
        };

        let ctx_choice = make_build_ctx(core, owner);
        let ctx = ctx_choice.as_ctx();
        let view = core.view().clone();
        let child_view = build_or_recover(core, owner, "PersistentHeaderElement", move || {
            // `BoxedView` is a newtype over `Box<dyn View>`; unwrap it for
            // the reconciler, which speaks `Box<dyn View>`.
            view.delegate
                .build(ctx, shrink.shrink_offset, shrink.overlaps_content)
                .0
        });
        single_child_views(core, child_view, "PersistentHeaderBehavior")
    }

    /// Create the render object, then mint the cell, install it, and register
    /// it under the render id.
    fn on_mount(
        &mut self,
        core: &mut ElementCore<PersistentHeaderView<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // Step 1: the inner behavior creates and inserts the render object.
        self.inner.on_mount(core, owner);

        let Some(render_id) = self.inner.render_id else {
            tracing::warn!(
                "PersistentHeaderBehavior::on_mount: no render object was created \
                 (no PipelineOwner?) — the shrink seam is inert for this element"
            );
            return;
        };

        // Step 2: mint the mailbox and install it on the render object. The
        // element mints (unlike `RenderLayoutBuilder`, whose constructor owns
        // its cell) because the same `Arc` must also land in the registry
        // below — one mailbox, two holders, minted where both are in reach.
        let cell = Arc::new(HeaderShrinkCell::new());
        let installed = core.pipeline_owner().is_some_and(|pipeline| {
            pipeline.with_mut(|owner| {
                owner
                    .render_tree_mut()
                    .get_mut(render_id)
                    .and_then(|node| node.downcast_render_object_mut::<R>())
                    .map(|render_object| {
                        render_object.install_shrink_cell(Arc::clone(&cell));
                    })
                    .is_some()
            })
        });

        if !installed {
            tracing::warn!(
                ?render_id,
                "PersistentHeaderBehavior::on_mount: could not install the shrink cell \
                 on the render object"
            );
            return;
        }

        // Step 3: register. `self_id` is stamped by `ElementTree::insert`
        // before `on_mount` fires (same ordering `LayoutBuilderBehavior`
        // relies on).
        let Some(self_id) = core.self_id() else {
            tracing::warn!(
                ?render_id,
                "PersistentHeaderBehavior::on_mount: no self_id stamped — cannot register"
            );
            return;
        };

        self.cell = Some(Arc::clone(&cell));
        owner.register_layout_builder(render_id, self_id, cell);
    }

    /// Unregister before the render object is disposed.
    ///
    /// `service_layout_builders` also prunes entries whose element or render
    /// node has vanished, but that is a safety net for reconcile races, not
    /// the cleanup path.
    fn on_unmount(
        &mut self,
        core: &mut ElementCore<PersistentHeaderView<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        if let Some(render_id) = self.inner.render_id {
            owner.unregister_layout_builder(render_id);
        }
        self.cell = None;
        self.inner.on_unmount(core, owner);
    }

    fn on_update(
        &mut self,
        core: &ElementCore<PersistentHeaderView<R>, Variable>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.inner.on_update(core, owner);
    }
}
