//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

use crate::common::{lay_out, loose};
use flui_painting::typography::TextStyle;
use flui_widgets::{DefaultTextStyle, MediaQuery, MediaQueryData, Text};

/// A preference must reach paragraph layout, not merely a data-reader widget.
pub(crate) fn media_text_scaling_changes_the_laid_out_text() {
    for scale in [
        0.0,
        -1.0,
        f64::MAX,
        f64::INFINITY,
        f64::NAN,
        f64::from_bits(1),
    ] {
        let text = || Text::new("nested scale").style(TextStyle::default().with_font_size(16.0));
        let baseline = lay_out(text(), loose(1000.0));
        let nested = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_scale_factor: 2.0,
                    ..MediaQueryData::default()
                },
                MediaQuery::new(
                    MediaQueryData {
                        text_scale_factor: scale,
                        ..MediaQueryData::default()
                    },
                    text(),
                ),
            ),
            loose(1000.0),
        );
        assert_eq!(
            nested.size(nested.root()),
            baseline.size(baseline.root()),
            "invalid inherited scale {scale}"
        );
    }
    // Exercise snapshot admission through the actual paragraph consumer. A
    // failed native observation falls back to unknown, never a non-finite size.
    for observed in [f64::MAX, f64::from_bits(1)] {
        let preferences = flui_platform_api::SystemPreferences::default()
            .with_text_scale(observed)
            .unwrap_or_default();
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_scale_factor: preferences.text_scale().unwrap_or(1.0),
                    ..MediaQueryData::default()
                },
                Text::new("safe preference").style(TextStyle::default().with_font_size(16.0)),
            ),
            loose(1000.0),
        );
        let baseline = lay_out(
            Text::new("safe preference").style(TextStyle::default().with_font_size(16.0)),
            loose(1000.0),
        );
        assert_eq!(
            paragraph.size(paragraph.root()),
            baseline.size(baseline.root())
        );
    }
    for observed in [1.0 / 64.0, 64.0] {
        let preferences = flui_platform_api::SystemPreferences::default()
            .with_text_scale(observed)
            .expect("supported scale boundary");
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_scale_factor: preferences.text_scale().expect("observed scale"),
                    ..MediaQueryData::default()
                },
                Text::new("boundary").style(TextStyle::default().with_font_size(16.0)),
            ),
            loose(100_000.0),
        );
        let size = paragraph.size(paragraph.root());
        assert!(size.width.is_finite() && size.width > 0.0);
        assert!(size.height.is_finite() && size.height > 0.0);
    }
    let text =
        || Text::new("accessibility sizing").style(TextStyle::default().with_font_size(16.0));
    let normal = lay_out(
        MediaQuery::new(MediaQueryData::default(), text()),
        loose(1000.0),
    );
    let enlarged = lay_out(
        MediaQuery::new(
            MediaQueryData {
                text_scale_factor: 2.0,
                ..MediaQueryData::default()
            },
            text(),
        ),
        loose(1000.0),
    );
    let normal_size = normal.size(normal.root());
    let enlarged_size = enlarged.size(enlarged.root());
    assert!(normal_size.width > 0.0 && normal_size.height > 0.0);
    assert!(
        enlarged_size.width > normal_size.width * 1.5
            && enlarged_size.height > normal_size.height * 1.5,
        "text scaling must enlarge the shaped paragraph: normal={normal_size:?}, enlarged={enlarged_size:?}"
    );
}

// ============================================================================
// DefaultTextStyle (text.dart:55-136, consumed by Text.build :716-765)
// ============================================================================

/// An enclosing `DefaultTextStyle` styles a bare `Text` run: the ambient
/// `font_size` shapes the glyphs, so the box grows with it (`text.dart:720`).
///
/// Red-check: drop the `depend_on::<DefaultTextStyle, _>` read from `Text::build`
/// — both boxes measure identically.
pub(crate) fn an_enclosing_default_text_style_styles_a_bare_run() {
    let bare = lay_out(Text::new("ambient type"), loose(1000.0));
    let styled = lay_out(
        DefaultTextStyle::new(
            TextStyle::default().with_font_size(40.0),
            Text::new("ambient type"),
        ),
        loose(1000.0),
    );

    assert!(
        styled.size(styled.root()).height > bare.size(bare.root()).height,
        "the ambient 40pt style must produce a taller box than the default type"
    );
}
