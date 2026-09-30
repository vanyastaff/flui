//! Measurement against paint on the host's own faces (ADR-0092 §7).
//!
//! The collection is the one the app builds, fed from the process font
//! system (`FontCollection::with_host_faces`); the painter measures on Parley
//! over it and paints the cosmic-text layout the process font system shapes.
//! Both sides resolve the style's family by one rule and fall back past it in
//! one order, so the paragraph measured is the paragraph painted: CJK, emoji
//! and a family chain the host only partly carries included. Its own binary,
//! because it builds the process font system from the host's fonts.

use flui_foundation::geometry::Offset;
use flui_painting::display_list::DrawOp;
use flui_painting::testing::{
    collection_holds, host_chain_covers, host_covers, host_family_names, host_sans_serif_family,
};
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, FontCollection, TextContext, TextPainter, shared_font_system};

const LATIN: &str = "Hamburgefonstiv 0123";
const SIZES: [f64; 2] = [16.0, 32.0];
/// The same face and the same shaper input on both sides leave only float
/// accumulation between them.
const TOLERANCE: f64 = 0.05;

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
fn mis_cased_family() -> Option<String> {
    let sans_serif = host_sans_serif_family();
    host_family_names()
        .into_iter()
        .filter(|name| *name != sans_serif)
        .map(|name| name.to_lowercase())
        .find(|lower| !host_family_names().contains(lower))
}

/// A text family the host carries under its own name, spelled exactly: the
/// host's UI face where it has a well-known one, else any family other than
/// the sans-serif generic's. Measured and painted in that host face.
fn named_host_family() -> Option<String> {
    let names = host_family_names();
    let sans_serif = host_sans_serif_family();
    ["Segoe UI", "DejaVu Sans", "Helvetica", "Arial", "Noto Sans"]
        .into_iter()
        .map(str::to_owned)
        .find(|name| names.contains(name))
        .or_else(|| names.into_iter().find(|name| *name != sans_serif))
}

/// The rows: a name, the style, the text.
fn rows() -> Vec<(&'static str, TextStyle, &'static str)> {
    let mis_cased = mis_cased_family().expect("the host names a family with an upper-case letter");
    let named = named_host_family().expect("the host carries a family of its own");
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

/// `(measured, painted)` widths and heights of `text` in `style` at `size`.
fn measure_and_paint(
    context: &mut TextContext,
    style: &TextStyle,
    text: &str,
    size: f64,
) -> ((f64, f64), (f64, f64)) {
    let style = TextStyle {
        font_size: Some(size),
        ..style.clone()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(text, style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(context, 0.0, f64::INFINITY);
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    let painted = list
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { layout, .. } => Some(layout.metrics()),
            _ => None,
        })
        .expect("paint records the paragraph");
    (
        (painter.width(), painter.height()),
        (painted.width, painted.height),
    )
}

/// Every row measures within [`TOLERANCE`] of the width and height it paints
/// at. Fails on a collection holding only the bundled faces
/// (`FontCollection::new()`): there CJK and emoji have no face to measure in,
/// and a host-named family measures in Roboto.
#[test]
fn measured_width_equals_painted_width_on_host_faces() {
    let fonts = FontCollection::with_host_faces(&shared_font_system());
    let mut context = TextContext::new(&fonts);
    let mut failures = Vec::new();
    let mut latin_rows = 0;
    for (name, style, text) in rows() {
        if !host_chain_covers(text) {
            let missing: Vec<_> = text
                .chars()
                .filter(|c| !c.is_whitespace() && !host_chain_covers(&c.to_string()))
                .map(|c| {
                    let reach = if host_covers(&c.to_string()) {
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
            let ((width, height), (painted_width, painted_height)) =
                measure_and_paint(&mut context, &style, text, size);
            println!(
                "{name} at {size} px: measured {width:.2} x {height:.2}, painted \
                 {painted_width:.2} x {painted_height:.2}"
            );
            if (width - painted_width).abs() > TOLERANCE
                || (height - painted_height).abs() > TOLERANCE
            {
                failures.push(format!(
                    "{name} at {size} px: measured {width:.2} x {height:.2}, painted \
                     {painted_width:.2} x {painted_height:.2}"
                ));
            }
        }
    }
    assert_eq!(latin_rows, 7, "every Latin row runs on any host");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every family the process font system carries is a family of the fed
/// collection: the two scans of the same files name families alike, so the
/// family rule resolves the same family on both sides.
#[test]
fn every_family_the_process_font_system_carries_resolves_in_the_collection() {
    let fonts = FontCollection::with_host_faces(&shared_font_system());
    let names = host_family_names();
    assert!(!names.is_empty(), "the process font system holds faces");
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
