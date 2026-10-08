//! Mounted color resolution: selective dependencies, live contrast and
//! brightness changes, authored colors and nested media overrides.

#![expect(clippy::unwrap_used)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{CupertinoColor, CupertinoDynamicColor, CupertinoTheme, CupertinoThemeData};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, ProxyView};
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

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

    let _laid = lay_out(
        CupertinoTheme::new(
            CupertinoThemeData::default().with_brightness(Brightness::Dark),
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
struct RetainedChild(BoxedView);

impl View for RetainedChild {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::proxy(self)
    }

    fn should_skip_rebuild(&self, _previous: &Self) -> bool {
        true
    }
}

impl ProxyView for RetainedChild {
    fn child(&self) -> &dyn View {
        &*self.0.0
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
        if let Some(brightness) = explicit {
            theme = theme.with_brightness(brightness);
        }
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
