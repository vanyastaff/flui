//! A text context keeps the paragraphs it lays out a second time, and a
//! third layout of the same spec shares the kept layout instead of shaping.

use flui_foundation::TextDirection;
use flui_foundation::memory::MemoryPressureLevel;
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::styling::Color;
use flui_painting::testing::text_context_shapings;
use flui_painting::typography::{FontWeight, TextAlign, TextSpan, TextStyle};

use flui_painting::{FontCollection, TextContext, TextLayoutResult, TextPainter};

const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
const TEXT: &str = "Kept paragraphs are shared, not shaped again.";

fn fonts() -> FontCollection {
    let fonts = FontCollection::new();
    fonts.register_font(ROBOTO).expect("the fixture font loads");
    fonts
}

fn spec(spans: &[(String, Option<TextStyle>)]) -> ParagraphSpec<'_> {
    ParagraphSpec {
        spans,
        default_style: None,
        font_size: 16.0,
        font_weight_adjustment: 0,
        max_width: Some(400.0),
        min_width: 0.0,
        text_align: TextAlign::Start,
        line_height: None,
        direction: TextDirection::Ltr,
        max_lines: None,
        ellipsis: None,
    }
}

fn plain(text: &str) -> Vec<(String, Option<TextStyle>)> {
    vec![(text.to_owned(), None)]
}

/// Lays `spec` out and returns its metrics and how many shapings it took.
fn lay_out(context: &mut TextContext, spec: &ParagraphSpec<'_>) -> (TextLayoutResult, u64) {
    let before = text_context_shapings(context);
    let layout = context.shape(spec).expect("the fixture spec shapes");
    let metrics = layout.metrics();
    (metrics, text_context_shapings(context) - before)
}

fn extent(metrics: &TextLayoutResult) -> (f64, f64, usize) {
    (metrics.width, metrics.height, metrics.line_count)
}

/// Lays `spec` out twice, so the context keeps it.
fn keep(context: &mut TextContext, spec: &ParagraphSpec<'_>) {
    for _ in 0..2 {
        lay_out(context, spec);
    }
}

/// The first two layouts shape; the third shares the kept layout, which
/// measures what the shaped one did, after another paragraph came between.
pub(crate) fn a_paragraph_laid_out_a_third_time_is_not_shaped() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let paragraph = spec(&spans);
    let (first, shaped_first) = lay_out(&mut context, &paragraph);
    let (_, shaped_second) = lay_out(&mut context, &paragraph);
    let other = plain("Another paragraph between them, on two lines at this width.");
    let mut narrow = spec(&other);
    narrow.max_width = Some(120.0);
    let (between, _) = lay_out(&mut context, &narrow);
    let (third, shaped_third) = lay_out(&mut context, &paragraph);
    assert_eq!(
        (shaped_first, shaped_second, shaped_third),
        (1, 1, 0),
        "a paragraph is kept on its second layout and shared on its third"
    );
    assert_eq!(
        extent(&third),
        extent(&first),
        "the kept layout measures as the shaped one"
    );
    assert_ne!(
        extent(&between),
        extent(&third),
        "the kept layout is the paragraph's own, not the last one laid out"
    );
}

/// A layout seen once is not kept: a paragraph that comes back only after
/// more distinct ones than the context remembers is shaped every time.
pub(crate) fn a_paragraph_seen_once_is_not_kept() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let paragraph = spec(&spans);
    let rows: Vec<_> = (0..100).map(|row| plain(&format!("Row {row}"))).collect();
    for _ in 0..3 {
        let (_, shaped) = lay_out(&mut context, &paragraph);
        assert_eq!(
            shaped, 1,
            "a paragraph between a hundred others is shaped again"
        );
        for row in &rows {
            lay_out(&mut context, &spec(row));
        }
    }
}

/// Each field of the spec is part of what a kept paragraph is found by: a
/// spec that differs from a kept one in any field is shaped.
pub(crate) fn every_field_of_the_spec_keys_a_kept_paragraph() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let base = spec(&spans);
    keep(&mut context, &base);
    assert_eq!(lay_out(&mut context, &base).1, 0, "the base spec is kept");

    let bold = TextStyle {
        font_weight: Some(FontWeight::W700),
        ..Default::default()
    };
    let red = TextStyle::default().with_color(Color::rgb(255, 0, 0));
    let styled = vec![(TEXT.to_owned(), Some(bold.clone()))];
    let recolored = vec![(TEXT.to_owned(), Some(red))];
    let (head, tail) = TEXT.split_at(4);
    let split = vec![(head.to_owned(), None), (tail.to_owned(), None)];
    let variants: [(&str, ParagraphSpec<'_>); 13] = [
        (
            "max_width",
            ParagraphSpec {
                max_width: Some(200.0),
                ..base
            },
        ),
        (
            "min_width",
            ParagraphSpec {
                min_width: 500.0,
                ..base
            },
        ),
        (
            "text_align",
            ParagraphSpec {
                text_align: TextAlign::Center,
                ..base
            },
        ),
        (
            "line_height",
            ParagraphSpec {
                line_height: Some(30.0),
                ..base
            },
        ),
        (
            "direction",
            ParagraphSpec {
                direction: TextDirection::Rtl,
                ..base
            },
        ),
        (
            "max_lines",
            ParagraphSpec {
                max_lines: Some(1),
                ..base
            },
        ),
        (
            "ellipsis",
            ParagraphSpec {
                ellipsis: Some("…"),
                ..base
            },
        ),
        (
            "font_size",
            ParagraphSpec {
                font_size: 17.0,
                ..base
            },
        ),
        (
            "font_weight_adjustment",
            ParagraphSpec {
                font_weight_adjustment: 100,
                ..base
            },
        ),
        (
            "default_style",
            ParagraphSpec {
                default_style: Some(&bold),
                ..base
            },
        ),
        (
            "span style",
            ParagraphSpec {
                spans: &styled,
                ..base
            },
        ),
        (
            "span colour",
            ParagraphSpec {
                spans: &recolored,
                ..base
            },
        ),
        (
            "span boundaries",
            ParagraphSpec {
                spans: &split,
                ..base
            },
        ),
    ];
    for (field, variant) in variants {
        assert!(
            lay_out(&mut context, &variant).1 > 0,
            "a spec that differs in {field} is shaped, not found"
        );
    }
}

/// A face registered on the collection may shape the same text in another
/// face: the kept paragraphs are dropped and the next layout shapes.
pub(crate) fn a_registered_face_drops_the_kept_paragraphs() {
    let fonts = fonts();
    let mut context = TextContext::new(&fonts);
    let spans = plain(TEXT);
    let paragraph = spec(&spans);
    keep(&mut context, &paragraph);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        0,
        "the paragraph is kept"
    );
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe font loads");
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        1,
        "a registration drops what was shaped on the fonts before it"
    );
}

/// Memory pressure drops the kept paragraphs with the other rebuildable
/// buffers.
pub(crate) fn reclaim_drops_the_kept_paragraphs() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let paragraph = spec(&spans);
    keep(&mut context, &paragraph);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        0,
        "the paragraph is kept"
    );
    context.reclaim(MemoryPressureLevel::Moderate);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        1,
        "reclaim drops the kept paragraphs"
    );
}

/// A paragraph past the size an entry may have is shaped every time.
pub(crate) fn a_long_paragraph_is_not_kept() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(&TEXT.repeat(40));
    let paragraph = spec(&spans);
    keep(&mut context, &paragraph);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        1,
        "a paragraph of 1800 characters is not kept"
    );
}

/// The painting a kept paragraph remembers counts toward the size an entry
/// may have: 450 characters lay out within it, and their glyphs painted take
/// them past it, so they are shaped every time.
pub(crate) fn a_paragraph_whose_painting_passes_the_entry_size_is_not_kept() {
    let mut context = TextContext::new(&fonts());
    let text = TEXT.chars().cycle().take(450).collect::<String>();
    let spans = plain(&text);
    let paragraph = spec(&spans);
    keep(&mut context, &paragraph);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        1,
        "a painted paragraph of 450 characters is not kept"
    );
}

/// Past the count the context keeps, the paragraph used longest ago leaves
/// first, and the one used since stays.
pub(crate) fn the_least_recently_used_paragraph_leaves_first() {
    let mut context = TextContext::new(&fonts());
    let rows: Vec<_> = (0..130).map(|row| plain(&format!("Row {row}"))).collect();
    for row in &rows[..128] {
        keep(&mut context, &spec(row));
    }
    assert_eq!(
        lay_out(&mut context, &spec(&rows[0])).1,
        0,
        "the oldest row is used again"
    );
    keep(&mut context, &spec(&rows[128]));
    keep(&mut context, &spec(&rows[129]));
    for kept in [0, 3, 127, 128, 129] {
        assert_eq!(
            lay_out(&mut context, &spec(&rows[kept])).1,
            0,
            "row {kept} stayed: it was used after rows 1 and 2"
        );
    }
    for left in [1, 2] {
        assert_eq!(
            lay_out(&mut context, &spec(&rows[left])).1,
            1,
            "row {left} left: the two rows past the count push out the two used longest ago"
        );
    }
}

/// A context that has shaped nothing counts no shaping, and its first layout
/// counts one.
pub(crate) fn a_fresh_context_counts_its_first_shaping() {
    let mut context = TextContext::new(&fonts());
    assert_eq!(text_context_shapings(&context), 0, "nothing shaped yet");
    let spans = plain(TEXT);
    lay_out(&mut context, &spec(&spans));
    assert_eq!(
        text_context_shapings(&context),
        1,
        "one paragraph shaped once"
    );
}

/// Two paragraphs laid out in turn, a label toggling between two strings,
/// are both kept and both shared from their third layout on.
pub(crate) fn two_paragraphs_in_turn_are_both_kept() {
    let mut context = TextContext::new(&fonts());
    let first = plain("label 1");
    let second = plain("label 2");
    for _ in 0..2 {
        lay_out(&mut context, &spec(&first));
        lay_out(&mut context, &spec(&second));
    }
    assert_eq!(
        (
            lay_out(&mut context, &spec(&first)).1,
            lay_out(&mut context, &spec(&second)).1
        ),
        (0, 0),
        "both strings are kept"
    );
}

/// A kept paragraph whose lines were cut at an ellipsis is shared with the
/// cut: it measures and reports truncation as the shaped one did.
pub(crate) fn a_kept_ellipsized_paragraph_stays_truncated() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let paragraph = ParagraphSpec {
        max_width: Some(120.0),
        max_lines: Some(1),
        ellipsis: Some("…"),
        ..spec(&spans)
    };
    let (shaped, _) = lay_out(&mut context, &paragraph);
    lay_out(&mut context, &paragraph);
    let (kept, shapings) = lay_out(&mut context, &paragraph);
    assert_eq!(shapings, 0, "the ellipsized paragraph is kept");
    assert!(shaped.truncated, "the fixture is cut at the ellipsis");
    assert_eq!(
        (kept.truncated, extent(&kept)),
        (shaped.truncated, extent(&shaped)),
        "the kept paragraph is cut where the shaped one was"
    );
}

/// The bytes bound evicts before the count does: paragraphs of a few
/// hundred characters fill it with far fewer than 128 entries, and the
/// first of them leaves. Each is small enough to be kept on its own.
pub(crate) fn the_bytes_bound_evicts_before_the_count() {
    let mut context = TextContext::new(&fonts());
    let paragraphs: Vec<_> = (0..64)
        .map(|index| plain(&format!("{index} {}", TEXT.repeat(8))))
        .collect();
    let first = spec(&paragraphs[0]);
    keep(&mut context, &first);
    assert_eq!(
        lay_out(&mut context, &first).1,
        0,
        "a 370-character paragraph is kept"
    );
    for paragraph in &paragraphs[1..] {
        keep(&mut context, &spec(paragraph));
    }
    for recent in [63, 62] {
        assert_eq!(
            lay_out(&mut context, &spec(&paragraphs[recent])).1,
            0,
            "paragraph {recent} stayed: an eviction gives back the bytes of what it drops"
        );
    }
    assert_eq!(
        lay_out(&mut context, &first).1,
        1,
        "64 such paragraphs pass the bytes bound, and the oldest left"
    );
}

/// A painter laid out again after its text changed builds into the layout it
/// replaces: the second layout allocates less than the first did on the
/// same, already warm, context.
pub(crate) fn a_relayout_builds_into_the_layout_it_replaces() {
    let mut context = TextContext::new(&fonts());
    let painter = |text: &str| {
        TextPainter::new()
            .with_text(TextSpan::new(text))
            .with_text_direction(TextDirection::Ltr)
    };
    let mut warm = painter("Warms the context's fonts and scratch.");
    warm.layout(&mut context, 0.0, 400.0)
        .expect("the fixture lays out");
    let mut subject = painter("First text, laid out into a new layout.");
    let (fresh, ()) = crate::foreign_image::allocations_during(|| {
        subject
            .layout(&mut context, 0.0, 400.0)
            .expect("the fixture lays out");
    });
    subject.set_text(Some(
        TextSpan::new("Second text, into the first one's.").into(),
    ));
    let (reused, ()) = crate::foreign_image::allocations_during(|| {
        subject
            .layout(&mut context, 0.0, 400.0)
            .expect("the fixture lays out");
    });
    assert!(
        reused + 8 <= fresh,
        "a relayout into the replaced layout asked the heap for {reused} blocks, a fresh one {fresh}"
    );
}

/// A paragraph that comes back after 40 others is still remembered from its
/// first layout, and kept on its second.
pub(crate) fn a_paragraph_back_after_forty_others_is_kept() {
    let mut context = TextContext::new(&fonts());
    let spans = plain(TEXT);
    let paragraph = spec(&spans);
    let rows: Vec<_> = (0..40).map(|row| plain(&format!("Row {row}"))).collect();
    lay_out(&mut context, &paragraph);
    for row in &rows {
        lay_out(&mut context, &spec(row));
    }
    lay_out(&mut context, &paragraph);
    assert_eq!(
        lay_out(&mut context, &paragraph).1,
        0,
        "the second layout, 40 paragraphs after the first, kept it"
    );
}

/// A layout the cache drops, held by nothing else, is what the next
/// paragraph shaped is built into: that shaping allocates less than the one
/// after it, which finds no layout to build into.
pub(crate) fn an_evicted_layout_is_built_into() {
    let mut context = TextContext::new(&fonts());
    let rows: Vec<_> = (0..129).map(|row| plain(&format!("Row {row}"))).collect();
    for row in &rows {
        keep(&mut context, &spec(row));
    }
    let after_eviction = plain("A paragraph laid out once, after an eviction.");
    let without_spare = plain("A paragraph laid out once, with no spare left.");
    let (into_evicted, _) = crate::foreign_image::allocations_during(|| {
        context
            .shape(&spec(&after_eviction))
            .expect("the fixture shapes")
    });
    let (fresh, _) = crate::foreign_image::allocations_during(|| {
        context
            .shape(&spec(&without_spare))
            .expect("the fixture shapes")
    });
    assert!(
        into_evicted + 5 <= fresh,
        "a shaping into the evicted layout asked the heap for {into_evicted} blocks, a fresh one {fresh}"
    );
}

/// The paragraph `painter` paints, as its display list carries it.
fn painted(painter: &TextPainter) -> std::sync::Arc<flui_painting::ShapedParagraph> {
    use flui_foundation::geometry::Offset;
    use flui_painting::{Canvas, DrawOp};
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(std::sync::Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter paints its paragraph")
}

/// A painter of `span`, laid out on `context` at 400 pixels.
fn laid_out(context: &mut TextContext, span: TextSpan) -> TextPainter {
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(context, 0.0, 400.0)
        .expect("the fixture lays out");
    painter
}

/// The painter whose layout the context keeps paints the glyphs it built,
/// and every later painter of that paragraph paints those same glyphs
/// instead of collecting them from the layout again; the painter before the
/// paragraph was kept built its own.
pub(crate) fn the_painters_of_a_kept_paragraph_share_its_glyphs() {
    let mut context = TextContext::new(&fonts());
    let first = laid_out(&mut context, TextSpan::new(TEXT));
    let kept = laid_out(&mut context, TextSpan::new(TEXT));
    let shared = laid_out(&mut context, TextSpan::new(TEXT));
    assert!(
        std::sync::Arc::ptr_eq(&painted(&kept), &painted(&shared)),
        "a layout of a kept paragraph paints the glyphs its first painting built"
    );
    assert!(
        !std::sync::Arc::ptr_eq(&painted(&first), &painted(&kept)),
        "a paragraph laid out once is not kept, and its painter's glyphs are its own"
    );
    let glyphs = |painter: &TextPainter| -> Vec<flui_painting::ShapedGlyph> {
        painted(painter)
            .runs()
            .flat_map(|run| run.glyphs().to_vec())
            .collect()
    };
    assert_eq!(
        glyphs(&first),
        glyphs(&shared),
        "the shared glyphs are the ones the layout gives"
    );
}

/// A root colour is part of what keys a kept paragraph, through the root
/// style its spans inherit: a painter under another root over the same span
/// lays its own paragraph out and paints its own glyphs, and a painter under
/// the kept root shares them.
pub(crate) fn a_kept_paragraph_under_another_root_colour_is_painted_apart() {
    let mut context = TextContext::new(&fonts());
    let blue = TextStyle::default().with_color(Color::rgb(0, 0, 255));
    let under = |root: Color| {
        TextSpan::with_children(vec![TextSpan::styled(TEXT, blue.clone())])
            .with_style(TextStyle::default().with_color(root))
    };
    let red = Color::rgb(255, 0, 0);
    laid_out(&mut context, under(red));
    let kept = laid_out(&mut context, under(red));
    let before = text_context_shapings(&context);
    let other_root = laid_out(&mut context, under(Color::rgb(0, 255, 0)));
    assert_eq!(
        text_context_shapings(&context) - before,
        1,
        "another root colour is another paragraph to the cache"
    );
    let same_root = laid_out(&mut context, under(red));
    assert!(
        std::sync::Arc::ptr_eq(&painted(&kept), &painted(&same_root)),
        "under the root it was painted with, the kept paragraph's glyphs are shared"
    );
    assert!(
        !std::sync::Arc::ptr_eq(&painted(&kept), &painted(&other_root)),
        "under another root the paragraph is painted apart"
    );
}
