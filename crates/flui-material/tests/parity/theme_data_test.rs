//! Parity: `packages/flutter/test/material/theme_data_test.dart` (oracle tag
//! `3.44.0`).
//!
//! ## Ported
//!
//! - The non-`lerp` half of `'Theme data control test'`: `dark ==
//!   dark.copyWith()` — as [`copy_with_no_args_is_identity`].
//! - `'copyWith, ==, hashCode basics'`, the equality half (`hashCode` has no
//!   FLUI equivalent — `ThemeData` does not need to be a `HashMap` key) —
//!   as [`copy_with_no_args_is_identity`] (same claim as the control test,
//!   ported once).
//! - `'Default text theme contrasts with brightness'` — the oracle asserts
//!   `theme.textTheme.bodyLarge!.color != theme.primaryColorDark` (a proxy
//!   for "the default text color reads legibly against the theme's
//!   surface"); this crate's stronger, exact claim — every role's default
//!   color equals `color_scheme.on_surface`, the actual M3 contract — is
//!   [`default_text_theme_role_colors_equal_on_surface`], which subsumes the
//!   oracle's weaker inequality check.
//!
//! ## Not ported, with reason
//!
//! - `'Theme data control test'`'s `ThemeData.lerp` half — `AnimatedTheme`/
//!   `ColorScheme`/`TextTheme` lerp are deferred (see crate root docs).
//! - `'ThemeData objects with .styleFrom() members are equal'`, `'==` and
//!   `hashCode` include focusColor and hoverColor'`, and the button/slider/
//!   dialog component-theme `copyWith` tests — all exercise component-theme
//!   slots (`ElevatedButtonThemeData`, `SliderThemeData`, …) this crate
//!   does not implement yet (see crate root docs: component themes land
//!   with their owning widgets).
//! - `'Defaults to the default typography for the platform'` — exercises
//!   `Typography.material2018` (the M2 typography table) and
//!   `useMaterial3: false`; this crate is M3-only (no M2 mode).
//! - `'Default icon theme contrasts with brightness'` / `'Default primary
//!   icon theme contrasts with primary brightness'` — `iconTheme` is a
//!   deferred `ThemeData` slot.
//! - `'light, dark and fallback constructors support useMaterial3'`,
//!   `'Can control fontFamily default'`, `'Can estimate brightness -
//!   directly'`, `'cursorColor'`, the `colorSchemeSeed` tests, and
//!   `'ThemeData diagnostics include all properties'` — all exercise
//!   API surface (`useMaterial3` toggle, `fontFamily`, `cursorColor`,
//!   `colorSchemeSeed`, `Diagnosticable`) this crate does not implement.

use flui_material::{ThemeData, ThemeDataOverrides};
use flui_types::platform::Brightness;

/// Ports `dark == dark.copyWith()` from `'Theme data control test'` and the
/// equivalent half of `'copyWith, ==, hashCode basics'` — both assert the
/// same claim (`copyWith()`/`copy_with` with no overrides is the identity),
/// so one test here covers both oracle test bodies.
#[test]
fn copy_with_no_args_is_identity() {
    let dark = ThemeData::dark();
    assert_eq!(dark, dark.copy_with(ThemeDataOverrides::default()));

    let light = ThemeData::default();
    assert_eq!(light, light.copy_with(ThemeDataOverrides::default()));
}

/// Ports the *contract* `'Default text theme contrasts with brightness'`
/// gestures at (default text is legible against the theme's surfaces) as
/// the exact M3 claim FLUI implements: every default `TextTheme` role's
/// color is `color_scheme.on_surface` — see `default_text_theme`'s doc
/// comment in `src/theme_data.rs` for the derivation.
#[test]
fn default_text_theme_role_colors_equal_on_surface() {
    // A helper `fn`, not a `[ThemeData; 2]`/`vec![...]` loop — see
    // `src/theme_data.rs`'s identical fix: a stack array of two
    // `ThemeData`s trips clippy's `large_stack_arrays` lint, and a `Vec`
    // consumed immediately by the loop trips `useless_vec` right back.
    fn assert_role_colors_equal_on_surface(theme: &ThemeData) {
        for role in theme.text_theme.roles() {
            let style = role.expect("every englishLike2021 role is populated");
            assert_eq!(
                style.color,
                Some(theme.color_scheme.on_surface),
                "default text theme role color must equal on_surface for brightness {:?}",
                theme.brightness()
            );
        }
    }

    assert_role_colors_equal_on_surface(&ThemeData::light());
    assert_role_colors_equal_on_surface(&ThemeData::dark());
}

#[test]
fn brightness_matches_both_presets() {
    assert_eq!(ThemeData::light().brightness(), Brightness::Light);
    assert_eq!(ThemeData::dark().brightness(), Brightness::Dark);
}
