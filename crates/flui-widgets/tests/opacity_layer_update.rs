//! An `Opacity` widget rebuild reaches the composited layer.
//!
//! Every other test for issue #536's update-only commits drives the render
//! tree directly. This one goes through the widget layer, which is the seam an
//! application actually uses: `Opacity`'s `update_render_object` calls
//! `RenderOpacity::set_opacity` and hands the reported `RenderUpdateImpact` to
//! the owner. A mechanism that works only when a test pokes the render object
//! is the defect class this repository keeps finding — correct code that no
//! production path reaches.
//!
//! What it pins is exactly that WIRING: dropping the widget's `impact |=`
//! forwarding turns it red. It does **not** show the frame took the cheap arm —
//! the alpha oracle here is satisfied by a full repaint too, and proving the
//! child was spared belongs to the render-level tests that can count paints
//! (`an_alpha_change_updates_the_layer_without_repainting_the_subtree`).

use flui_widgets::testing::{lay_out, tight};
use flui_widgets::{Opacity, SizedBox};

/// The alpha of the only `OpacityLayer` in the pumped frame, as the u8 the
/// pipeline stores.
///
/// Compared as a u8 because an opacity round-trips through `opacity_to_alpha`:
/// asserting on the float that was set would be asserting about the rounding
/// rather than about the update.
fn opacity_alpha_u8(harness: &flui_widgets::testing::LaidOut) -> Option<u8> {
    fn find(tree: &flui_rendering::layer::LayerTree, id: flui_foundation::LayerId) -> Option<u8> {
        let node = tree.get(id)?;
        if let flui_rendering::layer::Layer::Opacity(o) = node.layer() {
            return Some((o.alpha() * 255.0).round() as u8);
        }
        node.children().iter().find_map(|&c| find(tree, c))
    }
    let tree = harness.layer_tree()?;
    find(tree, tree.root()?)
}

#[test]
fn rebuilding_an_opacity_widget_updates_its_layer() {
    let mut harness = lay_out(
        Opacity::new(0.5).child(SizedBox::new(40.0, 40.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(
        opacity_alpha_u8(&harness),
        Some((0.5_f32 * 255.0).round() as u8),
        "precondition: the first frame composites the initial alpha",
    );

    harness.pump_widget(Opacity::new(0.25).child(SizedBox::new(40.0, 40.0)));

    assert_eq!(
        opacity_alpha_u8(&harness),
        Some((0.25_f32 * 255.0).round() as u8),
        "a rebuild with a new opacity must reach the composited layer through \
         the widget's own update path, not only through a direct setter call",
    );
}

/// Rebuilding with the SAME opacity is a no-op that composites nothing new.
///
/// The control for the test above: without it, an implementation that repaints
/// unconditionally on every rebuild would satisfy the alpha assertion just as
/// well, and the update path would be doing nothing for the widget layer.
#[test]
fn rebuilding_an_opacity_widget_with_an_unchanged_value_paints_nothing() {
    let mut harness = lay_out(
        Opacity::new(0.5).child(SizedBox::new(40.0, 40.0)),
        tight(200.0, 200.0),
    );
    let painted = harness.painted_frame_count();

    harness.pump_widget(Opacity::new(0.5).child(SizedBox::new(40.0, 40.0)));

    assert_eq!(
        harness.painted_frame_count(),
        painted,
        "an unchanged opacity reports RenderUpdateImpact::NONE, so the frame \
         must not repaint at all",
    );
}
