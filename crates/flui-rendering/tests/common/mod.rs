//! Scaffolding shared by the `rendering_it` integration-test modules.
//!
//! Every root test file compiles as a module of the single `rendering_it`
//! binary (see `main.rs`), so plumbing that used to be copied per file —
//! boxed-object aliases, layout drivers, geometry readers — lives here
//! once. Mock render objects (hosts, probes, stub slivers) stay local to
//! the tests that exercise them; only protocol-neutral plumbing belongs
//! in this module.

use flui_foundation::RenderId;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    pipeline::{PipelineOwner, phase::Layout},
    protocol::{BoxProtocol, SliverProtocol},
    testing::{inspect, sliver as sliver_presets},
    traits::RenderObject,
};
use flui_types::{Size, geometry::px};

/// A boxed Box-protocol render object, as stored in the pipeline tree.
pub type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;

/// A boxed Sliver-protocol render object, as stored in the pipeline tree.
pub type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

/// A fresh pipeline owner already advanced into the Layout phase. Tests
/// build the tree via `render_tree_mut` (phase-agnostic accessor) before
/// driving layout.
pub fn fresh_layout_pipeline() -> PipelineOwner<Layout> {
    PipelineOwner::new().into_layout()
}

/// Installs `root` with the given root constraints, runs layout, and
/// returns the owner in the Layout phase for inspection.
pub fn laid_out_with(
    mut owner: PipelineOwner,
    root: RenderId,
    constraints: BoxConstraints,
) -> PipelineOwner<Layout> {
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(constraints));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    owner
}

/// [`laid_out_with`] under a tight 300×100 root — the shared viewport of
/// the sliver-family harness tests.
pub fn laid_out_tight_300x100(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(
        owner,
        root,
        BoxConstraints::tight(Size::new(px(300.0), px(100.0))),
    )
}

/// [`laid_out_with`] under a tight 100×100 root.
pub fn laid_out_tight_100x100(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(
        owner,
        root,
        BoxConstraints::tight(Size::new(px(100.0), px(100.0))),
    )
}

/// [`laid_out_with`] under a loose 0–200 × 0–200 root.
pub fn laid_out_loose_200x200(owner: PipelineOwner, root: RenderId) -> PipelineOwner<Layout> {
    laid_out_with(
        owner,
        root,
        BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)),
    )
}

/// Reads the committed [`SliverGeometry`] for `id`, panicking when layout
/// has not produced one.
pub fn sliver_geometry(owner: &PipelineOwner<Layout>, id: RenderId) -> SliverGeometry {
    inspect::sliver_geometry(owner, id).expect("sliver geometry is committed")
}

/// Vertical sliver constraints for the shared 300-wide × 100-tall test
/// viewport: 100 px of paint room, a 120 px cache window starting 20 px
/// before the leading edge.
pub fn vertical_constraints(scroll_offset: f32) -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(100.0)
        .cross_axis_extent(300.0)
        .viewport_main_axis_extent(100.0)
        .remaining_cache_extent(120.0)
        .cache_origin(-20.0)
        .build()
}

/// Horizontal counterpart of [`vertical_constraints`]: a 300-long main
/// axis, 100 px cross axis, 320 px cache window starting 20 px before the
/// leading edge.
pub fn horizontal_constraints(scroll_offset: f32) -> SliverConstraints {
    sliver_presets::horizontal()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(300.0)
        .cross_axis_extent(100.0)
        .viewport_main_axis_extent(300.0)
        .remaining_cache_extent(320.0)
        .cache_origin(-20.0)
        .build()
}
