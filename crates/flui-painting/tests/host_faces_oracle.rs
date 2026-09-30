//! Measurement against paint on the host's own faces (ADR-0092 §7).
//!
//! The collection is the one the app builds, fed from a scan of the host's
//! fonts (`FontCollection::with_host_fonts`); the painter measures, paints
//! and places carets on Parley over it, so what is measured is by identity
//! what is painted. What the host decides is whether each row finds a face:
//! the style's family is resolved by the family rule and fallen back past in
//! the host's order, CJK, emoji and a family chain the host only partly
//! carries included.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_painting::testing::{
    collection_holds, host_chain_covers, host_covers, host_family_names, host_sans_serif_family,
};
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, HostFonts, ShapedParagraph, TextContext, TextPainter,
};

const LATIN: &str = "Hamburgefonstiv 0123";
const SIZES: [f64; 2] = [16.0, 32.0];

/// Cupertino's text chain: a family no host carries, then the system-font
/// names each platform aliases, then the generic that ends it.
fn system_ui_chain(weight: FontWeight) -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_owned()),
        font_family_fallback: [
            "-apple-system",
            "system-ui",
            "Segoe UI",
            "Helvetica Neue",
            "Arial",
            "sans-serif",
        ]
        .map(str::to_owned)
        .to_vec(),
        font_weight: Some(weight),
        ..TextStyle::default()
    }
}

fn weighted(family: Option<&str>, weight: FontWeight) -> TextStyle {
    TextStyle {
        font_family: family.map(str::to_owned),
        font_weight: Some(weight),
        ..TextStyle::default()
    }
}

/// A host family other than the sans-serif generic's, spelled in lower case
/// where that differs from how the host names it: both shapers match family
/// names exactly, as fontdb does, so both degrade it to the sans-serif
/// generic rather than one finding the family and the other not.
fn mis_cased_family(host: &HostFonts) -> Option<String> {
    let sans_serif = host_sans_serif_family(host);
    let names = host_family_names(host);
    names
        .iter()
        .filter(|name| **name != sans_serif)
        .map(|name| name.to_lowercase())
        .find(|lower| !names.contains(lower))
}

/// A text family the host carries under its own name, spelled exactly: the
/// host's UI face where it has a well-known one, else any family other than
/// the sans-serif generic's. Measured and painted in that host face.
fn named_host_family(host: &HostFonts) -> Option<String> {
    let names = host_family_names(host);
    let sans_serif = host_sans_serif_family(host);
    ["Segoe UI", "DejaVu Sans", "Helvetica", "Arial", "Noto Sans"]
        .into_iter()
        .map(str::to_owned)
        .find(|name| names.contains(name))
        .or_else(|| names.into_iter().find(|name| *name != sans_serif))
}

/// The rows: a name, the style, the text.
fn rows(host: &HostFonts) -> Vec<(&'static str, TextStyle, &'static str)> {
    let mis_cased =
        mis_cased_family(host).expect("the host names a family with an upper-case letter");
    let named = named_host_family(host).expect("the host carries a family of its own");
    vec![
        (
            "mis_cased_family",
            weighted(Some(&mis_cased), FontWeight::W400),
            LATIN,
        ),
        (
            "named_host_family",
            weighted(Some(&named), FontWeight::W400),
            LATIN,
        ),
        ("latin_default", weighted(None, FontWeight::W400), LATIN),
        ("latin_bold", weighted(None, FontWeight::W700), LATIN),
        (
            "monospace",
            weighted(Some("monospace"), FontWeight::W400),
            LATIN,
        ),
        ("system_ui_chain", system_ui_chain(FontWeight::W400), LATIN),
        (
            "system_ui_chain_semibold",
            system_ui_chain(FontWeight::W600),
            LATIN,
        ),
        ("cjk", TextStyle::default(), "你好世界"),
        ("emoji", TextStyle::default(), "😀"),
        ("mixed", TextStyle::default(), "你好世界 emoji 😀"),
    ]
}

/// The paragraph `painter` records.
fn painted(painter: &TextPainter) -> Arc<ShapedParagraph> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// `(measured, painted)` widths and heights of `text` in `style` at `size`,
/// and how many glyphs painted as `.notdef` (glyph 0), the glyph a run with
/// no face for its text shapes to.
fn measure_and_paint(
    context: &mut TextContext,
    style: &TextStyle,
    text: &str,
    size: f64,
) -> ((f64, f64), (f64, f64), usize) {
    let style = TextStyle {
        font_size: Some(size),
        ..style.clone()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(text, style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(context, 0.0, f64::INFINITY);
    let paragraph = painted(&painter);
    let notdef = paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter())
        .filter(|glyph| glyph.id == 0)
        .count();
    (
        (painter.width(), painter.height()),
        (paragraph.size().width, paragraph.size().height),
        notdef,
    )
}

/// Every row the host covers paints what it measured, and paints no glyph as
/// `.notdef`. The `.notdef` count is the assertion that can fail: it fails on
/// a collection holding only the bundled faces (`FontCollection::new()`),
/// where CJK and emoji have no face to paint in. Measured and painted size
/// come from one cached layout, so their comparison guards only against a
/// paint path that stops reading it.
#[test]
fn measured_width_equals_painted_width_on_host_faces() {
    let host = HostFonts::scan();
    let fonts = FontCollection::with_host_fonts(&host);
    let mut context = TextContext::new(&fonts);
    let mut failures = Vec::new();
    let mut latin_rows = 0;
    for (name, style, text) in rows(&host) {
        if !host_chain_covers(&host, text) {
            let missing: Vec<_> = text
                .chars()
                .filter(|c| !c.is_whitespace() && !host_chain_covers(&host, &c.to_string()))
                .map(|c| {
                    let reach = if host_covers(&host, &c.to_string()) {
                        "is covered only past the fallback chain"
                    } else {
                        "is covered by no host face"
                    };
                    format!("U+{:04X} {reach}", u32::from(c))
                })
                .collect();
            println!("{name}: skipped, {}", missing.join("; "));
            continue;
        }
        if text == LATIN {
            latin_rows += 1;
        }
        for size in SIZES {
            let ((width, height), (painted_width, painted_height), notdef) =
                measure_and_paint(&mut context, &style, text, size);
            println!(
                "{name} at {size} px: measured {width:.2} x {height:.2}, painted \
                 {painted_width:.2} x {painted_height:.2}, {notdef} .notdef"
            );
            if (width - painted_width).abs() > 1e-9
                || (height - painted_height).abs() > 1e-9
                || notdef > 0
            {
                failures.push(format!(
                    "{name} at {size} px: measured {width:.2} x {height:.2}, painted \
                     {painted_width:.2} x {painted_height:.2}, {notdef} .notdef"
                ));
            }
        }
    }
    assert_eq!(latin_rows, 7, "every Latin row runs on any host");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every family the host scan found is a family of the fed collection:
/// fontdb and fontique read the same files' names alike, so a style naming a
/// host family by the name the scan reports finds it in the collection.
#[test]
fn every_family_the_host_scan_finds_resolves_in_the_collection() {
    let host = HostFonts::scan();
    let fonts = FontCollection::with_host_fonts(&host);
    let names = host_family_names(&host);
    assert!(!names.is_empty(), "the host has fonts");
    let missing: Vec<_> = names
        .iter()
        .filter(|name| !collection_holds(&fonts, name))
        .collect();
    assert!(
        missing.is_empty(),
        "{} of {} families are not in the collection: {missing:?}",
        missing.len(),
        names.len()
    );
}
