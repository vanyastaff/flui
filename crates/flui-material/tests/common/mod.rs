//! Minimal headless mount harness shared by `flui-material`'s integration
//! tests — a trimmed copy of `flui-widgets`' `tests/common/mod.rs` scoped to
//! what `Theme` tests need: mount, root-swap, drive a frame. No geometry
//! inspection, no pointer dispatch, no animation ticking — `Theme` is a pure
//! inherited-data widget, not a render object.

#![allow(dead_code)] // each test binary uses a different subset of the harness

use std::sync::Arc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::ElementId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree, View};
use parking_lot::RwLock;

/// A mounted widget tree, holding the element + render trees alive inside a
/// tree-bound [`HeadlessBinding`], re-driven via [`LaidOut::pump_widget`].
pub struct LaidOut {
    binding: HeadlessBinding,
    root_element_id: ElementId,
}

/// Loose constraints from `0` up to `max × max` on both axes.
pub fn loose(max: f32) -> BoxConstraints {
    BoxConstraints::loose(Size::new(px(max), px(max)))
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
        root_element_id: root_id,
    }
}

impl LaidOut {
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
}
