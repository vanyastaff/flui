//! Layout parity tests for [`Stack`], [`AspectRatio`], and
//! [`FractionallySizedBox`].

use crate::common::{lay_out, loose, size};
use flui_widgets::{AspectRatio, SizedBox};

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
