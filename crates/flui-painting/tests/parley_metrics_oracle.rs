//! Parley's paragraph metrics against cosmic-text's on the bundled Roboto
//! (ADR-0092 §8 gate 8, the measurement side).
//!
//! The same `TextPainter` input is measured twice on one context: once by the
//! default backend (cosmic-text) and once pinned to Parley. Roboto is named
//! explicitly on both sides so neither resolves the generic family to a host
//! face. The comparison is the one today's painter makes observable: a
//! baseline placed on the device grid as `(line_y * scale).round()`
//! (`TextLayout::placed_glyphs`), the paragraph height, and a single line's
//! width.

use flui_painting::testing::measure_with_parley;
use flui_painting::typography::{TextDirection, TextSpan, TextStyle};
use flui_painting::{FontCollection, TextBaseline, TextContext, TextPainter};

const SIZES: [f64; 5] = [13.0, 14.0, 16.0, 18.0, 32.0];
const HEIGHTS: [Option<f64>; 2] = [None, Some(1.5)];
const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const TEXT: &str = "Hamburgefonstiv 0123";
const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");

struct Measured {
    width: f64,
    height: f64,
    alphabetic: f64,
}

fn measure(context: &mut TextContext, size: f64, height: Option<f64>, parley: bool) -> Measured {
    let style = TextStyle {
        font_family: Some("Roboto".to_owned()),
        font_size: Some(size),
        height,
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(TEXT, style))
        .with_text_direction(TextDirection::Ltr);
    if parley {
        measure_with_parley(&mut painter);
    }
    painter.layout(context, 0.0, f64::INFINITY);
    Measured {
        width: painter.width(),
        height: painter.height(),
        alphabetic: painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
    }
}

/// Every size, line height and scale factor places the first baseline on the
/// same device row on both paths, with equal paragraph height and a single
/// line's width within a hundredth of a pixel.
#[test]
fn parley_metrics_round_to_todays_baseline() {
    // The cosmic-text path loads the bundled Roboto only on a host with no
    // fonts at all; elsewhere "Roboto" falls back to a host face. Registered
    // here, in this binary's own process, both paths shape the same bytes.
    flui_painting::shared_font_system()
        .register_font(ROBOTO)
        .expect("the bundled Roboto loads");
    let mut context = TextContext::new(&FontCollection::new());
    let mut failures = Vec::new();
    for size in SIZES {
        for height in HEIGHTS {
            let cosmic = measure(&mut context, size, height, false);
            let parley = measure(&mut context, size, height, true);
            if (parley.height - cosmic.height).abs() > 1e-3 {
                failures.push(format!(
                    "{size} px, height {height:?}: height parley {} cosmic {}",
                    parley.height, cosmic.height
                ));
            }
            if (parley.width - cosmic.width).abs() > 0.01 {
                failures.push(format!(
                    "{size} px, height {height:?}: width parley {} cosmic {}",
                    parley.width, cosmic.width
                ));
            }
            for scale in SCALES {
                let device = |baseline: f64| (baseline * scale).round();
                if device(parley.alphabetic) != device(cosmic.alphabetic) {
                    failures.push(format!(
                        "{size} px, height {height:?}, scale {scale}: baseline parley {} \
                         cosmic {}",
                        parley.alphabetic, cosmic.alphabetic
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
