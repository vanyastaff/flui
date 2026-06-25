//! Headless view-level layout harness shared by the `flui-widgets` integration
//! tests — the Core.1 parity-oracle infrastructure.
//!
//! It mounts a root [`View`] (a widget tree) directly as the render-tree root,
//! runs a build pass (reconciling + mounting the whole subtree's render
//! objects), then drives a real headless frame and exposes the resulting
//! render-node geometry. No GPU, no window, no `WidgetsBinding` singleton —
//! so the tests are order-independent and can run in parallel.

#![allow(dead_code)] // each test binary uses a different subset of the harness

use std::sync::Arc;

use flui_foundation::{ElementId, RenderId};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree, View};
use parking_lot::RwLock;

/// A laid-out widget tree, holding the element + render trees alive so geometry
/// can be queried after layout — and re-driven via [`LaidOut::pump`].
pub struct LaidOut {
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    root_render_id: RenderId,
    root_element_id: ElementId,
    build_owner: BuildOwner,
    tree: ElementTree,
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

    let root_id = tree.mount_root_with_pipeline_owner(
        &root,
        Some(Arc::clone(&pipeline_owner)),
        &mut build_owner.element_owner_mut(),
    );

    // Reconcile + mount the whole subtree (children's render objects attach to
    // their parent render objects during this pass).
    build_owner.schedule_build_for(root_id, 0);
    build_owner.build_scope(&mut tree);

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
        // Mirror the production frame path: swap the owner out (leaving a
        // Default placeholder under the still-shared Arc), run all phases by
        // value, then restore.
        let owner = std::mem::take(&mut *guard);
        let (owner, result) = owner.run_frame();
        result.expect("headless frame should succeed");
        *guard = owner;
    }

    LaidOut {
        pipeline_owner,
        root_render_id,
        root_element_id: root_id,
        build_owner,
        tree,
    }
}

impl LaidOut {
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

    /// The paint offset of a render node relative to its parent.
    pub fn offset(&self, id: RenderId) -> Offset {
        inspect::render_offset(&self.pipeline_owner.read(), id)
            .expect("render node should have an offset after layout")
    }

    /// Drive one more frame after external state has changed — the headless
    /// equivalent of what `setState` schedules: mark the root dirty, rebuild
    /// the subtree, and re-run layout/paint. Used by the `setState` (contract
    /// C1) test, where the root's `ViewState` reads a value mutated between
    /// frames.
    pub fn pump(&mut self) {
        if let Some(node) = self.tree.get_mut(self.root_element_id) {
            node.element_mut().mark_needs_build();
        }
        self.build_owner.schedule_build_for(self.root_element_id, 0);
        self.build_owner.build_scope(&mut self.tree);

        let mut guard = self.pipeline_owner.write();
        let owner = std::mem::take(&mut *guard);
        let (owner, result) = owner.run_frame();
        result.expect("rebuild frame should succeed");
        *guard = owner;
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
