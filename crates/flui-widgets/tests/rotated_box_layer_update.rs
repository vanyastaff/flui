//! A `RotatedBox` widget rebuild reaches the composited layer.
//!
//! Mirrors `transform_layer_update.rs` for the `RenderRotatedBox` slice of
//! issue #996: `RotatedBox::update_render_object` calls
//! `RenderRotatedBox::set_quarter_turns` and hands the reported
//! `RenderUpdateImpact` to the owner
//! (`crates/flui-widgets/src/layout/rotated_box.rs`) — a forwarding call no
//! rebuild test exercised before this change.
//!
//! What it pins is exactly that WIRING, and nothing more: dropping
//! `RotatedBox::update_render_object`'s `set_quarter_turns` forwarding turns
//! it red. It cannot see which arm the render object's own impact algebra
//! took — a same-parity update-only commit and a parity-change repaint both
//! leave the TransformLayer carrying the new matrix, so this test is
//! satisfied either way. Proving the update-only arm actually ran (and
//! spares the subtree) belongs to the render-level test that can count
//! paints:
//! `a_rotated_box_quarter_turn_update_patches_the_layer_and_writes_back` in
//! `crates/flui-rendering/tests/retained_boundary_layers.rs`.

use flui_widgets::testing::{lay_out, tight};
use flui_widgets::{RotatedBox, SizedBox};

#[test]
fn rebuilding_a_rotated_box_widget_updates_its_layer() {
    let mut harness = lay_out(
        RotatedBox::new(1).child(SizedBox::new(60.0, 40.0)),
        tight(200.0, 200.0),
    );
    let turn1 = harness.transform_layer_matrices().first().copied();
    assert!(
        turn1.is_some(),
        "precondition: the first frame composites a TransformLayer for a \
         non-trivial rotation",
    );

    // 1 -> 3: same parity, so the render object's own algebra reports an
    // update-only composited-layer commit, not a repaint.
    harness.pump_widget(RotatedBox::new(3).child(SizedBox::new(60.0, 40.0)));
    let turn3 = harness.transform_layer_matrices().first().copied();

    let fresh = lay_out(
        RotatedBox::new(3).child(SizedBox::new(60.0, 40.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(
        turn3,
        fresh.transform_layer_matrices().first().copied(),
        "a rebuild with a new (same-parity) quarter-turn count must reach \
         the composited layer through the widget's own update path — the \
         matrix must equal what mounting RotatedBox::new(3) fresh produces",
    );
    assert_ne!(
        turn3, turn1,
        "and it must differ from the turn-1 matrix — otherwise the update \
         path could be a no-op that happened to pass",
    );
}

/// Rebuilding with the SAME quarter-turn count is a no-op that composites
/// nothing new.
///
/// The control for the test above: without it, an implementation that
/// repaints unconditionally on every rebuild would satisfy the matrix
/// assertion just as well, and the update path would be doing nothing for
/// the widget layer.
#[test]
fn rebuilding_a_rotated_box_widget_with_an_unchanged_value_paints_nothing() {
    let mut harness = lay_out(
        RotatedBox::new(1).child(SizedBox::new(60.0, 40.0)),
        tight(200.0, 200.0),
    );
    let painted = harness.painted_frame_count();

    harness.pump_widget(RotatedBox::new(1).child(SizedBox::new(60.0, 40.0)));

    assert_eq!(
        harness.painted_frame_count(),
        painted,
        "an unchanged quarter-turn count reports RenderUpdateImpact::NONE, so \
         the frame must not repaint at all",
    );
}
