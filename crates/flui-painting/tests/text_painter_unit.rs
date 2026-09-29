//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{TextAlign, TextDirection, TextPosition, TextSpan};
use flui_painting::{Canvas, TextBaseline, TextPainter};

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

#[test]
fn test_text_painter_new() {
    let painter = TextPainter::new();
    assert!(painter.text().is_none());
    assert_eq!(painter.text_align(), TextAlign::Start);
    assert!(painter.text_direction().is_none());
}

#[test]
fn test_text_painter_builder() {
    let painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_align(TextAlign::Center);

    assert!(painter.text().is_some());
    assert_eq!(painter.text_align(), TextAlign::Center);
    assert_eq!(painter.text_direction(), Some(TextDirection::Ltr));
}

#[test]
fn test_text_painter_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    assert!(painter.has_layout());
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

#[test]
fn test_text_painter_setters_invalidate_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);
    assert!(painter.has_layout());

    // Alignment is a paint offset over the shaped lines (shaped/paint
    // split) — the layout cache survives the change.
    painter.set_text_align(TextAlign::Center);
    assert!(painter.has_layout());

    // Layout-affecting setters still drop the cache.
    painter.set_max_lines(Some(1));
    assert!(!painter.has_layout());
}

#[test]
fn test_text_painter_max_lines() {
    let painter = TextPainter::new().with_max_lines(Some(3));

    assert_eq!(painter.max_lines(), Some(3));
}

#[test]
fn test_text_baseline() {
    assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
}

#[test]
fn test_get_offset_for_caret() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    let start = painter.get_offset_for_caret(TextPosition::upstream(0));
    let mid = painter.get_offset_for_caret(TextPosition::upstream(5));
    let end = painter.get_offset_for_caret(TextPosition::upstream(13));

    assert!(start.dx >= 0.0);
    assert!(mid.dx > start.dx);
    assert!(end.dx > mid.dx);
}

#[test]
fn test_get_position_for_offset() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    let pos = painter.get_position_for_offset(Offset::new(0.0, 5.0));
    assert_eq!(pos.offset, 0);

    let pos = painter.get_position_for_offset(Offset::new(1000.0, 5.0));
    assert!(pos.offset <= 5);
}

#[test]
fn test_get_line_metrics() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Line 1\nLine 2"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    let metrics = painter.get_line_metrics();
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0].line_number, 0);
    assert_eq!(metrics[1].line_number, 1);
}

#[test]
fn test_get_boxes_for_selection() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    let boxes = painter.get_boxes_for_selection(1, 5);
    assert!(!boxes.is_empty());

    let first_box = &boxes[0];
    assert!(first_box.rect.width() > 0.0);
    assert!(first_box.rect.height() > 0.0);
}

#[test]
fn test_get_word_boundary() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello World"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(&mut text_cx(), 0.0, 200.0);

    let boundary = painter.get_word_boundary(TextPosition::upstream(2));
    assert!(boundary.start <= 2);
    assert!(boundary.end >= 2);
}

/// A painted span must contribute its laid-out box to the display list's
/// bounds.
///
/// `DrawTextSpan` used to report no bounds at all, so a picture whose only
/// content was rich text claimed an empty rect: every consumer that culls,
/// computes damage, or sizes a repaint boundary from those bounds would have
/// dropped visible text, and the failure would have read as a culling bug.
/// The rect is the laid-out size, not the ink extent — the same rect Flutter's
/// `RenderBox.paintBounds` (`Offset.zero & size`) reports for a
/// `RenderParagraph`.
#[test]
fn painted_span_contributes_its_laid_out_box_to_display_list_bounds() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, FLUI!"))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);

    let size = painter.size();
    assert!(
        size.width > 0.0 && size.height > 0.0,
        "precondition: the text must lay out to a non-empty box, got {size:?}"
    );

    let origin = Offset::new(7.0, 11.0);
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, origin);
    let bounds = canvas.finish().bounds().expect("a painted span has bounds");

    // `TextAlign::Start` under an unbounded width leaves the alignment
    // paint-offset at zero, so the box lands exactly at `origin`.
    assert_eq!(bounds.left(), origin.dx);
    assert_eq!(bounds.top(), origin.dy);
    assert_eq!(bounds.width(), size.width);
    assert_eq!(bounds.height(), size.height);
}

/// `max_lines` must not erase min-content width on the zero-width intrinsic
/// probe (#1085 / ARCHITECTURE mapping decision #10).
#[test]
fn max_lines_does_not_collapse_min_intrinsic_width() {
    let phrase = "a WWWWWWWWWW";
    let full = TextPainter::new()
        .with_text(TextSpan::new(phrase))
        .with_text_direction(TextDirection::Ltr);
    let clipped = TextPainter::new()
        .with_text(TextSpan::new(phrase))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1));

    let full_min = full.min_intrinsic_width(&mut text_cx());
    let full_max = full.max_intrinsic_width(&mut text_cx());
    let clipped_min = clipped.min_intrinsic_width(&mut text_cx());
    let clipped_max = clipped.max_intrinsic_width(&mut text_cx());

    assert!(
        full_min > 0.0,
        "uncapped min intrinsic must be positive, got {full_min}"
    );
    assert!(
        clipped_min > 0.0,
        "max_lines must not collapse min intrinsic to zero, got {clipped_min}"
    );
    assert!(
        clipped_min <= clipped_max,
        "min-content {clipped_min} must be <= max-content {clipped_max}"
    );
    // Width intrinsics ignore max_lines, so capped and uncapped agree.
    assert!(
        (clipped_min - full_min).abs() < 0.01,
        "capped min {clipped_min} must match uncapped min {full_min}"
    );
    assert!(
        (clipped_max - full_max).abs() < 0.01,
        "capped max {clipped_max} must match uncapped max {full_max}"
    );

    // Cached path after layout must match the uncached probe.
    let mut laid_out = TextPainter::new()
        .with_text(TextSpan::new(phrase))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1));
    laid_out.layout(&mut text_cx(), 0.0, 200.0);
    let cached_min = laid_out.min_intrinsic_width(&mut text_cx());
    let cached_max = laid_out.max_intrinsic_width(&mut text_cx());
    assert!(
        (cached_min - clipped_min).abs() < 0.01,
        "cached min {cached_min} must match uncached {clipped_min}"
    );
    assert!(
        (cached_max - clipped_max).abs() < 0.01,
        "cached max {cached_max} must match uncached {clipped_max}"
    );
}

#[test]
fn unbreakable_run_min_intrinsic_stays_positive_under_max_lines() {
    // "W"*N may still soft-break between glyphs in cosmic-text; the
    // contract under test is only that max_lines does not zero the probe.
    let full = TextPainter::new()
        .with_text(TextSpan::new("WWWWWWWWWW"))
        .with_text_direction(TextDirection::Ltr);
    let clipped = TextPainter::new()
        .with_text(TextSpan::new("WWWWWWWWWW"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1));
    let full_min = full.min_intrinsic_width(&mut text_cx());
    let clipped_min = clipped.min_intrinsic_width(&mut text_cx());
    let clipped_max = clipped.max_intrinsic_width(&mut text_cx());
    assert!(
        clipped_min > 0.0,
        "long run min intrinsic must be positive, got {clipped_min}"
    );
    assert!(clipped_min <= clipped_max);
    assert!(
        (clipped_min - full_min).abs() < 0.01,
        "max_lines must not change min intrinsic for a long run: {clipped_min} vs {full_min}"
    );
}

#[test]
fn hard_newlines_keep_positive_width_intrinsics_under_max_lines() {
    // Hard breaks would be truncated by max_lines=1 in committed layout, but
    // width intrinsics still measure the full shaped content (#1085 contract).
    let full = TextPainter::new()
        .with_text(TextSpan::new("short\nWWWWWWWWWW\nmid"))
        .with_text_direction(TextDirection::Ltr);
    let clipped = TextPainter::new()
        .with_text(TextSpan::new("short\nWWWWWWWWWW\nmid"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1));

    let full_min = full.min_intrinsic_width(&mut text_cx());
    let clipped_min = clipped.min_intrinsic_width(&mut text_cx());
    let clipped_max = clipped.max_intrinsic_width(&mut text_cx());
    assert!(
        clipped_min > 0.0,
        "hard-newline min intrinsic must stay positive"
    );
    assert!(
        (clipped_min - full_min).abs() < 0.01,
        "max_lines must not change min intrinsic with hard newlines: {clipped_min} vs {full_min}"
    );
    assert!(clipped_min <= clipped_max);
}

#[test]
fn max_lines_with_ellipsis_keeps_positive_min_intrinsic_and_still_truncates_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(
            "a soft wrapping phrase that exceeds one line under a narrow width",
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));

    let min = painter.min_intrinsic_width(&mut text_cx());
    let max = painter.max_intrinsic_width(&mut text_cx());
    assert!(
        min > 0.0,
        "ellipsis + max_lines must not zero min intrinsic, got {min}"
    );
    assert!(min <= max);

    // Dry layout still enforces truncation — distinct from width intrinsics.
    let dry = painter.dry_size(&mut text_cx(), 0.0, 40.0);
    assert!(
        dry.width > 0.0 && dry.width <= 40.0 + 0.01,
        "dry layout under narrow width must stay within the constraint, got {}",
        dry.width
    );
    assert!(
        dry.width + 0.01 < max,
        "truncated dry width {} must be narrower than max intrinsic {max}",
        dry.width
    );

    painter.layout(&mut text_cx(), 0.0, 40.0);
    assert!(
        painter.did_exceed_max_lines(),
        "committed narrow layout must still enforce max_lines truncation"
    );
    // After layout, cached intrinsics must still ignore the truncation.
    assert!(
        (painter.min_intrinsic_width(&mut text_cx()) - min).abs() < 0.01,
        "layout must not rewrite min intrinsic from the truncating pass"
    );
}

/// When the ellipsis is wider than the text's narrowest run, skipping
/// truncation must not under-report min intrinsic below the ellipsis
/// (Codex review on #1089).
#[test]
fn wide_ellipsis_floors_min_intrinsic_width() {
    let ellipsis = "………";
    let text_only = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr);
    let ellipsis_only = TextPainter::new()
        .with_text(TextSpan::new(ellipsis))
        .with_text_direction(TextDirection::Ltr);
    let truncated = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some(ellipsis.to_string()));

    let text_min = text_only.min_intrinsic_width(&mut text_cx());
    let ellipsis_width = ellipsis_only.max_intrinsic_width(&mut text_cx());
    let floored = truncated.min_intrinsic_width(&mut text_cx());

    assert!(
        ellipsis_width > text_min + 0.01,
        "precondition: ellipsis {ellipsis_width} must exceed text min {text_min}"
    );
    assert!(
        floored + 0.01 >= ellipsis_width,
        "min intrinsic {floored} must not under-report below ellipsis {ellipsis_width}"
    );

    // Cached path after layout must keep the same floor.
    let mut laid_out = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some(ellipsis.to_string()));
    laid_out.layout(&mut text_cx(), 0.0, 200.0);
    assert!(
        (laid_out.min_intrinsic_width(&mut text_cx()) - floored).abs() < 0.01,
        "cached min must keep the ellipsis floor"
    );
}

#[test]
fn intrinsic_height_still_honors_max_lines() {
    let phrase = "one two three four five six seven eight nine ten";
    let uncapped = TextPainter::new()
        .with_text(TextSpan::new(phrase))
        .with_text_direction(TextDirection::Ltr);
    let capped = TextPainter::new()
        .with_text(TextSpan::new(phrase))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1));

    let narrow = 40.0;
    let tall = uncapped.intrinsic_height(&mut text_cx(), narrow);
    let short = capped.intrinsic_height(&mut text_cx(), narrow);
    assert!(tall > 0.0 && short > 0.0);
    assert!(
        short + 0.01 < tall,
        "height probe must still apply max_lines: capped {short} vs uncapped {tall}"
    );
}

/// Parley measurement on the lent context (ADR-0092 §10 step 3). The painter
/// is pinned to Parley, so these run under `parley` without `parley-layout`.
#[cfg(feature = "parley")]
mod parley_measurement {
    use flui_painting::parley_text::ParagraphSpec;
    use flui_painting::testing::measure_with_parley;
    use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
    use flui_painting::{FontCollection, TextContext, TextPainter};

    const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
    const SIZE: f64 = 20.0;
    /// Finite, so a repeated layout is served from the painter's cache when
    /// nothing it is keyed on changed.
    const WIDTH: f64 = 1000.0;

    /// The probe face maps `A` one em wide, so four `A`s measure exactly
    /// four em where it is registered; elsewhere the family falls back to
    /// Roboto, whose `A` is narrower.
    fn probe_style() -> TextStyle {
        TextStyle {
            font_family: Some("FLUI Probe Mono".to_owned()),
            font_weight: Some(FontWeight::W100),
            font_size: Some(SIZE),
            ..TextStyle::default()
        }
    }

    fn probe_painter() -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::styled("AAAA", probe_style()))
            .with_text_direction(TextDirection::Ltr);
        measure_with_parley(&mut painter);
        painter
    }

    /// The same painter measures through whichever context it is lent: the
    /// probe width through the realm whose collection holds the face, the
    /// fallback width through one that does not, and on the first exactly
    /// what that context shapes the paragraph to.
    #[test]
    fn measurement_follows_the_context_it_is_given() {
        let with_probe = FontCollection::new();
        with_probe
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let mut a = TextContext::new(&with_probe);
        let mut b = TextContext::new(&FontCollection::new());
        let mut painter = probe_painter();

        painter.layout(&mut a, 0.0, WIDTH);
        let through_a = painter.size();
        painter.layout(&mut b, 0.0, WIDTH);
        let through_b = painter.size();

        assert!(
            (through_a.width - 4.0 * SIZE).abs() < 0.01,
            "four one-em `A`s through A's context are {} px, got {}",
            4.0 * SIZE,
            through_a.width
        );
        assert!(
            (through_b.width - through_a.width).abs() > 1.0,
            "B's collection lacks the face, so the same painter measures the \
             fallback: A {} vs B {}",
            through_a.width,
            through_b.width
        );

        let spans = vec![("AAAA".to_owned(), Some(probe_style()))];
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the probe size is exact in f32"
        )]
        let shaped = a
            .shape(&ParagraphSpec {
                spans: &spans,
                default_style: None,
                font_size: SIZE as f32,
                max_width: None,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
            })
            .metrics();
        painter.layout(&mut a, 0.0, WIDTH);
        assert!((painter.size().width - shaped.width).abs() < f64::EPSILON);
        assert!((painter.size().height - shaped.height).abs() < f64::EPSILON);
    }

    /// A face registered on the collection after the painter measured makes
    /// the next layout measure again under the same constraints.
    #[test]
    fn a_registration_on_the_collection_invalidates_the_painter_cache() {
        let fonts = FontCollection::new();
        let mut context = TextContext::new(&fonts);
        let mut painter = probe_painter();

        painter.layout(&mut context, 0.0, WIDTH);
        let before = painter.width();
        fonts
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        painter.layout(&mut context, 0.0, WIDTH);
        let after = painter.width();

        assert!(
            (before - 4.0 * SIZE).abs() > 1.0,
            "before registration the family falls back, got {before}"
        );
        assert!(
            (after - 4.0 * SIZE).abs() < 0.01,
            "after registration the cached fallback width is re-measured, got {after}"
        );
    }
}
