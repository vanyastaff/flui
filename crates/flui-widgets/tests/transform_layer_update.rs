//! A `Transform` widget rebuild reaches the composited layer.
//!
//! Mirrors `opacity_layer_update.rs` for the `RenderTransform` slice of issue
//! #536: `Transform`'s `update_render_object` calls `RenderTransform::set_transform`
//! (plus `set_alignment`/`set_origin`/`set_transform_hit_tests`) and hands the
//! reported `RenderUpdateImpact` to the owner. This is the seam
//! `ScaleTransition`/`RotationTransition` actually drive every animation frame
//! (`crates/flui-widgets/src/layout/transform.rs`) — a mechanism that only
//! works when a test pokes the render object directly is the defect class
//! this repository keeps finding.
//!
//! What it pins is exactly that WIRING, and nothing more: dropping
//! `Transform::update_render_object`'s `set_transform` forwarding turns it red.
//! Narrowing the render object's impact back to unconditional `PAINT` does
//! **not** — the boundary then repaints and rebuilds the layer from the live
//! matrix, so the oracle here is satisfied either way. This test was green
//! before the layer-update path existed and is green after it. It does not show
//! the frame took the cheap arm; proving the subtree was spared belongs to the
//! render-level tests that can count paints
//! (`a_transform_change_updates_the_layer_without_repainting_the_subtree` in
//! `crates/flui-rendering/tests/retained_boundary_layers.rs`).

use flui_geometry::Matrix4;
use flui_widgets::testing::{lay_out, tight};
use flui_widgets::{SizedBox, Transform};

#[test]
fn rebuilding_a_transform_widget_updates_its_layer() {
    // Scale, not translation: a pure translation is painted as a plain
    // offset with no TransformLayer at all, which would make the precondition
    // assertion below vacuous.
    let mut harness = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0)).child(SizedBox::new(40.0, 40.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(
        harness.transform_layer_matrices().first().copied(),
        Some(Matrix4::scaling(2.0, 2.0, 1.0)),
        "precondition: the first frame composites the initial matrix",
    );

    harness.pump_widget(
        Transform::new(Matrix4::scaling(3.0, 3.0, 1.0)).child(SizedBox::new(40.0, 40.0)),
    );

    assert_eq!(
        harness.transform_layer_matrices().first().copied(),
        Some(Matrix4::scaling(3.0, 3.0, 1.0)),
        "a rebuild with a new matrix must reach the composited layer through \
         the widget's own update path, not only through a direct setter call",
    );
}

/// Rebuilding with the SAME matrix is a no-op that composites nothing new.
///
/// The control for the test above: without it, an implementation that
/// repaints unconditionally on every rebuild would satisfy the matrix
/// assertion just as well, and the update path would be doing nothing for the
/// widget layer.
#[test]
fn rebuilding_a_transform_widget_with_an_unchanged_value_paints_nothing() {
    let mut harness = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0)).child(SizedBox::new(40.0, 40.0)),
        tight(200.0, 200.0),
    );
    let painted = harness.painted_frame_count();

    harness.pump_widget(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0)).child(SizedBox::new(40.0, 40.0)),
    );

    assert_eq!(
        harness.painted_frame_count(),
        painted,
        "an unchanged matrix (with alignment/origin/transform_hit_tests also \
         unchanged) reports RenderUpdateImpact::NONE from every setter \
         Transform::update_render_object unions, so the frame must not \
         repaint at all",
    );
}
