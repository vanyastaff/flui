//! Integration tests for [`Theme`] — the Material inherited theming widget.
//!
//! Migrated from `flui-widgets/tests/inherited_app.rs` (the `Theme`-specific
//! subset) when `Theme` moved to this crate; `MediaQuery`'s tests stayed in
//! `flui-widgets`.
//!
//! Each test drives the real layout pipeline through the headless harness and
//! proves that the inherited read works end-to-end: a descendant view
//! captures the data provided by the ancestor into a shared cell during
//! `build()`.
//!
//! ## Correctness invariants tested
//!
//! * `Theme::maybe_of` MUST return the *provided* data, not a default — the
//!   assertions fail if `maybe_of` returns any other value.
//! * `maybe_of` MUST return `None` when no ancestor exists.
//!
//! ## False-pass prevention
//!
//! The capture cell is `Option<Option<ThemeData>>`:
//! * `None` (outer) — `build()` was never invoked.
//! * `Some(None)` — `build()` ran; `maybe_of` found no ancestor.
//! * `Some(Some(data))` — `build()` ran; `maybe_of` returned `data`.
//!
//! "No ancestor" tests first assert `is_some()` on the outer option so they
//! fail loudly if the framework skips `build()` entirely.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::option_option, clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::{lay_out, loose};
use flui_material::{
    AppBar, AppBarThemeData, ButtonStyle, ColorSchemeOverrides, ElevatedButton,
    ElevatedButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_types::platform::Brightness;
use flui_types::styling::Color;
use flui_view::prelude::*;
use flui_widgets::{
    InheritedTheme, MediaQuery, MediaQueryData, SizedBox, Text, WidgetStateProperty,
};

/// Captures whatever [`Theme::maybe_of`] returns during `build()`.
///
/// Outer `None` = `build` not called (framework bug or harness not wired).
/// Inner `None` = `build` ran; no `Theme` ancestor found.
/// Inner `Some(data)` = `build` ran; ancestor data captured.
#[derive(Clone, Debug, StatelessView)]
struct ThemeCapture {
    captured: Arc<Mutex<Option<Option<ThemeData>>>>,
}

impl StatelessView for ThemeCapture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.lock().unwrap() = Some(Theme::maybe_of(ctx));
        SizedBox::shrink()
    }
}

/// A descendant reads `Theme::maybe_of` inside `build` and the captured value
/// equals the data the ancestor `Theme` provided. The assertion fails if:
/// - `maybe_of` returns `None` (no lookup performed), or
/// - `maybe_of` returns `Some(wrong_data)` (wrong scope resolved).
#[test]
fn theme_of_returns_ancestor_theme_data() {
    let captured: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));
    // Sentinel primary color distinct from both presets so the assertion
    // fails if `maybe_of` returns any preset instead of the provided value.
    let sentinel = Color::from_argb(0xFF00_C864);
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
            ThemeCapture {
                captured: Arc::clone(&captured),
            },
        ),
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    let got = outer
        .expect("ThemeCapture::build was never called — the harness did not traverse the subtree")
        .expect(
            "Theme::maybe_of returned None even though a Theme ancestor was present in the tree",
        );

    assert_eq!(
        got, provided,
        "Theme::maybe_of should return exactly the data provided by the ancestor Theme, \
         not a default or wrong scope"
    );
    assert_eq!(got.brightness(), Brightness::Dark);
    assert_eq!(got.color_scheme.primary, sentinel);
}

/// `Theme::maybe_of` returns `None` when no `Theme` ancestor is present.
/// Proves the lookup is honest, not returning a hidden default.
#[test]
fn theme_maybe_of_returns_none_without_ancestor() {
    let captured: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));

    let _laid = lay_out(
        ThemeCapture {
            captured: Arc::clone(&captured),
        },
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    // If the outer is None, build() was never called — the harness is broken.
    let inner = outer
        .expect("ThemeCapture::build was never called — the harness did not traverse the subtree");

    assert!(
        inner.is_none(),
        "Theme::maybe_of should return None when no Theme ancestor is present, \
         got: {inner:?}"
    );
}

/// The task-level proof this crate's component-theme slots exist for: a
/// SINGLE `Theme`, configured with BOTH a custom `elevated_button_theme`
/// style AND a custom `app_bar_theme` background, must reach BOTH mounted
/// widgets simultaneously — not just one slot in isolation (every other
/// `*_theme_slot_reaches_the_mounted_*` test in this crate proves exactly
/// one consumer at a time; this one proves the slots are independent,
/// per-widget reads off the SAME `ThemeData`, not a global "last theme
/// change wins" shared value).
#[test]
fn a_themed_subtree_carries_both_the_elevated_button_and_app_bar_theme_simultaneously() {
    use flui_widgets::Column;

    let themed_button_background = Color::from_argb(0xFF11_2233);
    let themed_app_bar_background = Color::from_argb(0xFF44_5566);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        elevated_button_theme: Some(ElevatedButtonThemeData {
            style: Some(ButtonStyle {
                background_color: Some(WidgetStateProperty::all(Some(themed_button_background))),
                ..Default::default()
            }),
        }),
        app_bar_theme: Some(AppBarThemeData {
            background_color: Some(themed_app_bar_background),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(
            theme,
            MediaQuery::new(
                MediaQueryData::default(),
                Column::new(vec![
                    AppBar::new().title(Text::new("Title")).boxed(),
                    ElevatedButton::new(Text::new("Save"))
                        .on_pressed(|| {})
                        .boxed(),
                ]),
            ),
        ),
        loose(400.0),
    );

    let materials = laid.find_all_by_render_type("RenderPhysicalShape");
    assert_eq!(
        materials.len(),
        2,
        "both the AppBar and the ElevatedButton must compose their own Material surface"
    );
    let colors: std::collections::HashSet<String> = materials
        .iter()
        .filter_map(|id| laid.render_property(*id, "color"))
        .collect();

    assert!(
        colors.contains(&format!("{themed_app_bar_background:?}")),
        "the AppBar's Material must resolve the theme's app_bar_theme.background_color",
    );
    assert!(
        colors.contains(&format!("{themed_button_background:?}")),
        "the ElevatedButton's Material must resolve the theme's \
         elevated_button_theme.style.background_color, from the SAME Theme ancestor",
    );
}

/// `ThemeData::light()` and `ThemeData::dark()` must differ on at least
/// `brightness` and `color_scheme.primary`.
#[test]
fn theme_data_light_and_dark_presets_are_distinct() {
    let light = ThemeData::light();
    let dark = ThemeData::dark();

    assert_ne!(light, dark, "light and dark themes should not be equal");
    assert_eq!(light.brightness(), Brightness::Light);
    assert_eq!(dark.brightness(), Brightness::Dark);
    assert_ne!(
        light.color_scheme.primary, dark.color_scheme.primary,
        "light and dark presets should have different primary colors"
    );
}

// ============================================================================
// InheritedTheme::wrap
// ============================================================================

/// A host whose `build()` calls [`InheritedTheme::wrap`] on a throwaway
/// source `Theme` to re-wrap `ThemeCapture`, then returns the wrapped
/// subtree — this is the one call site that exercises `wrap`'s actual
/// contract: the child it wraps must see the *source* `Theme`'s data.
#[derive(Clone, StatelessView)]
struct WrapHost {
    source_data: ThemeData,
    captured: Arc<Mutex<Option<Option<ThemeData>>>>,
}

impl StatelessView for WrapHost {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // The source `Theme`'s own child (`SizedBox::shrink()`) is discarded
        // by `wrap` — only its `data` carries through to the returned widget.
        let source = Theme::new(self.source_data.clone(), SizedBox::shrink());
        source.wrap(
            ctx,
            ThemeCapture {
                captured: Arc::clone(&self.captured),
            }
            .boxed(),
        )
    }
}

/// `InheritedTheme::wrap(child)` must yield a widget that provides the
/// wrapping `Theme`'s data to `child` — the behavior the trait exists for
/// (a future capture/re-parent mechanism reuses `wrap` to carry a theme
/// across a subtree boundary; see [`flui_widgets::InheritedTheme`]'s module
/// docs). The assertion fails if `wrap` drops the data, substitutes a
/// default, or fails to actually provide `child` with any `Theme` ancestor
/// at all.
#[test]
fn inherited_theme_wrap_provides_the_source_theme_data_to_the_wrapped_child() {
    let captured: Arc<Mutex<Option<Option<ThemeData>>>> = Arc::new(Mutex::new(None));
    // Sentinel distinct from both presets so the assertion fails if `wrap`
    // silently substituted a default theme instead of the source's data.
    let sentinel = Color::from_argb(0xFF00_AA55);
    let base = ThemeData::light();
    let scheme = base.color_scheme.copy_with(ColorSchemeOverrides {
        primary: Some(sentinel),
        ..Default::default()
    });
    let source_data = base.copy_with(ThemeDataOverrides {
        color_scheme: Some(scheme),
        ..Default::default()
    });

    let _laid = lay_out(
        WrapHost {
            source_data: source_data.clone(),
            captured: Arc::clone(&captured),
        },
        loose(100.0),
    );

    let outer = captured.lock().unwrap().clone();
    let got = outer
        .expect("WrapHost::build was never called — the harness did not traverse the subtree")
        .expect("wrap's returned widget did not provide any Theme ancestor to the wrapped child");

    assert_eq!(
        got, source_data,
        "InheritedTheme::wrap should provide the source Theme's data to the \
         wrapped child, not a default or the wrong scope"
    );
}
