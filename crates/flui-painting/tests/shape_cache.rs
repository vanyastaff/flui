//! A text context keeps the paragraphs it lays out a second time, and a
//! third layout of the same spec shares the kept layout instead of shaping.

use flui_foundation::TextDirection;
use flui_foundation::memory::MemoryPressureLevel;
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::styling::Color;
use flui_painting::testing::text_context_shapings;
use flui_painting::typography::{FontWeight, TextAlign, TextStyle};
use flui_painting::{FontCollection, TextContext, TextLayoutResult};

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

/// Past the count the context keeps, the paragraph used longest ago leaves
/// first, and the one used since stays.
pub(crate) fn the_least_recently_used_paragraph_leaves_first() {
    let mut context = TextContext::new(&fonts());
    let rows: Vec<_> = (0..200).map(|row| plain(&format!("Row {row}"))).collect();
    let first = spec(&rows[0]);
    let second = spec(&rows[1]);
    keep(&mut context, &first);
    keep(&mut context, &second);
    for row in &rows[2..120] {
        keep(&mut context, &spec(row));
    }
    assert_eq!(lay_out(&mut context, &first).1, 0, "the first row is kept");
    for row in &rows[120..] {
        keep(&mut context, &spec(row));
    }
    assert_eq!(
        lay_out(&mut context, &second).1,
        1,
        "the row used longest ago left"
    );
    assert_eq!(
        lay_out(&mut context, &first).1,
        0,
        "the row used since stayed"
    );
}
