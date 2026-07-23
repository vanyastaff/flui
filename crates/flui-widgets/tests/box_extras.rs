//! Layout parity tests for [`Stack`], [`AspectRatio`], and
//! [`FractionallySizedBox`].

use crate::common::{lay_out, loose, offset, size, tight};
use flui_view::ViewExt;
use flui_widgets::{AspectRatio, FractionallySizedBox, SizedBox, Stack};

#[test]
fn stack_sizes_to_largest_child_and_aligns_top_left() {
    // Loose fit + TOP_LEFT default: stack = max width × max height; both
    // children pinned to (0, 0).
    let laid = lay_out(
        Stack::new(vec![
            SizedBox::new(100.0, 60.0).boxed(),
            SizedBox::new(40.0, 80.0).boxed(),
        ]),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 80.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 0.0));
    assert_eq!(laid.size(laid.child(root, 0)), size(100.0, 60.0));
    assert_eq!(laid.size(laid.child(root, 1)), size(40.0, 80.0));
}

#[test]
fn aspect_ratio_picks_largest_box_with_ratio() {
    // ratio = width/height = 2.0 under loose 0..200: biggest box keeping the
    // ratio is 200×100.
    let laid = lay_out(
        AspectRatio::new(2.0).child(SizedBox::expand()),
        loose(200.0),
    );
    assert_eq!(laid.size(laid.root()), size(200.0, 100.0));
}

#[test]
fn fractionally_sized_box_sizes_child_to_a_fraction() {
    // Fills the tight 200×200; the child is sized to 0.5 × each axis → 100×100.
    let laid = lay_out(
        FractionallySizedBox::new()
            .width_factor(0.5)
            .height_factor(0.5)
            .child(SizedBox::expand()),
        tight(200.0, 200.0),
    );
    assert_eq!(laid.size(laid.root()), size(200.0, 200.0));
    assert_eq!(laid.size(laid.only_child(laid.root())), size(100.0, 100.0));
}
