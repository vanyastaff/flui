//! Mounted color resolution: selective dependencies, live contrast,
//! brightness and interface-level changes, authored colors, nested media
//! overrides and the palette's values.

#![expect(
    clippy::unwrap_used,
    reason = "a panic from `unwrap` is the test failure report (docs/PANIC-POLICY.md)"
)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{
    CupertinoColor, CupertinoColors, CupertinoDynamicColor, CupertinoTheme, CupertinoThemeData,
    CupertinoUserInterfaceLevel, CupertinoUserInterfaceLevelData, ResolvedCupertinoTheme,
};
use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, ProxyView};
use flui_sdk::widgets::SizedBox;

/// Captures `CupertinoColor::Static(sentinel).resolve(ctx)` during `build()`
/// — proving `resolve` actually runs against a real mounted `BuildContext`
/// (not just constructed and equality-checked against itself).
#[derive(Clone, Debug, StatelessView)]
struct StaticResolveCapture {
    sentinel: Color,
    captured: Arc<Mutex<Option<Color>>>,
}

impl StatelessView for StaticResolveCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let resolved = CupertinoColor::Static(self.sentinel).resolve(ctx);
        *self.captured.lock().unwrap() = Some(resolved);
        SizedBox::shrink()
    }
}

/// `CupertinoColor::Static::resolve` returns its color unchanged, verified
/// against a real mounted `BuildContext` under a `CupertinoTheme::brightness`
/// of `Dark` — a mutation that made `Static::resolve` secretly route through
/// brightness resolution (e.g. treating the sentinel as if it were a
/// `Dynamic` color's light variant) would still pass a construct-and-compare
/// unit test but fails here, since a real ambient theme is present and could
/// have perturbed the result if `resolve` consulted it.
pub fn static_color_resolves_to_itself_through_a_real_context() {
    let sentinel = Color::rgba(11, 22, 33, 200);
    let captured: Arc<Mutex<Option<Color>>> = Arc::new(Mutex::new(None));

    let mut theme = CupertinoThemeData::default();
    theme.brightness = Some(Brightness::Dark);
    let _laid = lay_out(
        CupertinoTheme::new(
            theme,
            StaticResolveCapture {
                sentinel,
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let resolved = captured
        .lock()
        .unwrap()
        .expect("build should have run and captured a resolved color");
    assert_eq!(resolved, sentinel);
}

/// Suppresses parent-driven rebuilds: only inherited dependencies can reach
/// the mounted resolver when the outer provider changes.
#[derive(Clone)]
pub(crate) struct RetainedChild(pub(crate) BoxedView);

impl View for RetainedChild {
    fn create_element(self: std::rc::Rc<Self>) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::proxy(self)
    }

    fn should_skip_rebuild(&self, _previous: &Self) -> bool {
        true
    }
}

impl ProxyView for RetainedChild {
    fn child(&self) -> &BoxedView {
        &self.0
    }
}

#[derive(Clone, StatelessView)]
struct ColorReader {
    color: CupertinoColor,
    seen: std::rc::Rc<std::cell::RefCell<Vec<Color>>>,
}

impl StatelessView for ColorReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.seen.borrow_mut().push(self.color.resolve(ctx));
        SizedBox::square(10.0)
    }
}

pub fn retained_colors_follow_contrast_and_explicit_brightness() {
    use std::{cell::RefCell, rc::Rc};

    let light = Color::rgb(10, 20, 30);
    let dark = Color::rgb(40, 50, 60);
    let light_contrast = Color::rgb(70, 80, 90);
    let dark_contrast = Color::rgb(100, 110, 120);
    let palette = CupertinoDynamicColor::with_brightness_and_contrast(
        light,
        dark,
        light_contrast,
        dark_contrast,
    );
    for explicit in [None, Some(Brightness::Light), Some(Brightness::Dark)] {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut theme = CupertinoThemeData::default();
        theme.brightness = explicit;
        let child = RetainedChild(
            CupertinoTheme::new(
                theme,
                ColorReader {
                    color: palette.into(),
                    seen: Rc::clone(&seen),
                },
            )
            .boxed(),
        );
        let mut data = MediaQueryData::default();
        let mut tree = lay_out(MediaQuery::new(data.clone(), child.clone()), loose(400.0));
        let expected = if explicit == Some(Brightness::Dark) {
            dark
        } else {
            light
        };
        assert_eq!(*seen.borrow(), [expected]);

        data.high_contrast = true;
        tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
        let expected_contrast = if explicit == Some(Brightness::Dark) {
            dark_contrast
        } else {
            light_contrast
        };
        assert_eq!(
            *seen.borrow(),
            [expected, expected_contrast],
            "contrast reaches retained color"
        );

        let count = seen.borrow().len();
        data.size.width += 1.0;
        tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
        assert_eq!(
            seen.borrow().len(),
            count,
            "color does not depend on window size"
        );

        data.platform_brightness = Brightness::Dark;
        tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
        let effective_dark = explicit != Some(Brightness::Light);
        assert_eq!(
            seen.borrow().last(),
            Some(&if effective_dark {
                dark_contrast
            } else {
                light_contrast
            })
        );
        if explicit.is_some() {
            assert_eq!(
                seen.borrow().len(),
                count,
                "explicit brightness masks platform brightness"
            );
        }
        data.high_contrast = false;
        tree.pump_widget(MediaQuery::new(data, child));
        assert_eq!(
            seen.borrow().last(),
            Some(&if effective_dark { dark } else { light })
        );
    }
}

pub fn authored_and_nested_colors_ignore_outer_contrast() {
    use std::{cell::RefCell, rc::Rc};

    let base = Color::rgb(10, 20, 30);
    for (color, nested) in [
        (CupertinoColor::Static(base), false),
        (
            CupertinoDynamicColor::with_brightness(base, Color::rgb(40, 50, 60)).into(),
            false,
        ),
        (
            CupertinoDynamicColor::with_brightness_and_contrast(
                base,
                base,
                Color::rgb(70, 80, 90),
                Color::rgb(70, 80, 90),
            )
            .into(),
            true,
        ),
    ] {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let reader = ColorReader {
            color,
            seen: Rc::clone(&seen),
        };
        let child = RetainedChild(if nested {
            MediaQuery::new(MediaQueryData::default(), reader).boxed()
        } else {
            reader.boxed()
        });
        let mut tree = lay_out(
            MediaQuery::new(MediaQueryData::default(), child.clone()),
            loose(400.0),
        );
        tree.pump_widget(MediaQuery::new(
            MediaQueryData {
                high_contrast: true,
                ..MediaQueryData::default()
            },
            child,
        ));
        assert_eq!(
            *seen.borrow(),
            [base],
            "irrelevant outer contrast does not rebuild or recolor"
        );
    }
}

/// The colour `color` resolves to under a mounted `MediaQuery` of
/// `brightness` and `high_contrast`, with no theme or level above it.
fn resolved_under(color: CupertinoColor, brightness: Brightness, high_contrast: bool) -> Color {
    use std::{cell::RefCell, rc::Rc};

    let seen = Rc::new(RefCell::new(Vec::new()));
    let data = MediaQueryData {
        platform_brightness: brightness,
        high_contrast,
        ..MediaQueryData::default()
    };
    let _laid = lay_out(
        MediaQuery::new(
            data,
            ColorReader {
                color,
                seen: Rc::clone(&seen),
            },
        ),
        loose(100.0),
    );
    let seen = seen.borrow();
    *seen.last().expect("the reader built")
}

/// A sample of the palette, the constants Flutter added since the first
/// table and the fixed greys, resolved through a mounted tree to Flutter's
/// values (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/colors.dart`):
/// light, dark, light high-contrast, dark high-contrast.
pub fn palette_constants_resolve_to_flutters_values() {
    let rows: [(&str, CupertinoColor, [Color; 4]); 11] = [
        (
            "SYSTEM_GREEN",
            CupertinoColors::SYSTEM_GREEN.into(),
            [
                Color::rgb(52, 199, 89),
                Color::rgb(48, 209, 88),
                Color::rgb(36, 138, 61),
                Color::rgb(48, 219, 91),
            ],
        ),
        (
            "ACTIVE_GREEN",
            CupertinoColors::ACTIVE_GREEN.into(),
            [
                Color::rgb(52, 199, 89),
                Color::rgb(48, 209, 88),
                Color::rgb(36, 138, 61),
                Color::rgb(48, 219, 91),
            ],
        ),
        (
            "SYSTEM_ORANGE",
            CupertinoColors::SYSTEM_ORANGE.into(),
            [
                Color::rgb(255, 149, 0),
                Color::rgb(255, 159, 10),
                Color::rgb(201, 52, 0),
                Color::rgb(255, 179, 64),
            ],
        ),
        (
            "ACTIVE_ORANGE",
            CupertinoColors::ACTIVE_ORANGE.into(),
            [
                Color::rgb(255, 149, 0),
                Color::rgb(255, 159, 10),
                Color::rgb(201, 52, 0),
                Color::rgb(255, 179, 64),
            ],
        ),
        (
            "QUATERNARY_LABEL",
            CupertinoColors::QUATERNARY_LABEL.into(),
            [
                Color::rgba(60, 60, 67, 45),
                Color::rgba(235, 235, 245, 40),
                Color::rgba(60, 60, 67, 66),
                Color::rgba(235, 235, 245, 61),
            ],
        ),
        (
            "SYSTEM_GROUPED_BACKGROUND",
            CupertinoColors::SYSTEM_GROUPED_BACKGROUND.into(),
            [
                Color::rgb(242, 242, 247),
                Color::rgb(0, 0, 0),
                Color::rgb(235, 235, 240),
                Color::rgb(0, 0, 0),
            ],
        ),
        (
            "LINK",
            CupertinoColors::LINK.into(),
            [
                Color::rgb(0, 122, 255),
                Color::rgb(9, 132, 255),
                Color::rgb(0, 122, 255),
                Color::rgb(9, 132, 255),
            ],
        ),
        (
            "PLACEHOLDER_TEXT",
            CupertinoColors::PLACEHOLDER_TEXT.into(),
            [
                Color::rgba(60, 60, 67, 76),
                Color::rgba(235, 235, 245, 76),
                Color::rgba(60, 60, 67, 96),
                Color::rgba(235, 235, 245, 96),
            ],
        ),
        (
            "LIGHT_BACKGROUND_GRAY",
            CupertinoColors::LIGHT_BACKGROUND_GRAY.into(),
            [Color::rgb(0xE5, 0xE5, 0xEA); 4],
        ),
        (
            "EXTRA_LIGHT_BACKGROUND_GRAY",
            CupertinoColors::EXTRA_LIGHT_BACKGROUND_GRAY.into(),
            [Color::rgb(0xEF, 0xEF, 0xF4); 4],
        ),
        (
            "DARK_BACKGROUND_GRAY",
            CupertinoColors::DARK_BACKGROUND_GRAY.into(),
            [Color::rgb(0x17, 0x17, 0x17); 4],
        ),
    ];
    let settings = [
        (Brightness::Light, false),
        (Brightness::Dark, false),
        (Brightness::Light, true),
        (Brightness::Dark, true),
    ];
    for (name, color, expected) in rows {
        for ((brightness, high_contrast), expected) in settings.into_iter().zip(expected) {
            assert_eq!(
                resolved_under(color, brightness, high_contrast),
                expected,
                "{name} under {brightness:?}, high contrast {high_contrast}"
            );
        }
    }
}

/// A colour whose eight variants all differ, so each one names the
/// brightness, contrast and level that picked it.
const EIGHT_VARIANTS: CupertinoDynamicColor = CupertinoDynamicColor {
    color: Color::rgb(1, 0, 0),
    dark_color: Color::rgb(2, 0, 0),
    high_contrast_color: Color::rgb(3, 0, 0),
    dark_high_contrast_color: Color::rgb(4, 0, 0),
    elevated_color: Color::rgb(5, 0, 0),
    dark_elevated_color: Color::rgb(6, 0, 0),
    high_contrast_elevated_color: Color::rgb(7, 0, 0),
    dark_high_contrast_elevated_color: Color::rgb(8, 0, 0),
};

/// `child` under `level`, or under no level at all.
fn at_level(level: Option<CupertinoUserInterfaceLevelData>, child: impl IntoView) -> BoxedView {
    match level {
        Some(level) => CupertinoUserInterfaceLevel::new(level, child).boxed(),
        None => child.into_view().boxed(),
    }
}

/// Below a `CupertinoUserInterfaceLevel`, a dynamic colour resolves the
/// variant of that level, in light and dark and at either contrast; at the
/// base level or with no level above it, the base variant. A retained
/// reader follows a live change of the level, and a colour with no elevated
/// variant does not depend on the level.
pub fn an_elevated_level_picks_the_elevated_variants() {
    use std::{cell::RefCell, rc::Rc};

    use CupertinoUserInterfaceLevelData::{Base, Elevated};

    let system_background: CupertinoColor = CupertinoColors::SYSTEM_BACKGROUND.into();
    let eight: CupertinoColor = EIGHT_VARIANTS.into();
    let rows: [(
        CupertinoColor,
        Option<CupertinoUserInterfaceLevelData>,
        [Color; 4],
    ); 6] = [
        (
            eight,
            None,
            [
                EIGHT_VARIANTS.color,
                EIGHT_VARIANTS.dark_color,
                EIGHT_VARIANTS.high_contrast_color,
                EIGHT_VARIANTS.dark_high_contrast_color,
            ],
        ),
        (
            eight,
            Some(Base),
            [
                EIGHT_VARIANTS.color,
                EIGHT_VARIANTS.dark_color,
                EIGHT_VARIANTS.high_contrast_color,
                EIGHT_VARIANTS.dark_high_contrast_color,
            ],
        ),
        (
            eight,
            Some(Elevated),
            [
                EIGHT_VARIANTS.elevated_color,
                EIGHT_VARIANTS.dark_elevated_color,
                EIGHT_VARIANTS.high_contrast_elevated_color,
                EIGHT_VARIANTS.dark_high_contrast_elevated_color,
            ],
        ),
        // Flutter's values: the elevated dark background is a grey, not black.
        (
            system_background,
            None,
            [
                Color::rgb(255, 255, 255),
                Color::rgb(0, 0, 0),
                Color::rgb(255, 255, 255),
                Color::rgb(0, 0, 0),
            ],
        ),
        (
            system_background,
            Some(Base),
            [
                Color::rgb(255, 255, 255),
                Color::rgb(0, 0, 0),
                Color::rgb(255, 255, 255),
                Color::rgb(0, 0, 0),
            ],
        ),
        (
            system_background,
            Some(Elevated),
            [
                Color::rgb(255, 255, 255),
                Color::rgb(28, 28, 30),
                Color::rgb(255, 255, 255),
                Color::rgb(36, 36, 38),
            ],
        ),
    ];
    let settings = [
        (Brightness::Light, false),
        (Brightness::Dark, false),
        (Brightness::Light, true),
        (Brightness::Dark, true),
    ];
    for (color, level, expected) in rows {
        for ((brightness, high_contrast), expected) in settings.into_iter().zip(expected) {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let data = MediaQueryData {
                platform_brightness: brightness,
                high_contrast,
                ..MediaQueryData::default()
            };
            let reader = ColorReader {
                color,
                seen: Rc::clone(&seen),
            };
            let _laid = lay_out(MediaQuery::new(data, at_level(level, reader)), loose(100.0));
            assert_eq!(
                seen.borrow().last(),
                Some(&expected),
                "{color:?} at {level:?} under {brightness:?}, high contrast {high_contrast}"
            );
        }
    }

    // A live change of the level reaches a retained reader, and the change
    // back restores the base variant.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let child = RetainedChild(
        ColorReader {
            color: eight,
            seen: Rc::clone(&seen),
        }
        .boxed(),
    );
    let mut tree = lay_out(
        CupertinoUserInterfaceLevel::new(Base, child.clone()),
        loose(100.0),
    );
    tree.pump_widget(CupertinoUserInterfaceLevel::new(Elevated, child.clone()));
    tree.pump_widget(CupertinoUserInterfaceLevel::new(Base, child));
    assert_eq!(
        *seen.borrow(),
        [
            EIGHT_VARIANTS.color,
            EIGHT_VARIANTS.elevated_color,
            EIGHT_VARIANTS.color
        ],
        "the level change rebuilds the retained reader"
    );

    // A colour whose elevated variants equal its base ones reads no level.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let child = RetainedChild(
        ColorReader {
            color: CupertinoColors::SYSTEM_GREEN.into(),
            seen: Rc::clone(&seen),
        }
        .boxed(),
    );
    let mut tree = lay_out(
        CupertinoUserInterfaceLevel::new(Base, child.clone()),
        loose(100.0),
    );
    tree.pump_widget(CupertinoUserInterfaceLevel::new(Elevated, child));
    assert_eq!(
        *seen.borrow(),
        [Color::rgb(52, 199, 89)],
        "a colour without elevated variants does not rebuild on a level change"
    );

    assert_eq!(
        CupertinoUserInterfaceLevelData::default(),
        Base,
        "a subtree without a level is at the base level"
    );
}

/// Captures `CupertinoTheme::of(ctx)` in each build.
#[derive(Clone, StatelessView)]
struct ThemeReader {
    seen: std::rc::Rc<std::cell::RefCell<Vec<std::rc::Rc<ResolvedCupertinoTheme>>>>,
}

impl StatelessView for ThemeReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.seen.borrow_mut().push(CupertinoTheme::of(ctx));
        SizedBox::square(10.0)
    }
}

/// Two readers of one theme, the first at the base level and the second
/// below an elevated level: the second gets a resolution of its own, whose
/// scaffold background is the elevated variant, while the first keeps the
/// base one. The level is part of the theme's resolution key, so the
/// resolution the first reader cached is not handed to the second.
pub fn a_theme_below_an_elevated_level_resolves_apart() {
    use std::{cell::RefCell, rc::Rc};

    use CupertinoUserInterfaceLevelData::{Base, Elevated};
    use flui_sdk::widgets::Column;

    for (brightness, base, elevated) in [
        (
            Brightness::Dark,
            Color::rgb(0, 0, 0),
            Color::rgb(28, 28, 30),
        ),
        (
            Brightness::Light,
            EIGHT_VARIANTS.color,
            EIGHT_VARIANTS.elevated_color,
        ),
    ] {
        let (at_base, at_elevated) = (
            Rc::new(RefCell::new(Vec::new())),
            Rc::new(RefCell::new(Vec::new())),
        );
        let mut data = CupertinoThemeData::default();
        data.brightness = Some(brightness);
        if brightness == Brightness::Light {
            data.scaffold_background_color = Some(EIGHT_VARIANTS.into());
        }
        let _laid = lay_out(
            CupertinoTheme::new(
                data,
                Column::new(vec![
                    CupertinoUserInterfaceLevel::new(
                        Base,
                        ThemeReader {
                            seen: Rc::clone(&at_base),
                        },
                    )
                    .boxed(),
                    CupertinoUserInterfaceLevel::new(
                        Elevated,
                        ThemeReader {
                            seen: Rc::clone(&at_elevated),
                        },
                    )
                    .boxed(),
                ]),
            ),
            loose(400.0),
        );
        let (at_base, at_elevated) = (
            Rc::clone(at_base.borrow().last().expect("the base reader built")),
            Rc::clone(
                at_elevated
                    .borrow()
                    .last()
                    .expect("the elevated reader built"),
            ),
        );
        assert_eq!(
            (
                at_base.scaffold_background_color,
                at_elevated.scaffold_background_color
            ),
            (base, elevated),
            "{brightness:?}: the base reader keeps the base variant, the elevated reader gets the elevated one"
        );
        assert!(
            !Rc::ptr_eq(&at_base, &at_elevated),
            "{brightness:?}: the two levels are resolved apart"
        );
    }
}
