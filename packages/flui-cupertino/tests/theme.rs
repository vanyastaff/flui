//! Integration tests for [`CupertinoTheme::of`] and brightness resolution
//! through a mounted [`BuildContext`]: what the authored data turns into for
//! a reader, which roles an author can set, and when readers share one
//! resolution.
//!
//! A reader gets a [`ResolvedCupertinoTheme`], whose colours are concrete
//! `Color`s by type, so a capture asserts the colour itself; no further
//! `.resolve(ctx)` runs on top of it.

#![expect(
    clippy::unwrap_used,
    reason = "a panic from `unwrap` is the test failure report (docs/PANIC-POLICY.md)"
)]

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{
    CupertinoColors, CupertinoTextThemeData, CupertinoTheme, CupertinoThemeData,
    ResolvedCupertinoTextTheme, ResolvedCupertinoTheme,
};
use flui_sdk::painting::styling::Color;
use flui_sdk::painting::typography::TextStyle;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::SizedBox;

#[derive(Clone, Debug, StatelessView)]
struct PaintedCupertinoRoles;

impl StatelessView for PaintedCupertinoRoles {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        use flui_sdk::widgets::{Column, Text};

        let theme = CupertinoTheme::of(ctx);
        let roles = &theme.text_theme;
        Column::new(vec![
            Text::new("content role")
                .style(roles.text_style.clone())
                .boxed(),
            Text::new("navigation role")
                .style(roles.nav_title_text_style.clone())
                .boxed(),
            Text::new("small action role")
                .style(roles.action_small_text_style.clone())
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
                for glyph in run.placed_glyphs(key, (0.0, 0.0), (0.0, 0.0), 1.0) {
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

/// Captures `CupertinoTheme::of(ctx).primary_color` during `build()`.
#[derive(Clone, Debug, StatelessView)]
struct PrimaryColorCapture {
    captured: Arc<Mutex<Option<Color>>>,
}

impl StatelessView for PrimaryColorCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let primary_color = CupertinoTheme::of(ctx).primary_color;
        *self.captured.lock().unwrap() = Some(primary_color);
        SizedBox::shrink()
    }
}

fn mount_and_capture(root_builder: impl FnOnce(PrimaryColorCapture) -> BoxedView) -> Color {
    let captured: Arc<Mutex<Option<Color>>> = Arc::new(Mutex::new(None));
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

/// An unset primary colour follows the accent the host reports, live, and
/// falls back to system blue without one; an authored primary keeps its own.
/// Goes through the runtime's preference delivery, not an authored
/// `MediaQuery`. Red-check: resolve an unset primary to system blue always.
pub fn an_unset_primary_follows_the_system_accent() {
    use flui_sdk::platform::{SrgbColor, SystemPreferences};

    const SYSTEM_BLUE: Color = Color::rgb(0, 122, 255);
    let accented = SystemPreferences::default()
        .with_accent_color(Some(SrgbColor::from_rgba8(255, 45, 85, 255)));
    for (authored, with_accent) in [
        (None, Color::rgb(255, 45, 85)),
        (Some(Color::rgb(10, 20, 30)), Color::rgb(10, 20, 30)),
    ] {
        let captured: Arc<Mutex<Option<Color>>> = Arc::new(Mutex::new(None));
        let mut theme = CupertinoThemeData::default();
        theme.primary_color = authored.map(Into::into);
        let mut laid = lay_out(
            CupertinoTheme::new(
                theme,
                PrimaryColorCapture {
                    captured: Arc::clone(&captured),
                },
            ),
            loose(100.0),
        );
        let primary = || captured.lock().unwrap().expect("the capture built");
        let without_accent = authored.unwrap_or(SYSTEM_BLUE);
        assert_eq!(primary(), without_accent, "no accent");
        laid.set_system_preferences(accented.clone());
        laid.pump();
        assert_eq!(primary(), with_accent, "{authored:?}");
        laid.set_system_preferences(SystemPreferences::default());
        laid.pump();
        assert_eq!(primary(), without_accent, "the accent withdrawn");
    }
}

/// Captures every resolved `CupertinoTheme::of(ctx)`, one per `build()`.
#[derive(Clone, Debug, StatelessView)]
struct ThemeCapture {
    captured: Rc<RefCell<Vec<Rc<ResolvedCupertinoTheme>>>>,
}

impl ThemeCapture {
    fn new() -> Self {
        Self {
            captured: Rc::default(),
        }
    }

    fn last(&self) -> Rc<ResolvedCupertinoTheme> {
        let captured = self.captured.borrow();
        Rc::clone(captured.last().expect("the capture built"))
    }

    fn builds(&self) -> usize {
        self.captured.borrow().len()
    }
}

impl StatelessView for ThemeCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.captured.borrow_mut().push(CupertinoTheme::of(ctx));
        SizedBox::shrink()
    }
}

/// The resolution a reader under `theme` gets on the headless (light)
/// platform.
fn resolved(theme: CupertinoThemeData) -> ResolvedCupertinoTheme {
    let capture = ThemeCapture::new();
    let _laid = lay_out(CupertinoTheme::new(theme, capture.clone()), loose(100.0));
    (*capture.last()).clone()
}

/// The resolved theme of `theme` on the headless (light) platform, once the
/// host reports `accent`.
fn resolved_under_accent(theme: CupertinoThemeData, accent: Color) -> ResolvedCupertinoTheme {
    use flui_sdk::platform::{SrgbColor, SystemPreferences};

    let capture = ThemeCapture::new();
    let mut laid = lay_out(CupertinoTheme::new(theme, capture.clone()), loose(100.0));
    let [red, green, blue, alpha] = accent.to_rgba8();
    laid.set_system_preferences(
        SystemPreferences::default()
            .with_accent_color(Some(SrgbColor::from_rgba8(red, green, blue, alpha))),
    );
    laid.pump();
    (*capture.last()).clone()
}

fn brightness_forced(brightness: Brightness) -> CupertinoThemeData {
    let mut theme = CupertinoThemeData::default();
    theme.brightness = Some(brightness);
    theme
}

/// The host reports the accent resolved for the platform's appearance, so a
/// theme forced to the other brightness keeps system blue with its own
/// variant; and the colour on an accent is whichever of white and black
/// contrasts more with it. Red-check: take the accent whatever the theme's
/// brightness, or keep white on every accent.
pub fn a_forced_brightness_or_a_light_accent_keeps_the_theme_legible() {
    let pink = Color::rgb(255, 45, 85);
    let dark = resolved_under_accent(brightness_forced(Brightness::Dark), pink);
    assert_eq!(
        dark.primary_color,
        Color::rgb(10, 132, 255),
        "a dark theme on a light platform keeps system blue's dark variant"
    );
    let light = resolved_under_accent(brightness_forced(Brightness::Light), pink);
    assert_eq!(
        light.primary_color, pink,
        "a theme of the platform's brightness takes the accent"
    );
    for (accent, contrasting) in [
        (Color::rgb(255, 185, 0), Color::BLACK),
        (Color::rgb(0, 0, 128), Color::WHITE),
    ] {
        assert_eq!(
            resolved_under_accent(CupertinoThemeData::default(), accent).primary_contrasting_color,
            contrasting,
            "{accent:?}"
        );
    }
    let mut authored = CupertinoThemeData::default();
    authored.primary_contrasting_color = Some(Color::WHITE.into());
    assert_eq!(
        resolved_under_accent(authored, Color::rgb(255, 185, 0)).primary_contrasting_color,
        Color::WHITE,
        "an authored contrasting colour stays"
    );
}

/// An explicit `CupertinoThemeData::brightness` takes precedence over a
/// conflicting ambient `MediaQuery::platform_brightness` — Flutter's
/// `brightness ?? MediaQuery...` chain short-circuits on the theme's own
/// value, never consulting `MediaQuery` at all when it is set.
pub fn explicit_theme_brightness_overrides_media_query() {
    let primary_color = mount_and_capture(|capture| {
        MediaQuery::new(
            MediaQueryData {
                platform_brightness: Brightness::Dark,
                ..MediaQueryData::default()
            },
            CupertinoTheme::new(brightness_forced(Brightness::Light), capture),
        )
        .boxed()
    });
    // Light, not dark — the theme's explicit brightness won, not the
    // (conflicting) ambient MediaQuery.
    assert_eq!(primary_color, Color::rgb(0, 122, 255));
}

/// One text role: its authored slot and its resolved style.
type TextRole = (
    &'static str,
    fn(&mut CupertinoTextThemeData) -> &mut Option<TextStyle>,
    fn(&ResolvedCupertinoTextTheme) -> &TextStyle,
);

const TEXT_ROLES: [TextRole; 9] = [
    ("text", |t| &mut t.text_style, |r| &r.text_style),
    (
        "action",
        |t| &mut t.action_text_style,
        |r| &r.action_text_style,
    ),
    (
        "action small",
        |t| &mut t.action_small_text_style,
        |r| &r.action_small_text_style,
    ),
    (
        "tab label",
        |t| &mut t.tab_label_text_style,
        |r| &r.tab_label_text_style,
    ),
    (
        "nav title",
        |t| &mut t.nav_title_text_style,
        |r| &r.nav_title_text_style,
    ),
    (
        "nav large title",
        |t| &mut t.nav_large_title_text_style,
        |r| &r.nav_large_title_text_style,
    ),
    (
        "nav action",
        |t| &mut t.nav_action_text_style,
        |r| &r.nav_action_text_style,
    ),
    (
        "picker",
        |t| &mut t.picker_text_style,
        |r| &r.picker_text_style,
    ),
    (
        "date time picker",
        |t| &mut t.date_time_picker_text_style,
        |r| &r.date_time_picker_text_style,
    ),
];

/// Each of the nine text roles set through `CupertinoThemeData::text_theme`
/// reaches a mounted reader as written, and only that role: every other one
/// keeps its default. The text theme's own primary colour recolours the
/// default action roles only. Red-check: resolve one role from another's
/// slot, or drop a set role for its default.
pub fn every_text_role_set_on_the_theme_reaches_a_reader() {
    let defaults = resolved(CupertinoThemeData::default()).text_theme;
    for (index, (name, slot, _)) in TEXT_ROLES.iter().enumerate() {
        let authored = TextStyle {
            font_size: Some(100.0 + f64::from(u8::try_from(index).unwrap())),
            color: Some(Color::rgb(1, 2, 3)),
            ..TextStyle::default()
        };
        let mut theme = CupertinoThemeData::default();
        *slot(&mut theme.text_theme) = Some(authored.clone());
        let roles = resolved(theme).text_theme;
        for (other, (other_name, _, read)) in TEXT_ROLES.iter().enumerate() {
            let expected = if other == index {
                &authored
            } else {
                read(&defaults)
            };
            assert_eq!(read(&roles), expected, "{other_name} with {name} set");
        }
    }

    let mut theme = CupertinoThemeData::default();
    theme.text_theme.primary_color = Some(CupertinoColors::SYSTEM_RED.into());
    let resolved = resolved(theme);
    let red = Some(Color::rgb(255, 59, 48));
    for (name, _, read) in &TEXT_ROLES {
        let expected = if name.contains("action") {
            red
        } else {
            read(&defaults).color
        };
        assert_eq!(read(&resolved.text_theme).color, expected, "{name}");
    }
    assert_eq!(
        resolved.primary_color,
        Color::rgb(0, 122, 255),
        "the text theme's primary does not change the theme's"
    );
}

/// Readers under one theme and one context share one resolution, and a
/// contrast or brightness change through `MediaQuery` rebuilds them with
/// another; flipping back reuses the first. Red-check: resolve afresh on
/// every `CupertinoTheme::of`.
pub fn readers_under_one_context_share_one_resolution() {
    use flui_sdk::widgets::Column;

    let (first, second) = (ThemeCapture::new(), ThemeCapture::new());
    let child = crate::colors::RetainedChild(
        CupertinoTheme::new(
            CupertinoThemeData::default(),
            Column::new(vec![first.clone().boxed(), second.clone().boxed()]),
        )
        .boxed(),
    );
    let mut data = MediaQueryData::default();
    let mut tree = lay_out(MediaQuery::new(data.clone(), child.clone()), loose(400.0));
    let light = first.last();
    assert!(
        Rc::ptr_eq(&light, &second.last()),
        "two readers under one context share the resolution"
    );

    data.high_contrast = true;
    let builds = first.builds();
    tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
    assert_eq!(first.builds(), builds + 1, "a contrast change rebuilds");
    let contrast = first.last();
    assert!(!Rc::ptr_eq(&light, &contrast), "contrast resolves anew");
    assert_eq!(contrast.primary_color, Color::rgb(0, 64, 221));
    assert!(Rc::ptr_eq(&contrast, &second.last()));

    data.high_contrast = false;
    tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
    assert!(
        Rc::ptr_eq(&light, &first.last()),
        "the light resolution is reused when contrast flips back"
    );

    data.platform_brightness = Brightness::Dark;
    let builds = first.builds();
    tree.pump_widget(MediaQuery::new(data, child));
    assert_eq!(first.builds(), builds + 1, "a brightness change rebuilds");
    let dark = first.last();
    assert_eq!(
        (dark.brightness, dark.primary_color),
        (Brightness::Dark, Color::rgb(10, 132, 255))
    );
    assert!(Rc::ptr_eq(&dark, &second.last()));
}
