//! `LayoutBuilder` view + element — the element half of the build-during-layout
//! seam (ADR-0017, unit U3).
//!
//! # What this wires together
//!
//! - **U1** (`owner/layout_builder.rs`): `BuildOwner::service_layout_builders`
//!   and the bounded layout↔build fixpoint both bindings drive.
//! - **U2** (`flui_objects::RenderLayoutBuilder`): the render half, which
//!   publishes the real incoming `BoxConstraints` into a shared
//!   [`LayoutConstraintsCell`] on every layout pass.
//! - **U3** (this module): the element that owns the cell, registers
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
//! builder ran. This is deliberately *not* the one-frame-late shape lazy
//! `SliverList` uses (see ADR-0017's rejected alternatives).
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
//! Cross-checked against `.flutter/packages/flutter/lib/src/widgets/layout_builder.dart`
//! and `packages/flutter/test/widgets/layout_builder_test.dart` (Flutter master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`) as ADR-0017 U4. `performLayout`, the skip
//! condition, and the update/error semantics match; the intrinsics, dry-layout,
//! and double-invocation divergences are recorded in ADR-0017's *Parity findings*.

use std::sync::Arc;

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
/// `Arc<dyn Fn…>` (rather than a generic parameter) so the view stays
/// `Clone + Send + Sync + 'static` and object-safe as a `dyn View`, matching
/// `SliverList`'s item-builder shape.
pub(crate) type LayoutWidgetBuilder =
    Arc<dyn Fn(&dyn BuildContext, BoxConstraints) -> BoxedView + Send + Sync>;

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
///     if constraints.max_width.get() > 600.0 {
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
/// place of Flutter's throw. See `flui_objects::RenderLayoutBuilder`.
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
        F: Fn(&dyn BuildContext, BoxConstraints) -> R + Send + Sync + 'static,
        R: IntoView,
    {
        Self {
            builder: Arc::new(move |ctx, constraints| {
                builder(ctx, constraints).into_view().boxed()
            }),
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
    fn create_render_object(&self) -> Self::RenderObject {
        RenderLayoutBuilder::new(Arc::new(LayoutConstraintsCell::new()))
    }

    /// Deliberately empty: the builder closure lives on the view, never on the
    /// render object, and the cell must survive rebuilds untouched.
    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}

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
            pipeline
                .write()
                .render_tree_mut()
                .get_mut(render_id)
                .and_then(|node| node.downcast_render_object_mut::<RenderLayoutBuilder>())
                .map(|render_object| Arc::clone(render_object.cell()))
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
        // `on_mount` fires (same ordering `SliverListAdaptorBehavior` relies on).
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

    fn on_update(&mut self, core: &ElementCore<LayoutBuilder, Variable>) {
        self.inner.on_update(core);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::{ElementId, RenderId};
    use flui_objects::{RenderConstrainedBox, RenderSizedBox};
    use flui_rendering::pipeline::PipelineOwner;
    use flui_types::{Size, geometry::px};
    use parking_lot::RwLock;

    use crate::{BuildOwner, IntoView, tree::ElementTree, view::ViewExt};

    /// A leaf view of a fixed size — the child a builder returns.
    #[derive(Clone, Debug)]
    struct FixedBox(f32, f32);

    impl RenderView for FixedBox {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(self.0)), Some(px(self.1)))
        }

        fn update_render_object(&self, render_object: &mut Self::RenderObject) {
            *render_object = RenderSizedBox::new(Some(px(self.0)), Some(px(self.1)));
        }
    }

    impl View for FixedBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// A structurally different leaf — used to prove reconcile replaces the
    /// child when the builder switches shape.
    #[derive(Clone, Debug)]
    struct TightBox(f32);

    impl RenderView for TightBox {
        type Protocol = BoxProtocol;
        type RenderObject = RenderConstrainedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderConstrainedBox::new(BoxConstraints::tight(Size::new(px(self.0), px(self.0))))
        }

        fn update_render_object(&self, render_object: &mut Self::RenderObject) {
            *render_object =
                RenderConstrainedBox::new(BoxConstraints::tight(Size::new(px(self.0), px(self.0))));
        }
    }

    impl View for TightBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// The three things a frame needs, wired as the bindings wire them.
    struct Harness {
        owner: BuildOwner,
        tree: ElementTree,
        pipeline: Arc<RwLock<PipelineOwner>>,
        root: ElementId,
        root_render: RenderId,
    }

    impl Harness {
        fn mount(view: &dyn View, constraints: BoxConstraints) -> Self {
            let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
            let mut owner = BuildOwner::new();
            let mut tree = ElementTree::new();

            let root = tree.mount_root_with_pipeline_owner(
                view,
                Some(Arc::clone(&pipeline)),
                &mut owner.element_owner_mut(),
            );

            // Reconcile + mount the whole subtree, so a `StatelessView` root
            // has produced its render-object descendants. Same shape as
            // `flui-widgets`' `tests/common::lay_out`.
            owner.schedule_build_for(root, 0);
            owner.build_scope(&mut tree);

            // The render root is the single render node with no render parent —
            // works whether the root view is itself a `RenderView` or a
            // `StatelessView` whose composition owns the outermost render object.
            let root_render = {
                let guard = pipeline.read();
                let render_tree = guard.render_tree();
                let mut roots = render_tree
                    .iter()
                    .map(|(id, _)| id)
                    .filter(|id| render_tree.parent(*id).is_none());
                let found = roots.next().expect("the subtree must have a render root");
                assert!(roots.next().is_none(), "exactly one render root expected");
                found
            };

            {
                let mut guard = pipeline.write();
                guard.set_root_id(Some(root_render));
                guard.set_root_constraints(Some(constraints));
            }

            Self {
                owner,
                tree,
                pipeline,
                root,
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
                .write()
                .set_root_constraints(Some(constraints));
        }

        fn root_size(&self) -> Size {
            flui_rendering::testing::inspect::box_geometry(&*self.pipeline.read(), self.root_render)
                .expect("root must have committed geometry")
        }
    }

    /// A builder that records every constraint it was called with.
    fn recording_builder(log: Arc<parking_lot::Mutex<Vec<BoxConstraints>>>) -> LayoutWidgetBuilder {
        Arc::new(move |_ctx, constraints| {
            log.lock().push(constraints);
            FixedBox(20.0, 20.0).into_view().boxed()
        })
    }

    fn tight(w: f32, h: f32) -> BoxConstraints {
        BoxConstraints::tight(Size::new(px(w), px(h)))
    }

    // ── 1. first frame ──────────────────────────────────────────────────────

    /// The builder receives the REAL incoming constraints on the first frame,
    /// and the child it returns is laid out in that same frame — no second
    /// pump, no placeholder constraints.
    #[test]
    fn layout_builder_first_frame_builds_with_real_constraints() {
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let view = LayoutBuilder {
            builder: recording_builder(Arc::clone(&log)),
        };
        let incoming = tight(120.0, 80.0);

        let mut h = Harness::mount(&view, incoming);
        h.frame();

        assert_eq!(
            log.lock().as_slice(),
            &[incoming],
            "the builder must be called exactly once, with the real constraints"
        );
        assert_eq!(
            h.root_size(),
            Size::new(px(120.0), px(80.0)),
            "the builder's node is laid out under its own constraints"
        );

        // Same-frame proof: the child element exists and its render object has
        // committed geometry after ONE frame.
        let child_render = child_render_id(&h);
        assert_eq!(
            flui_rendering::testing::inspect::box_geometry(&*h.pipeline.read(), child_render),
            Some(Size::new(px(120.0), px(80.0))),
            "the child returned by the builder must be laid out in the SAME frame; \
             a one-frame-late seam leaves it without committed geometry"
        );
    }

    /// The `RenderId` of the layout builder's single child.
    ///
    /// The layout builder is the render root in most of these tests; where it is
    /// not (a `StatelessView` parent owns no render object, so the builder is
    /// still the render root), this stays correct.
    fn child_render_id(h: &Harness) -> RenderId {
        let guard = h.pipeline.read();
        let children = guard
            .render_tree()
            .get(h.root_render)
            .expect("root render node")
            .children()
            .to_vec();
        assert_eq!(
            children.len(),
            1,
            "layout builder must have exactly one child"
        );
        children[0]
    }

    /// Flutter's own `LayoutBuilder parent size` oracle
    /// (`.flutter/packages/flutter/test/widgets/layout_builder_test.dart:10`),
    /// transcribed: under **loose** constraints the builder's node sizes to
    /// `constraints.constrain(child.size)` — it follows its child, it does not
    /// fill the space. Tight constraints cannot tell those two apart, so this is
    /// the test that actually pins `performLayout`'s final-size rule.
    #[test]
    fn layout_builder_loose_constraints_size_follows_the_child() {
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        // Flutter: Center > ConstrainedBox(maxWidth: 100, maxHeight: 200).
        let incoming = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(200.0));
        let view = LayoutBuilder {
            builder: Arc::new(move |_ctx, constraints: BoxConstraints| {
                log.lock().push(constraints);
                // Flutter's builder returns SizedBox(biggest/2).
                FixedBox(
                    constraints.max_width.get() / 2.0,
                    constraints.max_height.get() / 2.0,
                )
                .into_view()
                .boxed()
            }),
        };

        let mut h = Harness::mount(&view, incoming);
        h.frame();

        assert_eq!(
            h.root_size(),
            Size::new(px(50.0), px(100.0)),
            "size = constraints.constrain(child.size); it must NOT be constraints.biggest \
             (100x200) — the intermediate no-child pass must never survive into the frame"
        );
        assert_eq!(
            flui_rendering::testing::inspect::box_geometry(
                &*h.pipeline.read(),
                child_render_id(&h)
            ),
            Some(Size::new(px(50.0), px(100.0))),
        );
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
        assert_eq!(h.root_size(), Size::new(px(60.0), px(40.0)));
        assert_eq!(
            flui_rendering::testing::inspect::box_geometry(
                &*h.pipeline.read(),
                child_render_id(&h)
            ),
            Some(Size::new(px(60.0), px(40.0))),
            "the rebuilt child must be relaid out in the same frame"
        );
    }

    // ── 3. same constraints ─────────────────────────────────────────────────

    /// Unchanged constraints must not re-invoke the builder. This is what makes
    /// the fixpoint converge instead of rebuilding every frame forever.
    #[test]
    fn layout_builder_same_constraints_do_not_reinvoke_the_builder() {
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let view = LayoutBuilder {
            builder: recording_builder(Arc::clone(&log)),
        };
        let constraints = tight(120.0, 80.0);

        let mut h = Harness::mount(&view, constraints);
        h.frame();
        assert_eq!(log.lock().len(), 1);

        // Force more layout passes with identical constraints.
        for _ in 0..3 {
            h.pipeline.write().mark_needs_layout(h.root_render);
            h.frame();
        }

        assert_eq!(
            log.lock().len(),
            1,
            "the builder must run once; unchanged constraints are not a rebuild trigger"
        );
    }

    // ── 4. registration lifecycle ───────────────────────────────────────────

    /// Mount registers exactly one entry; unmount deregisters it. Stale pruning
    /// is a safety net for reconcile races, not the cleanup path.
    #[test]
    fn layout_builder_registers_on_mount_and_deregisters_on_unmount() {
        let view = LayoutBuilder {
            builder: Arc::new(|_ctx, _c| FixedBox(10.0, 10.0).into_view().boxed()),
        };
        let mut h = Harness::mount(&view, tight(50.0, 50.0));

        assert_eq!(
            h.owner.layout_builder_count(),
            1,
            "on_mount must register exactly one entry"
        );

        h.frame();
        assert_eq!(
            h.owner.layout_builder_count(),
            1,
            "a frame must not duplicate it"
        );

        // Unmount the subtree through the normal element path.
        let root = h.root;
        h.tree.remove(root, &mut h.owner.element_owner_mut());

        assert_eq!(
            h.owner.layout_builder_count(),
            0,
            "on_unmount must deregister — not leave it for the stale-entry prune"
        );
    }

    // ── 5. reconciliation ───────────────────────────────────────────────────

    /// A builder that returns a different child TYPE across a breakpoint —
    /// the whole point of `LayoutBuilder`. The old child must be replaced.
    #[test]
    fn layout_builder_replaces_the_child_when_the_builder_switches_shape() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_builder = Arc::clone(&calls);
        let view = LayoutBuilder {
            builder: Arc::new(move |_ctx, constraints: BoxConstraints| {
                calls_for_builder.fetch_add(1, Ordering::Relaxed);
                if constraints.max_width.get() > 100.0 {
                    FixedBox(30.0, 30.0).into_view().boxed()
                } else {
                    TightBox(15.0).into_view().boxed()
                }
            }),
        };

        let mut h = Harness::mount(&view, tight(120.0, 120.0));
        h.frame();
        let wide_child = child_render_id(&h);
        assert_eq!(
            flui_rendering::testing::inspect::box_geometry(&*h.pipeline.read(), wide_child),
            Some(Size::new(px(120.0), px(120.0))),
            "the wide branch's RenderSizedBox is stretched by the tight constraints"
        );

        // Cross the breakpoint: the builder now returns a different view type.
        h.set_constraints(tight(80.0, 80.0));
        h.frame();

        assert_eq!(calls.load(Ordering::Relaxed), 2);
        let narrow_child = child_render_id(&h);
        assert_ne!(
            narrow_child, wide_child,
            "a different view type must remount, not update in place"
        );
        assert!(
            h.pipeline.read().render_tree().get(wide_child).is_none(),
            "the replaced child's render object must be removed from the tree"
        );
    }

    // ── builder update semantics ────────────────────────────────────────────

    /// Flutter's `AbstractLayoutBuilder.updateShouldRebuild` defaults to `true`:
    /// a parent rebuild that supplies a **new builder closure** re-invokes it,
    /// even when the constraints are unchanged
    /// (`.flutter/packages/flutter/lib/src/widgets/layout_builder.dart`,
    /// `_LayoutBuilderElement.update` → `_needsBuild = true`).
    ///
    /// FLUI reaches the same observable behavior through the ordinary
    /// dirty-element path: reconcile updates the child element, sees it dirty,
    /// and schedules it (`tree/id_reconcile.rs`); `build_into_views` then calls
    /// the *new* closure with the last published constraints. The rebuild is
    /// driven through a real parent here, not a direct `tree.update`, precisely
    /// so that scheduling step is exercised rather than assumed.
    #[test]
    fn layout_builder_new_builder_closure_is_honored_on_update() {
        /// Rebuilds into a `LayoutBuilder` whose closure depends on `variant`.
        #[derive(Clone, Debug)]
        struct Parent {
            variant: Arc<AtomicUsize>,
            calls: Arc<AtomicUsize>,
        }

        impl crate::view::StatelessView for Parent {
            fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
                let calls = Arc::clone(&self.calls);
                let variant = self.variant.load(Ordering::Relaxed);
                LayoutBuilder {
                    builder: Arc::new(move |_ctx, _constraints| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        if variant == 0 {
                            FixedBox(10.0, 10.0).into_view().boxed()
                        } else {
                            TightBox(15.0).into_view().boxed()
                        }
                    }),
                }
            }
        }

        impl View for Parent {
            fn create_element(&self) -> crate::element::ElementKind {
                crate::element::ElementKind::stateless(self)
            }
        }

        let variant = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let parent = Parent {
            variant: Arc::clone(&variant),
            calls: Arc::clone(&calls),
        };

        let mut h = Harness::mount(&parent, tight(120.0, 80.0));
        h.frame();
        assert_eq!(calls.load(Ordering::Relaxed), 1, "first frame builds once");
        let first_child = child_render_id(&h);

        // Parent rebuild supplies a NEW closure; constraints are unchanged.
        variant.store(1, Ordering::Relaxed);
        let root = h.root;
        let depth = h.tree.get(root).map_or(0, |node| node.depth);
        h.tree.mark_needs_build(root);
        h.owner.schedule_build_for(root, depth);
        h.frame();

        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "a new builder closure must be invoked even though the constraints did not change"
        );
        assert_ne!(
            child_render_id(&h),
            first_child,
            "the new closure's differently-typed child must replace the old one"
        );
    }

    // ── error recovery ──────────────────────────────────────────────────────

    /// A panicking builder is caught by `build_or_recover` and substituted with
    /// the error view — Flutter's `_rebuildWithConstraints` does the same with
    /// `ErrorWidget.builder`. The frame must still settle, and the registry must
    /// not be corrupted (the cell is committed, so the fixpoint converges instead
    /// of spinning until the pass bound trips).
    #[test]
    fn layout_builder_panicking_builder_recovers_and_the_frame_settles() {
        let view = LayoutBuilder {
            builder: Arc::new(|_ctx, _c| panic!("builder blew up")),
        };
        let mut h = Harness::mount(&view, tight(60.0, 60.0));

        // Must not panic, must not hang, must not trip the non-convergence guard.
        h.frame();

        assert_eq!(
            h.owner.layout_builder_count(),
            1,
            "the registry entry must survive a builder panic"
        );
        // A second frame with unchanged constraints must not re-invoke anything.
        h.frame();
        assert_eq!(h.owner.layout_builder_count(), 1);
    }

    // ── 6. nesting ──────────────────────────────────────────────────────────

    /// A `LayoutBuilder` inside a `LayoutBuilder` settles within the pass bound:
    /// the inner one's constraints only become known once the outer one's fresh
    /// child has been laid out, so it needs one extra pass.
    #[test]
    fn layout_builder_nested_converges_within_the_pass_bound() {
        let inner_log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let inner_log_for_builder = Arc::clone(&inner_log);

        let outer = LayoutBuilder {
            builder: Arc::new(move |_ctx, _outer_constraints| {
                let inner_log = Arc::clone(&inner_log_for_builder);
                LayoutBuilder {
                    builder: Arc::new(move |_ctx, constraints| {
                        inner_log.lock().push(constraints);
                        FixedBox(10.0, 10.0).into_view().boxed()
                    }),
                }
                .into_view()
                .boxed()
            }),
        };

        let incoming = tight(90.0, 70.0);
        let mut h = Harness::mount(&outer, incoming);
        h.frame();

        assert_eq!(
            inner_log.lock().as_slice(),
            &[incoming],
            "the nested builder must also see real constraints, in the same frame"
        );
        assert_eq!(
            h.owner.layout_builder_count(),
            2,
            "both builders registered"
        );
    }
}
