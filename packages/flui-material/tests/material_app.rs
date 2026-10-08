//! Integration tests for [`MaterialApp`] — theme selection ([`ThemeMode`] ×
//! ambient platform brightness), publication through [`Theme`], and the
//! shell's composition bands, all through mounted trees.
//!
//! The live-republish test drives brightness through the same mechanism the
//! UI runtime's root `MediaQuery` uses in production (`flui-app`'s
//! `media_query_root.rs`): an owner-local shared cell re-published by a
//! stateful wrapper through the `RebuildHandle` it captured at mount
//! (ADR-0018). The UI runtime half (platform appearance event → source update)
//! is pinned in `flui-app`; these tests pin the widget half (ambient
//! republish → `ThemeMode::System` re-resolution).

// `unwrap` in test code: a panic IS the failure report (docs/PANIC-POLICY.md).
#![expect(clippy::unwrap_used)]

use crate::common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_material::{ColorSchemeOverrides, MaterialApp, Theme, ThemeData, ThemeMode};
use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

/// A light theme with a sentinel primary color no preset uses, so an
/// assertion can tell "my theme arrived" from "some default arrived".
fn light_sentinel() -> ThemeData {
    let mut theme = ThemeData::light();
    theme.color_scheme = theme.color_scheme.copy_with(ColorSchemeOverrides {
        primary: Some(Color::from_argb(0xFF10_2030)),
        ..ColorSchemeOverrides::default()
    });
    theme
}

/// A dark theme with a different sentinel primary color.
fn dark_sentinel() -> ThemeData {
    let mut theme = ThemeData::dark();
    theme.color_scheme = theme.color_scheme.copy_with(ColorSchemeOverrides {
        primary: Some(Color::from_argb(0xFF40_5060)),
        ..ColorSchemeOverrides::default()
    });
    theme
}

fn media(brightness: Brightness) -> MediaQueryData {
    MediaQueryData {
        platform_brightness: brightness,
        ..MediaQueryData::default()
    }
}

/// Captures [`Theme::of`] during `build()`. Stays `None` when the probe
/// never builds (a silently-childless seam), which the `.expect` in each
/// test turns into a loud failure.
#[derive(Clone, Debug, StatelessView)]
struct ThemeCapture {
    captured: Arc<Mutex<Option<ThemeData>>>,
    builds: std::rc::Rc<std::cell::Cell<usize>>,
}

impl StatelessView for ThemeCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        *self.captured.lock().unwrap() = Some(Theme::of(ctx));
        SizedBox::shrink()
    }
}

fn theme_capture() -> (ThemeCapture, Arc<Mutex<Option<ThemeData>>>) {
    let captured = Arc::new(Mutex::new(None));
    (
        ThemeCapture {
            captured: Arc::clone(&captured),
            builds: std::rc::Rc::default(),
        },
        captured,
    )
}

fn captured_theme(cell: &Arc<Mutex<Option<ThemeData>>>) -> ThemeData {
    cell.lock()
        .unwrap()
        .clone()
        .expect("the probe below MaterialApp must have built")
}

// ============================================================================
// Live brightness republish — the ui_runtime-source pattern
// ============================================================================

pub fn two_presentations_resolve_different_themes_simultaneously() {
    // The issue's per-presentation criterion: appearance is scoped to one
    // window's tree (ADR-0027, ADR-0042 §1). Two presentations — two
    // headless bindings in one process, the same isolation boundary two
    // ui_runtime-backed windows have — mount the SAME app configuration under
    // different ambient brightness and must hold different resolved themes
    // AT THE SAME TIME, with nothing process-global to fight over.
    let app_under = |ambient: Brightness| {
        let (probe, captured) = theme_capture();
        let app = MaterialApp::new(probe)
            .theme(light_sentinel())
            .dark_theme(dark_sentinel())
            .theme_mode(ThemeMode::System);
        (
            lay_out(MediaQuery::new(media(ambient), app), loose(800.0)),
            captured,
        )
    };

    let (_tree_light, captured_light) = app_under(Brightness::Light);
    let (_tree_dark, captured_dark) = app_under(Brightness::Dark);

    // Both trees are alive here; read both while mounted.
    assert_eq!(captured_theme(&captured_light), light_sentinel());
    assert_eq!(captured_theme(&captured_dark), dark_sentinel());
}

// ============================================================================
// Composition bands
// ============================================================================

pub fn theme_mode_switch_on_a_live_app_updates_descendants() {
    // The issue's acceptance criterion: MaterialApp switches between light
    // and dark when `theme_mode` changes on a rebuild.
    let (probe, captured) = theme_capture();
    let configured = |mode: ThemeMode, probe: ThemeCapture| {
        MaterialApp::new(probe)
            .theme(light_sentinel())
            .dark_theme(dark_sentinel())
            .theme_mode(mode)
    };
    let mut tree = lay_out(configured(ThemeMode::Light, probe.clone()), loose(800.0));
    assert_eq!(captured_theme(&captured), light_sentinel());

    tree.pump_widget(configured(ThemeMode::Dark, probe));
    assert_eq!(
        captured_theme(&captured),
        dark_sentinel(),
        "flipping theme_mode on a live app must re-resolve and reach descendants"
    );
}

pub fn contrast_selects_authored_themes_in_a_retained_app() {
    use crate::rebuild_exactness::StaticChild;

    let high_light = {
        let mut theme = light_sentinel();
        theme.color_scheme = theme.color_scheme.copy_with(ColorSchemeOverrides {
            primary: Some(Color::rgb(70, 80, 90)),
            ..ColorSchemeOverrides::default()
        });
        theme
    };
    let high_dark = {
        let mut theme = dark_sentinel();
        theme.color_scheme = theme.color_scheme.copy_with(ColorSchemeOverrides {
            primary: Some(Color::rgb(100, 110, 120)),
            ..ColorSchemeOverrides::default()
        });
        theme
    };
    // Each configuration exercises its complete normal -> high -> normal
    // lifetime, including absent slots and explicit brightness overrides.
    for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
        for brightness in [Brightness::Light, Brightness::Dark] {
            for (has_dark, has_high_light, has_high_dark) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, false, true),
                (true, false, true),
                (false, true, true),
                (true, true, true),
            ] {
                let (probe, captured) = theme_capture();
                let builds = std::rc::Rc::clone(&probe.builds);
                let mut app = MaterialApp::new(probe)
                    .theme(light_sentinel())
                    .theme_mode(mode);
                if has_dark {
                    app = app.dark_theme(dark_sentinel());
                }
                if has_high_light {
                    app = app.high_contrast_theme(high_light.clone());
                }
                if has_high_dark {
                    app = app.high_contrast_dark_theme(high_dark.clone());
                }
                let child = StaticChild { inner: app.boxed() };
                let mut data = media(brightness);
                let mut tree = lay_out(MediaQuery::new(data.clone(), child.clone()), loose(800.0));
                let dark = mode == ThemeMode::Dark
                    || (mode == ThemeMode::System && brightness == Brightness::Dark);
                let normal = if dark && has_dark {
                    dark_sentinel()
                } else {
                    light_sentinel()
                };
                assert_eq!(captured_theme(&captured), normal);
                data.high_contrast = true;
                tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
                let expected = match (dark, has_high_light, has_high_dark) {
                    (true, _, true) => &high_dark,
                    (false, true, _) => &high_light,
                    _ => &normal,
                };
                assert_eq!(
                    &captured_theme(&captured),
                    expected,
                    "mode={mode:?}, brightness={brightness:?}, slots={has_dark}/{has_high_light}/{has_high_dark}"
                );
                let count = builds.get();
                data.size.width += 1.0;
                tree.pump_widget(MediaQuery::new(data.clone(), child.clone()));
                assert_eq!(
                    builds.get(),
                    count,
                    "theme selection does not depend on size"
                );
                data.high_contrast = false;
                tree.pump_widget(MediaQuery::new(data, child));
                assert_eq!(captured_theme(&captured), normal);
            }
        }
    }
}
