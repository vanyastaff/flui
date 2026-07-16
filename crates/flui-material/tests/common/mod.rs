//! Headless mount harness shared by `flui-material`'s integration tests — a
//! copy of `flui-widgets`' `tests/common/mod.rs`, extended (beyond the
//! original `Theme`-only trim) with pointer dispatch and vsync-driven
//! ticking so `InkWell`'s hover/tap/press-timing can be driven at the
//! widget level. See `tests/ink_well.rs`'s module doc for the spike that
//! established this is possible.

#![allow(dead_code)] // each test binary uses a different subset of the harness

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{AnimationController, Vsync};
use flui_binding::HeadlessBinding;
use flui_foundation::{ElementId, RenderId};
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree, View};
use parking_lot::RwLock;

/// A mounted widget tree, holding the element + render trees alive inside a
/// tree-bound [`HeadlessBinding`], re-driven via [`LaidOut::pump`]/[`LaidOut::pump_for`].
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
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let mut binding = HeadlessBinding::new();

    // The binding is created FIRST so its async driver can be installed on
    // the `BuildOwner` before the mount `build_scope` below.
    binding.install_build_capabilities(&mut build_owner);

    let root_id = binding.enter_owner_scope(|| {
        let root_id = tree.mount_root_with_pipeline_owner(
            &root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );
        build_owner.schedule_build_for(root_id, 0);
        build_owner.build_scope(&mut tree);
        root_id
    });

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
        guard.set_root_constraints(Some(constraints));
    }

    binding.enter_owner_scope(|| {
        build_owner
            .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
            .expect("headless frame should succeed");
    });

    binding.bind_tree(build_owner, tree, Arc::clone(&pipeline_owner));

    LaidOut {
        binding,
        pipeline_owner,
        root_render_id,
        root_element_id: root_id,
    }
}

/// [`lay_out`], but the binding adopts `vsync` so it ticks every
/// [`AnimationController`] a descendant `VsyncScope` (built from the same
/// `vsync`) registered during the mount build pass — needed to drive
/// `InkWell`'s press-deactivation timer under [`LaidOut::pump_for`].
pub fn lay_out_animated(root: impl View, constraints: BoxConstraints, vsync: Vsync) -> LaidOut {
    let mut laid = lay_out(root, constraints);
    laid.binding.adopt_vsync(vsync);
    laid
}

impl LaidOut {
    /// The render id of the root widget's render object.
    pub fn root(&self) -> RenderId {
        self.root_render_id
    }

    /// Replace the root widget with `new_root` and drive a frame — Flutter's
    /// `tester.pumpWidget(w2)` called a second time (root-swap).
    pub fn pump_widget(&mut self, new_root: impl View) {
        self.binding.swap_root_view(self.root_element_id, &new_root);
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Drive one more frame after external state has changed (marks the root
    /// dirty first) — the headless equivalent of what `setState` schedules.
    pub fn pump(&mut self) {
        if let Some(node) = self.binding.tree_mut().get_mut(self.root_element_id) {
            node.element_mut().mark_needs_build();
        }
        self.binding
            .build_owner_mut()
            .schedule_build_for(self.root_element_id, 0);
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Drive a frame WITHOUT marking the root dirty — the headless
    /// equivalent of a vsync/animation tick (drains whatever a listenable
    /// change scheduled into the build inbox between frames).
    pub fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Advance `dt` of virtual time and drive a frame — ticks every
    /// controller registered against the binding's adopted vsync (see
    /// [`lay_out_animated`]), then rebuilds whatever that scheduled.
    pub fn pump_for(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// Register `controller` with the binding so [`pump`](Self::pump) /
    /// [`tick`](Self::tick) / [`pump_for`](Self::pump_for) advances it.
    pub fn register_controller(&mut self, controller: AnimationController) {
        self.binding.register_controller(controller);
    }

    /// Hit-test at root-local `(x, y)` and dispatch `event` to the entries
    /// hit there — the route step a binding runs before the arena lifecycle.
    ///
    /// The hit-test itself, not just the dispatch, runs inside
    /// `enter_owner_scope`: a render object's `hit_test` can resolve an
    /// owner-lane target synchronously (e.g. `RenderPhysicalShape` resolving
    /// its registered `PathClipTarget` via
    /// `flui_interaction::routing::resolve_path_clip_target`, which reads
    /// the *currently active* lane off a thread-local — there is no stored
    /// handle to fall back to). Hit-testing outside the scope silently
    /// degrades every such resolution to its no-active-lane fallback (here,
    /// `RenderPhysicalShape`'s whole-box default clip) instead of erroring
    /// loudly, which made an earlier version of this harness pass shape
    /// hit-tests that should have failed — the owner scope must wrap the
    /// whole hit-test + dispatch sequence, matching how a real frame runs
    /// (`AppBinding::handle_input` executes entirely inside the lane).
    fn route_event(&self, event: &flui_interaction::PointerEvent, x: f32, y: f32) {
        let position = Offset::new(px(x), px(y));
        self.binding.enter_owner_scope(|| {
            let owner = self.pipeline_owner.read();
            let mut result = HitTestResult::new();
            owner.hit_test(position, &mut result);
            drop(owner);
            result.dispatch(event);
        });
    }

    /// Dispatch a synthetic pointer-down at `(x, y)` — the headless analogue
    /// of a platform pointer-down reaching the framework.
    pub fn dispatch_pointer_down(&self, x: f32, y: f32) {
        let position = Offset::new(px(x), px(y));
        let event = make_down_event(position, PointerType::Mouse);
        self.route_event(&event, x, y);
    }

    /// As [`dispatch_pointer_down`](Self::dispatch_pointer_down), but a
    /// pointer-up.
    pub fn dispatch_pointer_up(&self, x: f32, y: f32) {
        let position = Offset::new(px(x), px(y));
        let event = make_up_event(position, PointerType::Mouse);
        self.route_event(&event, x, y);
    }

    /// A pointer-move to `(x, y)` — drives hover enter/exit.
    pub fn dispatch_pointer_move(&self, x: f32, y: f32) {
        let position = Offset::new(px(x), px(y));
        let event = make_move_event(position, PointerType::Mouse);
        self.route_event(&event, x, y);
    }

    /// Every render node whose short type name (generic parameters stripped)
    /// equals `render_type_name`.
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
    /// Panics when more than one node matches (use
    /// [`find_all_by_render_type`](Self::find_all_by_render_type) when
    /// duplicates are expected). Returns `None`, not a panic, when nothing
    /// matches — callers that assert "not (yet) mounted" need that case to
    /// be a value, not a panic.
    pub fn find_by_render_type(&self, render_type_name: &str) -> Option<RenderId> {
        let matches = self.find_all_by_render_type(render_type_name);
        match matches.as_slice() {
            [id] => Some(*id),
            [] => None,
            _ => panic!(
                "find_by_render_type: {} render nodes named {render_type_name:?}; \
                 use find_all_by_render_type when duplicates are expected",
                matches.len()
            ),
        }
    }
}

/// Strips generic parameters (`Foo<Bar>` → `Foo`) so a diagnostics name
/// comparison doesn't need to spell out every monomorphization.
fn base_type_name(type_name: &str) -> &str {
    type_name.split('<').next().unwrap_or(type_name)
}
