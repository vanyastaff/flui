//! The contract of [`HeadlessBinding::mount_root`]: the bootstrap frame is the
//! same frame `pump_frame` runs.
//!
//! This is the property eight hand-rolled copies of the bootstrap kept losing.
//! The `flui` golden-screenshot suite drove its capture with a bare
//! `PipelineOwner::run_frame`, which never services build-during-layout
//! content, so every `SliverAppBar` delegate child it photographed was
//! unbuilt — and nothing failed, because no test asserted the bootstrap ran
//! the fixpoint at all.
//!
//! `layout_builder_seam.rs` pins the same seam for `pump_frame`; this pins it
//! for the bootstrap. Both plant a registry entry by hand rather than mounting
//! a real `LayoutBuilder`, so they stay pure wiring tests of the frame path.

use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::testing::inspect;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::{RenderView, View};

/// A leaf of a fixed size, so the bootstrap frame has real geometry to commit.
#[derive(Clone)]
struct SizedLeaf {
    size: Size,
}

impl RenderView for SizedLeaf {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::new(Some(self.size.width), Some(self.size.height))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for SizedLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn leaf(width: f64, height: f64) -> SizedLeaf {
    SizedLeaf {
        size: Size::new(width, height),
    }
}

pub(crate) fn mount_root_installs_the_render_root_and_lays_it_out() {
    let mut binding = HeadlessBinding::new();
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());
    let mounted = binding.mount_root(
        &leaf(40.0, 25.0),
        MountOwners::with_pipeline_owner(pipeline_owner.clone()),
        MountOptions::new(flui_rendering::constraints::BoxConstraints::loose(
            Size::new(200.0, 200.0),
        )),
    );

    assert_eq!(
        pipeline_owner.with(flui_rendering::PipelineOwner::root_id),
        Some(mounted.render_root),
        "RootRenderElement installs the RenderView as the pipeline root",
    );
    // RootRenderView seeds from constraints.biggest(); RenderView then lays
    // its child under tight constraints of that size (production shape).
    assert_eq!(
        pipeline_owner.with(|owner| inspect::box_geometry(owner, mounted.render_root)),
        Some(Size::new(200.0, 200.0)),
        "the bootstrap frame lays the RenderView out at the seeded root size",
    );
    assert_eq!(
        pipeline_owner.with(|owner| inspect::box_geometry(owner, mounted.logical_render_root())),
        Some(Size::new(200.0, 200.0)),
        "the caller's leaf under the RenderView receives the view's tight size",
    );
    assert!(
        mounted.painted,
        "a tree with real geometry paints on its bootstrap frame",
    );
}

pub(crate) fn the_bound_binding_keeps_pumping_from_where_the_bootstrap_left_off() {
    // The bootstrap ends bound, so the next frame is an ordinary pump: no
    // second mount, no re-rooting, and the committed layer tree survives a
    // frame that has no paint work.
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &leaf(10.0, 10.0),
        MountOwners::fresh(),
        MountOptions::tight(50.0, 50.0),
    );
    let after_bootstrap = binding.painted_frame_count();

    binding.pump_frame(Duration::from_millis(16));

    assert!(
        binding.layer_tree().is_some(),
        "the committed layer tree is retained across a frame with no paint work",
    );
    assert!(
        binding.painted_frame_count() >= after_bootstrap,
        "a settled frame never un-counts an earlier paint",
    );
}
