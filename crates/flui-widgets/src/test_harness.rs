//! A headless widget harness for `flui-widgets`' **in-crate** unit tests.
//!
//! `tests/common::lay_out` is an integration-test module and cannot be reached
//! from `src/`, so private modules — `overlay` (ADR-0019 U1) and `navigator` (U3)
//! — need their own. This is the trimmed equivalent: it keeps `lay_out`'s
//! load-bearing ordering (**binding first, so the async driver is installed before
//! the mount `build_scope`**, ADR-0018 U6) and drops the geometry helpers.

use std::sync::Arc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::ElementId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::View;
use parking_lot::RwLock;

/// A mounted, laid-out widget tree.
pub(crate) struct Harness {
    binding: HeadlessBinding,
    root_element: ElementId,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

/// Mount `root` as the render-tree root and drive one frame.
pub(crate) fn mount(root: impl View) -> Harness {
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    let mut build_owner = flui_view::BuildOwner::new();
    let mut tree = flui_view::ElementTree::new();

    let mut binding = HeadlessBinding::new();
    build_owner.set_async_driver(binding.scheduler().async_driver().clone());

    let root_element = tree.mount_root_with_pipeline_owner(
        &root,
        Some(Arc::clone(&pipeline_owner)),
        &mut build_owner.element_owner_mut(),
    );

    build_owner.schedule_build_for(root_element, 0);
    build_owner.build_scope(&mut tree);

    let root_render = {
        let owner = pipeline_owner.read();
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("the mounted subtree should have a render root")
    };
    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(root_render));
        guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(800.0), px(600.0)))));
    }
    build_owner
        .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
        .expect("headless frame should succeed");

    binding.bind_tree(build_owner, tree, Arc::clone(&pipeline_owner));

    Harness {
        binding,
        root_element,
        pipeline_owner,
    }
}

impl Harness {
    /// The root element id.
    pub(crate) fn root(&self) -> ElementId {
        self.root_element
    }

    /// Drive a frame **without** dirtying the root, so only what an
    /// `OverlayHandle` / `OverlayEntry` scheduled through its `RebuildHandle`
    /// rebuilds. Every rebuild assertion depends on this: a root-dirtying pump
    /// would rebuild the whole tree and prove nothing.
    pub(crate) fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Replace the root view and settle.
    ///
    /// Goes through `ElementTree::update`, whose dispatch is keyed by `TypeId`, so
    /// the root's *type* must not change between frames. Toggling a field on one
    /// root type is how a subtree gets unmounted.
    pub(crate) fn swap_root(&mut self, new_root: impl View) {
        self.binding.swap_root_view(self.root_element, &new_root);
        self.binding.pump_frame(Duration::ZERO);
    }

    /// The ordered children of `parent`, read through the public `ElementNode`
    /// surface (`parent()` + `slot()`); `child_ids()` is crate-private.
    pub(crate) fn children_of(&mut self, parent: ElementId) -> Vec<ElementId> {
        let mut kids: Vec<(usize, ElementId)> = self
            .binding
            .tree_mut()
            .iter_nodes()
            .filter(|(_, node)| node.parent() == Some(parent))
            .map(|(id, node)| (node.slot(), id))
            .collect();
        kids.sort_unstable();
        kids.into_iter().map(|(_, id)| id).collect()
    }

    /// The binding's **own** scheduler — never `Scheduler::instance()`.
    ///
    /// A post-frame callback registered here is drained by `pump_frame`'s
    /// `Scheduler::drive_frame`, after the pipeline commits layout (ADR-0021 §7c).
    pub(crate) fn scheduler(&self) -> &flui_scheduler::Scheduler {
        self.binding.scheduler()
    }

    /// The shared pipeline owner, so a post-frame callback can read committed
    /// geometry from inside the frame.
    pub(crate) fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        Arc::clone(&self.pipeline_owner)
    }

    /// The `debug_name()` of every render object currently in the tree.
    ///
    /// The one structural probe a widget-level test has: it says *which* render
    /// objects a view built, without duplicating the render-layer harness in
    /// `flui-objects`, which is where their behavior is pinned.
    pub(crate) fn render_debug_names(&self) -> Vec<&'static str> {
        let owner = self.pipeline_owner.read();
        owner
            .render_tree()
            .iter()
            .map(|(_, node)| node.debug_name())
            .collect()
    }

    /// The only child of `parent`.
    pub(crate) fn only_child(&mut self, parent: ElementId) -> ElementId {
        let kids = self.children_of(parent);
        assert_eq!(kids.len(), 1, "expected exactly one child of {parent:?}");
        kids[0]
    }
}
