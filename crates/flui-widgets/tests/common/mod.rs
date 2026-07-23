//! Headless view-level layout harness shared by the `flui-widgets` integration
//! tests — the Core.1 parity-oracle infrastructure.
//!
//! It mounts a root [`View`] (a widget tree) directly as the render-tree root,
//! runs a build pass (reconciling + mounting the whole subtree's render
//! objects), then drives a real headless frame and exposes the resulting
//! render-node geometry. No GPU, no window, no `WidgetsBinding` singleton —
//! so the tests are order-independent and can run in parallel.

#![allow(dead_code)] // each test binary uses a different subset of the harness

use std::any::TypeId;
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_animation::{AnimationController, Vsync};
use flui_binding::HeadlessBinding;
use flui_foundation::{ElementId, RenderId};
use flui_geometry::Matrix4;
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerButtons, PointerEvent, PointerType, make_cancel_event_for_id, make_down_event_for_id,
    make_down_event_for_id_with_button, make_move_event_for_id, make_up_event_for_id,
    make_up_event_for_id_with_button,
};
use flui_objects::{
    RenderAnimatedOpacity, RenderClipOval, RenderClipPath, RenderClipRRect, RenderClipRect,
    RenderFittedBox, RenderImage, RenderOpacity, RenderParagraph, RenderTransform,
};
use flui_rendering::constraints::{BoxConstraints, SliverGeometry};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::storage::IntrinsicDimension;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_types::{Offset, Pixels, Rect, Size};
use flui_view::{BuildOwner, ElementTree, View};
use flui_widgets::{FocusRoot, GestureArenaScope};
use parking_lot::RwLock;

/// A laid-out widget tree, holding the element + render trees alive (inside a
/// tree-bound [`HeadlessBinding`]) so geometry can be queried after layout — and
/// re-driven via [`LaidOut::pump`] / [`LaidOut::tick`] / [`LaidOut::pump_for`].
///
/// `pipeline_owner` is the harness's own clone of the same shared
/// `Arc<RwLock<PipelineOwner>>` the binding drives, so geometry reads observe the
/// frame the binding just ran.
pub struct LaidOut {
    binding: HeadlessBinding,
    focus_manager: Rc<flui_interaction::FocusManager>,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    root_render_id: RenderId,
    root_element_id: ElementId,
    /// Concrete identity of the caller's root below the presentation scopes.
    logical_root_type: TypeId,
    /// Next contact's pointer id. Every Down gets a fresh identity so
    /// sequential contacts cannot collide in the binding-owned arena.
    next_pointer: Cell<u64>,
    /// Pointer id of the in-flight contact, shared by its Move/Up/Cancel.
    current_pointer: Cell<u64>,
}

/// Loose constraints from `0` up to `max × max` on both axes.
pub fn loose(max: f32) -> BoxConstraints {
    BoxConstraints::loose(Size::new(px(max), px(max)))
}

/// Tight constraints forcing exactly `width × height`.
pub fn tight(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(width), px(height)))
}

/// Build `root`, mount it as the render-tree root, and lay it out under
/// `constraints`. Panics on any pipeline error so a regression is loud.
pub fn lay_out(root: impl View, constraints: BoxConstraints) -> LaidOut {
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    lay_out_with_pipeline_owner_and_binding(
        root,
        constraints,
        pipeline_owner,
        HeadlessBinding::new(),
    )
}

/// [`lay_out`] with a caller-provided pipeline owner, for probes that must retain
/// the same owner before their lifecycle callback is mounted.
pub fn lay_out_with_pipeline_owner(
    root: impl View,
    constraints: BoxConstraints,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
) -> LaidOut {
    lay_out_with_pipeline_owner_and_binding(
        root,
        constraints,
        pipeline_owner,
        HeadlessBinding::new(),
    )
}

fn lay_out_with_pipeline_owner_and_binding(
    root: impl View,
    constraints: BoxConstraints,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    mut binding: HeadlessBinding,
) -> LaidOut {
    let logical_root_type = root.view_type_id();
    let mut build_owner = BuildOwner::new();
    let focus_manager = build_owner.focus_manager();
    let mut tree = ElementTree::new();

    // The binding is created FIRST so its async driver can be installed on the
    // `BuildOwner` before the mount `build_scope` below. `FutureBuilder` /
    // `StreamBuilder` subscribe in `init_state`, which runs inside that pass — with
    // no driver installed they would silently never poll.
    binding.install_build_capabilities(&mut build_owner);

    let root = GestureArenaScope::new(binding.arena().clone(), FocusRoot::new(root));
    let root_id = binding.enter_owner_scope(|| {
        let root_id = tree.mount_root_with_pipeline_owner(
            &root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );

        // Reconcile + mount the whole subtree (children's render objects attach to
        // their parent render objects during this pass).
        build_owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
        build_owner.build_scope(&mut tree);
        root_id
    });

    // `FocusRoot` installs one transparent traversal anchor as the
    // presentation render root. The pipeline must retain that parentless
    // anchor, while geometry probes keep their historical meaning: the
    // caller's logical render root immediately below it.
    let (presentation_render_root_id, root_render_id) = {
        let owner = pipeline_owner.read();
        let render_tree = owner.render_tree();
        let mut roots = render_tree
            .iter()
            .map(|(id, _)| id)
            .filter(|id| render_tree.parent(*id).is_none());
        let root = roots
            .next()
            .expect("the mounted subtree should have a render root");
        assert!(
            roots.next().is_none(),
            "expected exactly one render-tree root after mount",
        );
        let children = render_tree.children(root);
        assert_eq!(
            children.len(),
            1,
            "the presentation traversal anchor must wrap exactly one logical render root",
        );
        (root, children[0])
    };

    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(presentation_render_root_id));
        // Setting fresh root constraints marks the root dirty for layout.
        guard.set_root_constraints(Some(constraints));
    }

    {
        // Mirror the production frame path exactly: `HeadlessBinding::pump_frame`
        // and `AppBinding::draw_frame` both run the ADR-0017 layout<->build
        // fixpoint, not a bare `PipelineOwner::run_frame`. Bootstrapping with
        // `run_frame` here would leave a `LayoutBuilder`'s child unbuilt on the
        // very frame these tests assert about.
        binding.enter_owner_scope(|| {
            build_owner
                .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
                .expect("headless frame should succeed");
        });
    }

    // Bootstrap done (mounted, rooted, first frame run): hand the three owners to
    // the tree-bound binding, keeping our own clone of the shared pipeline-owner Arc
    // for geometry reads. `pump`/`tick`/`pump_for` route through the binding.
    binding.bind_tree(build_owner, tree, Arc::clone(&pipeline_owner));

    LaidOut {
        binding,
        focus_manager,
        pipeline_owner,
        root_render_id,
        root_element_id: root_id,
        logical_root_type,
        next_pointer: Cell::new(1),
        current_pointer: Cell::new(0),
    }
}

/// Like [`lay_out`], but drives implicitly-animated widgets: the binding adopts
/// `vsync` so it ticks every controller a descendant `VsyncScope` (built from
/// the same `vsync`) registered during the mount build pass.
///
/// The caller threads `vsync` into the root widget (so its build wraps the
/// animated subtree in `VsyncScope::new(vsync.clone(), …)`) AND passes the same
/// handle here, so the scope a descendant reads and the registry the binding
/// drives are one and the same.
pub fn lay_out_animated(root: impl View, constraints: BoxConstraints, vsync: Vsync) -> LaidOut {
    let mut laid = lay_out(root, constraints);
    laid.binding.adopt_vsync(vsync);
    laid
}

impl LaidOut {
    /// Focus manager that owns this mounted widget tree.
    pub fn focus_manager(&self) -> Rc<flui_interaction::FocusManager> {
        Rc::clone(&self.focus_manager)
    }

    /// Run an owner-side action under the headless binding's local runtime scope.
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.binding.enter_owner_scope(callback)
    }
    /// The render id of the root widget's render object.
    pub fn root(&self) -> RenderId {
        self.root_render_id
    }

    /// Recompute the caller's logical render root below the presentation's
    /// transparent traversal anchor. May differ from [`LaidOut::root`] if a
    /// rebuild remounted the caller's root subtree.
    pub fn current_root(&self) -> RenderId {
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        let presentation_root = render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("a presentation render-tree root after layout");
        let children = render_tree.children(presentation_root);
        assert_eq!(
            children.len(),
            1,
            "the presentation traversal anchor must wrap exactly one logical render root",
        );
        children[0]
    }

    /// Number of nodes in the caller's logical render subtree.
    ///
    /// Presentation-owned traversal anchors are deliberately excluded: this
    /// helper measures the widget tree supplied to [`lay_out`], not the
    /// infrastructure that owns and presents it.
    pub fn render_node_count(&self) -> usize {
        let root = self.current_root();
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        let mut pending = vec![root];
        let mut count = 0;

        while let Some(id) = pending.pop() {
            count += 1;
            pending.extend(render_tree.children(id).iter().copied());
        }

        count
    }

    /// Number of elements currently in the element tree (mounted + soft-removed
    /// pending `finalize_tree`). Grows with each `ensure` call, shrinks after
    /// each `finalize_tree` or eager `remove`. Useful for asserting that
    /// `SparseChildren::evict` + `finalize_tree` fully drains lazy children from
    /// both trees, not just the render tree.
    pub fn element_node_count(&mut self) -> usize {
        self.binding.tree_mut().len()
    }

    /// The `i`-th render-tree child of `id`.
    pub fn child(&self, id: RenderId, index: usize) -> RenderId {
        self.pipeline_owner.read().render_tree().children(id)[index]
    }

    /// The first render-tree child of `id`.
    pub fn only_child(&self, id: RenderId) -> RenderId {
        self.child(id, 0)
    }

    /// The laid-out size of a render node.
    pub fn size(&self, id: RenderId) -> Size {
        inspect::box_geometry(&self.pipeline_owner.read(), id)
            .expect("render node should have box geometry after layout")
    }

    /// The committed sliver geometry of a render node.
    pub fn sliver_geometry(&self, id: RenderId) -> SliverGeometry {
        inspect::sliver_geometry(&self.pipeline_owner.read(), id)
            .expect("render node should have sliver geometry after layout")
    }

    /// The paint offset of a render node relative to its parent.
    pub fn offset(&self, id: RenderId) -> Offset {
        inspect::render_offset(&self.pipeline_owner.read(), id)
            .expect("render node should have an offset after layout")
    }

    /// The screen-space (root-local) offset of `id`, by summing paint offsets
    /// up the render-tree ancestry.
    ///
    /// Use this instead of [`offset`](Self::offset) when the node whose own
    /// parent-relative offset carries a change (e.g. scroll translation) is
    /// not `id` itself but some ancestor of it (a sliver adapter, say) — a
    /// bare `offset(id)` would silently read 0 in that case. Only valid when
    /// every node between the root and `id` translates (no scale/rotation),
    /// which holds for the box-protocol trees these tests build.
    pub fn absolute_offset(&self, id: RenderId) -> Offset {
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut current = id;
        loop {
            if let Some(node_offset) = inspect::render_offset(&owner, current) {
                x += node_offset.dx.get();
                y += node_offset.dy.get();
            }
            match render_tree.parent(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        offset(x, y)
    }

    /// Drive one more frame after external state has changed — the headless
    /// equivalent of what `setState` schedules: mark the root dirty, then pump a
    /// zero-time frame (rebuild the subtree + re-run layout/paint). Used by the
    /// `setState` (contract C1) test, where the root's `ViewState` reads a value
    /// mutated between frames.
    ///
    /// `Duration::ZERO` is faithful: today's `pump` advances no clock, it only
    /// drives a frame — so step 1 is a no-op, step 2 finds no crossed deadline,
    /// step 3 ticks the (here empty) registry, then `build_scope` + `run_frame`.
    pub fn pump(&mut self) {
        let logical_root_type = self.logical_root_type;
        let (logical_root, logical_depth) = self
            .binding
            .tree_mut()
            .iter_nodes()
            .filter(|(_, node)| node.element().view_type_id() == logical_root_type)
            .min_by_key(|(_, node)| node.depth())
            .map(|(id, node)| (id, node.depth()))
            .expect("the caller's logical root must remain mounted below presentation scopes");

        if let Some(node) = self.binding.tree_mut().get_mut(logical_root) {
            node.element_mut().mark_needs_build();
        }
        self.binding.build_owner_mut().schedule_build_for(
            logical_root,
            logical_depth,
            flui_view::RebuildReason::StateChange,
        );
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Drive a frame WITHOUT marking the root dirty — the headless equivalent of
    /// a vsync/animation tick. `build_scope` (inside `pump_frame`) drains whatever
    /// the external inbox holds (an `AnimatedView` scheduled by a listenable
    /// change between frames), rebuilds those elements, and re-runs layout/paint.
    /// This is what distinguishes an animation-driven rebuild from a
    /// `setState`/`pump` one.
    pub fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Register `controller` with the binding so each [`pump`](Self::pump) /
    /// [`tick`](Self::tick) / [`pump_for`](Self::pump_for) advances it on the
    /// virtual timeline (restart-aware). Register before starting the controller.
    pub fn register_controller(&mut self, controller: AnimationController) {
        self.binding.register_controller(controller);
    }

    /// Adopt `vsync` into the presentation binding. Descendants must receive
    /// the same handle through their `VsyncScope`.
    pub fn adopt_vsync(&mut self, vsync: Vsync) {
        self.binding.adopt_vsync(vsync);
    }

    /// Advance `dt` of virtual time and drive a frame — the animation-frame
    /// analogue: ticks registered controllers (whose listenable notifications
    /// schedule the dependent `AnimatedView`/`FadeTransition` rebuild into the
    /// build inbox), drains it, and re-runs layout/paint. No root dirtying.
    pub fn pump_for(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// The binding's **own** scheduler — never `Scheduler::instance()`.
    ///
    /// `pump_for` drives this one; a post-frame callback parked anywhere else is
    /// never drained.
    pub fn binding_scheduler(&self) -> flui_scheduler::Scheduler {
        self.binding.scheduler().clone()
    }

    /// The shared pipeline owner, so a post-frame callback can read committed
    /// geometry from inside the frame.
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        Arc::clone(&self.pipeline_owner)
    }

    /// The committed opacity of a [`RenderOpacity`] node (e.g. the one a
    /// `FadeTransition` builds) or a [`RenderAnimatedOpacity`] node (the one
    /// `AnimatedOpacity` builds). The latter reads the composed animation's
    /// raw `f32` value (`RenderAnimatedOpacity::opacity_value`), not the
    /// quantized `u8` alpha cache — the `1/255` rounding would blow the
    /// implicit-animation tests' `< 1e-4` tolerance. Panics if `id` is
    /// neither.
    pub fn opacity(&self, id: RenderId) -> f32 {
        let mut owner = self.pipeline_owner.write();
        let node = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("render node should exist");
        if let Some(render) = node.downcast_render_object_mut::<RenderOpacity>() {
            return render.opacity();
        }
        if let Some(render) = node.downcast_render_object_mut::<RenderAnimatedOpacity>() {
            return render.opacity_value();
        }
        panic!("render node should be a RenderOpacity or RenderAnimatedOpacity");
    }

    /// The [`RenderOpacity`] node's `paint_alpha()` — `None` when the node
    /// paints via a fast-path passthrough (opacity `1.0`, no `OpacityLayer`
    /// needed) or when it is fully transparent without
    /// `always_needs_compositing` (opacity `0.0`, subtree skipped, also no
    /// layer needed); `Some(alpha)` otherwise. This is the exact quantity the
    /// pipeline reads through `&dyn RenderObject<BoxProtocol>` to decide
    /// whether to allocate a compositing layer. Panics if `id` is not a
    /// `RenderOpacity`.
    pub fn opacity_paint_alpha(&self, id: RenderId) -> Option<u8> {
        use flui_rendering::traits::RenderBox;

        let mut owner = self.pipeline_owner.write();
        let node = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("render node should exist");
        let render = node
            .downcast_render_object_mut::<RenderOpacity>()
            .expect("render node should be a RenderOpacity");
        render.paint_alpha()
    }

    /// Whether the [`RenderOpacity`] node at `id` suppresses painting its
    /// child entirely — Flutter's `RenderOpacity.paint`: `if (_alpha == 0)
    /// return;`. Panics if `id` is not a `RenderOpacity`.
    pub fn opacity_skip_paint(&self, id: RenderId) -> bool {
        use flui_rendering::traits::RenderBox;

        let mut owner = self.pipeline_owner.write();
        let node = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("render node should exist");
        let render = node
            .downcast_render_object_mut::<RenderOpacity>()
            .expect("render node should be a RenderOpacity");
        render.skip_paint()
    }

    /// The [`Clip`] behavior of a clip-family render node (`RenderClipRect`,
    /// `RenderClipRRect`, `RenderClipOval`, `RenderClipPath`) or a
    /// [`RenderFittedBox`] (which stores `clip_behavior` today even though
    /// active clip-painting is still pending — see its module doc). Panics
    /// if `id` is none of the five.
    pub fn clip_behavior(&self, id: RenderId) -> Clip {
        let mut owner = self.pipeline_owner.write();
        let node = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("render node should exist");
        if let Some(render) = node.downcast_render_object_mut::<RenderClipRect>() {
            return render.clip_behavior();
        }
        if let Some(render) = node.downcast_render_object_mut::<RenderClipRRect>() {
            return render.clip_behavior();
        }
        if let Some(render) = node.downcast_render_object_mut::<RenderClipOval>() {
            return render.clip_behavior();
        }
        if let Some(render) = node.downcast_render_object_mut::<RenderClipPath>() {
            return render.clip_behavior();
        }
        if let Some(render) = node.downcast_render_object_mut::<RenderFittedBox>() {
            return render.clip_behavior();
        }
        panic!(
            "render node should be a clip-family render object (Rect/RRect/Oval/Path) or a RenderFittedBox"
        );
    }

    /// The installed [`BorderRadius`] of a `RenderClipRRect` node. Panics if
    /// `id` is not a `RenderClipRRect`, or it carries no border radius.
    pub fn clip_rrect_border_radius(&self, id: RenderId) -> BorderRadius {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderClipRRect>())
            .expect("render node should be a RenderClipRRect")
            .border_radius()
            .expect("RenderClipRRect should carry a border radius")
    }

    /// The x-scale (matrix `[0][0]`) of a [`RenderTransform`] node — the factor a
    /// `ScaleTransition` writes. Panics if `id` is not a `RenderTransform`.
    pub fn transform_scale(&self, id: RenderId) -> f32 {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderTransform>())
            .map(|render| render.transform().get(0, 0))
            .expect("render node should be a RenderTransform")
    }

    /// The Z-rotation (radians) of a [`RenderTransform`] node — what a
    /// `RotationTransition` writes — recovered from the matrix as
    /// `atan2(m[1][0], m[0][0])`. Panics if `id` is not a `RenderTransform`.
    pub fn transform_rotation(&self, id: RenderId) -> f32 {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderTransform>())
            .map(|render| {
                let matrix = render.transform();
                matrix.get(1, 0).atan2(matrix.get(0, 0))
            })
            .expect("render node should be a RenderTransform")
    }

    /// One intrinsic dimension of a box-protocol render node at `extent`,
    /// queried through the live pipeline — Flutter's
    /// `RenderBox.getMinIntrinsicWidth`/`getMaxIntrinsicWidth`/
    /// `getMinIntrinsicHeight`/`getMaxIntrinsicHeight` family, all four of
    /// which route through the same `computeMinIntrinsicWidth`-style
    /// dispatch on the Dart side.
    ///
    /// # Panics
    ///
    /// Panics if `id` is stale, foreign, or a sliver node (box intrinsics are
    /// undefined there) — see [`PipelineOwner::box_intrinsic_dimension`].
    pub fn intrinsic_dimension(
        &self,
        id: RenderId,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        self.pipeline_owner
            .write()
            .box_intrinsic_dimension(id, dimension, extent)
            .expect("box_intrinsic_dimension should succeed for a live box-protocol node")
    }

    /// The composed translate-then-scale transform of a [`RenderFittedBox`]
    /// node — the same matrix `paint_transform` hands the pipeline and
    /// `hit_test` inverts. Panics if `id` is not a `RenderFittedBox`.
    pub fn fitted_box_transform(&self, id: RenderId) -> Matrix4 {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderFittedBox>())
            .map(|render| render.effective_transform())
            .expect("render node should be a RenderFittedBox")
    }

    /// Whether a [`RenderImage`] node currently holds a decoded image (as
    /// opposed to the empty placeholder it paints nothing for). Panics if
    /// `id` is not a `RenderImage`.
    pub fn image_has_image(&self, id: RenderId) -> bool {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderImage>())
            .map(|render| render.image().is_some())
            .expect("render node should be a RenderImage")
    }

    /// The forced logical width of a [`RenderImage`] node (`Image::width`),
    /// or `None` when unset — used to prove a builder's config actually
    /// reaches the render object it currently owns after a rebuild/reorder
    /// (not just at initial creation). Panics if `id` is not a `RenderImage`.
    pub fn image_width(&self, id: RenderId) -> Option<Pixels> {
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderImage>())
            .map(|render| render.width())
            .expect("render node should be a RenderImage")
    }

    /// The destination rectangle [`RenderImage::paint_rect_in`] computes for
    /// its CURRENT committed box size — where the image content actually
    /// paints once `fit`/`alignment` are applied, not merely the box's own
    /// size. `None` when the node carries no image (nothing to paint) or a
    /// degenerate (zero) intrinsic size. Panics if `id` is not a
    /// `RenderImage`.
    pub fn image_paint_rect(&self, id: RenderId) -> Option<Rect> {
        let box_size = self.size(id);
        let mut owner = self.pipeline_owner.write();
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.downcast_render_object_mut::<RenderImage>())
            .map(|render| render.paint_rect_in(box_size))
            .expect("render node should be a RenderImage")
    }

    /// The paint-space left edge of a `RenderParagraph` node's first laid-out
    /// line — where `TextAlign` actually shifts the glyph run, as opposed to
    /// the node's own box (which stays whatever size layout constrained it
    /// to, regardless of alignment).
    ///
    /// Backed by [`TextPainter::get_boxes_for_selection`], which folds in
    /// the alignment-driven paint offset (unlike `get_line_metrics`, whose
    /// `left` is the pre-alignment layout-local value). Panics if `id` is
    /// not a `RenderParagraph`, carries no text, or has no laid-out line —
    /// all of which indicate the paragraph was queried before layout ran.
    pub fn paragraph_first_line_left(&self, id: RenderId) -> f32 {
        let mut owner = self.pipeline_owner.write();
        let node = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("render node should exist");
        let paragraph = node
            .downcast_render_object_mut::<RenderParagraph>()
            .expect("render node should be a RenderParagraph");
        let text_len = paragraph
            .painter()
            .text()
            .expect("a laid-out RenderParagraph carries a text span")
            .to_plain_text()
            .len();
        paragraph
            .painter()
            .get_boxes_for_selection(0, text_len)
            .first()
            .map(|text_box| text_box.rect.left().get())
            .expect("a laid-out non-empty paragraph has at least one selection box")
    }

    /// Replace the root widget with `new_root` and drive a frame — Flutter's
    /// `tester.pumpWidget(w2)` called a second time (root-swap).
    ///
    /// Delegates to [`HeadlessBinding::swap_root_view`] which updates the stored
    /// view config on the root element via a split borrow, schedules a rebuild,
    /// then [`pump_frame(ZERO)`](HeadlessBinding::pump_frame) settles the tree.
    pub fn pump_widget(&mut self, new_root: impl View) {
        self.logical_root_type = new_root.view_type_id();
        let root = GestureArenaScope::new(self.binding.arena().clone(), FocusRoot::new(new_root));
        self.binding.swap_root_view(self.root_element_id, &root);
        self.binding.pump_frame(std::time::Duration::ZERO);
    }

    /// All render nodes whose short type name equals `render_type_name`.
    ///
    /// Walks the caller's logical render subtree and matches each node's
    /// [`DiagnosticsNode`](flui_foundation::DiagnosticsNode) name against
    /// `render_type_name` (the short, crate-unqualified type name such as
    /// `"RenderConstrainedBox"` or `"RenderCenter"`). Returns all matching ids
    /// in slab-iteration order (not geometry order). Presentation-owned nodes
    /// above the caller's root are deliberately excluded.
    ///
    /// Compares **base** names — the part before any `<...>` — on both
    /// sides: `Diagnosticable::to_diagnostics_node`'s short name keeps full
    /// generic fidelity (a `RenderViewport<ScrollPosition>` node names
    /// itself exactly that), but a caller querying "by render type" wants
    /// the base name regardless of which generic argument a render object
    /// happens to be monomorphized over.
    pub fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        let logical_root = self.current_root();
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        let mut logical_ids = HashSet::new();
        let mut pending = vec![logical_root];
        while let Some(id) = pending.pop() {
            logical_ids.insert(id);
            pending.extend(render_tree.children(id).iter().copied());
        }

        let queried = base_type_name(render_type_name);
        render_tree
            .iter()
            .filter_map(|(id, _node)| {
                if !logical_ids.contains(&id) {
                    return None;
                }
                let diagnostics = owner.debug_node_diagnostics(id)?;
                (diagnostics.name().map(base_type_name) == Some(queried)).then_some(id)
            })
            .collect()
    }

    /// The unique render node whose short type name equals `render_type_name`.
    ///
    /// # Panics
    ///
    /// Panics when no node matches (likely a wrong type name or the widget is
    /// not yet mounted) or when more than one node matches (use
    /// [`find_all_by_render_type`](Self::find_all_by_render_type) when
    /// duplicates are expected).
    pub fn find_by_render_type(&self, render_type_name: &str) -> RenderId {
        let matches = self.find_all_by_render_type(render_type_name);
        match matches.as_slice() {
            [id] => *id,
            [] => {
                panic!("find_by_render_type: no render node named {render_type_name:?} in the tree")
            }
            _ => panic!(
                "find_by_render_type: {} render nodes named {render_type_name:?}; \
                 use find_all_by_render_type when duplicates are expected",
                matches.len()
            ),
        }
    }

    /// Find the unique `RenderParagraph` node that contains `text` as its
    /// plain-text content.
    ///
    /// Requires the `RenderParagraph` diagnostics to emit a `"text"` property
    /// (added to [`flui_objects::RenderParagraph::debug_fill_properties`] for
    /// this purpose). Returns `None` when no paragraph matches.
    ///
    /// # Panics
    ///
    /// Panics when more than one `RenderParagraph` emits the same `text`.
    pub fn find_text(&self, text: &str) -> Option<RenderId> {
        let owner = self.pipeline_owner.read();
        let mut found: Option<RenderId> = None;
        for (id, _node) in owner.render_tree().iter() {
            let Some(diagnostics) = owner.debug_node_diagnostics(id) else {
                continue;
            };
            if diagnostics.name() != Some("RenderParagraph") {
                continue;
            }
            if diagnostics.get_property("text") == Some(text) {
                assert!(
                    found.is_none(),
                    "find_text: multiple RenderParagraph nodes contain {text:?}"
                );
                found = Some(id);
            }
        }
        found
    }

    /// Hit-test at a root-local position and return the canonical data-only
    /// path to the binding-owned input pipeline.
    ///
    /// Hit-testing runs inside the binding's interaction-lane scope:
    /// production (`crates/flui-app/src/app/runner.rs`,
    /// `realm.enter(|realm| event.run(realm))`) hit-tests and dispatches from
    /// inside the same lane entry, and hit-testing itself can now resolve
    /// lane-registered owner-local state (`ClipPath`'s custom path clipper via
    /// `resolve_path_clip_target`) — scoping only the dispatch half left
    /// `hit_test` silently falling back to the default (whole-box) clip
    /// whenever a caller hit-tested outside an active lane.
    pub fn hit_test_pointer(&self, position: Offset) -> flui_rendering::hit_testing::HitTestResult {
        use flui_rendering::hit_testing::HitTestResult;

        let mut result = HitTestResult::new();
        let owner = self.pipeline_owner.read();
        owner.hit_test(position, &mut result);
        result
    }

    /// Dispatch an already-constructed pointer event through the same complete
    /// binding pipeline as platform input.
    pub fn dispatch_pointer_event(&self, event: &PointerEvent) {
        self.binding
            .dispatch_pointer(event, |position| self.hit_test_pointer(position));
    }

    /// Ensure consecutive velocity samples receive distinct timestamps.
    fn advance_gesture_clock() {
        let t0 = Instant::now();
        while Instant::now() == t0 {
            std::hint::spin_loop();
        }
    }

    /// Allocate a fresh pointer id for a new contact and remember it.
    fn begin_contact(&self) -> PointerId {
        let id = self.next_pointer.get();
        self.next_pointer.set(
            id.checked_add(1)
                .expect("BUG: headless pointer id space exhausted"),
        );
        self.current_pointer.set(id);
        PointerId::new(id).expect("BUG: headless pointer ids start at one")
    }

    /// Resolve the pointer id of the in-flight contact.
    fn current_contact(&self) -> PointerId {
        PointerId::new(self.current_pointer.get())
            .expect("BUG: pointer Down must precede Move, Up, or Cancel")
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down
    /// event there — the headless analogue of a platform pointer-down reaching
    /// the framework (`AppBinding::handle_input` → hit_test → dispatch). Used by
    /// the `Listener` test to assert its callback fires.
    ///
    /// See [`hit_test_pointer`](Self::hit_test_pointer) for why hit-testing runs inside
    /// the lane scope alongside dispatch.
    pub fn dispatch_pointer_down(&self, x: f32, y: f32) {
        Self::advance_gesture_clock();
        let event = make_down_event_for_id(self.begin_contact(), offset(x, y), PointerType::Mouse);
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    /// As [`dispatch_pointer_down`](Self::dispatch_pointer_down), but a
    /// pointer-up — to assert `on_pointer_up` routing.
    pub fn dispatch_pointer_up(&self, x: f32, y: f32) {
        let event = make_up_event_for_id(self.current_contact(), offset(x, y), PointerType::Mouse);
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    /// A contact move to `(x, y)` — to drive slop / drag handling.
    pub fn dispatch_pointer_move(&self, x: f32, y: f32) {
        Self::advance_gesture_clock();
        let event =
            make_move_event_for_id(self.current_contact(), offset(x, y), PointerType::Mouse);
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    /// A mouse hover move to `(x, y)` with no active contact.
    pub fn dispatch_pointer_hover(&self, x: f32, y: f32) {
        Self::advance_gesture_clock();
        let mut event =
            make_move_event_for_id(PointerId::PRIMARY, offset(x, y), PointerType::Mouse);
        let PointerEvent::Move(update) = &mut event else {
            unreachable!("the test move constructor must produce PointerEvent::Move");
        };
        update.current.buttons = PointerButtons::new();
        update.current.pressure = 0.0;
        self.dispatch_pointer_event(&event);
    }

    /// Cancel the in-flight contact on its cached Down route.
    pub fn dispatch_pointer_cancel(&self) {
        let event = make_cancel_event_for_id(self.current_contact(), PointerType::Mouse);
        self.dispatch_pointer_event(&event);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic secondary-button
    /// (right-click) pointer-down event — the headless analogue of a right-mouse
    /// button press reaching the framework. Used by `GestureDetector` tests to
    /// assert `on_secondary_tap` fires on right-click.
    pub fn dispatch_secondary_down(&self, x: f32, y: f32) {
        use flui_interaction::events::pointer::PointerButton;

        Self::advance_gesture_clock();
        let event = make_down_event_for_id_with_button(
            self.begin_contact(),
            offset(x, y),
            PointerType::Mouse,
            PointerButton::Secondary,
        );
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }

    /// As [`dispatch_secondary_down`](Self::dispatch_secondary_down), but a
    /// secondary-button pointer-up — to complete the right-click gesture.
    pub fn dispatch_secondary_up(&self, x: f32, y: f32) {
        use flui_interaction::events::pointer::PointerButton;

        let event = make_up_event_for_id_with_button(
            self.current_contact(),
            offset(x, y),
            PointerType::Mouse,
            PointerButton::Secondary,
        );
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test_pointer(position));
    }
}

/// Convenience: a `Size` in logical pixels.
pub fn size(width: f32, height: f32) -> Size {
    Size::new(px(width), px(height))
}

/// Convenience: an `Offset` in logical pixels.
pub fn offset(dx: f32, dy: f32) -> Offset {
    Offset::new(px(dx), px(dy))
}

/// The part of `type_name` before its first `<`, if any — the base name
/// ignoring generic parameters ("RenderViewport<ScrollPosition>" ->
/// "RenderViewport"; "RenderConstrainedBox" -> "RenderConstrainedBox"
/// unchanged). Mirrors `flui_foundation::debug`'s private helper of the
/// same name (not public — this harness has its own tiny copy rather than
/// growing the library's public surface for a test-only concern).
fn base_type_name(type_name: &str) -> &str {
    type_name.split('<').next().unwrap_or(type_name)
}
