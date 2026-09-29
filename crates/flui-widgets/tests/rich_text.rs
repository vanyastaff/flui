//! Layout test for [`RichText`] — proves the widget measures a real
//! multi-style span tree headlessly through the same `RenderParagraph`
//! [`Text`](flui_widgets::Text) uses, and that styling actually reaches the
//! shaped glyph run (not just the top-level span).

use crate::common::{lay_out, loose};
use flui_painting::typography::{FontWeight, TextSpan, TextStyle};
use flui_widgets::RichText;

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

    let baseline_width = baseline.size(baseline.root()).width;
    let styled_width = styled_child.size(styled_child.root()).width;

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
    let narrow_but_tall = flui_rendering::constraints::BoxConstraints::new(80.0, 80.0, 0.0, 1000.0);

    let unlimited = lay_out(RichText::new(long_span.clone()), narrow_but_tall);
    let capped = lay_out(RichText::new(long_span).max_lines(1), narrow_but_tall);

    let unlimited_height = unlimited.size(unlimited.root()).height;
    let capped_height = capped.size(capped.root()).height;

    assert!(
        capped_height < unlimited_height,
        "max_lines(1) must produce a shorter box than unlimited wrapping: \
         capped={capped_height}, unlimited={unlimited_height}",
    );
}
