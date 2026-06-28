//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

mod common;

use common::{lay_out, loose, tight};
use flui_widgets::{Center, Padding, Text};

#[test]
fn text_measures_to_a_nonempty_box() {
    let laid = lay_out(Text::new("hello harness"), loose(1000.0));
    let measured = laid.size(laid.root());
    // Real shaping ran: the glyph run has positive width and height. Exact
    // metrics are font-dependent, so we assert non-degeneracy (would fail on a
    // Size::ZERO stub) rather than pinning fragile pixel values.
    assert!(
        measured.width.get() > 0.0,
        "measured text width should be positive, got {measured:?}",
    );
    assert!(
        measured.height.get() > 0.0,
        "measured text height should be positive, got {measured:?}",
    );
}

#[test]
fn text_composes_as_a_leaf_child() {
    // Padding(4) around centered text inside a tight 300×200: the whole tree
    // lays out and the text node measures to a non-empty box.
    let laid = lay_out(
        Padding::all(4.0).child(Center::new().child(Text::new("composed"))),
        tight(300.0, 200.0),
    );
    assert_eq!(laid.size(laid.root()), common::size(300.0, 200.0));

    let center = laid.only_child(laid.root());
    let text = laid.only_child(center);
    let measured = laid.size(text);
    assert!(measured.width.get() > 0.0 && measured.height.get() > 0.0);
}
