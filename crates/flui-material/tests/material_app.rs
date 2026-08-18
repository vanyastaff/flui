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
#![allow(clippy::unwrap_used)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_material::{
    ColorSchemeOverrides, MaterialApp, ScaffoldMessengerScope, Theme, ThemeData,
    ThemeDataOverrides, ThemeMode,
};
use flui_types::Color;
use flui_types::platform::Brightness;
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};
use flui_widgets::{Localizations, MediaQuery, MediaQueryData, SizedBox};

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

#[test]
fn resolved_theme_data_reaches_descendants() {
    let (probe, captured) = theme_capture();
    let _tree = lay_out(
        MaterialApp::new(probe).theme(light_sentinel()),
        loose(800.0),
    );
    assert_eq!(
        captured_theme(&captured),
        light_sentinel(),
        "descendants must read the exact resolved ThemeData through Theme::of"
    );
}

#[test]
fn theme_mode_light_ignores_dark_ambient_brightness() {
    // The oracle: mode == light never consults platformBrightness.
    let (probe, captured) = theme_capture();
    let app = MaterialApp::new(probe)
        .theme(light_sentinel())
        .dark_theme(dark_sentinel())
        .theme_mode(ThemeMode::Light);
    let _tree = lay_out(MediaQuery::new(media(Brightness::Dark), app), loose(800.0));
    assert_eq!(
        captured_theme(&captured),
        light_sentinel(),
        "ThemeMode::Light must pick `theme` even under a dark platform"
    );
}

#[test]
fn theme_mode_dark_selects_the_dark_theme() {
    let (probe, captured) = theme_capture();
    let app = MaterialApp::new(probe)
        .theme(light_sentinel())
        .dark_theme(dark_sentinel())
        .theme_mode(ThemeMode::Dark);
    let _tree = lay_out(MediaQuery::new(media(Brightness::Light), app), loose(800.0));
    assert_eq!(
        captured_theme(&captured),
        dark_sentinel(),
        "ThemeMode::Dark must pick `dark_theme` even under a light platform"
    );
}

#[test]
fn theme_mode_dark_without_a_dark_theme_falls_back_to_theme() {
    // The oracle's `useDarkTheme && widget.darkTheme != null` guard: dark
    // selection with no dark theme falls back to `theme` — never an
    // automatically derived dark variant.
    let (probe, captured) = theme_capture();
    let app = MaterialApp::new(probe)
        .theme(light_sentinel())
        .theme_mode(ThemeMode::Dark);
    let _tree = lay_out(app, loose(800.0));
    assert_eq!(
        captured_theme(&captured),
        light_sentinel(),
        "dark selection without a dark_theme must fall back to `theme`, not derive one"
    );
}

#[test]
fn no_theme_at_all_resolves_the_m3_light_baseline() {
    // The oracle's final fallback: `theme ??= widget.theme ?? ThemeData()`.
    let (probe, captured) = theme_capture();
    let _tree = lay_out(MaterialApp::new(probe), loose(800.0));
    assert_eq!(
        captured_theme(&captured),
        ThemeData::default(),
        "a MaterialApp with no themes must publish the ThemeData() baseline"
    );
}

#[test]
fn system_mode_resolves_against_ambient_platform_brightness() {
    // ThemeMode::System actually reads the platform signal — the exact
    // behavior ADR-0042 §2 calls out (the removed pre-ADR implementation
    // mapped System to light unconditionally).
    for ambient in [Brightness::Light, Brightness::Dark] {
        let expected = match ambient {
            Brightness::Light => light_sentinel(),
            Brightness::Dark => dark_sentinel(),
        };
        let (probe, captured) = theme_capture();
        let app = MaterialApp::new(probe)
            .theme(light_sentinel())
            .dark_theme(dark_sentinel())
            .theme_mode(ThemeMode::System);
        let _tree = lay_out(MediaQuery::new(media(ambient), app), loose(800.0));
        assert_eq!(
            captured_theme(&captured),
            expected,
            "System under {ambient:?} must resolve the matching theme"
        );
    }
}

// ============================================================================
// Live brightness republish — the realm-source pattern
// ============================================================================

/// Owner-local shared cell for ambient media data — the test-side replica of
/// the realm's `MediaQuerySource` (`flui-app`'s `media_query_root.rs`): the
/// test mutates it from outside the tree; the [`BrightnessRoot`] wrapper
/// republishes on the rebuild handle captured at mount.
#[derive(Default)]
struct BrightnessSource {
    data: RefCell<MediaQueryData>,
    rebuild: Cell<Option<RebuildHandle>>,
}

impl BrightnessSource {
    fn set_brightness(&self, brightness: Brightness) {
        self.data.borrow_mut().platform_brightness = brightness;
        if let Some(handle) = self.rebuild.take() {
            handle.schedule(flui_view::RebuildReason::StateChange);
            self.rebuild.set(Some(handle));
        }
    }
}

/// Publishes the [`BrightnessSource`] as the tree's `MediaQuery` — the
/// realm's `MediaQueryRoot`, in miniature.
#[derive(Clone)]
struct BrightnessRoot {
    source: Rc<BrightnessSource>,
    child: BoxedView,
}

impl std::fmt::Debug for BrightnessRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrightnessRoot").finish_non_exhaustive()
    }
}

impl View for BrightnessRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

struct BrightnessRootState {
    source: Rc<BrightnessSource>,
}

impl flui_view::StatefulView for BrightnessRoot {
    type State = BrightnessRootState;

    fn create_state(&self) -> Self::State {
        BrightnessRootState {
            source: Rc::clone(&self.source),
        }
    }
}

impl flui_view::ViewState<BrightnessRoot> for BrightnessRootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.source.rebuild.set(Some(ctx.rebuild_handle()));
    }

    fn build(&self, view: &BrightnessRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        MediaQuery::new(self.source.data.borrow().clone(), view.child.clone())
    }
}

#[test]
fn a_live_brightness_republish_re_resolves_system_mode() {
    // The issue's acceptance criterion: an OS appearance change updates the
    // window tree's `platform_brightness` and re-resolves ThemeMode::System
    // — driven here through the realm-source republish pattern.
    let source = Rc::new(BrightnessSource::default());
    source.data.borrow_mut().platform_brightness = Brightness::Light;
    let (probe, captured) = theme_capture();
    let app = MaterialApp::new(probe)
        .theme(light_sentinel())
        .dark_theme(dark_sentinel())
        .theme_mode(ThemeMode::System);
    let mut tree = lay_out(
        BrightnessRoot {
            source: Rc::clone(&source),
            child: app.into_view().boxed(),
        },
        loose(800.0),
    );
    assert_eq!(captured_theme(&captured), light_sentinel());

    // The appearance flip, exactly as the realm's appearance arm delivers
    // it: mutate the source, let the scheduled republish rebuild — no
    // root-dirtying pump, so only what the republish scheduled rebuilds.
    source.set_brightness(Brightness::Dark);
    tree.tick();
    assert_eq!(
        captured_theme(&captured),
        dark_sentinel(),
        "a live platform-brightness change must re-resolve ThemeMode::System to dark"
    );

    // And back — the resolution is a function of the live signal, not a
    // one-shot latch.
    source.set_brightness(Brightness::Light);
    tree.tick();
    assert_eq!(captured_theme(&captured), light_sentinel());
}

#[test]
fn two_presentations_resolve_different_themes_simultaneously() {
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

#[test]
fn the_builder_hook_resolves_the_published_theme() {
    // The oracle's builder-inside-a-Builder contract: `Theme.of` inside the
    // caller's builder must resolve the theme MaterialApp just selected.
    let seen: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));
    let seen_in_builder = Arc::clone(&seen);
    let app = MaterialApp::with_builder(move |ctx, _child| {
        *seen_in_builder.lock().unwrap() = Some(Theme::maybe_of(ctx));
        SizedBox::shrink().boxed()
    })
    .theme(light_sentinel());
    let _tree = lay_out(app, loose(800.0));
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some(Some(light_sentinel())),
        "the builder's context must sit below the published Theme"
    );
}

#[test]
fn scaffold_messenger_and_localizations_are_available_below_material_app() {
    // Composition proof: home sits below the ScaffoldMessenger band this
    // shell installs AND below the Localizations band WidgetsApp installs.
    let seen: Arc<Mutex<Option<(bool, bool)>>> = Arc::new(Mutex::new(None));

    #[derive(Clone, Debug, StatelessView)]
    struct BandsProbe {
        seen: Arc<Mutex<Option<(bool, bool)>>>,
    }

    impl StatelessView for BandsProbe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            *self.seen.lock().unwrap() = Some((
                ScaffoldMessengerScope::maybe_of(ctx).is_some(),
                Localizations::maybe_locale_of(ctx).is_some(),
            ));
            SizedBox::shrink()
        }
    }

    let probe = BandsProbe {
        seen: Arc::clone(&seen),
    };
    let _tree = lay_out(
        MaterialApp::new(probe).theme(light_sentinel()),
        loose(800.0),
    );
    let (messenger, localizations) = seen
        .lock()
        .unwrap()
        .expect("home must build below the shell's bands");
    assert!(messenger, "a ScaffoldMessenger must enclose the home route");
    assert!(localizations, "Localizations must enclose the home route");
}

#[test]
fn theme_mode_switch_on_a_live_app_updates_descendants() {
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

/// `ThemeDataOverrides` is imported to prove the patch-struct surface stays
/// reachable next to the shell (a compile-time check, free at runtime).
#[test]
fn theme_data_overrides_surface_is_reachable() {
    let _ = ThemeDataOverrides::default();
}
