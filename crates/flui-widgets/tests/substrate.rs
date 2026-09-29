//! A tree mounted on the substrate driver, `flui_testing::HeadlessBinding`,
//! for the configurations a realm cannot express.
//!
//! A realm always installs the post-frame capability on its build owner and
//! always wraps its root in a `VsyncScope` and a `MediaQuery`. A test of how
//! a widget behaves without them — `LifecycleContext::post_frame_handle()`
//! returning `None` (an embedder that drives frames itself), or no ambient
//! registry or media query above it — mounts here instead, under only a
//! gesture arena and a focus root.

use std::time::Duration;

use flui_painting::Alignment;
use flui_rendering::pipeline::PipelineCell;
use flui_testing::bootstrap::{BuildCapabilities, MountOptions, MountOwners};
use flui_testing::{A11yTree, HeadlessBinding};
use flui_view::View;
use flui_widgets::{Align, FocusRoot, GestureArenaScope};

/// A tree mounted on the substrate driver.
pub struct SubstrateTree {
    binding: HeadlessBinding,
    pipeline_owner: PipelineCell,
}

impl std::fmt::Debug for SubstrateTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubstrateTree").finish_non_exhaustive()
    }
}

/// Mount `root` on an 800 × 600 surface, top-left aligned under the binding's
/// gesture arena and a focus root, with the build `capabilities` given.
pub fn mount(root: impl View + Clone + 'static, capabilities: BuildCapabilities) -> SubstrateTree {
    let mut binding = HeadlessBinding::new();
    let owners = MountOwners::fresh();
    let pipeline_owner = owners.pipeline_owner.clone();
    let scoped = GestureArenaScope::new(binding.arena().clone(), FocusRoot::new(root));
    let aligned = Align::new(Alignment::TOP_LEFT).child(scoped);
    let _mounted = binding.mount_root(
        &aligned,
        owners,
        MountOptions::tight(800.0, 600.0).with_capabilities(capabilities),
    );
    SubstrateTree {
        binding,
        pipeline_owner,
    }
}

impl SubstrateTree {
    /// Drive one frame at the same instant.
    pub fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Run an owner-side action inside the binding's owner scope.
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.binding.enter_owner_scope(callback)
    }

    /// Turn semantics on for the next frame.
    pub fn enable_semantics(&mut self) {
        self.binding
            .enable_semantics()
            .expect("a mounted binding is tree-bound");
    }

    /// The accessibility tree the last frame assembled.
    pub fn a11y_tree(&self) -> Option<A11yTree> {
        self.binding.a11y_tree()
    }

    /// The shared pipeline owner.
    pub fn pipeline_owner(&self) -> PipelineCell {
        self.pipeline_owner.clone()
    }
}
