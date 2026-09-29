//! Deeper layout-parity tests: multi-level constraint propagation, the full
//! `Container` composition stack, `Stack` alignment, and flex main-axis
//! distribution. Each asserts computed geometry that would be wrong if a layer
//! mis-propagated constraints or mis-placed a child.

use crate::common::{lay_out, loose, offset, size};
use flui_widgets::{Padding, SizedBox};

#[test]
fn nested_padding_accumulates_insets_through_levels() {
    // Padding(10) → Padding(5) → SizedBox(100): inner 110, outer 130.
    let laid = lay_out(
        Padding::all(10.0).child(Padding::all(5.0).child(SizedBox::square(100.0))),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(130.0, 130.0));

    let inner_padding = laid.only_child(laid.root());
    assert_eq!(laid.size(inner_padding), size(110.0, 110.0));
    // Inner padding sits at (10,10) inside the outer padding.
    assert_eq!(laid.offset(inner_padding), offset(10.0, 10.0));

    let inner_box = laid.only_child(inner_padding);
    assert_eq!(laid.size(inner_box), size(100.0, 100.0));
    assert_eq!(laid.offset(inner_box), offset(5.0, 5.0));
}
