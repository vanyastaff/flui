//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/text_test.dart`
//! Pattern ported: `RenderParagraph` produces a non-degenerate box after real
//! text shaping (non-zero width and height). Flutter's text tests assert on
//! specific pixel metrics; FLUI avoids brittle font-metric pins and instead
//! asserts non-degeneracy (non-zero bounds) per the existing `text.rs`
//! harness convention.
//!
//! Widget → render-object mapping:
//! - `Text("…")` → `RenderParagraph`
//!
//! New harness primitive exercised: `find_by_render_type("RenderParagraph")`.
//! This also validates that `RenderParagraph::debug_fill_properties` emits the
//! `"text"` property (added in C1.13) and that `find_text` can match on it.
//!
//! Divergence: Flutter asserts exact pixel dimensions (font-specific). FLUI
//! asserts positivity only — the shaping ran and produced a real glyph run.

use flui_widgets::{Center, Text};

use crate::harness;

/// `Text("hello")` shaped by `RenderParagraph` must produce a non-degenerate
/// box (positive width and height).
///
/// Flutter parity: text_test.dart — `RenderParagraph` non-degenerate box
/// after real text measurement through the cosmic-text shaping pipeline.
#[test]
fn text_measures_to_nonempty_box_via_render_paragraph_finder() {
    let laid = harness::pump_widget(
        Center::new().child(Text::new("hello parity")),
        harness::screen(),
    );

    let paragraph_id = laid.find_by_render_type("RenderParagraph");
    let measured = laid.size(paragraph_id);
    assert!(
        measured.width.get() > 0.0,
        "RenderParagraph must have positive width after shaping; got {measured:?}"
    );
    assert!(
        measured.height.get() > 0.0,
        "RenderParagraph must have positive height after shaping; got {measured:?}"
    );
}

/// `find_text` locates `RenderParagraph` by plain-text content.
///
/// Validates that `RenderParagraph::debug_fill_properties` emits the `"text"`
/// diagnostics property (added in C1.13) and that `LaidOut::find_text` finds
/// it correctly.
#[test]
fn find_text_locates_render_paragraph_by_content() {
    let laid = harness::pump_widget(Center::new().child(Text::new("find me")), harness::screen());

    let paragraph_id = laid.find_text("find me");
    assert!(
        paragraph_id.is_some(),
        "find_text(\"find me\") must locate the RenderParagraph with that content"
    );

    // Consistency: find_text and find_by_render_type must agree on the node.
    let by_type_id = laid.find_by_render_type("RenderParagraph");
    assert_eq!(
        paragraph_id.unwrap(),
        by_type_id,
        "find_text and find_by_render_type must identify the same RenderParagraph node"
    );
}

/// Different text strings produce distinct non-degenerate boxes.
///
/// Flutter parity: text_test.dart — longer strings produce wider boxes than
/// shorter ones under the same constraints (proves shaping is live, not
/// a cached constant).
#[test]
fn longer_text_produces_wider_box_than_shorter_text() {
    // Wrap in Center so RenderParagraph receives loose constraints and sizes
    // to its natural (intrinsic) width rather than filling the tight surface.
    let short = harness::pump_widget(Center::new().child(Text::new("hi")), harness::screen());
    let long = harness::pump_widget(
        Center::new().child(Text::new("hello parity test harness")),
        harness::screen(),
    );

    let short_width = short
        .size(short.find_by_render_type("RenderParagraph"))
        .width;
    let long_width = long.size(long.find_by_render_type("RenderParagraph")).width;

    assert!(
        long_width > short_width,
        "longer text must produce a wider RenderParagraph box; \
         short={short_width:?} long={long_width:?}"
    );
}
