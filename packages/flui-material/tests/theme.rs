//! Integration test for [`Theme::of`] — the panicking ancestor accessor.
//!
//! `Theme::of` wraps `Theme::maybe_of`'s ancestor lookup with `.expect(...)`,
//! and this file proves its success path returns the ancestor's data
//! unchanged.
//!
//! Migrated from `flui-widgets/tests/theme.rs` when `Theme` moved to this
//! crate; the panic (no-ancestor) branch is still deliberately **not**
//! tested here for the same reason as before: a panic inside `build()` is
//! caught by the framework's build-error boundary (`build_owner.rs`
//! substitutes an `ErrorView` for the panicking node) rather than unwinding
//! out to the test, so `#[should_panic]` around the harness would not
//! observe it.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use crate::common;

use common::{lay_out, loose};
use flui_material::{ColorSchemeOverrides, Theme, ThemeData, ThemeDataOverrides};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;

/// Captures whatever [`Theme::of`] returns during `build()`.
///
/// `Option` (not `Option<Option<_>>`): if `Theme::of` panics, `build()`
/// never reaches the assignment and the harness substitutes an `ErrorView`,
/// so `captured` simply stays `None` — the `.expect(...)` below turns that
/// into a loud failure rather than a silent false-pass.
#[derive(Clone, Debug, StatelessView)]
struct ThemeOfCapture {
    captured: Arc<Mutex<Option<ThemeData>>>,
}

impl StatelessView for ThemeOfCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Theme::of(ctx));
        SizedBox::shrink()
    }
}

/// `Theme::of` returns exactly the ancestor's data when a `Theme` is present
/// — the success path of the panicking accessor.
pub fn theme_of_panicking_accessor_returns_ancestor_theme_data() {
    let captured: Arc<Mutex<Option<ThemeData>>> = Arc::new(Mutex::new(None));
    // Sentinel primary color distinct from both presets so the assertion
    // fails if `Theme::of` returned a preset instead of the provided value.
    let sentinel = Color::from_argb(0xFF0A_141E);
    let base = ThemeData::dark();
    let scheme = base.color_scheme.copy_with(ColorSchemeOverrides {
        primary: Some(sentinel),
        ..Default::default()
    });
    let provided = base.copy_with(ThemeDataOverrides {
        color_scheme: Some(scheme),
        ..Default::default()
    });

    let _laid = lay_out(
        Theme::new(
            provided.clone(),
            ThemeOfCapture {
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let got = captured.lock().unwrap().clone().expect(
        "ThemeOfCapture::build never populated `captured` — either it was not called, \
         or Theme::of panicked (the Theme ancestor is present, so it should not have)",
    );

    assert_eq!(
        got, provided,
        "Theme::of should return exactly the data provided by the ancestor Theme, \
         not a default or wrong scope"
    );
    assert_eq!(got.brightness(), Brightness::Dark);
    assert_eq!(got.color_scheme.primary, sentinel);
}

#[derive(Clone, Debug, StatelessView)]
struct PaintedThemeRoles;

impl StatelessView for PaintedThemeRoles {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        use flui_painting::typography::TextSpan;
        use flui_sdk::foundation::{TextScaleProfile, TextSizingIntent};
        use flui_sdk::painting::TextStyle;
        use flui_sdk::widgets::{Column, RichText, Text};

        let theme = Theme::of(ctx).text_theme;
        let title = theme.title_small.expect("Material title style");
        let label = theme.label_large.expect("Material label style");
        let body = theme.body_medium.expect("Material body style");
        let nested = TextSpan::new("A")
            .with_style(title.clone().with_color(Color::BLACK))
            .with_child(TextSpan::new("B").with_style(TextStyle::default().with_color(Color::RED)))
            .with_child(
                TextSpan::new("C").with_style(
                    TextStyle::default()
                        .with_color(Color::GREEN)
                        .with_sizing(TextSizingIntent::Profile(TextScaleProfile::Body)),
                ),
            )
            .with_child(
                TextSpan::new("D").with_style(
                    TextStyle::default()
                        .with_color(Color::BLUE)
                        .with_sizing(TextSizingIntent::Fixed),
                ),
            );
        Column::new(vec![
            Text::new("title role").style(title).boxed(),
            Text::new("label role").style(label).boxed(),
            Text::new("body role").style(body).boxed(),
            RichText::new(nested).boxed(),
        ])
    }
}

/// Theme roles with equal authored sizes must retain different growth profiles
/// through style merging, measurement and the submitted paragraph glyphs.
pub fn retained_theme_roles_paint_distinct_numeric_sizing_answers() {
    use flui_painting::glyphs::FontRegistry;
    use flui_painting::{DrawOp, TextSizing};
    use flui_sdk::foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_sdk::widgets::{MediaQuery, MediaQueryData};

    let view = |text_sizing| {
        MediaQuery::new(
            MediaQueryData {
                text_sizing,
                ..MediaQueryData::default()
            },
            Theme::new(ThemeData::light(), PaintedThemeRoles),
        )
    };
    let mut laid = lay_out(view(TextSizing::fixed()), loose(1000.0));
    let names = ["title role", "label role", "body role", "ABCD"];
    let ids = names.map(|name| laid.find_text(name).expect("painted theme paragraph"));
    let original_heights = ids.map(|id| laid.size(id).height);
    let exact = |answers: [f64; 3]| {
        TextSizing::exact(
            [
                TextScaleProfile::Subheadline,
                TextScaleProfile::Callout,
                TextScaleProfile::Body,
            ]
            .into_iter()
            .zip(answers)
            .map(|(profile, answer)| {
                (
                    TextSizeRequest {
                        size: TextSize::new(14.0).expect("authored size"),
                        profile,
                    },
                    TextSize::new(answer).expect("answer size"),
                )
            }),
        )
        .expect("one answer per role")
    };
    for (policy, expected) in [
        (exact([21.0, 28.0, 35.0]), [21.0, 28.0, 35.0]),
        (exact([24.0, 30.0, 36.0]), [24.0, 30.0, 36.0]),
        (TextSizing::fixed(), [14.0; 3]),
        (
            TextSizing::linear(1.5).expect("valid linear policy"),
            [21.0; 3],
        ),
        (TextSizing::fixed(), [14.0; 3]),
    ] {
        // Author the nearest provider, as an application does. This exercises
        // package role propagation, not native preference publication.
        laid.pump_widget(view(policy));
        assert!(
            laid.did_paint_last_frame(),
            "policy publication must repaint"
        );
        let mut registry = FontRegistry::new();
        let mut observed = [false; 4];
        let mut nested_colors = [false; 4];
        for command in laid.draw_ops() {
            let DrawOp::Paragraph {
                paragraph, color, ..
            } = command.op
            else {
                continue;
            };
            let Some(index) = names.iter().position(|name| *name == paragraph.text()) else {
                continue;
            };
            for run in paragraph.runs() {
                let key = registry.prepare_run(&run).expect("painted face");
                for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                    assert_ne!(glyph.key.glyph_id(), 0, "actual theme glyph must exist");
                    let wanted = if index < 3 {
                        expected[index]
                    } else {
                        let color_index = [Color::BLACK, Color::RED, Color::GREEN, Color::BLUE]
                            .iter()
                            .position(|expected_color| {
                                *expected_color == glyph.color.unwrap_or(color)
                            })
                            .expect("nested span color survives shaping");
                        nested_colors[color_index] = true;
                        [expected[0], expected[0], expected[2], 14.0][color_index]
                    };
                    assert_eq!(
                        f64::from(glyph.key.size()),
                        wanted,
                        "painted role {}",
                        names[index]
                    );
                    observed[index] = true;
                }
            }
        }
        assert_eq!(observed, [true; 4], "all theme text must reach paint");
        assert_eq!(
            nested_colors, [true; 4],
            "all inherited and override spans must paint"
        );
        for (index, id) in ids.into_iter().enumerate() {
            assert_eq!(
                laid.find_text(names[index]),
                Some(id),
                "publication retains the render object"
            );
            let height = laid.size(id).height;
            if expected == [14.0; 3] {
                assert!(
                    (height - original_heights[index]).abs() < 0.01,
                    "removal restores authored geometry"
                );
            } else {
                assert!(
                    height > original_heights[index],
                    "resolved sizes affect layout"
                );
            }
        }
    }
}
