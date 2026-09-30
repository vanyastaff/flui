//! `LayoutBuilder` view + element — the element half of the build-during-layout
//! seam.
//!
//! # What this wires together
//!
//! - `owner/layout_builder.rs`: `BuildOwner::service_layout_builders`
//!   and the bounded layout↔build fixpoint both bindings drive.
//! - `flui_objects::RenderLayoutBuilder`: the render half, which
//!   publishes the real incoming `BoxConstraints` into a shared
//!   [`LayoutConstraintsCell`] on every layout pass.
//! - This module: the element that owns the cell, registers
//!   `RenderId -> (ElementId, cell)` at mount, and — between layout passes —
//!   rebuilds its child by handing the *published* constraints to a user
//!   builder.
//!
//! # Same-frame settling
//!
//! ```text
//! run_layout      → RenderLayoutBuilder publishes C, raises needs_build
//! service_*       → this element rebuilds; builder(C) produces the child;
//!                   reconcile mounts it; cell.commit(); mark_needs_layout
//! run_layout      → the fresh child is laid out under C
//! service_*       → C republished == committed ⇒ clean ⇒ fixpoint converges
//! run_frame       → compositing / paint
//! ```
//!
//! The child is therefore laid out **and painted in the same frame** the
//! builder ran. Lazy `SliverList` originally shipped one frame late (ADR-0017's
//! rejected alternative for this widget); it has since adopted this same
//! fixpoint — `service_child_requests` runs beside `service_layout_builders`
//! in every pass — so both deferred-build seams now settle within the frame.
//!
//! # The first build has no constraints, and does not invent any
//!
//! Before the very first layout pass nothing has published, so the builder
//! **cannot** be called: there is no honest `BoxConstraints` to hand it. This
//! element then builds **no child** — `RenderLayoutBuilder` sizes itself to
//! `constraints.biggest()` for that one pass, publishes, and the fixpoint's
//! next iteration builds the real child in the same frame. It never passes
//! `BoxConstraints::UNCONSTRAINED` or a default to the builder; that placeholder
//! is exactly what the pre-rewrite `LayoutBuilder` (commit `bb58a8fa`) did, and
//! what ADR-0017 exists to avoid.
//!
//! # Public surface
//!
//! [`LayoutBuilder`] is public (re-exported as `flui_widgets::LayoutBuilder` and
//! from `flui_widgets::prelude`). The element, behavior, and erased builder alias
//! stay `pub(crate)` — nothing outside this crate needs them.
//!
//! The intrinsics, dry-layout, and double-invocation divergences are recorded
//! in ADR-0017.

use std::{rc::Rc, sync::Arc};

use flui_objects::{LayoutConstraintsCell, RenderLayoutBuilder};
use flui_rendering::{constraints::BoxConstraints, protocol::BoxProtocol};

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
    view::{IntoView, RenderView, View, ViewExt},
};

// ============================================================================
// VIEW CONFIG
// ============================================================================

/// The erased builder closure stored on [`LayoutBuilder`].
///
/// `Rc<dyn Fn…>` (rather than a generic parameter) so the view stays cheaply
/// cloneable and object-safe as a `dyn View` while keeping the builder
/// UI-owner-local under ADR-0027.
pub(crate) type LayoutWidgetBuilder = Rc<dyn Fn(&dyn BuildContext, BoxConstraints) -> BoxedView>;

/// A widget whose child is built from the constraints its parent imposes.
///
/// The builder runs during layout, with the **real** incoming
/// [`BoxConstraints`], and its child is laid out and painted in the same frame.
/// Use it to pick a layout from the space actually available:
///
/// ```
/// use flui_view::element::LayoutBuilder;
/// use flui_view::view::ErrorView;
///
/// let responsive = LayoutBuilder::new(|_ctx, constraints| {
///     if constraints.max_width > 600.0 {
///         ErrorView::new("wide layout")
///     } else {
///         ErrorView::new("narrow layout")
///     }
/// });
/// ```
///
/// The builder is **not** re-invoked when the parent passes the same
/// constraints again; it *is* re-invoked when the constraints change, when this
/// widget is rebuilt with a new builder, or when a dependency it read changes.
///
/// The widget's final size is `constraints.constrain(child.size)` — it follows
/// its child. With no child it fills `constraints.biggest()`.
///
/// # Unsupported
///
/// Intrinsic dimensions and dry layout, because both would require running the
/// builder speculatively. They answer `0.0` / `Size::ZERO` and log an error, in
/// place of a throw. See `flui_objects::RenderLayoutBuilder`.
#[derive(Clone)]
pub struct LayoutBuilder {
    /// Called with the constraints published by the render object.
    builder: LayoutWidgetBuilder,
}

impl LayoutBuilder {
    /// Build a child from the constraints this widget's parent imposes.
    ///
    /// The closure is called during layout with the real incoming constraints.
    pub fn new<F, R>(builder: F) -> Self
    where
        F: Fn(&dyn BuildContext, BoxConstraints) -> R + 'static,
        R: IntoView,
    {
        Self {
            builder: Rc::new(move |ctx, constraints| builder(ctx, constraints).into_view().boxed()),
        }
    }
}

impl std::fmt::Debug for LayoutBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutBuilder").finish_non_exhaustive()
    }
}

// ============================================================================
// RenderView impl
// ============================================================================

impl RenderView for LayoutBuilder {
    type Protocol = BoxProtocol;
    type RenderObject = RenderLayoutBuilder;

    /// Mints the render object **and** the cell it publishes into.
    ///
    /// The cell is created here, not on the view, because a view is rebuilt
    /// (and so reconstructed) on every parent rebuild — a cell owned by the
    /// view would be a fresh `Arc` each time, silently orphaning the one the
    /// render object and the registry hold. `create_render_object` runs exactly
    /// once per mount; `LayoutBuilderBehavior::on_mount` reads the cell back out
    /// of the render object, which is therefore the single source of truth.
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderLayoutBuilder::new(Arc::new(LayoutConstraintsCell::new()))
    }

    /// Deliberately empty: the builder closure lives on the view, never on the
    /// render object, and the cell must survive rebuilds untouched.
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    /// The child is produced by `build_into_views` from the published
    /// constraints, not carried on the view — so there is nothing static to
    /// visit. Same invariant as `SliverList`.
    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {}
}

impl View for LayoutBuilder {
    fn create_element(&self) -> crate::element::ElementKind {
        // Custom behavior (not the generic `RenderBehavior`) so `on_mount`
        // registers the cell in `BuildOwner::layout_builder_registry` and
        // `build_into_views` builds from the published constraints.
        crate::element::ElementKind::RenderVariable(Box::new(LayoutBuilderElement::new(
            self,
            LayoutBuilderBehavior::new(),
        )))
    }

    fn should_skip_rebuild(&self, previous: &Self) -> bool {
        Rc::ptr_eq(&self.builder, &previous.builder)
    }
}

/// `LayoutBuilder` uses a custom behavior, so it needs its own
/// `RenderElementBase<Variable>` tag to route into `ElementKind::RenderVariable`
/// — the `RenderBehavior` blanket impl does not cover this behavior.
impl crate::element::RenderElementBase<Variable> for LayoutBuilderElement {}

/// The concrete element type for [`LayoutBuilder`].
pub(crate) type LayoutBuilderElement = Element<LayoutBuilder, Variable, LayoutBuilderBehavior>;

// ============================================================================
// BEHAVIOR
// ============================================================================

/// Element behavior for [`LayoutBuilder`].
///
/// Wraps the generic [`RenderBehavior`] for render-object creation / disposal
/// and adds the two things the seam needs: registry lifecycle, and a
/// `build_into_views` that reads the published constraints.
pub(crate) struct LayoutBuilderBehavior {
    /// Handles `RenderLayoutBuilder` creation, attachment, and removal.
    inner: RenderBehavior<LayoutBuilder>,
    /// The cell shared with the render object. `None` until `on_mount` reads it
    /// back out of the freshly created render object.
    cell: Option<Arc<LayoutConstraintsCell>>,
}

impl std::fmt::Debug for LayoutBuilderBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutBuilderBehavior")
            .field("render_id", &self.inner.render_id)
            .field(
                "published",
                &self.cell.as_ref().and_then(|c| c.constraints()),
            )
            .finish_non_exhaustive()
    }
}

impl LayoutBuilderBehavior {
    pub(crate) fn new() -> Self {
        Self {
            inner: RenderBehavior::new(),
            cell: None,
        }
    }
}

impl ElementBehavior<LayoutBuilder, Variable> for LayoutBuilderBehavior {
    fn debug_kind(&self) -> &'static str {
        "LayoutBuilderElement"
    }

    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.inner.render_id
    }

    /// Build the child from the constraints the render object published.
    ///
    /// Runs inside `BuildOwner::service_layout_builders`' `build_scope`, i.e.
    /// *between* layout passes, with no pipeline lock and no arena borrow held.
    fn build_into_views(
        &mut self,
        core: &mut ElementCore<LayoutBuilder, Variable>,
        owner: &mut ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        if !should_build_with_trace(core, "LayoutBuilderBehavior") {
            return Vec::new();
        }

        // No layout pass has run yet, so no constraints exist. Build no child
        // rather than inventing one: `RenderLayoutBuilder` sizes to
        // `constraints.biggest()` for this single pass, publishes, and the
        // fixpoint rebuilds us with real constraints before the frame paints.
        let Some(constraints) = self.cell.as_ref().and_then(|cell| cell.constraints()) else {
            tracing::debug!(
                "LayoutBuilderBehavior: no constraints published yet — deferring the child \
                 to this frame's next fixpoint pass"
            );
            core.clear_dirty();
            return Vec::new();
        };

        let ctx_choice = make_build_ctx(core, owner);
        let ctx = ctx_choice.as_ctx();
        let view = core.view().clone();
        let child_view = build_or_recover(core, owner, "LayoutBuilderElement", move || {
            // `BoxedView` is a newtype over `Box<dyn View>`; unwrap it for the
            // reconciler, which speaks `Box<dyn View>`.
            (view.builder)(ctx, constraints).0
        });
        single_child_views(core, child_view, "LayoutBuilderBehavior")
    }

    /// Create the render object, then register its cell under its `RenderId`.
    fn on_mount(
        &mut self,
        core: &mut ElementCore<LayoutBuilder, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // Step 1: the inner behavior creates and inserts `RenderLayoutBuilder`.
        self.inner.on_mount(core, owner);

        // Step 2: read the cell back out of the render object it just made. The
        // render object is the single owner of that `Arc`; cloning it here is
        // what makes the element and the render half share one channel.
        let Some(render_id) = self.inner.render_id else {
            tracing::warn!(
                "LayoutBuilderBehavior::on_mount: no render object was created \
                 (no PipelineOwner?) — the layout-builder seam is inert for this element"
            );
            return;
        };

        let cell = core.pipeline_owner().and_then(|pipeline| {
            pipeline.with_mut(|owner| {
                owner
                    .render_tree_mut()
                    .get_mut(render_id)
                    .and_then(|node| node.downcast_render_object_mut::<RenderLayoutBuilder>())
                    .map(|render_object| Arc::clone(render_object.cell()))
            })
        });

        let Some(cell) = cell else {
            tracing::warn!(
                ?render_id,
                "LayoutBuilderBehavior::on_mount: could not read the constraints cell \
                 back from the render object"
            );
            return;
        };

        // Step 3: register. `self_id` is stamped by `ElementTree::insert` before
        // `on_mount` fires (same ordering `SliverAdaptorBehavior` relies on).
        let Some(self_id) = core.self_id() else {
            tracing::warn!(
                ?render_id,
                "LayoutBuilderBehavior::on_mount: no self_id stamped — cannot register"
            );
            return;
        };

        self.cell = Some(Arc::clone(&cell));
        owner.register_layout_builder(render_id, self_id, cell);
    }

    /// Unregister before the render object is disposed.
    ///
    /// `service_layout_builders` also prunes entries whose element or render
    /// node has vanished, but that is a **safety net for reconcile races**, not
    /// the cleanup path — relying on it is how the sliver adaptor grew its
    /// stale-entry bug.
    fn on_unmount(
        &mut self,
        core: &mut ElementCore<LayoutBuilder, Variable>,
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
        core: &ElementCore<LayoutBuilder, Variable>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.inner.on_update(core, owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use flui_foundation::RenderId;
    use flui_foundation::geometry::Size;
    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

    use crate::{BuildOwner, IntoView, tree::ElementTree, view::ViewExt};

    /// A leaf view of a fixed size — the child a builder returns.
    #[derive(Clone, Debug)]
    struct FixedBox(f64, f64);

    impl RenderView for FixedBox {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(self.0), Some(self.1))
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.set_size(Some(self.0), Some(self.1))
        }
    }

    impl View for FixedBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// The three things a frame needs, wired as the bindings wire them.
    struct Harness {
        owner: BuildOwner,
        tree: ElementTree,
        pipeline: PipelineCell,
        root_render: RenderId,
    }

    impl Harness {
        fn mount(view: &dyn View, constraints: BoxConstraints) -> Self {
            let pipeline = PipelineCell::new(PipelineOwner::new(
                flui_rendering::TextContextHandle::standalone(),
            ));
            let mut owner = BuildOwner::new();
            let mut tree = ElementTree::new();

            let root = tree.mount_root_with_pipeline_owner(
                view,
                Some(pipeline.clone()),
                &mut owner.element_owner_mut(),
            );

            // Reconcile + mount the whole subtree, so a `StatelessView` root
            // has produced its render-object descendants. Same shape as
            // `flui-widgets`' `tests/common::lay_out`.
            owner.schedule_build_for(root, 0, crate::RebuildReason::InitialMount);
            owner.build_scope(&mut tree);

            // The render root is the single render node with no render parent —
            // works whether the root view is itself a `RenderView` or a
            // `StatelessView` whose composition owns the outermost render object.
            let root_render = pipeline.with(|owner| {
                let render_tree = owner.render_tree();
                let mut roots = render_tree
                    .iter()
                    .map(|(id, _)| id)
                    .filter(|id| render_tree.parent(*id).is_none());
                let found = roots.next().expect("the subtree must have a render root");
                assert!(roots.next().is_none(), "exactly one render root expected");
                found
            });

            pipeline.with_mut(|owner| {
                owner.set_root_id(Some(root_render));
                owner.set_root_constraints(Some(constraints));
            });

            Self {
                owner,
                tree,
                pipeline,
                root_render,
            }
        }

        /// One frame, exactly as `HeadlessBinding::pump_frame` drives it.
        fn frame(&mut self) {
            self.owner.build_scope(&mut self.tree);
            self.owner
                .run_frame_with_layout_builders(&mut self.tree, &self.pipeline)
                .expect("frame must succeed");
        }

        fn set_constraints(&mut self, constraints: BoxConstraints) {
            self.pipeline
                .with_mut(|owner| owner.set_root_constraints(Some(constraints)));
        }

        fn root_size(&self) -> Size {
            self.pipeline
                .with(|owner| {
                    flui_rendering::testing::inspect::box_geometry(owner, self.root_render)
                })
                .expect("root must have committed geometry")
        }
    }

    /// A builder that records every constraint it was called with.
    fn recording_builder(log: Arc<parking_lot::Mutex<Vec<BoxConstraints>>>) -> LayoutWidgetBuilder {
        Rc::new(move |_ctx, constraints| {
            log.lock().push(constraints);
            FixedBox(20.0, 20.0).into_view().boxed()
        })
    }

    fn tight(w: f64, h: f64) -> BoxConstraints {
        BoxConstraints::tight(Size::new(w, h))
    }

    // ── 1. first frame ──────────────────────────────────────────────────────

    /// The `RenderId` of the layout builder's single child.
    ///
    /// The layout builder is the render root in most of these tests; where it is
    /// not (a `StatelessView` parent owns no render object, so the builder is
    /// still the render root), this stays correct.
    fn child_render_id(h: &Harness) -> RenderId {
        let children = h.pipeline.with(|owner| {
            owner
                .render_tree()
                .get(h.root_render)
                .expect("root render node")
                .children()
                .to_vec()
        });
        assert_eq!(
            children.len(),
            1,
            "layout builder must have exactly one child"
        );
        children[0]
    }

    // ── 2. constraint change ────────────────────────────────────────────────

    #[test]
    fn layout_builder_constraint_change_rebuilds_in_the_same_frame() {
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let view = LayoutBuilder {
            builder: recording_builder(Arc::clone(&log)),
        };

        let first = tight(120.0, 80.0);
        let second = tight(60.0, 40.0);

        let mut h = Harness::mount(&view, first);
        h.frame();
        assert_eq!(log.lock().as_slice(), &[first]);

        h.set_constraints(second);
        h.frame();

        assert_eq!(
            log.lock().as_slice(),
            &[first, second],
            "a resized parent must re-invoke the builder with the new constraints"
        );
        assert_eq!(h.root_size(), Size::new(60.0, 40.0));
        let child_render = child_render_id(&h);
        assert_eq!(
            h.pipeline
                .with(|owner| flui_rendering::testing::inspect::box_geometry(owner, child_render)),
            Some(Size::new(60.0, 40.0)),
            "the rebuilt child must be relaid out in the same frame"
        );
    }

    // ── 3. same constraints ─────────────────────────────────────────────────

    // ── 4. registration lifecycle ───────────────────────────────────────────

    // ── 5. reconciliation ───────────────────────────────────────────────────

    // ── builder update semantics ────────────────────────────────────────────

    // ── error recovery ──────────────────────────────────────────────────────

    // ── 6. nesting ──────────────────────────────────────────────────────────
}
