//! [`ColorScheme`] — the Material 3 color-role palette.
//!
//! The field list and the `light`/`dark` default hex values are the two M3
//! baseline schemes.
//!
//! ## Deferred: seed-color derivation
//!
//! Deriving a full palette from one seed color needs the HCT/tonal-palette
//! algorithm from Material's color utilities. That needs its own crate (or an
//! equivalent dependency) validated against a literal expected-output table —
//! out of scope for this theming-foundation unit, which ships the two fixed M3
//! baseline schemes ([`ColorScheme::light`], [`ColorScheme::dark`]) only.
//! Tracked as a named follow-up gated on a standalone spike.
//!
//! ## No deprecated roles
//!
//! Flutter still carries `surfaceVariant`, `background` and `onBackground`
//! as `@Deprecated` roles (`flutter/flutter@3.50.1
//! packages/flutter/lib/src/material/color_scheme.dart:1027,1142,1155`).
//! This scheme has none of them: read
//! [`surface_container_highest`](ColorScheme::surface_container_highest),
//! [`surface`](ColorScheme::surface) and
//! [`on_surface`](ColorScheme::on_surface) in their place.

use flui_sdk::painting::styling::Color;
use flui_sdk::platform::Brightness;

/// The Material 3 color roles
/// (<https://m3.material.io/styles/color/the-color-system/color-roles>).
///
/// Construct one of the two M3 baseline schemes with
/// [`ColorScheme::light`] / [`ColorScheme::dark`], then assign the roles to
/// change:
///
/// ```
/// use flui_material::ColorScheme;
/// use flui_sdk::painting::styling::Color;
///
/// let mut scheme = ColorScheme::light();
/// scheme.primary = Color::from_argb(0xFF00_66CC);
/// assert_eq!(scheme.primary, Color::from_argb(0xFF00_66CC));
/// ```
///
/// `#[non_exhaustive]`: the M3 role list has grown over time (most recently
/// the `*Fixed`/`*FixedDim` roles), so construction always goes through a
/// named constructor rather than a struct literal.
///
/// 47 fields: 46 color roles plus `brightness`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorScheme {
    /// Whether this scheme is designed for a light or dark surface.
    pub brightness: Brightness,

    /// The primary color: high-emphasis fills and text on the surface.
    pub primary: Color,

    /// Content (text, icons) drawn on [`primary`](Self::primary).
    pub on_primary: Color,

    /// A lower-emphasis fill for containers tied to the primary color.
    pub primary_container: Color,

    /// Content drawn on [`primary_container`](Self::primary_container).
    pub on_primary_container: Color,

    /// A primary-tone fill that stays the same in light and dark schemes.
    pub primary_fixed: Color,

    /// A dimmer variant of [`primary_fixed`](Self::primary_fixed).
    pub primary_fixed_dim: Color,

    /// Content drawn on [`primary_fixed`](Self::primary_fixed).
    pub on_primary_fixed: Color,

    /// Lower-emphasis content drawn on [`primary_fixed`](Self::primary_fixed).
    pub on_primary_fixed_variant: Color,

    /// The secondary color: less prominent components such as filter chips.
    pub secondary: Color,

    /// Content drawn on [`secondary`](Self::secondary).
    pub on_secondary: Color,

    /// A lower-emphasis fill for containers tied to the secondary color.
    pub secondary_container: Color,

    /// Content drawn on [`secondary_container`](Self::secondary_container).
    pub on_secondary_container: Color,

    /// A secondary-tone fill that stays the same in light and dark schemes.
    pub secondary_fixed: Color,

    /// A dimmer variant of [`secondary_fixed`](Self::secondary_fixed).
    pub secondary_fixed_dim: Color,

    /// Content drawn on [`secondary_fixed`](Self::secondary_fixed).
    pub on_secondary_fixed: Color,

    /// Lower-emphasis content drawn on
    /// [`secondary_fixed`](Self::secondary_fixed).
    pub on_secondary_fixed_variant: Color,

    /// The tertiary color: contrasting accents that balance primary and
    /// secondary.
    pub tertiary: Color,

    /// Content drawn on [`tertiary`](Self::tertiary).
    pub on_tertiary: Color,

    /// A lower-emphasis fill for containers tied to the tertiary color.
    pub tertiary_container: Color,

    /// Content drawn on [`tertiary_container`](Self::tertiary_container).
    pub on_tertiary_container: Color,

    /// A tertiary-tone fill that stays the same in light and dark schemes.
    pub tertiary_fixed: Color,

    /// A dimmer variant of [`tertiary_fixed`](Self::tertiary_fixed).
    pub tertiary_fixed_dim: Color,

    /// Content drawn on [`tertiary_fixed`](Self::tertiary_fixed).
    pub on_tertiary_fixed: Color,

    /// Lower-emphasis content drawn on
    /// [`tertiary_fixed`](Self::tertiary_fixed).
    pub on_tertiary_fixed_variant: Color,

    /// The color for errors and destructive states.
    pub error: Color,

    /// Content drawn on [`error`](Self::error).
    pub on_error: Color,

    /// A lower-emphasis fill for error containers.
    pub error_container: Color,

    /// Content drawn on [`error_container`](Self::error_container).
    pub on_error_container: Color,

    /// The base surface color for components such as cards and sheets.
    pub surface: Color,

    /// Content drawn on [`surface`](Self::surface).
    pub on_surface: Color,

    /// The dimmest surface tone, for surfaces that recede.
    pub surface_dim: Color,

    /// The brightest surface tone, for surfaces that stand out.
    pub surface_bright: Color,

    /// The lowest of the five surface-container elevations.
    pub surface_container_lowest: Color,

    /// A surface container one step above the lowest elevation.
    pub surface_container_low: Color,

    /// The default surface container elevation.
    pub surface_container: Color,

    /// A surface container one step below the highest elevation.
    pub surface_container_high: Color,

    /// The highest of the five surface-container elevations.
    pub surface_container_highest: Color,

    /// Lower-emphasis content drawn on a surface.
    pub on_surface_variant: Color,

    /// A subtle border color, for example around text fields.
    pub outline: Color,

    /// A lower-emphasis border color, for example for dividers.
    pub outline_variant: Color,

    /// The color of drop shadows.
    pub shadow: Color,

    /// The color of scrims drawn over content behind modal surfaces.
    pub scrim: Color,

    /// A surface with the opposite brightness, for example for snack bars.
    pub inverse_surface: Color,

    /// Content drawn on [`inverse_surface`](Self::inverse_surface).
    pub on_inverse_surface: Color,

    /// A primary-tone accent for use on
    /// [`inverse_surface`](Self::inverse_surface).
    pub inverse_primary: Color,

    /// The tint applied to surfaces to indicate elevation.
    ///
    /// Set to the same color as `primary` in both baseline tables.
    pub surface_tint: Color,
}

impl ColorScheme {
    /// The default Material 3 light color scheme.
    ///
    /// Every hex value below is the fixed M3 baseline, not recomputed. This
    /// is the scheme a plain `ThemeData::default()` gets, not the legacy M2
    /// baseline.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            brightness: Brightness::Light,
            primary: Color::from_argb(0xFF67_50A4), // primary
            on_primary: Color::from_argb(0xFFFF_FFFF), // onPrimary
            primary_container: Color::from_argb(0xFFEA_DDFF), // primaryContainer
            on_primary_container: Color::from_argb(0xFF4F_378B), // onPrimaryContainer
            primary_fixed: Color::from_argb(0xFFEA_DDFF), // primaryFixed
            primary_fixed_dim: Color::from_argb(0xFFD0_BCFF), // primaryFixedDim
            on_primary_fixed: Color::from_argb(0xFF21_005D), // onPrimaryFixed
            on_primary_fixed_variant: Color::from_argb(0xFF4F_378B), // onPrimaryFixedVariant
            secondary: Color::from_argb(0xFF62_5B71), // secondary
            on_secondary: Color::from_argb(0xFFFF_FFFF), // onSecondary
            secondary_container: Color::from_argb(0xFFE8_DEF8), // secondaryContainer
            on_secondary_container: Color::from_argb(0xFF4A_4458), // onSecondaryContainer
            secondary_fixed: Color::from_argb(0xFFE8_DEF8), // secondaryFixed
            secondary_fixed_dim: Color::from_argb(0xFFCC_C2DC), // secondaryFixedDim
            on_secondary_fixed: Color::from_argb(0xFF1D_192B), // onSecondaryFixed
            on_secondary_fixed_variant: Color::from_argb(0xFF4A_4458), // onSecondaryFixedVariant
            tertiary: Color::from_argb(0xFF7D_5260), // tertiary
            on_tertiary: Color::from_argb(0xFFFF_FFFF), // onTertiary
            tertiary_container: Color::from_argb(0xFFFF_D8E4), // tertiaryContainer
            on_tertiary_container: Color::from_argb(0xFF63_3B48), // onTertiaryContainer
            tertiary_fixed: Color::from_argb(0xFFFF_D8E4), // tertiaryFixed
            tertiary_fixed_dim: Color::from_argb(0xFFEF_B8C8), // tertiaryFixedDim
            on_tertiary_fixed: Color::from_argb(0xFF31_111D), // onTertiaryFixed
            on_tertiary_fixed_variant: Color::from_argb(0xFF63_3B48), // onTertiaryFixedVariant
            error: Color::from_argb(0xFFB3_261E),   // error
            on_error: Color::from_argb(0xFFFF_FFFF), // onError
            error_container: Color::from_argb(0xFFF9_DEDC), // errorContainer
            on_error_container: Color::from_argb(0xFF8C_1D18), // onErrorContainer
            surface: Color::from_argb(0xFFFE_F7FF), // surface
            on_surface: Color::from_argb(0xFF1D_1B20), // onSurface
            surface_dim: Color::from_argb(0xFFDE_D8E1), // surfaceDim
            surface_bright: Color::from_argb(0xFFFE_F7FF), // surfaceBright
            surface_container_lowest: Color::from_argb(0xFFFF_FFFF), // surfaceContainerLowest
            surface_container_low: Color::from_argb(0xFFF7_F2FA), // surfaceContainerLow
            surface_container: Color::from_argb(0xFFF3_EDF7), // surfaceContainer
            surface_container_high: Color::from_argb(0xFFEC_E6F0), // surfaceContainerHigh
            surface_container_highest: Color::from_argb(0xFFE6_E0E9), // surfaceContainerHighest
            on_surface_variant: Color::from_argb(0xFF49_454F), // onSurfaceVariant
            outline: Color::from_argb(0xFF79_747E), // outline
            outline_variant: Color::from_argb(0xFFCA_C4D0), // outlineVariant
            shadow: Color::from_argb(0xFF00_0000),  // shadow
            scrim: Color::from_argb(0xFF00_0000),   // scrim
            inverse_surface: Color::from_argb(0xFF32_2F35), // inverseSurface
            on_inverse_surface: Color::from_argb(0xFFF5_EFF7), // onInverseSurface
            inverse_primary: Color::from_argb(0xFFD0_BCFF), // inversePrimary
            surface_tint: Color::from_argb(0xFF67_50A4), // surfaceTint
        }
    }

    /// The default Material 3 dark color scheme.
    ///
    /// The fixed M3 dark baseline — see [`ColorScheme::light`]'s doc comment
    /// for the defaulting rationale, mirrored here for the dark branch.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            brightness: Brightness::Dark,
            primary: Color::from_argb(0xFFD0_BCFF), // primary
            on_primary: Color::from_argb(0xFF38_1E72), // onPrimary
            primary_container: Color::from_argb(0xFF4F_378B), // primaryContainer
            on_primary_container: Color::from_argb(0xFFEA_DDFF), // onPrimaryContainer
            primary_fixed: Color::from_argb(0xFFEA_DDFF), // primaryFixed
            primary_fixed_dim: Color::from_argb(0xFFD0_BCFF), // primaryFixedDim
            on_primary_fixed: Color::from_argb(0xFF21_005D), // onPrimaryFixed
            on_primary_fixed_variant: Color::from_argb(0xFF4F_378B), // onPrimaryFixedVariant
            secondary: Color::from_argb(0xFFCC_C2DC), // secondary
            on_secondary: Color::from_argb(0xFF33_2D41), // onSecondary
            secondary_container: Color::from_argb(0xFF4A_4458), // secondaryContainer
            on_secondary_container: Color::from_argb(0xFFE8_DEF8), // onSecondaryContainer
            secondary_fixed: Color::from_argb(0xFFE8_DEF8), // secondaryFixed
            secondary_fixed_dim: Color::from_argb(0xFFCC_C2DC), // secondaryFixedDim
            on_secondary_fixed: Color::from_argb(0xFF1D_192B), // onSecondaryFixed
            on_secondary_fixed_variant: Color::from_argb(0xFF4A_4458), // onSecondaryFixedVariant
            tertiary: Color::from_argb(0xFFEF_B8C8), // tertiary
            on_tertiary: Color::from_argb(0xFF49_2532), // onTertiary
            tertiary_container: Color::from_argb(0xFF63_3B48), // tertiaryContainer
            on_tertiary_container: Color::from_argb(0xFFFF_D8E4), // onTertiaryContainer
            tertiary_fixed: Color::from_argb(0xFFFF_D8E4), // tertiaryFixed
            tertiary_fixed_dim: Color::from_argb(0xFFEF_B8C8), // tertiaryFixedDim
            on_tertiary_fixed: Color::from_argb(0xFF31_111D), // onTertiaryFixed
            on_tertiary_fixed_variant: Color::from_argb(0xFF63_3B48), // onTertiaryFixedVariant
            error: Color::from_argb(0xFFF2_B8B5),   // error
            on_error: Color::from_argb(0xFF60_1410), // onError
            error_container: Color::from_argb(0xFF8C_1D18), // errorContainer
            on_error_container: Color::from_argb(0xFFF9_DEDC), // onErrorContainer
            surface: Color::from_argb(0xFF14_1218), // surface
            on_surface: Color::from_argb(0xFFE6_E0E9), // onSurface
            surface_dim: Color::from_argb(0xFF14_1218), // surfaceDim
            surface_bright: Color::from_argb(0xFF3B_383E), // surfaceBright
            surface_container_lowest: Color::from_argb(0xFF0F_0D13), // surfaceContainerLowest
            surface_container_low: Color::from_argb(0xFF1D_1B20), // surfaceContainerLow
            surface_container: Color::from_argb(0xFF21_1F26), // surfaceContainer
            surface_container_high: Color::from_argb(0xFF2B_2930), // surfaceContainerHigh
            surface_container_highest: Color::from_argb(0xFF36_343B), // surfaceContainerHighest
            on_surface_variant: Color::from_argb(0xFFCA_C4D0), // onSurfaceVariant
            outline: Color::from_argb(0xFF93_8F99), // outline
            outline_variant: Color::from_argb(0xFF49_454F), // outlineVariant
            shadow: Color::from_argb(0xFF00_0000),  // shadow
            scrim: Color::from_argb(0xFF00_0000),   // scrim
            inverse_surface: Color::from_argb(0xFFE6_E0E9), // inverseSurface
            on_inverse_surface: Color::from_argb(0xFF32_2F35), // onInverseSurface
            inverse_primary: Color::from_argb(0xFF67_50A4), // inversePrimary
            surface_tint: Color::from_argb(0xFFD0_BCFF), // surfaceTint
        }
    }
}

impl Default for ColorScheme {
    /// The M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}
