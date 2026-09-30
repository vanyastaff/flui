//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use flui_painting::TextPainter;
use flui_painting::typography::{TextDirection, TextSpan};

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

/// When the ellipsis is wider than the text's narrowest run, skipping
/// truncation must not under-report min intrinsic below the ellipsis
/// (Codex review on #1089).
pub(crate) fn wide_ellipsis_floors_min_intrinsic_width() {
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

/// An empty paragraph measures the line box and baseline a line of text in
/// the same style measures, so an empty `Text` in a baseline-aligned row
/// sits on its neighbours' baseline (painting mapping decision 15). Every
/// root property that shapes the line counts, not only the font size the
/// painter passes separately: a line height and a family (the probe face,
/// whose line metrics are not Roboto's) each change the empty line too.
pub(crate) fn an_empty_paragraph_measures_a_line_of_its_style() {
    use flui_painting::TextBaseline;
    use flui_painting::typography::{FontWeight, TextStyle};

    const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");

    let fonts = flui_painting::FontCollection::new();
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    let probe = |size: f64| TextStyle {
        font_family: Some("FLUI Probe Mono".to_owned()),
        font_weight: Some(FontWeight::W100),
        font_size: Some(size),
        ..TextStyle::default()
    };
    let rows = [
        TextStyle {
            font_size: Some(14.0),
            ..TextStyle::default()
        },
        TextStyle {
            font_size: Some(32.0),
            ..TextStyle::default()
        },
        TextStyle {
            font_size: Some(14.0),
            height: Some(2.0),
            ..TextStyle::default()
        },
        probe(20.0),
        TextStyle {
            height: Some(2.0),
            ..probe(20.0)
        },
    ];
    let mut failures = Vec::new();
    for style in rows {
        let measure = |text: &str| {
            let mut painter = TextPainter::new()
                .with_text(TextSpan::styled(text, style.clone()))
                .with_text_direction(TextDirection::Ltr);
            painter.layout(
                &mut flui_painting::TextContext::new(&fonts),
                0.0,
                f64::INFINITY,
            );
            (
                painter.height(),
                painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
            )
        };
        let (empty, line) = (measure(""), measure("A"));
        if (empty.0 - line.0).abs() > 1e-3 || (empty.1 - line.1).abs() > 1e-3 {
            failures.push(format!(
                "{style:?}: empty {empty:?}, one line {line:?} (height, baseline)"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Measurement is Parley on the lent context in the default build: the probe
/// face is registered only on a collection, never on the process font system,
/// so a painter measuring on cosmic-text would never see it.
pub(crate) mod parley_measurement {
    use flui_painting::parley_text::ParagraphSpec;
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
        TextPainter::new()
            .with_text(TextSpan::styled("AAAA", probe_style()))
            .with_text_direction(TextDirection::Ltr)
    }

    /// The same painter measures through whichever context it is lent: the
    /// probe width through the realm whose collection holds the face, the
    /// fallback width through one that does not, and on the first exactly
    /// what that context shapes the paragraph to.
    pub(crate) fn measurement_follows_the_context_it_is_given() {
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
                ellipsis: None,
            })
            .metrics();
        painter.layout(&mut a, 0.0, WIDTH);
        assert!((painter.size().width - shaped.width).abs() < f64::EPSILON);
        assert!((painter.size().height - shaped.height).abs() < f64::EPSILON);
    }

    /// The intrinsic widths a layout cached answer only for the context that
    /// laid out: asked through a context over another collection, or after
    /// a face was registered on the same one, the painter measures again.
    pub(crate) fn intrinsic_widths_follow_the_context_they_are_asked_through() {
        let with_probe = FontCollection::new();
        with_probe
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let mut a = TextContext::new(&with_probe);
        let mut b = TextContext::new(&FontCollection::new());
        let mut painter = probe_painter();

        painter.layout(&mut a, 0.0, WIDTH);
        let max_through_a = painter.max_intrinsic_width(&mut a);
        let min_through_a = painter.min_intrinsic_width(&mut a);
        let max_through_b = painter.max_intrinsic_width(&mut b);
        let min_through_b = painter.min_intrinsic_width(&mut b);

        assert!(
            (max_through_a - 4.0 * SIZE).abs() < 0.01,
            "four one-em `A`s through A's context are {} px, got {max_through_a}",
            4.0 * SIZE
        );
        assert!(
            (max_through_b - max_through_a).abs() > 1.0,
            "B lacks the face, so its max intrinsic width is the fallback's: A {max_through_a} vs B {max_through_b}"
        );
        assert!(
            (min_through_b - min_through_a).abs() > 1.0,
            "B lacks the face, so its min intrinsic width is the fallback's: A {min_through_a} vs B {min_through_b}"
        );

        let later = FontCollection::new();
        let mut c = TextContext::new(&later);
        painter.layout(&mut c, 0.0, WIDTH);
        let before = painter.max_intrinsic_width(&mut c);
        later
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let after = painter.max_intrinsic_width(&mut c);
        assert!(
            (after - 4.0 * SIZE).abs() < 0.01,
            "a face registered since the layout is measured: before {before}, got {after}"
        );
    }

    /// A face registered on the collection after the painter measured makes
    /// the next layout measure again under the same constraints.
    pub(crate) fn a_registration_on_the_collection_invalidates_the_painter_cache() {
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
