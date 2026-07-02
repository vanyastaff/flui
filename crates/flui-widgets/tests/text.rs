//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

mod common;

use common::{lay_out, loose, tight};
use flui_types::typography::{TextDirection, TextStyle};
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

#[test]
fn max_lines_one_produces_a_shorter_box_than_unlimited_lines_for_wrapped_text() {
    // A long run in a narrow width wraps to multiple lines when unlimited,
    // but max_lines(1) caps it to a single line -- a real, comparative
    // property that holds regardless of the exact font metrics in use.
    let long_text = "one two three four five six seven eight nine ten";
    // Tight width so the run must wrap; loose height so line-wrapping is free
    // to grow the box (a fully tight height would force the same number
    // regardless of content, defeating the comparison).
    let narrow_but_tall = flui_rendering::constraints::BoxConstraints::new(
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(0.0),
        flui_types::geometry::px(1000.0),
    );

    let unlimited = lay_out(Text::new(long_text), narrow_but_tall);
    let capped = lay_out(Text::new(long_text).max_lines(1), narrow_but_tall);

    let unlimited_height = unlimited.size(unlimited.root()).height.get();
    let capped_height = capped.size(capped.root()).height.get();

    assert!(
        capped_height < unlimited_height,
        "max_lines(1) must produce a shorter box than unlimited wrapping: \
         capped={capped_height}, unlimited={unlimited_height}",
    );
}

#[test]
fn a_larger_font_size_measures_to_a_taller_box() {
    let small = lay_out(
        Text::new("size").style(TextStyle {
            font_size: Some(12.0),
            ..Default::default()
        }),
        loose(1000.0),
    );
    let large = lay_out(
        Text::new("size").style(TextStyle {
            font_size: Some(48.0),
            ..Default::default()
        }),
        loose(1000.0),
    );

    assert!(
        large.size(large.root()).height.get() > small.size(small.root()).height.get(),
        "a larger font_size must measure to a taller box",
    );
}

#[test]
fn text_direction_does_not_change_the_measured_size_of_the_same_content() {
    // `direction` governs bidi/shaping order, not the overall measured box
    // for a plain LTR-script run -- both directions must size identically.
    let ltr = lay_out(
        Text::new("same content").direction(TextDirection::Ltr),
        loose(1000.0),
    );
    let rtl = lay_out(
        Text::new("same content").direction(TextDirection::Rtl),
        loose(1000.0),
    );

    assert_eq!(ltr.size(ltr.root()), rtl.size(rtl.root()));
}
