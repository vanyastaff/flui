//! Parley's paragraph metrics against the cosmic-text layout the painter
//! records, on the bundled Roboto (ADR-0092 §8 gate 8, the measurement side).
//!
//! `TextPainter` measures on Parley and paints a cosmic-text layout until
//! ADR-0092 §10 step 4b, so measured and painted text agree only while both
//! shape the same face to the same metrics. Each case lays a painter out on
//! one context, then reads the `Paragraph` it paints, with Roboto named, with
//! no family and with the monospace generic, regular and bold, which both
//! sides must resolve to the bundled Roboto Regular rather than a host face
//! (painting mapping decision 16). The
//! comparison is the one the painter makes observable: a baseline placed on
//! the device grid as `(line_y * scale).round()`
//! (`TextLayout::placed_glyphs`), the paragraph height, and a single line's
//! width.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, TextBaseline, TextContext, TextLayoutResult, TextPainter,
};

const SIZES: [f64; 5] = [13.0, 14.0, 16.0, 18.0, 32.0];
const HEIGHTS: [Option<f64>; 2] = [None, Some(1.5)];
const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const TEXT: &str = "Hamburgefonstiv 0123";
const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
/// Roboto by name, the default family, and a generic other than sans-serif.
const FAMILIES: [Option<&str>; 3] = [Some("Roboto"), None, Some("monospace")];
/// Regular, and a weight the bundled Roboto has no face for.
const WEIGHTS: [FontWeight; 2] = [FontWeight::W400, FontWeight::W700];

struct Measured {
    width: f64,
    height: f64,
    alphabetic: f64,
}

/// What the painter measured, and the metrics of the layout it paints.
fn measure_and_paint(
    context: &mut TextContext,
    family: Option<&str>,
    weight: FontWeight,
    size: f64,
    height: Option<f64>,
) -> (Measured, Measured) {
    let style = TextStyle {
        font_family: family.map(str::to_owned),
        font_weight: Some(weight),
        font_size: Some(size),
        height,
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(TEXT, style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(context, 0.0, f64::INFINITY);
    let measured = Measured {
        width: painter.width(),
        height: painter.height(),
        alphabetic: painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
    };

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    let painted: TextLayoutResult = list
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { layout, .. } => Some(layout.metrics()),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph");
    let painted = Measured {
        width: painted.width,
        height: painted.height,
        alphabetic: painted.alphabetic_baseline,
    };
    (measured, painted)
}

/// Every family, weight, size, line height and scale factor places the first baseline
/// on the same device row in the measurement and the painted layout, with
/// equal paragraph height and a single line's width within a hundredth of a
/// pixel.
#[test]
fn parley_metrics_round_to_todays_baseline() {
    // With `bundled-fonts` the process font system already carries Roboto
    // and binds the generic families to it (painting mapping decision 16).
    // Registering it again adds a face but moves no generic binding, so the
    // default-family rows still depend on that one.
    flui_painting::shared_font_system()
        .register_font(ROBOTO)
        .expect("the bundled Roboto loads");
    let mut context = TextContext::new(&FontCollection::new());
    let mut failures = Vec::new();
    for (family, weight, size, height) in FAMILIES.into_iter().flat_map(|family| {
        WEIGHTS.into_iter().flat_map(move |weight| {
            SIZES.into_iter().flat_map(move |size| {
                HEIGHTS
                    .into_iter()
                    .map(move |height| (family, weight, size, height))
            })
        })
    }) {
        let (parley, cosmic) = measure_and_paint(&mut context, family, weight, size, height);
        let case = format!("{family:?} {weight:?} {size} px, height {height:?}");
        if (parley.height - cosmic.height).abs() > 1e-3 {
            failures.push(format!(
                "{case}: height measured {} painted {}",
                parley.height, cosmic.height
            ));
        }
        if (parley.width - cosmic.width).abs() > 0.01 {
            failures.push(format!(
                "{case}: width measured {} painted {}",
                parley.width, cosmic.width
            ));
        }
        for scale in SCALES {
            let device = |baseline: f64| (baseline * scale).round();
            if device(parley.alphabetic) != device(cosmic.alphabetic) {
                failures.push(format!(
                    "{case}, scale {scale}: baseline measured {} painted {}",
                    parley.alphabetic, cosmic.alphabetic
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
