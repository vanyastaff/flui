//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

use crate::common::{lay_out, loose, tight};
use flui_types::typography::{TextDirection, TextStyle};
use flui_widgets::{Center, DefaultTextStyle, Padding, Text};

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
    assert_eq!(laid.size(laid.root()), crate::common::size(300.0, 200.0));

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

// ============================================================================
// DefaultTextStyle (text.dart:55-136, consumed by Text.build :716-765)
// ============================================================================

/// An enclosing `DefaultTextStyle` styles a bare `Text` run: the ambient
/// `font_size` shapes the glyphs, so the box grows with it (`text.dart:720`).
///
/// Red-check: drop the `depend_on::<DefaultTextStyle, _>` read from `Text::build`
/// — both boxes measure identically.
#[test]
fn an_enclosing_default_text_style_styles_a_bare_run() {
    let bare = lay_out(Text::new("ambient type"), loose(1000.0));
    let styled = lay_out(
        DefaultTextStyle::new(
            TextStyle::default().with_font_size(40.0),
            Text::new("ambient type"),
        ),
        loose(1000.0),
    );

    assert!(
        styled.size(styled.root()).height.get() > bare.size(bare.root()).height.get(),
        "the ambient 40pt style must produce a taller box than the default type"
    );
}

/// The run's own style merges **over** the ambient one (`text.dart:718-720`): a
/// run that sets its own `font_size` under a larger ambient size measures like the
/// bare run with that size, not like the ambient.
///
/// Red-check: merge the other way (`own.merge(&ambient)`) — the ambient 40pt wins
/// and the box is taller than the 12pt reference.
#[test]
fn a_runs_own_style_wins_over_the_ambient_one() {
    let reference = lay_out(
        Text::new("own type").style(TextStyle::default().with_font_size(12.0)),
        loose(1000.0),
    );
    let under_ambient = lay_out(
        DefaultTextStyle::new(
            TextStyle::default().with_font_size(40.0),
            Text::new("own type").style(TextStyle::default().with_font_size(12.0)),
        ),
        loose(1000.0),
    );

    assert_eq!(
        under_ambient.size(under_ambient.root()),
        reference.size(reference.root()),
        "the run's own 12pt must override the ambient 40pt"
    );
}

/// `maxLines ?? defaultTextStyle.maxLines` (`text.dart:765`): a run with no cap of
/// its own inherits the ambient cap; a run with its own cap keeps it.
#[test]
fn ambient_max_lines_caps_a_run_that_sets_none() {
    let long_text = "one two three four five six seven eight nine ten";
    let narrow_but_tall = flui_rendering::constraints::BoxConstraints::new(
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(0.0),
        flui_types::geometry::px(1000.0),
    );

    let unlimited = lay_out(Text::new(long_text), narrow_but_tall);
    let ambient_capped = lay_out(
        DefaultTextStyle::new(TextStyle::default(), Text::new(long_text)).max_lines(1),
        narrow_but_tall,
    );
    let own_cap_wins = lay_out(
        DefaultTextStyle::new(TextStyle::default(), Text::new(long_text).max_lines(2)).max_lines(1),
        narrow_but_tall,
    );

    let unlimited_height = unlimited.size(unlimited.root()).height.get();
    let ambient_height = ambient_capped.size(ambient_capped.root()).height.get();
    let own_height = own_cap_wins.size(own_cap_wins.root()).height.get();
    assert!(
        ambient_height < unlimited_height,
        "the ambient cap must apply to a run that sets none \
         (ambient={ambient_height}, unlimited={unlimited_height})"
    );
    assert!(
        ambient_height < own_height && own_height < unlimited_height,
        "a run's own max_lines(2) must beat the ambient cap of 1 \
         (ambient={ambient_height}, own={own_height}, unlimited={unlimited_height})"
    );
}
