//! A `ClipRRect` widget rebuild reaches the composited layer.
//!
//! Mirrors `transform_layer_update.rs` for the `RenderClipRRect` slice of
//! issue #536: `ClipRRect`'s `update_render_object` calls
//! `RenderClipRRect::set_border_radius` (plus `set_clip_behavior`), and hands
//! the unioned `RenderUpdateImpact` to the owner. This is the seam an
//! implicit `AnimatedContainer`/`AnimatedContainer`-style radius animation
//! would drive every frame — a mechanism that only works when a test pokes
//! the render object directly is the defect class this repository keeps
//! finding.
//!
//! What it pins is exactly that WIRING, and nothing more: dropping
//! `ClipRRect::update_render_object`'s `set_border_radius` forwarding turns it
//! red. Narrowing the render object's impact back to unconditional `PAINT`
//! does **not** — the boundary then repaints and rebuilds the layer from the
//! live radius, so the oracle here is satisfied either way. This test was
//! green before the layer-update path existed and is green after it. It does
//! not show the frame took the cheap arm; proving the subtree was spared
//! belongs to the render-level test that can count paints
//! (`a_border_radius_change_updates_the_clip_layer_without_repainting_the_subtree`
//! in `crates/flui-rendering/tests/retained_boundary_layers.rs`).

use flui_types::geometry::{Radius, px};
use flui_widgets::testing::{lay_out, tight};
use flui_widgets::{ClipRRect, SizedBox};

#[test]
fn rebuilding_a_clip_rrect_widget_updates_its_layer() {
    let mut harness = lay_out(
        ClipRRect::circular(8.0).child(SizedBox::new(40.0, 40.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(
        harness
            .clip_rrect_layers()
            .first()
            .map(|rrect| rrect.top_left),
        Some(Radius::circular(px(8.0))),
        "precondition: the first frame composites the initial radius",
    );

    harness.pump_widget(ClipRRect::circular(2.0).child(SizedBox::new(40.0, 40.0)));

    assert_eq!(
        harness
            .clip_rrect_layers()
            .first()
            .map(|rrect| rrect.top_left),
        Some(Radius::circular(px(2.0))),
        "a rebuild with a new radius must reach the composited layer through \
         the widget's own update path, not only through a direct setter call",
    );
}
