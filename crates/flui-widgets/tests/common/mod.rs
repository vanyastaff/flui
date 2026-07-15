//! Headless view-level layout harness shared by the `flui-widgets` integration
//! tests — the Core.1 parity-oracle infrastructure.
//!
//! It mounts a root [`View`] (a widget tree) directly as the render-tree root,
//! runs a build pass (reconciling + mounting the whole subtree's render
//! objects), then drives a real headless frame and exposes the resulting
//! render-node geometry. No GPU, no window, no `WidgetsBinding` singleton —
//! so the tests are order-independent and can run in parallel.

#![allow(dead_code)] // each test binary uses a different subset of the harness

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_animation::{AnimationController, Vsync};
use flui_binding::HeadlessBinding;
use flui_foundation::{ElementId, RenderId};
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerEvent, PointerType, make_cancel_event_for_id, make_down_event_for_id,
    make_move_event_for_id, make_up_event_for_id,
};
use flui_objects::{RenderAnimatedOpacity, RenderOpacity, RenderParagraph, RenderTransform};
use flui_rendering::constraints::{BoxConstraints, SliverGeometry};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree, View};
use flui_widgets::GestureArenaScope;
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
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    root_render_id: RenderId,
    root_element_id: ElementId,
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
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // The binding is created FIRST so its async driver can be installed on the
    // `BuildOwner` before the mount `build_scope` below. `FutureBuilder` /
    // `StreamBuilder` subscribe in `init_state`, which runs inside that pass — with
    // no driver installed they would silently never poll.
    binding.install_build_capabilities(&mut build_owner);

    let root_id = binding.enter_owner_scope(|| {
        let root_id = tree.mount_root_with_pipeline_owner(
            &root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );

        // Reconcile + mount the whole subtree (children's render objects attach to
        // their parent render objects during this pass).
        build_owner.schedule_build_for(root_id, 0);
        build_owner.build_scope(&mut tree);
        root_id
    });

    // The render-tree root is the single render object with no render parent —
    // works whether the root widget is itself a `RenderView` (e.g. `Padding`)
    // or a `StatelessView` whose composition's outermost layer owns it (e.g.
    // `Container`).
    let root_render_id = {
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
        root
    };

    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(root_render_id));
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
        pipeline_owner,
        root_render_id,
        root_element_id: root_id,
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
    /// Run an owner-side action under the headless binding's local runtime scope.
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.binding.enter_owner_scope(callback)
    }
    /// The render id of the root widget's render object.
    pub fn root(&self) -> RenderId {
        self.root_render_id
    }

    /// Recompute the current render-tree root (the parentless render node). May
    /// differ from [`LaidOut::root`] if a rebuild remounted the root subtree.
    pub fn current_root(&self) -> RenderId {
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("a render-tree root after layout")
    }

    /// Number of nodes currently in the render tree.
    pub fn render_node_count(&self) -> usize {
        self.pipeline_owner.read().render_tree().iter().count()
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
        if let Some(node) = self.binding.tree_mut().get_mut(self.root_element_id) {
            node.element_mut().mark_needs_build();
        }
        self.binding
            .build_owner_mut()
            .schedule_build_for(self.root_element_id, 0);
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
        self.binding.swap_root_view(self.root_element_id, &new_root);
        self.binding.pump_frame(std::time::Duration::ZERO);
    }

    /// All render nodes whose short type name equals `render_type_name`.
    ///
    /// Walks the live render tree and matches each node's
    /// [`DiagnosticsNode`](flui_foundation::DiagnosticsNode) name against
    /// `render_type_name` (the short, crate-unqualified type name such as
    /// `"RenderConstrainedBox"` or `"RenderCenter"`). Returns all matching ids
    /// in slab-iteration order (not geometry order).
    ///
    /// Compares **base** names — the part before any `<...>` — on both
    /// sides: `Diagnosticable::to_diagnostics_node`'s short name keeps full
    /// generic fidelity (a `RenderViewport<ScrollPosition>` node names
    /// itself exactly that), but a caller querying "by render type" wants
    /// the base name regardless of which generic argument a render object
    /// happens to be monomorphized over.
    pub fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        let owner = self.pipeline_owner.read();
        let queried = base_type_name(render_type_name);
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, _node)| {
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

    /// Hit-test at root-local `(x, y)` and dispatch `event` to the entries hit
    /// there — the route step a binding runs before the arena lifecycle. The
    /// event already carries the pointer id; `(x, y)` is the hit-test position.
    pub fn route_event(&self, event: &PointerEvent, x: f32, y: f32) {
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        // Dispatch resolves owner-local targets, so it must run inside this
        // binding's interaction lane scope (same lane the tree mounted under).
        self.binding.enter_owner_scope(|| result.dispatch(event));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down
    /// event there — the headless analogue of a platform pointer-down reaching
    /// the framework (`AppBinding::handle_input` → hit_test → dispatch). Used by
    /// the `Listener` test to assert its callback fires.
    pub fn dispatch_pointer_down(&self, x: f32, y: f32) {
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_down_event(
            position,
            flui_interaction::events::PointerType::Mouse,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// As [`dispatch_pointer_down`](Self::dispatch_pointer_down), but a
    /// pointer-up — to assert `on_pointer_up` routing.
    pub fn dispatch_pointer_up(&self, x: f32, y: f32) {
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_up_event(
            position,
            flui_interaction::events::PointerType::Mouse,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// A pointer-move to `(x, y)` — to drive slop / drag handling.
    pub fn dispatch_pointer_move(&self, x: f32, y: f32) {
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_move_event(
            position,
            flui_interaction::events::PointerType::Mouse,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// A pointer-cancel routed to the entries hit at `(x, y)` — the headless
    /// analogue of the platform interrupting the contact that started there.
    pub fn dispatch_pointer_cancel(&self, x: f32, y: f32) {
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_cancel_event(
            flui_interaction::events::PointerType::Mouse,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic secondary-button
    /// (right-click) pointer-down event — the headless analogue of a right-mouse
    /// button press reaching the framework. Used by `GestureDetector` tests to
    /// assert `on_secondary_tap` fires on right-click.
    pub fn dispatch_secondary_down(&self, x: f32, y: f32) {
        use flui_interaction::events::pointer::PointerButton;
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_down_event_with_button(
            position,
            flui_interaction::events::PointerType::Mouse,
            PointerButton::Secondary,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// As [`dispatch_secondary_down`](Self::dispatch_secondary_down), but a
    /// secondary-button pointer-up — to complete the right-click gesture.
    pub fn dispatch_secondary_up(&self, x: f32, y: f32) {
        use flui_interaction::events::pointer::PointerButton;
        use flui_rendering::hit_testing::HitTestResult;

        let position = Offset::new(px(x), px(y));
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        let event = flui_interaction::events::make_up_event_with_button(
            position,
            flui_interaction::events::PointerType::Mouse,
            PointerButton::Secondary,
        );
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }
}

/// A laid-out widget tree wrapped in a [`GestureArenaScope`] over a
/// [`HeadlessBinding`]'s shared, clock-bound arena — plus the binding that
/// drives deadlines.
///
/// This is the headless analogue of a real `GestureBinding` above the tree: the
/// detectors below read the binding's arena ambiently, the binding closes that
/// arena after the down has been dispatched to the whole hit-test path
/// (`binding.dart` ordering), and `pump` advances the virtual clock + polls
/// gesture deadlines with no `thread::sleep`.
pub struct LaidOutScoped {
    laid: LaidOut,
    /// Next contact's pointer id (1-based; `0` is not a valid `PointerId`). Each
    /// `dispatch_pointer_down` allocates a fresh id so two sequential taps use
    /// distinct ids — what a real `GestureBinding` does per contact, and what
    /// keeps a genuine double-tap sound (the second down opens its own arena
    /// entry instead of re-adding clones into the held first entry).
    next_pointer: Cell<u64>,
    /// The current contact's pointer id (set on each down) so the matching
    /// up / move / cancel route to the same id.
    current_pointer: Cell<u64>,
}

/// Build `root` wrapped in a [`GestureArenaScope`] over a fresh
/// [`HeadlessBinding`]'s arena, then lay it out under `constraints`.
///
/// The detectors in `root` read the binding's arena in `init_state`, so they
/// compete in (and have their deadlines polled against) the same arena the
/// returned binding drives.
pub fn lay_out_with_arena(root: impl View, constraints: BoxConstraints) -> LaidOutScoped {
    let binding = HeadlessBinding::new();
    let scoped = GestureArenaScope::new(binding.arena().clone(), root);
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    let laid =
        lay_out_with_pipeline_owner_and_binding(scoped, constraints, pipeline_owner, binding);
    LaidOutScoped {
        laid,
        next_pointer: Cell::new(1),
        current_pointer: Cell::new(0),
    }
}

impl LaidOutScoped {
    /// The underlying laid-out tree, for geometry queries.
    pub fn laid(&self) -> &LaidOut {
        &self.laid
    }

    /// Advance the virtual clock by `dt` and fire any gesture deadline that has
    /// now elapsed — the deterministic, sleep-free frame tick.
    pub fn pump(&mut self, dt: Duration) {
        self.laid.binding.pump_frame(dt);
    }

    /// Spin until `Instant::now()` returns a value strictly greater than the
    /// one returned by the immediately preceding call — i.e. wait until the OS
    /// high-resolution timer has ticked at least once.
    ///
    /// Purpose: `DragGestureRecognizer::handle_move` calls `Instant::now()`
    /// internally to timestamp each velocity-tracker sample. When several
    /// `dispatch_pointer_*` calls happen within a single OS timer tick (common
    /// on systems with ~100 ns QPC resolution), all samples carry the same
    /// timestamp. The resulting least-squares system is singular and produces
    /// NaN velocity — which then propagates into scroll-physics simulations.
    ///
    /// Calling this before each dispatch that should count toward velocity
    /// (down and every move) guarantees that consecutive
    /// `velocity_tracker.add_position(Instant::now(), …)` calls in the
    /// recognizer receive strictly increasing timestamps.
    ///
    /// The busy-wait exits after at most one OS timer period (~100 ns on
    /// Windows with QPC), so the overhead per dispatch is negligible.
    fn advance_gesture_clock() {
        let t0 = Instant::now();
        while Instant::now() == t0 {
            std::hint::spin_loop();
        }
    }

    /// Allocate a fresh pointer id for a new contact, remembering it as current.
    fn begin_contact(&self) -> PointerId {
        let id = self.next_pointer.get();
        self.next_pointer.set(id + 1);
        self.current_pointer.set(id);
        PointerId::new(id).expect("contact ids start at 1")
    }

    /// The pointer id of the in-flight contact (set by the matching down).
    fn current_contact(&self) -> PointerId {
        PointerId::new(self.current_pointer.get()).expect("a down precedes up/move/cancel")
    }

    /// Dispatch a synthetic pointer-down at `(x, y)` through the binding: route
    /// to the hit-test path, THEN close the shared arena — `binding.dart`'s
    /// order (close after the down has reached the whole hit-test path, so every
    /// overlapping detector has added its recognizers before the single close).
    /// Each down allocates a fresh contact id.
    pub fn dispatch_pointer_down(&self, x: f32, y: f32) {
        // Ensure the OS timer has ticked at least once so the first
        // velocity-tracker sample (added inside `handle_down`) gets a timestamp
        // strictly greater than any previously recorded one.
        Self::advance_gesture_clock();
        let pointer = self.begin_contact();
        let event = make_down_event_for_id(pointer, offset(x, y), PointerType::Mouse);
        self.laid
            .binding
            .dispatch_pointer(&event, |e| self.laid.route_event(e, x, y));
    }

    /// Dispatch a synthetic pointer-up at `(x, y)` for the current contact:
    /// route, THEN sweep the shared arena (the contact ended).
    pub fn dispatch_pointer_up(&self, x: f32, y: f32) {
        let pointer = self.current_contact();
        let event = make_up_event_for_id(pointer, offset(x, y), PointerType::Mouse);
        self.laid
            .binding
            .dispatch_pointer(&event, |e| self.laid.route_event(e, x, y));
    }

    /// Dispatch a synthetic pointer-move at `(x, y)` for the current contact (no
    /// arena close/sweep — a move neither opens nor ends a contact).
    pub fn dispatch_pointer_move(&self, x: f32, y: f32) {
        // Advance the gesture clock before each move so consecutive velocity
        // tracker samples have strictly increasing `Instant::now()` timestamps
        // (see `advance_gesture_clock` for rationale).
        Self::advance_gesture_clock();
        let pointer = self.current_contact();
        let event = make_move_event_for_id(pointer, offset(x, y), PointerType::Mouse);
        self.laid
            .binding
            .dispatch_pointer(&event, |e| self.laid.route_event(e, x, y));
    }

    /// Dispatch a synthetic pointer-cancel at `(x, y)` for the current contact:
    /// route, THEN sweep the shared arena (the contact was interrupted).
    pub fn dispatch_pointer_cancel(&self, x: f32, y: f32) {
        let pointer = self.current_contact();
        let event = make_cancel_event_for_id(pointer, PointerType::Mouse);
        self.laid
            .binding
            .dispatch_pointer(&event, |e| self.laid.route_event(e, x, y));
    }

    /// Register `vsync` with the tree binding so [`pump_for`](Self::pump_for)
    /// ticks controllers that state objects registered against it during
    /// `init_state`. Call this with the same `Vsync` handle that was given to
    /// the `VsyncScope` wrapping the scrollable widget.
    pub fn adopt_vsync(&mut self, vsync: Vsync) {
        self.laid.binding.adopt_vsync(vsync);
    }

    /// Advance `dt` of virtual animation time through the TREE binding: ticks
    /// vsync-registered controllers (e.g. a fling controller registered by a
    /// `ScrollableState`), drains the scheduled rebuild inbox, and re-runs
    /// layout + paint. Does **not** advance the gesture-arena clock.
    ///
    /// Use this to drive ballistic scroll animations in tests after gesture
    /// events have already started a fling via `dispatch_pointer_up`.
    pub fn pump_for(&mut self, dt: Duration) {
        self.laid.binding.pump_frame(dt);
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
