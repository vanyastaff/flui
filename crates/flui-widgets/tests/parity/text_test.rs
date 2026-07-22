//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/text_test.dart`,
//! `default_text_style_test.dart` (tag `3.44.0`). FLUI's text stack is
//! cosmic-text, not Flutter's engine (no Ahem test font), so every case here
//! is geometry-**relative** (A vs B under the same constraints) rather than
//! an exact pixel assertion — Flutter's own tests pin exact glyph metrics
//! only because `flutter_test` loads the deterministic Ahem font; FLUI has
//! no equivalent, so a ported case is expressed as the same behavioral
//! *direction* Flutter asserts (taller, wider, capped, shifted) instead of a
//! specific number.
//!
//! Pattern ported (pre-existing): `RenderParagraph` produces a non-degenerate
//! box after real text shaping (non-zero width and height).
//!
//! Widget → render-object mapping:
//! - `Text("…")` → `RichText` → `RenderParagraph`
//! - `DefaultTextStyle` → `InheritedView<TextStyle>`, read via `Text::build`
//!
//! New harness primitive exercised: `find_by_render_type("RenderParagraph")`.
//! This also validates that `RenderParagraph::debug_fill_properties` emits the
//! `"text"` property (added in C1.13) and that `find_text` can match on it.
//!
//! ## Ported cases
//! - `'inline widgets works with ellipsis'` (`maxLines: 1` usage) and the
//!   general `RenderParagraph.maxLines` contract (`paragraph.dart`) — a
//!   1-line-capped paragraph is strictly shorter than the same text
//!   unbounded, ported at the `Text` widget layer (the `RichText` layer
//!   already has this in `tests/rich_text.rs`; this proves the cap survives
//!   `Text::build`'s `maxLines ?? defaultTextStyle.maxLines` merge,
//!   `text.dart:765`) —
//!   [`max_lines_one_produces_a_shorter_paragraph_than_unbounded_at_the_text_widget_layer`].
//! - The general text-wrapping contract every Flutter paragraph test relies
//!   on (a narrower box wraps to more lines) — ported at the `Text` widget
//!   layer (the unit-level `RenderParagraph` case already lives in
//!   `crates/flui-objects/src/text/paragraph.rs`) —
//!   [`narrow_width_wraps_text_widget_taller_than_wide_width`].
//! - `'Text can be created from TextSpans and uses defaultTextStyle'` and
//!   `'DefaultTextStyle changes propagate to Text'` (`default_text_style_test.dart`)
//!   — an ambient `DefaultTextStyle` reaches the descendant `Text`'s shaped
//!   run: a larger ambient font size measures wider and taller —
//!   [`default_text_style_ambient_font_size_widens_and_heightens_descendant_text`].
//! - `Text.build`'s merge order, `defaultTextStyle.style.merge(style)`
//!   (`text.dart:718-720`) — the run's own style wins over the ambient one
//!   field-by-field, observed as the own (larger) font size winning the
//!   measured width —
//!   [`text_own_style_overrides_ambient_default_text_style_per_field`].
//! - `Text.build`'s `maxLines ?? defaultTextStyle.maxLines` fallback
//!   (`text.dart:765`) — a `Text` that sets no cap of its own still gets
//!   capped by the ambient `DefaultTextStyle.maxLines` —
//!   [`default_text_style_max_lines_fallback_caps_a_text_that_sets_none`].
//! - `'textWidthBasis with textAlign still obeys parent alignment'`
//!   (`text_test.dart`), narrowed to the alignment half (FLUI has no
//!   `textWidthBasis` — see *Not ported*): a right-aligned line's paint
//!   position sits further right than a left-aligned line of the same text
//!   under the same box —
//!   [`right_aligned_text_paints_further_right_than_left_aligned_for_the_same_short_line`].
//! - Flutter's empty-paragraph contract — an empty string still occupies one
//!   line's height, not zero (`paragraph.dart`'s empty-text layout branch;
//!   FLUI's cosmic-text-backed equivalent is
//!   `flui-painting`'s `text_layout::layout::LayoutResult::metrics`'s
//!   `line_count == 0` branch, which synthesizes `self.line_height`) — ported
//!   as the relative fact expressible without an exact line-height number:
//!   empty text's height matches a single-character line's —
//!   [`empty_text_has_the_same_line_height_as_a_single_character_line`].
//!
//! ## Not ported
//! - Exact pixel/glyph assertions (`'Text respects textScaleFactor with
//!   default font size'`, the `textWidthBasis` suite, semantics-label tests,
//!   `WidgetSpan`/inline-widget cases) — these pin Ahem-font metrics or
//!   `WidgetSpan` layout FLUI's `RenderParagraph` does not implement
//!   (`crates/flui-objects/src/text/paragraph.rs`'s own "out of scope" note).
//! - `'Overflow is clipping correctly - …'` (`TextOverflow` clip/fade/ellipsis)
//!   and `'Text uses TextStyle.overflow'` — `Text`/`RichText` do not expose an
//!   `overflow` parameter yet (`default_text_style.rs`'s "Not ported, and
//!   named" list); only `RenderParagraph::with_ellipsis` carries it, exercised
//!   by `crates/flui-objects/src/text/paragraph.rs`'s
//!   `no_wrap_ellipsis_truncates_under_finite_constraints` unit test. No
//!   widget-layer path reaches it to port a widget-level case against.
//! - `'DefaultTextStyle.merge correctly merges arguments'` — the merged
//!   fields under test (`softWrap`, `overflow`, `textWidthBasis`,
//!   `textHeightBehavior`) have no FLUI counterpart
//!   (`default_text_style.rs`'s "Not ported, and named" list); the two
//!   fields FLUI's `DefaultTextStyle` does carry (style, max_lines) are
//!   covered above.
//!
//! Divergence: Flutter asserts exact pixel dimensions (font-specific). FLUI
//! asserts positivity and relative ordering only — the shaping ran and
//! produced a real glyph run, and the direction of the effect under test
//! matches Flutter's.

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::typography::{TextAlign, TextStyle};
use flui_widgets::{Center, DefaultTextStyle, Text};

use crate::harness;

/// A fixed-width, loosely-height-bounded box: `width` is tight (min == max),
/// height ranges `0..2000`. Used where a test needs to compare paragraph
/// *height* across constraints (wrapping, `maxLines`) — `harness::screen_of`
/// forces height tight too, which would hide any height difference behind
/// the box's own clamp.
fn fixed_width_loose_height(width: f32) -> BoxConstraints {
    BoxConstraints::new(px(width), px(width), px(0.0), px(2000.0))
}

/// A box loose on both axes up to `max_width` × `2000`. `TextAlign` only
/// shifts the paint offset relative to the *incoming* max width the
/// paragraph laid out against (`TextPainter::compute_paint_offset`), so a
/// tight width (min == max) is the wrong shape here — it forces the
/// paragraph's own content width up to fill the box, leaving no "extra
/// space" for alignment to distribute in the first place. A loose bound
/// keeps the box sized to its (short) content while still recording the
/// wider incoming max width the alignment shift is computed against.
fn loose_width(max_width: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(max_width), px(0.0), px(2000.0))
}

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

/// A `Text` capped at one line must be strictly shorter than the same text
/// laid out with no cap, under an identical narrow, wrap-forcing width.
///
/// Flutter parity: the general `RenderParagraph.maxLines` contract
/// (`paragraph.dart`), ported here at the `Text` widget layer to prove
/// `Text::max_lines` survives `Text::build`'s merge into `RichText`
/// (`text.dart:765`) — `tests/rich_text.rs`'s
/// `max_lines_one_produces_a_shorter_box_than_unlimited_lines_for_wrapped_spans`
/// already covers the `RichText` → `RenderParagraph` half.
#[test]
fn max_lines_one_produces_a_shorter_paragraph_than_unbounded_at_the_text_widget_layer() {
    let text = "one two three four five six seven eight nine ten";
    let narrow = fixed_width_loose_height(80.0);

    let unlimited = harness::pump_widget(Text::new(text), narrow);
    let capped = harness::pump_widget(Text::new(text).max_lines(1), narrow);

    let unlimited_height = unlimited
        .size(unlimited.find_by_render_type("RenderParagraph"))
        .height
        .get();
    let capped_height = capped
        .size(capped.find_by_render_type("RenderParagraph"))
        .height
        .get();

    assert!(
        capped_height < unlimited_height,
        "max_lines(1) must produce a shorter Text than unlimited wrapping: \
         capped={capped_height}, unlimited={unlimited_height}"
    );
}

/// The same text under a narrow width wraps to more lines — and so a taller
/// box — than under a wide width, at the `Text` widget layer.
///
/// Flutter parity: the general paragraph-wrapping contract every Flutter
/// text test relies on; the unit-level `RenderParagraph` case already lives
/// in `crates/flui-objects/src/text/paragraph.rs`'s
/// `narrow_constraints_wrap_taller_and_no_wider_than_single_line`. This ports
/// the same fact through the public `Text` widget.
#[test]
fn narrow_width_wraps_text_widget_taller_than_wide_width() {
    let text = "a b c d e f g h i j k l m n";

    let wide = harness::pump_widget(Text::new(text), fixed_width_loose_height(2000.0));
    let narrow = harness::pump_widget(Text::new(text), fixed_width_loose_height(60.0));

    let wide_height = wide
        .size(wide.find_by_render_type("RenderParagraph"))
        .height;
    let narrow_height = narrow
        .size(narrow.find_by_render_type("RenderParagraph"))
        .height;

    assert!(
        narrow_height > wide_height,
        "wrapping under a 60px width must be taller than a 2000px-wide single \
         line: narrow={narrow_height:?}, wide={wide_height:?}"
    );
}

/// An ambient `DefaultTextStyle` with a larger font size reaches the
/// descendant `Text`'s shaped run: the same string measures both wider and
/// taller than the unstyled baseline.
///
/// Flutter parity: `default_text_style_test.dart`'s `'DefaultTextStyle
/// changes propagate to Text'` and `text_test.dart`'s `'Text can be created
/// from TextSpans and uses defaultTextStyle'` — both assert the ambient
/// style reaches the built `RichText`; ported here as the geometry
/// consequence (a larger ambient font produces a larger measured box) since
/// FLUI has no `RichText.text.style` accessor to compare `TextStyle` values
/// directly.
#[test]
fn default_text_style_ambient_font_size_widens_and_heightens_descendant_text() {
    let text = "measure me";

    let plain = harness::pump_widget(Center::new().child(Text::new(text)), harness::screen());
    let styled = harness::pump_widget(
        Center::new().child(DefaultTextStyle::new(
            TextStyle::default().with_font_size(48.0),
            Text::new(text),
        )),
        harness::screen(),
    );

    let plain_size = plain.size(plain.find_by_render_type("RenderParagraph"));
    let styled_size = styled.size(styled.find_by_render_type("RenderParagraph"));

    assert!(
        styled_size.width.get() > plain_size.width.get(),
        "a larger ambient font size must widen the measured run: \
         styled={styled_size:?}, plain={plain_size:?}"
    );
    assert!(
        styled_size.height.get() > plain_size.height.get(),
        "a larger ambient font size must heighten the measured run: \
         styled={styled_size:?}, plain={plain_size:?}"
    );
}

/// A `Text`'s own style wins over the ambient `DefaultTextStyle`, field by
/// field — Flutter's `defaultTextStyle.style.merge(style)` merge order.
///
/// Flutter parity: `text.dart:718-720`'s merge order (the run's own style
/// takes precedence over the ambient one). Ported as the geometry
/// consequence: a larger own font size measures wider than the same text
/// reading only the (smaller) ambient font size.
#[test]
fn text_own_style_overrides_ambient_default_text_style_per_field() {
    let text = "override me";
    let ambient_style = TextStyle::default().with_font_size(12.0);

    let ambient_only = harness::pump_widget(
        Center::new().child(DefaultTextStyle::new(
            ambient_style.clone(),
            Text::new(text),
        )),
        harness::screen(),
    );
    let own_overrides = harness::pump_widget(
        Center::new().child(DefaultTextStyle::new(
            ambient_style,
            Text::new(text).style(TextStyle::default().with_font_size(48.0)),
        )),
        harness::screen(),
    );

    let ambient_width = ambient_only
        .size(ambient_only.find_by_render_type("RenderParagraph"))
        .width
        .get();
    let own_width = own_overrides
        .size(own_overrides.find_by_render_type("RenderParagraph"))
        .width
        .get();

    assert!(
        own_width > ambient_width,
        "the Text's own (larger) font size must win over the ambient one: \
         own={own_width}, ambient_only={ambient_width}"
    );
}

/// A `Text` that sets no `max_lines` of its own still gets capped by the
/// ambient `DefaultTextStyle.max_lines` — Flutter's `maxLines ??
/// defaultTextStyle.maxLines` fallback.
///
/// Flutter parity: `text.dart:765`.
#[test]
fn default_text_style_max_lines_fallback_caps_a_text_that_sets_none() {
    let text = "one two three four five six seven eight nine ten eleven twelve";
    let narrow = fixed_width_loose_height(80.0);

    let capped = harness::pump_widget(
        DefaultTextStyle::new(TextStyle::default(), Text::new(text)).max_lines(1),
        narrow,
    );
    let unlimited = harness::pump_widget(
        DefaultTextStyle::new(TextStyle::default(), Text::new(text)),
        narrow,
    );

    let capped_height = capped
        .size(capped.find_by_render_type("RenderParagraph"))
        .height
        .get();
    let unlimited_height = unlimited
        .size(unlimited.find_by_render_type("RenderParagraph"))
        .height
        .get();

    assert!(
        capped_height < unlimited_height,
        "a Text with no own max_lines must still be capped by the ambient \
         DefaultTextStyle.max_lines(1): capped={capped_height}, unlimited={unlimited_height}"
    );
}

/// A right-aligned line's paint position sits further right than a
/// left-aligned line of the same text, under the same (wide) box.
///
/// Flutter parity: `text_test.dart`'s `'textWidthBasis with textAlign still
/// obeys parent alignment'`, narrowed to the alignment half — FLUI has no
/// `textWidthBasis` (see the module doc's *Not ported* list). Uses
/// `LaidOut::paragraph_first_line_left`, which reads the alignment-adjusted
/// paint offset `TextPainter::get_boxes_for_selection` folds in — not the
/// node's own box, which stays whatever size the tight width constrained it
/// to regardless of alignment.
#[test]
fn right_aligned_text_paints_further_right_than_left_aligned_for_the_same_short_line() {
    let wide = loose_width(400.0);

    let left = harness::pump_widget(Text::new("hi").align(TextAlign::Left), wide);
    let right = harness::pump_widget(Text::new("hi").align(TextAlign::Right), wide);

    let left_line_start =
        left.paragraph_first_line_left(left.find_by_render_type("RenderParagraph"));
    let right_line_start =
        right.paragraph_first_line_left(right.find_by_render_type("RenderParagraph"));

    assert!(
        right_line_start > left_line_start,
        "a right-aligned line must start further right than a left-aligned \
         one in the same box: right={right_line_start}, left={left_line_start}"
    );
}

/// An empty string still occupies one line's height — the same height as a
/// single-character line — not zero.
///
/// Flutter parity: `paragraph.dart`'s empty-paragraph layout contract
/// (an empty run is one empty line, not a zero-height box). FLUI's
/// cosmic-text-backed equivalent is `flui-painting`'s
/// `text_layout::layout::LayoutResult::metrics`, whose `line_count == 0`
/// branch synthesizes the height from the font's line-height rather than
/// returning zero. Ported as the relative fact expressible without an exact
/// line-height number: empty and single-character text measure to the same
/// height (both are exactly one un-wrapped line of the same style).
#[test]
fn empty_text_has_the_same_line_height_as_a_single_character_line() {
    let empty = harness::pump_widget(Center::new().child(Text::new("")), harness::screen());
    let one_char = harness::pump_widget(Center::new().child(Text::new("x")), harness::screen());

    let empty_height = empty
        .size(empty.find_by_render_type("RenderParagraph"))
        .height
        .get();
    let one_char_height = one_char
        .size(one_char.find_by_render_type("RenderParagraph"))
        .height
        .get();

    assert!(
        empty_height > 0.0,
        "an empty Text must still occupy a non-zero line height, got {empty_height}"
    );
    assert!(
        (empty_height - one_char_height).abs() < 0.5,
        "an empty line and a single-character line must measure the same \
         height (both are one un-wrapped line of the same style): \
         empty={empty_height}, one_char={one_char_height}"
    );
}
