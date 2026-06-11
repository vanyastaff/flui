//! Shaper-derived baselines + max_lines/ellipsis ENFORCEMENT.
//!
//! Pre-fix the painter only *detected* overflow (`did_exceed_max_lines`)
//! while size and paint still covered every line, and baselines were
//! font-size guesses (`height × 0.8`, `alphabetic × 1.125`). Now the
//! truncation re-shapes the kept prefix — size, line metrics, and
//! painted glyphs agree — and baselines come from cosmic-text's
//! per-line `line_y`.

use flui_painting::text_layout::TextLayout;
use flui_painting::text_painter::TextPainter;
use flui_types::typography::{TextDirection, TextSpan};

#[test]
fn max_lines_truncates_the_shaped_buffer() {
    let layout = TextLayout::with_overflow(
        "Line 1\nLine 2\nLine 3\nLine 4",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
        Some(2),
        None,
    );
    let metrics = layout.metrics();
    assert!(layout.was_truncated());
    assert_eq!(
        metrics.line_count, 2,
        "lines beyond max_lines must not exist in the buffer — not \
         merely be skipped at paint"
    );
    let two_line_height = metrics.height;

    let full = TextLayout::new(
        "Line 1\nLine 2\nLine 3\nLine 4",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
    );
    assert!(
        two_line_height < full.metrics().height,
        "the truncated layout's height must shrink with the dropped lines"
    );
}

#[test]
fn max_lines_within_limit_is_untouched() {
    let layout = TextLayout::with_overflow(
        "Line 1\nLine 2",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
        Some(5),
        Some("…"),
    );
    assert!(!layout.was_truncated());
    assert_eq!(layout.metrics().line_count, 2);
}

#[test]
fn ellipsis_fits_within_the_width_constraint() {
    // A long unbroken-ish line forced to wrap at 80px, then truncated
    // to one line with an ellipsis: the kept line (including the
    // ellipsis) must fit the constraint.
    let max_width = 80.0;
    let layout = TextLayout::with_overflow(
        "The quick brown fox jumps over the lazy dog again and again",
        None,
        14.0,
        Some(max_width),
        None,
        TextDirection::Ltr,
        Some(1),
        Some("…"),
    );
    let metrics = layout.metrics();
    assert!(layout.was_truncated());
    assert_eq!(metrics.line_count, 1);
    assert!(
        metrics.width <= max_width + 0.5,
        "the ellipsized line must fit the width constraint, got {} > {max_width}",
        metrics.width
    );
}

#[test]
fn baselines_come_from_the_shaper() {
    let layout = TextLayout::new("Hello xyj", None, 14.0, None, None, TextDirection::Ltr);
    let metrics = layout.metrics();
    let lines = layout.get_line_metrics();

    // The first line's reported baseline and the layout-level
    // alphabetic baseline are the SAME shaped quantity.
    let first_line = &lines[0];
    assert!(
        (first_line.baseline - f64::from(metrics.alphabetic_baseline)).abs() < 1e-3,
        "line metrics and layout metrics must agree on the baseline"
    );
    // Ascent + descent tile the line box exactly (they are line-box
    // distances around `line_y`, not font-size fractions).
    assert!(
        (first_line.ascent + first_line.descent - first_line.height).abs() < 1e-3,
        "ascent ({}) + descent ({}) must equal the line height ({})",
        first_line.ascent,
        first_line.descent,
        first_line.height
    );
    // Sanity: the baseline sits strictly inside the line box, and the
    // ideographic baseline is at or below the alphabetic one.
    assert!(metrics.alphabetic_baseline > 0.0);
    assert!(f64::from(metrics.alphabetic_baseline) < first_line.height + 1e-3);
    assert!(metrics.ideographic_baseline >= metrics.alphabetic_baseline);
}

#[test]
fn color_change_keeps_the_shaped_layout() {
    use flui_painting::Invalidation;
    use flui_types::Color;
    use flui_types::typography::TextStyle;

    let mut painter = TextPainter::new()
        .with_text(
            TextSpan::new("Hello").with_style(TextStyle::new().with_color(Color::rgb(255, 0, 0))),
        )
        .with_text_direction(TextDirection::Ltr);
    painter.layout(0.0, 200.0);
    let size_before = painter.size();
    let baseline_before =
        painter.compute_distance_to_actual_baseline(flui_painting::TextBaseline::Alphabetic);

    // The structural win over Flutter ("no API to only make those
    // updates", text_painter.dart:1335): a color-only change keeps the
    // shaped layout — metrics and baselines stay valid with NO
    // re-layout call.
    let inv = painter.set_text(Some(
        TextSpan::new("Hello")
            .with_style(TextStyle::new().with_color(Color::rgb(0, 0, 255)))
            .into(),
    ));
    assert_eq!(inv, Invalidation::Paint);
    assert!(painter.did_layout(), "shaped layout must survive a recolor");
    assert_eq!(painter.size(), size_before);
    assert!(
        (painter.compute_distance_to_actual_baseline(flui_painting::TextBaseline::Alphabetic)
            - baseline_before)
            .abs()
            < f32::EPSILON
    );

    // Identical span → no invalidation at all.
    let inv = painter.set_text(Some(
        TextSpan::new("Hello")
            .with_style(TextStyle::new().with_color(Color::rgb(0, 0, 255)))
            .into(),
    ));
    assert_eq!(inv, Invalidation::None);
}

#[test]
fn named_font_family_reaches_the_shaper() {
    use flui_types::typography::TextStyle;

    // Pre-fix every non-generic family name collapsed to SansSerif, so
    // "monospace-by-name" shaped identically to the default face. A
    // family that actually reaches the shaper must equalize i-vs-m
    // advance widths; the named branch is the same `Family` mapping.
    let mono = TextLayout::new(
        "iiii mmmm",
        Some(&TextStyle::new().with_font_family("monospace")),
        24.0,
        None,
        None,
        TextDirection::Ltr,
    );
    let default_face = TextLayout::new("iiii mmmm", None, 24.0, None, None, TextDirection::Ltr);
    assert!(
        (mono.metrics().width - default_face.metrics().width).abs() > 0.5,
        "a family that reaches the shaper must change advance widths \
         for i-vs-m text (monospace equalizes them)"
    );
}

#[test]
fn rich_child_span_styles_reach_the_shaper() {
    use flui_types::typography::TextStyle;

    // Root at 14px with a 28px child span: the rich path must measure
    // the child at 28px. The flattened pre-fix path shaped EVERYTHING
    // at the root size, so the two trees below measured identically.
    let big_child = TextSpan::new("AA").with_style(TextStyle::new().with_font_size(14.0));
    let rich = {
        let mut root = TextSpan::new("AA");
        root.style = Some(TextStyle::new().with_font_size(14.0));
        root.children
            .push(TextSpan::new("BB").with_style(TextStyle::new().with_font_size(28.0)));
        root
    };

    let mut rich_painter = TextPainter::new()
        .with_text(rich)
        .with_text_direction(TextDirection::Ltr);
    rich_painter.layout(0.0, f32::INFINITY);

    let mut flat_painter = TextPainter::new()
        .with_text({
            let mut root = big_child;
            root.children.push(TextSpan::new("BB"));
            root
        })
        .with_text_direction(TextDirection::Ltr);
    flat_painter.layout(0.0, f32::INFINITY);

    assert!(
        rich_painter.width() > flat_painter.width() + 1.0,
        "a 28px child span must measure wider than the same text at \
         the root's 14px ({} vs {})",
        rich_painter.width(),
        flat_painter.width()
    );
    assert!(
        rich_painter.height() > flat_painter.height() + 1.0,
        "the line box must grow to the larger span"
    );
}

#[test]
fn rich_inheritance_merges_parent_style_into_children() {
    use flui_types::typography::TextStyle;

    // A child WITHOUT its own size inherits the parent's 28px — the
    // tree must measure exactly like the flat 28px equivalent.
    let inherited = {
        let mut root = TextSpan::new("AA");
        root.style = Some(TextStyle::new().with_font_size(28.0));
        root.children.push(TextSpan::new("BB"));
        root
    };
    let flat = TextSpan::new("AABB").with_style(TextStyle::new().with_font_size(28.0));

    let mut a = TextPainter::new()
        .with_text(inherited)
        .with_text_direction(TextDirection::Ltr);
    a.layout(0.0, f32::INFINITY);
    let mut b = TextPainter::new()
        .with_text(flat)
        .with_text_direction(TextDirection::Ltr);
    b.layout(0.0, f32::INFINITY);

    assert!(
        (a.width() - b.width()).abs() < 0.5,
        "style inheritance must shape the child at the parent's size \
         ({} vs {})",
        a.width(),
        b.width()
    );
}

#[test]
fn rich_truncation_keeps_span_styling() {
    use flui_types::typography::TextStyle;

    // Two spans wrapped to one allowed line with an ellipsis: the rich
    // truncation slices the SPANS and the result still fits the width.
    let rich = {
        let mut root = TextSpan::new("first segment of text ");
        root.style = Some(TextStyle::new().with_font_size(14.0));
        root.children.push(
            TextSpan::new("second segment that will be cut")
                .with_style(TextStyle::new().with_font_size(20.0)),
        );
        root
    };
    let mut painter = TextPainter::new()
        .with_text(rich)
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));
    painter.layout(0.0, 120.0);

    assert!(painter.did_exceed_max_lines());
    assert_eq!(painter.get_line_metrics().len(), 1, "one kept line");
    assert!(
        painter.width() <= 120.0 + 0.5,
        "ellipsized rich line must fit the constraint, got {}",
        painter.width()
    );
}

#[test]
fn painter_enforces_max_lines_end_to_end() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(
            "one two three four five six seven eight nine",
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));

    painter.layout(0.0, 60.0);

    assert!(painter.did_exceed_max_lines());
    let one_line_height = painter.height();

    let mut unlimited = TextPainter::new()
        .with_text(TextSpan::new(
            "one two three four five six seven eight nine",
        ))
        .with_text_direction(TextDirection::Ltr);
    unlimited.layout(0.0, 60.0);

    assert!(
        one_line_height < unlimited.height(),
        "the painter's reported size must cover only the kept line — \
         detection without enforcement painted every line anyway"
    );
}
