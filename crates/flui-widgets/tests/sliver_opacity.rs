//! Tests for `SliverOpacity` — a sliver-geometry pass-through (opacity is a
//! paint-time-only effect; `RenderSliverOpacity::perform_layout` forwards the
//! child's geometry unchanged), mirroring `tests/sliver_padding`-style tests
//! in `tests/scroll.rs` for the `Viewport`-mounting convention slivers need.

mod common;

use common::{lay_out, tight};
use flui_widgets::{SizedBox, SliverOpacity, SliverToBoxAdapter, Viewport};

#[test]
fn sliver_opacity_passes_the_childs_sliver_geometry_through_unchanged() {
    let laid = lay_out(
        Viewport::new((SliverOpacity::new(0.5)
            .child(SliverToBoxAdapter::new().child(SizedBox::new(200.0, 120.0))),)),
        tight(200.0, 300.0),
    );

    let viewport = laid.root();
    let sliver_opacity = laid.only_child(viewport);
    let adapter = laid.only_child(sliver_opacity);

    assert_eq!(
        laid.sliver_geometry(sliver_opacity),
        laid.sliver_geometry(adapter),
        "opacity must not affect the sliver's own layout geometry -- it is a \
         transparent pass-through, exactly like RenderOpacity's Box-protocol \
         sibling",
    );
}

#[test]
fn sliver_opacity_with_no_child_reports_zero_geometry() {
    let laid = lay_out(
        Viewport::new((SliverOpacity::new(1.0),)),
        tight(200.0, 300.0),
    );

    let viewport = laid.root();
    let sliver_opacity = laid.only_child(viewport);

    assert_eq!(
        laid.sliver_geometry(sliver_opacity),
        flui_rendering::constraints::SliverGeometry::ZERO,
    );
}
