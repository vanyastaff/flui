//! `Visibility` -- show/hide a child, optionally preserving its state via
//! `Offstage`. Untested anywhere in the suite; verifies each of the three
//! `build()` branches documented in `crates/flui-widgets/src/interaction/
//! visibility.rs` (Flutter oracle: `indexed_stack.dart` lines 452-473).

mod common;

use common::{lay_out, loose, size};
use flui_widgets::{SizedBox, Visibility};

#[test]
fn default_visible_shows_the_child_directly_with_no_offstage_wrapper() {
    let laid = lay_out(Visibility::new(SizedBox::new(30.0, 20.0)), loose(1000.0));

    assert!(
        laid.find_all_by_render_type("RenderOffstage").is_empty(),
        "the default (maintain_state = false) path must not wrap the child \
         in Offstage at all",
    );
    assert_eq!(laid.size(laid.root()), size(30.0, 20.0));
}

#[test]
fn hidden_without_maintain_state_shows_the_default_replacement() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0)).visible(false),
        loose(1000.0),
    );

    // Default replacement is SizedBox::shrink() -- the real 30x20 child must
    // be entirely absent, replaced by a zero-size box.
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

#[test]
fn hidden_without_maintain_state_uses_a_custom_replacement() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .visible(false)
            .replacement(SizedBox::new(5.0, 5.0)),
        loose(1000.0),
    );

    assert_eq!(laid.size(laid.root()), size(5.0, 5.0));
}

#[test]
fn maintain_state_true_and_visible_wraps_the_child_in_a_non_offstage_offstage() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0)).maintain_state(true),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    assert_eq!(
        laid.size(offstage_id),
        size(30.0, 20.0),
        "visible = true must report the child's real size through Offstage \
         (transparent-proxy branch, offstage = false)",
    );
}

#[test]
fn maintain_state_true_and_hidden_wraps_the_child_in_an_offstage_offstage() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .maintain_state(true)
            .visible(false),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    // `RenderOffstage` takes `constraints.smallest()` when offstage (Flutter's
    // `sizedByParent => offstage`). Under `loose(1000)` that is zero. The child
    // is laid out at its full size regardless — asserted in the test below.
    assert_eq!(
        laid.size(offstage_id),
        size(0.0, 0.0),
        "visible = false with maintain_state must take constraints.smallest() \
         while keeping the child attached (state preserved, not removed)",
    );
    // The child render node must still be present in the tree (state kept
    // alive), unlike the maintain_state = false replacement path.
    assert_eq!(laid.render_node_count(), 2, "RenderOffstage + the child");
}

/// The widget-level consequence of `RenderOffstage`'s layout contract: a
/// hidden-but-maintained child is laid out at its **full size**, not collapsed to zero.
///
/// Flutter's `RenderOffstage.performLayout` does `child?.layout(constraints)`
/// with the real constraints (`proxy_box.dart:3919-3925`); only the `Offstage`
/// box itself shrinks to `constraints.smallest`. This is what makes
/// `ModalRoute.offstage` able to measure a route at its final geometry.
///
/// Red-check: lay the child out at `BoxConstraints::tight(Size::ZERO)` in
/// `RenderOffstage::perform_layout`; the child measures 0×0.
#[test]
fn maintain_state_true_and_hidden_lays_the_child_out_at_full_size() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .maintain_state(true)
            .visible(false),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    assert_eq!(
        laid.size(offstage_id),
        size(0.0, 0.0),
        "the Offstage box takes constraints.smallest() — zero, under loose"
    );
    assert_eq!(
        laid.size(laid.only_child(offstage_id)),
        size(30.0, 20.0),
        "but the hidden child reaches its real geometry"
    );
}
