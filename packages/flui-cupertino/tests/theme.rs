//! Integration tests for [`CupertinoTheme::of`],
//! [`CupertinoTheme::maybe_of`] and brightness resolution — proving the resolve
//! chain actually reaches through a mounted [`BuildContext`], not just the pure
//! data-model unit tests in `src/theme.rs`/`src/colors.rs`.
//!
//! Resolution tests capture theme results **without** an extra
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
use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;

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
    use flui_sdk::foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_sdk::painting::{DrawOp, TextSizing};
    use flui_sdk::testing::painting::FontRegistry;

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
                    size: TextSize::try_new(authored).expect("authored size"),
                    profile,
                },
                TextSize::try_new(answer).expect("answer size"),
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

#[derive(Debug)]
struct ThemeLookup {
    maybe: Option<CupertinoThemeData>,
    of: Result<CupertinoThemeData, String>,
}

/// Captures both public ancestor accessors through the same mounted context.
#[derive(Clone, Debug, StatelessView)]
struct ThemeLookupCapture {
    captured: Arc<Mutex<Option<ThemeLookup>>>,
}

impl StatelessView for ThemeLookupCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let maybe = CupertinoTheme::maybe_of(ctx);
        // Catch here, inside the public build path: the framework otherwise
        // recovers a build panic by substituting an ErrorView.
        let of = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| CupertinoTheme::of(ctx)))
            .map_err(|payload| {
                let message = flui_sdk::foundation::panic::payload_text(&*payload)
                    .unwrap_or("non-string panic payload")
                    .to_owned();
                flui_sdk::foundation::panic::discard_payload(payload);
                message
            });
        *self.captured.lock().unwrap() = Some(ThemeLookup { maybe, of });
        SizedBox::shrink()
    }
}

fn mount_lookup(root_builder: impl FnOnce(ThemeLookupCapture) -> BoxedView) -> ThemeLookup {
    let captured: Arc<Mutex<Option<ThemeLookup>>> = Arc::new(Mutex::new(None));
    let capture = ThemeLookupCapture {
        captured: Arc::clone(&captured),
    };
    let _laid = lay_out(root_builder(capture), loose(100.0));
    captured
        .lock()
        .unwrap()
        .take()
        .expect("build should have run and captured both theme lookups")
}

pub fn missing_ancestor_is_optional_for_maybe_of_and_panics_for_of() {
    let lookup = mount_lookup(|capture| capture.into_view().boxed());

    assert!(
        lookup.maybe.is_none(),
        "maybe_of must not invent a default theme"
    );
    let panic = lookup
        .of
        .expect_err("of must panic without a CupertinoTheme ancestor");
    assert!(
        panic.contains("no CupertinoTheme ancestor"),
        "panic should explain the missing ancestor, got: {panic}"
    );
}

pub fn nearest_ancestor_is_resolved_for_of_and_maybe_of() {
    let outer_color = Color::rgb(200, 10, 20);
    let inner_color = Color::rgb(10, 20, 30);
    let lookup = mount_lookup(|capture| {
        CupertinoTheme::new(
            CupertinoThemeData::default()
                .with_brightness(Brightness::Light)
                .with_primary_color(outer_color),
            CupertinoTheme::new(
                CupertinoThemeData::default()
                    .with_brightness(Brightness::Dark)
                    .with_primary_color(inner_color),
                capture,
            ),
        )
        .boxed()
    });

    let maybe = lookup
        .maybe
        .expect("maybe_of should find the nearest ancestor");
    let of = lookup.of.expect("of should find the nearest ancestor");
    assert_eq!(of, maybe, "both accessors must resolve the same theme");
    for resolved in [&of, &maybe] {
        assert_eq!(resolved.brightness(), Some(Brightness::Dark));
        assert_eq!(
            resolved.primary_color(),
            CupertinoColor::Static(inner_color),
            "the nearest theme wins and its colors are already resolved"
        );
    }
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
