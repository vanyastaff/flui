//! Integration tests for [`CupertinoApp`] — theme publication and Apple's
//! brightness model (an optional theme override, ambient `MediaQuery`
//! otherwise; deliberately **no** `ThemeMode` — ADR-0042 §2), through
//! mounted trees.
//!
//! Flutter parity oracle: `cupertino/app.dart` `_CupertinoAppState.build`
//! (oracle tag `3.44.0`).
//!
//! The live-republish test drives brightness through the same mechanism the
//! realm's root `MediaQuery` uses in production (`flui-app`'s
//! `media_query_root.rs`): an owner-local shared cell re-published by a
//! stateful wrapper through the `RebuildHandle` it captured at mount
//! (ADR-0018).

// `unwrap` in test code: a panic IS the failure report (docs/PANIC-POLICY.md).
#![expect(clippy::unwrap_used)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_cupertino::{
    CupertinoApp, CupertinoColor, CupertinoColors, CupertinoTheme, CupertinoThemeData,
};
use flui_types::Color;
use flui_types::platform::Brightness;
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle};
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

fn media(brightness: Brightness) -> MediaQueryData {
    MediaQueryData {
        platform_brightness: brightness,
        ..MediaQueryData::default()
    }
}

/// What a descendant of the shell observes: the effective brightness, the
/// published theme's (already materialized) primary color, and a dynamic
/// color resolved at the DESCENDANT's own altitude — the three observation
/// points Apple's model distinguishes.
#[derive(Clone, Debug, PartialEq)]
struct Observed {
    brightness: Brightness,
    published_primary: CupertinoColor,
    label_resolved_here: Color,
}

#[derive(Clone, Debug, StatelessView)]
struct Probe {
    captured: Arc<Mutex<Option<Observed>>>,
}

impl StatelessView for Probe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Observed {
            brightness: CupertinoTheme::brightness_of(ctx),
            published_primary: CupertinoTheme::of(ctx).primary_color(),
            label_resolved_here: CupertinoColor::Dynamic(CupertinoColors::LABEL).resolve(ctx),
        });
        SizedBox::shrink()
    }
}

fn probe() -> (Probe, Arc<Mutex<Option<Observed>>>) {
    let captured = Arc::new(Mutex::new(None));
    (
        Probe {
            captured: Arc::clone(&captured),
        },
        captured,
    )
}

fn observed(cell: &Arc<Mutex<Option<Observed>>>) -> Observed {
    cell.lock()
        .unwrap()
        .clone()
        .expect("the probe below CupertinoApp must have built")
}

/// systemBlue light variant — tag-verified in `tests/colors.rs`.
const SYSTEM_BLUE_LIGHT: Color = Color::rgb(0, 122, 255);
/// systemBlue dark variant.
const SYSTEM_BLUE_DARK: Color = Color::rgb(10, 132, 255);
/// systemRed light variant.
const SYSTEM_RED_LIGHT: Color = Color::rgb(255, 59, 48);

#[test]
fn publishes_the_resolved_theme_to_descendants() {
    let (probe, captured) = probe();
    let theme = CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
    let _tree = lay_out(CupertinoApp::new(probe).theme(theme), loose(800.0));
    let seen = observed(&captured);
    assert_eq!(
        seen.published_primary,
        CupertinoColor::Static(SYSTEM_RED_LIGHT),
        "the caller's theme — not a default — must reach descendants, already resolved"
    );
}

#[test]
fn ambient_brightness_drives_resolution_with_no_theme_mode_involved() {
    // The issue's acceptance criterion: CupertinoApp resolves brightness
    // without a ThemeMode — the ambient platform signal alone flips both
    // the published theme's materialized colors and descendant-side
    // resolution.
    for (ambient, expected_primary, expected_label) in [
        (
            Brightness::Light,
            SYSTEM_BLUE_LIGHT,
            Color::rgb(0, 0, 0), // LABEL light variant
        ),
        (
            Brightness::Dark,
            SYSTEM_BLUE_DARK,
            Color::rgb(255, 255, 255), // LABEL dark variant
        ),
    ] {
        let (probe, captured) = probe();
        let _tree = lay_out(
            MediaQuery::new(media(ambient), CupertinoApp::new(probe)),
            loose(800.0),
        );
        let seen = observed(&captured);
        assert_eq!(
            seen.brightness, ambient,
            "brightness_of follows the platform"
        );
        assert_eq!(
            seen.published_primary,
            CupertinoColor::Static(expected_primary),
            "the app-level materialization must resolve against the ambient brightness"
        );
        assert_eq!(
            seen.label_resolved_here, expected_label,
            "descendant-side dynamic resolution must follow the same signal"
        );
    }
}

#[test]
fn an_explicit_theme_brightness_override_wins_for_descendants() {
    // Apple's model (the oracle's `effectiveThemeData.brightness ??
    // MediaQuery.platformBrightnessOf`): the theme's optional brightness
    // override beats the ambient platform signal for everything below the
    // published theme — under a LIGHT platform, descendants see Dark.
    let (probe, captured) = probe();
    let theme = CupertinoThemeData::default().with_brightness(Brightness::Dark);
    let _tree = lay_out(
        MediaQuery::new(
            media(Brightness::Light),
            CupertinoApp::new(probe).theme(theme),
        ),
        loose(800.0),
    );
    let seen = observed(&captured);
    assert_eq!(
        seen.brightness,
        Brightness::Dark,
        "the theme's brightness override must beat the light platform signal"
    );
    assert_eq!(
        seen.label_resolved_here,
        Color::rgb(255, 255, 255),
        "a dynamic color resolved below the published theme must use the override"
    );
    // The published theme's own colors were materialized at the app's
    // altitude — ABOVE the published override, where only the platform
    // signal is visible — so they resolve LIGHT. This is the oracle's exact
    // behavior (`resolveFrom(context)` runs in `_CupertinoAppState.build`,
    // above the `CupertinoTheme` it then publishes; dynamic-color
    // resolution reads `CupertinoTheme.maybeBrightnessOf`, which sees no
    // theme ancestor there): the override steers descendant-side
    // resolution, not the app-level materialization.
    assert_eq!(
        seen.published_primary,
        CupertinoColor::Static(SYSTEM_BLUE_LIGHT)
    );
}

// ============================================================================
// Live brightness republish — the realm-source pattern
// ============================================================================

/// Test-side replica of the realm's `MediaQuerySource` (`flui-app`'s
/// `media_query_root.rs`) — see `flui-material`'s `material_app.rs` tests
/// for the same pattern on the Material side.
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
fn a_live_brightness_republish_re_resolves_the_theme() {
    let source = Rc::new(BrightnessSource::default());
    source.data.borrow_mut().platform_brightness = Brightness::Light;
    let (probe, captured) = probe();
    let mut tree = lay_out(
        BrightnessRoot {
            source: Rc::clone(&source),
            child: CupertinoApp::new(probe).into_view().boxed(),
        },
        loose(800.0),
    );
    assert_eq!(
        observed(&captured).published_primary,
        CupertinoColor::Static(SYSTEM_BLUE_LIGHT)
    );

    // The appearance flip: mutate the source, let the scheduled republish
    // rebuild — no root-dirtying pump.
    source.set_brightness(Brightness::Dark);
    tree.tick();
    let seen = observed(&captured);
    assert_eq!(seen.brightness, Brightness::Dark);
    assert_eq!(
        seen.published_primary,
        CupertinoColor::Static(SYSTEM_BLUE_DARK),
        "a live platform-brightness change must re-materialize the published theme"
    );
}

#[test]
// Debug-only: the guard compiles out in release, where `#[should_panic]`
// would otherwise report "did not panic as expected" (release still panics,
// but later, during build — see the setter's doc).
#[cfg(debug_assertions)]
#[should_panic(expected = "requires at least one locale")]
fn empty_supported_locales_panics_at_construction() {
    let _ = CupertinoApp::new(SizedBox::shrink()).supported_locales(Vec::new());
}

#[test]
fn the_builder_hook_resolves_the_published_theme() {
    // The oracle publishes CupertinoTheme ABOVE the WidgetsApp, so the
    // caller's builder — passed straight through — resolves it with no
    // extra wrapper.
    let seen: Arc<Mutex<Option<CupertinoColor>>> = Arc::new(Mutex::new(None));
    let seen_in_builder = Arc::clone(&seen);
    let theme = CupertinoThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
    let app = CupertinoApp::with_builder(move |ctx, _child| {
        *seen_in_builder.lock().unwrap() = Some(CupertinoTheme::of(ctx).primary_color());
        SizedBox::shrink().boxed()
    })
    .theme(theme);
    let _tree = lay_out(app, loose(800.0));
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some(CupertinoColor::Static(SYSTEM_RED_LIGHT)),
        "the builder's context must sit below the published CupertinoTheme"
    );
}
