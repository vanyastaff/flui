//! Integration tests for [`CupertinoTheme::of`]/brightness resolution —
//! proving the resolve chain actually reaches through a mounted
//! [`BuildContext`], not just the pure data-model unit tests in
//! `src/theme.rs`/`src/colors.rs`.
//!
//! Every test captures [`CupertinoTheme::of`]'s result **without** an extra
//! `.resolve(ctx)` call on top — asserting the captured
//! [`CupertinoColor`](flui_cupertino::CupertinoColor) is *already*
//! `Static(expected)`. Calling `.resolve(ctx)` again in the test would mask
//! two distinct mutants: `CupertinoTheme::of` returning the ancestor's data
//! without calling `resolve_from` at all (still `Dynamic(...)`, but a
//! same-context re-resolve would silently fix it up to the right color
//! anyway), and `CupertinoThemeData::resolve_from` returning `self`
//! unchanged (ditto). Asserting the *unresolved-looking* enum variant
//! (`Static`, not `Dynamic`) — not just the final color — is what makes both
//! mutants observable.

#![expect(clippy::unwrap_used)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{CupertinoColor, CupertinoTheme, CupertinoThemeData};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

#[derive(Clone, Debug, StatelessView)]
struct PaintedCupertinoRoles;

impl StatelessView for PaintedCupertinoRoles {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        use flui_sdk::widgets::{Column, Text};

        let theme = CupertinoTheme::of(ctx).text_theme();
        Column::new(vec![
            Text::new("content role").style(theme.text_style()).boxed(),
            Text::new("navigation role")
                .style(theme.nav_title_text_style())
                .boxed(),
            Text::new("small action role")
                .style(theme.action_small_text_style())
                .boxed(),
        ])
    }
}

/// Default Cupertino roles pass through a mounted theme and reach actual glyphs.
/// This uses supplied numeric answers; it does not evaluate UIKit or claim SF parity.
pub fn mounted_text_roles_paint_distinct_numeric_sizing_answers() {
    use flui_painting::glyphs::FontRegistry;
    use flui_painting::{DrawOp, TextSizing};
    use flui_sdk::foundation::{TextScaleProfile, TextSize, TextSizeRequest};

    let view = |text_sizing| {
        MediaQuery::new(
            MediaQueryData {
                text_sizing,
                ..MediaQueryData::default()
            },
            CupertinoTheme::new(CupertinoThemeData::default(), PaintedCupertinoRoles),
        )
    };
    let mut laid = lay_out(view(TextSizing::fixed()), loose(1000.0));
    let names = ["content role", "navigation role", "small action role"];
    let ids = names.map(|name| laid.find_text(name).expect("mounted Cupertino text"));
    let heights = ids.map(|id| laid.size(id).height);
    let exact = TextSizing::exact(
        [
            (17.0, TextScaleProfile::Body, 34.0),
            (17.0, TextScaleProfile::Headline, 51.0),
            (15.0, TextScaleProfile::Callout, 30.0),
        ]
        .into_iter()
        .map(|(authored, profile, answer)| {
            (
                TextSizeRequest {
                    size: TextSize::new(authored).expect("authored size"),
                    profile,
                },
                TextSize::new(answer).expect("answer size"),
            )
        }),
    )
    .expect("distinct Cupertino requests");
    for (policy, expected) in [
        (exact, [34.0, 51.0, 30.0]),
        (TextSizing::fixed(), [17.0, 17.0, 15.0]),
    ] {
        laid.pump_widget(view(policy));
        assert!(
            laid.did_paint_last_frame(),
            "sizing publication must repaint"
        );
        let mut registry = FontRegistry::new();
        let mut observed = [false; 3];
        for command in laid.draw_ops() {
            let DrawOp::Paragraph { paragraph, .. } = command.op else {
                continue;
            };
            let Some(index) = names.iter().position(|name| *name == paragraph.text()) else {
                continue;
            };
            for run in paragraph.runs() {
                let key = registry
                    .prepare_run(&run)
                    .expect("real Cupertino fallback face");
                for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                    assert_ne!(glyph.key.glyph_id(), 0, "role glyph must exist");
                    assert_eq!(
                        f64::from(glyph.key.size()),
                        expected[index],
                        "painted role {}",
                        names[index]
                    );
                    observed[index] = true;
                }
            }
        }
        assert_eq!(
            observed, [true; 3],
            "each role must produce actual painted glyphs"
        );
        for (index, id) in ids.into_iter().enumerate() {
            assert_eq!(laid.find_text(names[index]), Some(id));
            if expected == [17.0, 17.0, 15.0] {
                assert!(
                    (laid.size(id).height - heights[index]).abs() < 0.01,
                    "removing answers restores authored layout"
                );
            } else {
                assert!(
                    laid.size(id).height > heights[index],
                    "numeric sizing affects layout"
                );
            }
        }
    }
}

/// Captures `CupertinoTheme::of(ctx).primary_color()` — with no further
/// resolution — during `build()`.
#[derive(Clone, Debug, StatelessView)]
struct PrimaryColorCapture {
    captured: Arc<Mutex<Option<CupertinoColor>>>,
}

impl StatelessView for PrimaryColorCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let primary_color = CupertinoTheme::of(ctx).primary_color();
        *self.captured.lock().unwrap() = Some(primary_color);
        SizedBox::shrink()
    }
}

fn mount_and_capture(
    root_builder: impl FnOnce(PrimaryColorCapture) -> BoxedView,
) -> CupertinoColor {
    let captured: Arc<Mutex<Option<CupertinoColor>>> = Arc::new(Mutex::new(None));
    let capture = PrimaryColorCapture {
        captured: Arc::clone(&captured),
    };
    let root = root_builder(capture);
    let _laid = lay_out(root, loose(100.0));
    captured
        .lock()
        .unwrap()
        .expect("build should have run and captured a primary color")
}

/// An explicit `CupertinoThemeData::brightness` takes precedence over a
/// conflicting ambient `MediaQuery::platform_brightness` — the oracle's
/// `brightness ?? MediaQuery...` chain short-circuits on the theme's own
/// value, never consulting `MediaQuery` at all when it is set.
pub fn explicit_theme_brightness_overrides_media_query() {
    let primary_color = mount_and_capture(|capture| {
        MediaQuery::new(
            MediaQueryData {
                platform_brightness: Brightness::Dark,
                ..MediaQueryData::default()
            },
            CupertinoTheme::new(
                CupertinoThemeData::default().with_brightness(Brightness::Light),
                capture,
            ),
        )
        .boxed()
    });
    // Light, not dark — the theme's explicit brightness won, not the
    // (conflicting) ambient MediaQuery.
    assert_eq!(
        primary_color,
        CupertinoColor::Static(Color::rgb(0, 122, 255))
    );
}
