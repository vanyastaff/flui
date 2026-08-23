//! The contract of [`HeadlessBinding::mount_root`]: the bootstrap frame is the
//! same frame `pump_frame` runs.
//!
//! This is the property four hand-rolled copies of the bootstrap kept losing.
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

use flui_foundation::{ElementId, RenderId};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::testing::inspect;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{BuildCapabilities, MountOptions, MountOwners};
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree, RenderView, View};

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

fn leaf(width: f32, height: f32) -> SizedLeaf {
    SizedLeaf {
        size: Size::new(px(width), px(height)),
    }
}

#[test]
fn the_bootstrap_frame_runs_the_layout_builder_seam() {
    // A stale registry entry — its element and render node do not exist, so
    // `service_layout_builders` prunes it on the pass it runs. Pruning is the
    // observable side effect available without a real `RenderLayoutBuilder`.
    //
    // The ids are deliberately far above anything this mount will mint: the
    // low ids `layout_builder_seam.rs` can use over an empty tree would be
    // handed straight to the leaf below, and an entry pointing at a node that
    // exists is not stale.
    let mut build_owner = BuildOwner::new();
    let cell =
        build_owner.register_layout_builder_for_test(RenderId::new(9_999), ElementId::new(9_999));
    assert_eq!(build_owner.layout_builder_count(), 1);
    drop(cell);

    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &leaf(10.0, 10.0),
        MountOwners {
            build_owner,
            tree: ElementTree::new(),
            pipeline_owner: PipelineCell::new(PipelineOwner::new()),
        },
        MountOptions::tight(100.0, 100.0),
    );

    assert_eq!(
        binding.build_owner_mut().layout_builder_count(),
        0,
        "the bootstrap frame must go through run_frame_with_layout_builders, \
         which prunes the stale entry — a bare PipelineOwner::run_frame would \
         leave it, and would leave a real LayoutBuilder's child unbuilt on the \
         very frame a screenshot captures",
    );
}

#[test]
fn mount_root_installs_the_render_root_and_lays_it_out() {
    let mut binding = HeadlessBinding::new();
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());
    let mounted = binding.mount_root(
        &leaf(40.0, 25.0),
        MountOwners::with_pipeline_owner(pipeline_owner.clone()),
        MountOptions::new(flui_rendering::constraints::BoxConstraints::loose(
            Size::new(px(200.0), px(200.0)),
        )),
    );

    assert_eq!(
        pipeline_owner.with(flui_rendering::PipelineOwner::root_id),
        Some(mounted.render_root),
        "the discovered parentless node is installed as the pipeline root",
    );
    assert_eq!(
        pipeline_owner.with(|owner| inspect::box_geometry(owner, mounted.render_root)),
        Some(Size::new(px(40.0), px(25.0))),
        "the bootstrap frame lays the root out under the mount constraints",
    );
    assert!(
        mounted.painted,
        "a tree with real geometry paints on its bootstrap frame",
    );
}

#[test]
fn a_single_root_view_mounts_with_no_presentation_anchor() {
    // Without presentation scopes the caller's own view IS the parentless
    // node, so `logical_render_root` reports it rather than inventing a child.
    let mut binding = HeadlessBinding::new();
    let mounted = binding.mount_root(
        &leaf(10.0, 10.0),
        MountOwners::fresh(),
        MountOptions::tight(50.0, 50.0),
    );

    assert!(mounted.render_root_children.is_empty());
    assert_eq!(mounted.logical_render_root(), mounted.render_root);
}

#[test]
fn the_bound_binding_keeps_pumping_from_where_the_bootstrap_left_off() {
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

#[test]
fn withholding_the_post_frame_capability_is_a_reachable_configuration() {
    // An embedder that drives frames itself installs no post-frame handle, and
    // code acquiring one must behave when it is absent. The async driver still
    // goes in — withholding it too would change which capability is under test.
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &leaf(10.0, 10.0),
        MountOwners::fresh(),
        MountOptions::tight(50.0, 50.0).with_capabilities(BuildCapabilities::AsyncDriverOnly),
    );

    // The mount pass itself depends on the async driver, so reaching here at
    // all is the assertion that `AsyncDriverOnly` still installs it.
    binding.pump_frame(Duration::from_millis(16));
}
