//! Integration tests for [`MaterialApp`] — theme selection ([`ThemeMode`] ×
//! ambient platform brightness), publication through [`Theme`], and the
//! shell's composition bands, all through mounted trees.
//!
//! Flutter parity oracle: `material/app.dart` `_MaterialAppState.
//! _themeBuilder` / `_materialBuilder` (oracle tag `3.44.0`).
//!
//! The live-republish test drives brightness through the same mechanism the
//! realm's root `MediaQuery` uses in production (`flui-app`'s
//! `media_query_root.rs`): an owner-local shared cell re-published by a
//! stateful wrapper through the `RebuildHandle` it captured at mount
//! (ADR-0018). The realm half (platform appearance event → source update)
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
}

impl StatelessView for ThemeCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Theme::of(ctx));
        SizedBox::shrink()
    }
}

fn theme_capture() -> (ThemeCapture, Arc<Mutex<Option<ThemeData>>>) {
    let captured = Arc::new(Mutex::new(None));
    (
        ThemeCapture {
            captured: Arc::clone(&captured),
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
// Live brightness republish — the realm-source pattern
// ============================================================================

pub fn two_presentations_resolve_different_themes_simultaneously() {
    // The issue's per-presentation criterion: appearance is scoped to one
    // window's tree (ADR-0027, ADR-0042 §1). Two presentations — two
    // headless bindings in one process, the same isolation boundary two
    // realm-backed windows have — mount the SAME app configuration under
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
