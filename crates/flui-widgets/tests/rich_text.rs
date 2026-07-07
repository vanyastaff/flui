//! Layout test for [`RichText`] — proves the widget measures a real
//! multi-style span tree headlessly through the same `RenderParagraph`
//! [`Text`](flui_widgets::Text) uses, and that styling actually reaches the
//! shaped glyph run (not just the top-level span).

use crate::common::{lay_out, loose, tight};
use flui_types::typography::{FontWeight, TextSpan, TextStyle};
use flui_widgets::{Center, Padding, RichText};

#[test]
fn rich_text_measures_to_a_nonempty_box() {
    let laid = lay_out(
        RichText::new(TextSpan::new("hello ").with_child(TextSpan::new("harness"))),
        loose(1000.0),
    );
    let measured = laid.size(laid.root());
    assert!(
        measured.width.get() > 0.0,
        "measured span-tree width should be positive, got {measured:?}",
    );
    assert!(
        measured.height.get() > 0.0,
        "measured span-tree height should be positive, got {measured:?}",
    );
}

#[test]
fn rich_text_composes_as_a_leaf_child() {
    let laid = lay_out(
        Padding::all(4.0).child(Center::new().child(RichText::new(TextSpan::new("composed")))),
        tight(300.0, 200.0),
    );
    assert_eq!(laid.size(laid.root()), crate::common::size(300.0, 200.0));

    let center = laid.only_child(laid.root());
    let rich_text = laid.only_child(center);
    let measured = laid.size(rich_text);
    assert!(measured.width.get() > 0.0 && measured.height.get() > 0.0);
}

#[test]
fn a_child_spans_style_widens_the_measured_run_beyond_the_unstyled_baseline() {
    // Same text content ("wide") in both cases; only the second tree's child
    // span carries an enlarged font_size. If child-span styling were dropped
    // on the way to the render object (rather than merged into the shaped
    // run), both would measure identically.
    let baseline = lay_out(RichText::new(TextSpan::new("wide")), loose(1000.0));

    let styled_child = lay_out(
        RichText::new(TextSpan::with_children(vec![TextSpan::styled(
            "wide",
            TextStyle {
                font_size: Some(48.0),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
        )])),
        loose(1000.0),
    );

    let baseline_width = baseline.size(baseline.root()).width.get();
    let styled_width = styled_child.size(styled_child.root()).width.get();

    assert!(
        styled_width > baseline_width,
        "a larger/bolder child span must measure wider than the plain baseline: \
         styled={styled_width}, baseline={baseline_width}",
    );
}

#[test]
fn max_lines_one_produces_a_shorter_box_than_unlimited_lines_for_wrapped_spans() {
    let long_span = TextSpan::new("one two three ")
        .with_child(TextSpan::new("four five six seven eight nine ten"));
    let narrow_but_tall = flui_rendering::constraints::BoxConstraints::new(
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(80.0),
        flui_types::geometry::px(0.0),
        flui_types::geometry::px(1000.0),
    );

    let unlimited = lay_out(RichText::new(long_span.clone()), narrow_but_tall);
    let capped = lay_out(RichText::new(long_span).max_lines(1), narrow_but_tall);

    let unlimited_height = unlimited.size(unlimited.root()).height.get();
    let capped_height = capped.size(capped.root()).height.get();

    assert!(
        capped_height < unlimited_height,
        "max_lines(1) must produce a shorter box than unlimited wrapping: \
         capped={capped_height}, unlimited={unlimited_height}",
    );
}
