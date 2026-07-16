//! `Material` widget-level coverage — proves the shape actually reaches
//! paint/hit-test through the real owner-lane path-clip registration, not
//! just that `MaterialShape::to_path` computes the right geometry in
//! isolation (that unit-level geometry is covered by `shape.rs`'s own
//! tests and `material.rs`'s `configured_shape_field_is_shape_sensitive_at_the_paint_size`).
//!
//! `RenderPhysicalModelBase::hit_test` always tests the resolved clip shape
//! before testing the child (`crates/flui-objects/src/proxy/physical_model.rs`
//! — a deliberate FLUI-wide divergence from the oracle, which gates this on
//! `_clipper != null`). For the `PathClip` variant `Material` uses,
//! `compute_clip` calls `flui_interaction::routing::resolve_path_clip_target`
//! against the **actually registered** `PathClipTarget` — the same
//! `sync_path_clip_target` wiring `create_render_object`/`update_render_object`
//! install during a real mount. So a corner-point hit-test through a fully
//! mounted `Material` exercises the live registration end to end: if
//! `sync_path_clip_target` ever stopped forwarding the configured `shape`
//! (e.g. closed over a stale/default value instead), these tests would flip.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{Material, MaterialShape};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector};

/// A point near the top-left corner of a 120x40 rect: outside the Stadium's
/// inscribed corner circle (radius = shortest_side/2 = 20, centered at
/// (20, 20); distance from (2, 2) is ≈25.5 > 20) but inside the plain
/// bounding rectangle a sharp-cornered shape would fill.
const CORNER_PROBE: (f32, f32) = (2.0, 2.0);

fn tap_counter() -> (Arc<AtomicUsize>, impl Fn() + 'static) {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    (taps, move || {
        counted.fetch_add(1, Ordering::SeqCst);
    })
}

#[test]
fn stadium_shape_excludes_a_corner_a_sharp_rectangle_would_include() {
    let (taps, on_tap) = tap_counter();
    let laid = lay_out(
        Material::new(Color::WHITE)
            .shape(MaterialShape::Stadium)
            .child(
                GestureDetector::new()
                    .on_tap(on_tap)
                    .child(ColoredBox::new(Color::rgb(200, 10, 10))),
            ),
        tight(120.0, 40.0),
    );

    laid.dispatch_pointer_down(CORNER_PROBE.0, CORNER_PROBE.1);
    laid.dispatch_pointer_up(CORNER_PROBE.0, CORNER_PROBE.1);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "Material's registered Stadium clip must exclude this corner point from hit-testing, \
         so the wrapped GestureDetector never sees the tap"
    );
}

#[test]
fn rectangle_shape_includes_the_same_corner_point() {
    let (taps, on_tap) = tap_counter();
    let laid = lay_out(
        Material::new(Color::WHITE)
            .shape(MaterialShape::rectangle())
            .child(
                GestureDetector::new()
                    .on_tap(on_tap)
                    .child(ColoredBox::new(Color::rgb(200, 10, 10))),
            ),
        tight(120.0, 40.0),
    );

    laid.dispatch_pointer_down(CORNER_PROBE.0, CORNER_PROBE.1);
    laid.dispatch_pointer_up(CORNER_PROBE.0, CORNER_PROBE.1);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "Material's registered plain-rectangle clip must include this corner point, \
         so the wrapped GestureDetector sees the tap — the same point the Stadium test \
         above proves is excluded, isolating the clip shape as the only variable"
    );
}
