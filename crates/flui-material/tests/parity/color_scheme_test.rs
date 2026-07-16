//! Parity: `packages/flutter/test/material/color_scheme_test.dart` and
//! `packages/flutter/test/material/theme_data_test.dart` (oracle tag
//! `3.44.0`).
//!
//! ## Ported
//!
//! - `'ThemeData can generate a default M3 light colorScheme when
//!   useMaterial3 is true'` (`theme_data_test.dart`) — every field the
//!   oracle asserts, as [`light_matches_default_m3_light_color_scheme`].
//!   This is the correct oracle anchor for [`ColorScheme::light`]: it is
//!   the literal `_colorSchemeLightM3` table (`theme_data.dart`) our
//!   `light()` ports, not `color_scheme_test.dart`'s `'light scheme matches
//!   the spec'` (see "Not ported" below).
//! - `'ThemeData.dark() can generate a default M3 dark colorScheme when
//!   useMaterial3 is true'` (`theme_data_test.dart`) — as
//!   [`dark_matches_default_m3_dark_color_scheme`].
//! - The intent of `'copyWith overrides given colors'`
//!   (`color_scheme_test.dart`) — every role passed to `copy_with` takes
//!   effect and every role omitted is unchanged — as
//!   [`copy_with_overrides_every_given_role`].
//!
//! ## Not ported, with reason
//!
//! - `'light scheme matches the spec'` / `'dark scheme matches the spec'`
//!   (`color_scheme_test.dart`) assert `ColorScheme.light()`/`.dark()` — the
//!   oracle's **M2 baseline fallback constructors** (most roles default to
//!   `primary`/`secondary`/`surface` via fallback logic, not independent
//!   values). This crate does not port the M2 baseline at all (see
//!   `src/color_scheme.rs` module docs) — `ColorScheme::light`/`dark` here
//!   port the M3 tables instead, so these two oracle tests do not apply to
//!   this crate's API.
//! - `'can generate a light/dark scheme from a seed color'`, `'can override
//!   specific colors in a generated scheme'`, and the `DynamicSchemeVariant`/
//!   contrast-level tests all exercise `ColorScheme.fromSeed`, which is
//!   deferred (see `src/color_scheme.rs` module docs).

use flui_material::{ColorScheme, ColorSchemeOverrides};
use flui_types::platform::Brightness;
use flui_types::styling::Color;

#[test]
fn light_matches_default_m3_light_color_scheme() {
    let scheme = ColorScheme::light();
    assert_eq!(scheme.primary, Color::from_argb(0xFF67_50A4));
    assert_eq!(scheme.on_primary, Color::from_argb(0xFFFF_FFFF));
    assert_eq!(scheme.primary_container, Color::from_argb(0xFFEA_DDFF));
    assert_eq!(scheme.on_primary_container, Color::from_argb(0xFF4F_378B));
    assert_eq!(scheme.primary_fixed, Color::from_argb(0xFFEA_DDFF));
    assert_eq!(scheme.primary_fixed_dim, Color::from_argb(0xFFD0_BCFF));
    assert_eq!(scheme.on_primary_fixed, Color::from_argb(0xFF21_005D));
    assert_eq!(
        scheme.on_primary_fixed_variant,
        Color::from_argb(0xFF4F_378B)
    );
    assert_eq!(scheme.secondary, Color::from_argb(0xFF62_5B71));
    assert_eq!(scheme.on_secondary, Color::from_argb(0xFFFF_FFFF));
    assert_eq!(scheme.secondary_container, Color::from_argb(0xFFE8_DEF8));
    assert_eq!(scheme.on_secondary_container, Color::from_argb(0xFF4A_4458));
    assert_eq!(scheme.secondary_fixed, Color::from_argb(0xFFE8_DEF8));
    assert_eq!(scheme.secondary_fixed_dim, Color::from_argb(0xFFCC_C2DC));
    assert_eq!(scheme.on_secondary_fixed, Color::from_argb(0xFF1D_192B));
    assert_eq!(
        scheme.on_secondary_fixed_variant,
        Color::from_argb(0xFF4A_4458)
    );
    assert_eq!(scheme.tertiary, Color::from_argb(0xFF7D_5260));
    assert_eq!(scheme.on_tertiary, Color::from_argb(0xFFFF_FFFF));
    assert_eq!(scheme.tertiary_container, Color::from_argb(0xFFFF_D8E4));
    assert_eq!(scheme.on_tertiary_container, Color::from_argb(0xFF63_3B48));
    assert_eq!(scheme.tertiary_fixed, Color::from_argb(0xFFFF_D8E4));
    assert_eq!(scheme.tertiary_fixed_dim, Color::from_argb(0xFFEF_B8C8));
    assert_eq!(scheme.on_tertiary_fixed, Color::from_argb(0xFF31_111D));
    assert_eq!(
        scheme.on_tertiary_fixed_variant,
        Color::from_argb(0xFF63_3B48)
    );
    assert_eq!(scheme.error, Color::from_argb(0xFFB3_261E));
    assert_eq!(scheme.on_error, Color::from_argb(0xFFFF_FFFF));
    assert_eq!(scheme.error_container, Color::from_argb(0xFFF9_DEDC));
    assert_eq!(scheme.on_error_container, Color::from_argb(0xFF8C_1D18));
    assert_eq!(scheme.outline, Color::from_argb(0xFF79_747E));
    assert_eq!(scheme.background, Color::from_argb(0xFFFE_F7FF));
    assert_eq!(scheme.on_background, Color::from_argb(0xFF1D_1B20));
    assert_eq!(scheme.surface, Color::from_argb(0xFFFE_F7FF));
    assert_eq!(scheme.on_surface, Color::from_argb(0xFF1D_1B20));
    assert_eq!(scheme.surface_variant, Color::from_argb(0xFFE7_E0EC));
    assert_eq!(scheme.on_surface_variant, Color::from_argb(0xFF49_454F));
    assert_eq!(scheme.surface_bright, Color::from_argb(0xFFFE_F7FF));
    assert_eq!(scheme.surface_dim, Color::from_argb(0xFFDE_D8E1));
    assert_eq!(scheme.surface_container, Color::from_argb(0xFFF3_EDF7));
    assert_eq!(
        scheme.surface_container_highest,
        Color::from_argb(0xFFE6_E0E9)
    );
    assert_eq!(scheme.surface_container_high, Color::from_argb(0xFFEC_E6F0));
    assert_eq!(
        scheme.surface_container_lowest,
        Color::from_argb(0xFFFF_FFFF)
    );
    assert_eq!(scheme.surface_container_low, Color::from_argb(0xFFF7_F2FA));
    assert_eq!(scheme.inverse_surface, Color::from_argb(0xFF32_2F35));
    assert_eq!(scheme.on_inverse_surface, Color::from_argb(0xFFF5_EFF7));
    assert_eq!(scheme.inverse_primary, Color::from_argb(0xFFD0_BCFF));
    assert_eq!(scheme.shadow, Color::from_argb(0xFF00_0000));
    assert_eq!(scheme.surface_tint, Color::from_argb(0xFF67_50A4));
    assert_eq!(scheme.brightness, Brightness::Light);
}

#[test]
fn dark_matches_default_m3_dark_color_scheme() {
    let scheme = ColorScheme::dark();
    assert_eq!(scheme.primary, Color::from_argb(0xFFD0_BCFF));
    assert_eq!(scheme.on_primary, Color::from_argb(0xFF38_1E72));
    assert_eq!(scheme.primary_container, Color::from_argb(0xFF4F_378B));
    assert_eq!(scheme.on_primary_container, Color::from_argb(0xFFEA_DDFF));
    assert_eq!(scheme.primary_fixed, Color::from_argb(0xFFEA_DDFF));
    assert_eq!(scheme.primary_fixed_dim, Color::from_argb(0xFFD0_BCFF));
    assert_eq!(scheme.on_primary_fixed, Color::from_argb(0xFF21_005D));
    assert_eq!(
        scheme.on_primary_fixed_variant,
        Color::from_argb(0xFF4F_378B)
    );
    assert_eq!(scheme.secondary, Color::from_argb(0xFFCC_C2DC));
    assert_eq!(scheme.on_secondary, Color::from_argb(0xFF33_2D41));
    assert_eq!(scheme.secondary_container, Color::from_argb(0xFF4A_4458));
    assert_eq!(scheme.on_secondary_container, Color::from_argb(0xFFE8_DEF8));
    assert_eq!(scheme.secondary_fixed, Color::from_argb(0xFFE8_DEF8));
    assert_eq!(scheme.secondary_fixed_dim, Color::from_argb(0xFFCC_C2DC));
    assert_eq!(scheme.on_secondary_fixed, Color::from_argb(0xFF1D_192B));
    assert_eq!(
        scheme.on_secondary_fixed_variant,
        Color::from_argb(0xFF4A_4458)
    );
    assert_eq!(scheme.tertiary, Color::from_argb(0xFFEF_B8C8));
    assert_eq!(scheme.on_tertiary, Color::from_argb(0xFF49_2532));
    assert_eq!(scheme.tertiary_container, Color::from_argb(0xFF63_3B48));
    assert_eq!(scheme.on_tertiary_container, Color::from_argb(0xFFFF_D8E4));
    assert_eq!(scheme.tertiary_fixed, Color::from_argb(0xFFFF_D8E4));
    assert_eq!(scheme.tertiary_fixed_dim, Color::from_argb(0xFFEF_B8C8));
    assert_eq!(scheme.on_tertiary_fixed, Color::from_argb(0xFF31_111D));
    assert_eq!(
        scheme.on_tertiary_fixed_variant,
        Color::from_argb(0xFF63_3B48)
    );
    assert_eq!(scheme.error, Color::from_argb(0xFFF2_B8B5));
    assert_eq!(scheme.on_error, Color::from_argb(0xFF60_1410));
    assert_eq!(scheme.error_container, Color::from_argb(0xFF8C_1D18));
    assert_eq!(scheme.on_error_container, Color::from_argb(0xFFF9_DEDC));
    assert_eq!(scheme.outline, Color::from_argb(0xFF93_8F99));
    assert_eq!(scheme.background, Color::from_argb(0xFF14_1218));
    assert_eq!(scheme.on_background, Color::from_argb(0xFFE6_E0E9));
    assert_eq!(scheme.surface, Color::from_argb(0xFF14_1218));
    assert_eq!(scheme.on_surface, Color::from_argb(0xFFE6_E0E9));
    assert_eq!(scheme.surface_variant, Color::from_argb(0xFF49_454F));
    assert_eq!(scheme.on_surface_variant, Color::from_argb(0xFFCA_C4D0));
    assert_eq!(scheme.surface_bright, Color::from_argb(0xFF3B_383E));
    assert_eq!(scheme.surface_dim, Color::from_argb(0xFF14_1218));
    assert_eq!(scheme.surface_container, Color::from_argb(0xFF21_1F26));
    assert_eq!(
        scheme.surface_container_highest,
        Color::from_argb(0xFF36_343B)
    );
    assert_eq!(scheme.surface_container_high, Color::from_argb(0xFF2B_2930));
    assert_eq!(
        scheme.surface_container_lowest,
        Color::from_argb(0xFF0F_0D13)
    );
    assert_eq!(scheme.surface_container_low, Color::from_argb(0xFF1D_1B20));
    assert_eq!(scheme.inverse_surface, Color::from_argb(0xFFE6_E0E9));
    assert_eq!(scheme.on_inverse_surface, Color::from_argb(0xFF32_2F35));
    assert_eq!(scheme.inverse_primary, Color::from_argb(0xFF67_50A4));
    assert_eq!(scheme.shadow, Color::from_argb(0xFF00_0000));
    assert_eq!(scheme.surface_tint, Color::from_argb(0xFFD0_BCFF));
    assert_eq!(scheme.brightness, Brightness::Dark);
}

/// Ports the intent of `'copyWith overrides given colors'`
/// (`color_scheme_test.dart`) against this crate's `copy_with`: every role
/// passed a value takes effect, and (the direction the oracle test doesn't
/// separately check, since it overrides every role at once) a role left
/// `None` keeps the base value.
#[test]
fn copy_with_overrides_every_given_role() {
    let base = ColorScheme::light();
    let sentinel = |n: u8| Color::from_argb(0xFF00_0000 | u32::from(n));

    let patched = base.copy_with(ColorSchemeOverrides {
        brightness: Some(Brightness::Dark),
        primary: Some(sentinel(1)),
        on_primary: Some(sentinel(2)),
        primary_container: Some(sentinel(3)),
        surface_tint: Some(sentinel(4)),
        background: Some(sentinel(5)),
        surface_variant: Some(sentinel(6)),
        ..Default::default()
    });

    assert_eq!(patched.brightness, Brightness::Dark);
    assert_eq!(patched.primary, sentinel(1));
    assert_eq!(patched.on_primary, sentinel(2));
    assert_eq!(patched.primary_container, sentinel(3));
    assert_eq!(patched.surface_tint, sentinel(4));
    assert_eq!(patched.background, sentinel(5));
    assert_eq!(patched.surface_variant, sentinel(6));

    // Every role NOT passed to `copy_with` must equal the base, unchanged.
    assert_eq!(patched.on_primary_container, base.on_primary_container);
    assert_eq!(patched.secondary, base.secondary);
    assert_eq!(patched.error, base.error);
    assert_eq!(patched.surface, base.surface);
    assert_eq!(patched.outline, base.outline);
    assert_eq!(patched.inverse_primary, base.inverse_primary);
}
