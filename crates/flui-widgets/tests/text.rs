//! Layout test for [`Text`] — proves the widget measures real text headlessly
//! through `RenderParagraph` (a non-empty box), and that it composes as a leaf
//! inside other widgets.

use crate::common::{lay_out, loose};
use flui_painting::typography::TextStyle;
use flui_widgets::{DefaultTextStyle, MediaQuery, MediaQueryData, Text};

/// An icon's authored sizing policy applies to its box and painted glyph once.
pub(crate) fn icon_text_sizing_keeps_box_and_glyph_together() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::TextSizing;
    use flui_painting::display_list::DrawOp;
    use flui_painting::glyphs::FontRegistry;
    use flui_view::ViewExt;
    use flui_widgets::{Icon, IconData, IconTheme, IconThemeData};

    for (code, theme_size, explicit_size, scaling, side) in [
        (Some(0xe88a), None, None, None, 24.0),
        (Some(0xe88a), Some(18.0), None, Some(false), 18.0),
        (Some(0xe88a), Some(18.0), Some(27.5), Some(false), 27.5),
        (Some(0xe88a), Some(18.0), None, Some(true), 18.0),
        (Some(0xe88a), Some(18.0), Some(27.5), Some(true), 27.5),
        (None, Some(18.0), None, Some(false), 18.0),
        (None, Some(18.0), None, Some(true), 18.0),
        (Some(0xd800), Some(18.0), None, Some(true), 18.0),
    ] {
        let content = || {
            let icon = match code {
                Some(code) => Icon::new(IconData::new(code).with_font_family("Material Icons")),
                None => Icon::none(),
            };
            let icon = match explicit_size {
                Some(size) => icon.size(size),
                None => icon,
            };
            IconTheme::new(
                IconThemeData {
                    size: theme_size,
                    apply_text_scaling: scaling,
                    ..IconThemeData::default()
                },
                crate::media_query_fields::StaticChild {
                    inner: icon.boxed(),
                },
            )
        };
        let view = |text_sizing| {
            MediaQuery::new(
                MediaQueryData {
                    text_sizing,
                    ..MediaQueryData::default()
                },
                content(),
            )
        };
        let exact = |answers: [f64; 3]| {
            TextSizing::exact([18.0, 24.0, 27.5].into_iter().zip(answers).map(
                |(authored, answer)| {
                    (
                        TextSizeRequest {
                            size: TextSize::new(authored).expect("icon authored size"),
                            profile: TextScaleProfile::Body,
                        },
                        TextSize::new(answer).expect("resolved icon size"),
                    )
                },
            ))
            .expect("distinct icon requests")
        };
        for tight in [false, true] {
            let constraints = if tight {
                crate::common::tight(40.25, 33.75)
            } else {
                loose(10000.0)
            };
            let mut laid = lay_out(view(TextSizing::fixed()), constraints);
            let icon_id = laid.find_by_render_type("RenderIcon");
            let answer_index = [18.0, 24.0, 27.5]
                .iter()
                .position(|size| *size == side)
                .expect("authored fixture size");
            let mut policies: Vec<_> = [1.0, 2.0, 0.75, 1.0 / 64.0, 64.0, 1.0]
                .into_iter()
                .map(|factor| {
                    (
                        TextSizing::linear(factor).expect("linear icon policy"),
                        side * factor,
                    )
                })
                .collect();
            for answers in [[29.0, 36.0, 43.25], [31.5, 37.0, 49.0]] {
                policies.push((exact(answers), answers[answer_index]));
            }
            policies.push((TextSizing::fixed(), side));
            for (policy, resolved) in policies {
                laid.pump_widget(view(policy));
                let expected = if scaling == Some(true) {
                    resolved
                } else {
                    side
                };
                let box_size = laid.pipeline_owner().with(|owner| {
                    owner
                        .render_tree()
                        .get(laid.root())
                        .expect("icon box")
                        .size()
                        .expect("laid out box")
                });
                // BoxProtocol normalizes constraints to hundredths. The actual
                // raster size must retain the authored fractional value below.
                let allocated = if tight {
                    crate::common::size(40.25, 33.75)
                } else {
                    crate::common::size(expected, expected)
                };
                assert!(
                    (box_size.width - allocated.width).abs() <= 0.005 + f64::EPSILON
                        && (box_size.height - allocated.height).abs() <= 0.005 + f64::EPSILON,
                    "icon box must use resolved size within constraints: actual={box_size:?}, allocated={allocated:?}"
                );
                assert_eq!(laid.find_by_render_type("RenderIcon"), icon_id);
                let mut registry = FontRegistry::new();
                let mut painted = 0;
                for command in laid.draw_ops() {
                    if let DrawOp::Paragraph { paragraph, .. } = command.op {
                        for run in paragraph.runs() {
                            let key = registry.prepare_run(&run).expect("real icon face");
                            for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                                assert_ne!(glyph.key.glyph_id(), 0, "the icon must not be tofu");
                                assert_eq!(
                                    f64::from(glyph.key.size()),
                                    expected,
                                    "painted icon retains resolved size under constraints: theme={theme_size:?}, explicit={explicit_size:?}, scaling={scaling:?}, tight={tight}"
                                );
                                painted += 1;
                            }
                        }
                    }
                }
                assert_eq!(
                    painted,
                    usize::from(code.is_some_and(|point| char::from_u32(point).is_some())),
                    "one actual glyph must reach paint; absent or invalid icons only reserve space"
                );
            }
        }
    }
}

/// Only an authored icon description is exposed to assistive technology.
pub(crate) fn icon_labels_reach_the_assembled_accessibility_tree() {
    use flui_testing::a11y::Role;
    use flui_widgets::{Icon, IconData};

    for code in [Some(0xe88a), None, Some(0xd800)] {
        let view = |label: Option<&str>| {
            let icon = match code {
                Some(code) => Icon::new(IconData::new(code).with_font_family("Material Icons")),
                None => Icon::none(),
            };
            match label {
                Some(label) => icon.semantic_label(label),
                None => icon,
            }
        };
        let mut laid = lay_out(view(None), loose(100.0));
        laid.enable_semantics();
        laid.tick();
        let id = laid.find_by_render_type("RenderIcon");
        for label in [None, Some("Open home"), Some("Go home"), None] {
            laid.pump_widget(view(label));
            assert_eq!(laid.find_by_render_type("RenderIcon"), id);
            let tree = laid.a11y_tree().expect("assembled accessibility tree");
            assert!(
                tree.find_by_label("\u{e88a}").is_err(),
                "a codepoint is not an icon description"
            );
            assert!(tree.find_by_label("Open home").is_err() || label == Some("Open home"));
            assert!(tree.find_by_label("Go home").is_err() || label == Some("Go home"));
            let images = tree.find_all(Role::Image);
            match label {
                Some(label) => {
                    assert_eq!(
                        images.len(),
                        1,
                        "labelled icons are images, including absent glyphs"
                    );
                    assert_eq!(images[0].label(), Some(label));
                    assert_eq!(tree.find_all_by_label(label).len(), 1);
                }
                None => assert!(images.is_empty(), "decorative icons are not announced"),
            }
        }
    }
}

/// The system text scale grows each span once, from that span's own size.
///
/// Spans of different sizes, unstyled text, ambient styles and a fixed span
/// sit under the runtime's root policy with no provider of their own. The
/// paragraph's intrinsics agree with its layout under every scale, and
/// clearing the observation restores the authored geometry.
pub(crate) fn the_system_text_scale_grows_each_span_once() {
    use flui_foundation::TextSizingIntent;
    use flui_painting::display_list::DrawOp;
    use flui_painting::glyphs::FontRegistry;
    use flui_painting::typography::TextSpan;
    use flui_platform::SystemPreferences;
    use flui_rendering::storage::IntrinsicDimension;
    use flui_view::ViewExt;
    use flui_widgets::RichText;

    fn fixed(size: f64) -> TextStyle {
        sized(size).with_sizing(TextSizingIntent::Fixed)
    }
    fn sized(size: f64) -> TextStyle {
        TextStyle::default().with_font_size(size)
    }
    let cases = [
        (
            "spans of different sizes, unstyled text and a fixed span",
            RichText::new(TextSpan::with_children(vec![
                TextSpan::new("unstyled "),
                TextSpan::styled("fixed ", fixed(20.0)),
                TextSpan::styled("small ", sized(12.0)),
                TextSpan::styled("large", sized(20.0)),
            ]))
            .boxed(),
            vec![14.0, 20.0, 12.0, 20.0],
            vec![28.0, 20.0, 24.0, 40.0],
        ),
        (
            "an ambient style is scaled by the paragraph, not before it",
            DefaultTextStyle::new(sized(18.0), Text::new("ambient")).boxed(),
            vec![18.0],
            vec![36.0],
        ),
        (
            "an ambient fixed style keeps its subtree at the authored size",
            DefaultTextStyle::new(fixed(18.0), Text::new("ambient")).boxed(),
            vec![18.0],
            vec![18.0],
        ),
    ];
    // Painted glyph sizes span by span: runs come in visual order, and a
    // fallback face can split one span into neighbouring runs of one size.
    let glyph_sizes = |laid: &crate::common::LaidOut| {
        let mut registry = FontRegistry::new();
        let mut sizes = Vec::new();
        for command in laid.draw_ops() {
            let DrawOp::Paragraph { paragraph, .. } = command.op else {
                continue;
            };
            for run in paragraph.runs() {
                let key = registry.prepare_run(&run).expect("real text face");
                sizes.extend(
                    run.placed_glyphs(key, (0.0, 0.0), 1.0)
                        .map(|glyph| glyph.key.size()),
                );
            }
        }
        sizes.dedup();
        sizes
    };
    // Intrinsics and layout must measure the same resolved sizes.
    let geometry = |laid: &crate::common::LaidOut, id| {
        let size = laid.size(id);
        let max_width = laid.intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY);
        let height = laid.intrinsic_dimension(id, IntrinsicDimension::MinHeight, size.width);
        assert!(
            (max_width - size.width).abs() < 0.01 && (height - size.height).abs() < 0.01,
            "intrinsics measure what layout laid out: size={size:?}, max width={max_width}, height={height}"
        );
        size
    };

    for (case, view, authored, doubled) in cases {
        let mut laid = lay_out(view, loose(10_000.0));
        let id = laid.find_by_render_type("RenderParagraph");
        assert_eq!(glyph_sizes(&laid), authored, "{case}");
        let original = geometry(&laid, id);

        laid.set_system_preferences(
            SystemPreferences::default()
                .with_text_scale(2.0)
                .expect("supported scale"),
        );
        laid.tick();
        assert_eq!(laid.find_by_render_type("RenderParagraph"), id, "{case}");
        assert_eq!(glyph_sizes(&laid), doubled, "{case}");
        let scaled = geometry(&laid, id);
        if authored == doubled {
            assert_eq!(scaled, original, "{case}");
        } else {
            assert!(
                scaled.width > original.width && scaled.height > original.height,
                "{case}: original={original:?}, scaled={scaled:?}"
            );
        }

        laid.set_system_preferences(SystemPreferences::default());
        laid.tick();
        assert_eq!(glyph_sizes(&laid), authored, "{case}");
        assert_eq!(geometry(&laid, id), original, "{case}");
    }
}

/// A preference must reach paragraph layout, not merely a data-reader widget.
pub(crate) fn media_text_scaling_changes_the_laid_out_text() {
    // Invalid policies cannot be authored through a writable scalar field.
    // Finite arithmetic overflow is separately refused by measurement.
    for factor in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(flui_painting::TextSizing::linear(factor).is_err());
    }
    // Exercise snapshot admission through the actual paragraph consumer. A
    // failed native observation falls back to unknown, never a non-finite size.
    for observed in [f64::MAX, f64::from_bits(1)] {
        let preferences = flui_platform::SystemPreferences::default()
            .with_text_scale(observed)
            .unwrap_or_default();
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_sizing: flui_painting::TextSizing::linear(
                        preferences.text_scale().unwrap_or(1.0),
                    )
                    .expect("admitted preference"),
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
        let preferences = flui_platform::SystemPreferences::default()
            .with_text_scale(observed)
            .expect("supported scale boundary");
        let paragraph = lay_out(
            MediaQuery::new(
                MediaQueryData {
                    text_sizing: flui_painting::TextSizing::linear(
                        preferences.text_scale().expect("observed scale"),
                    )
                    .expect("admitted preference"),
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
                text_sizing: flui_painting::TextSizing::linear(2.0).expect("valid test policy"),
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
