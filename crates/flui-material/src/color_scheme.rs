//! [`ColorScheme`] — the full Material 3 color-role palette.
//!
//! Flutter parity: `material/color_scheme.dart` `ColorScheme` (oracle tag
//! `3.44.0`). Field list, order, and the `light`/`dark` default hex values are
//! ported verbatim from the oracle's `ColorScheme` constructor and its
//! generated `_colorSchemeLightM3` / `_colorSchemeDarkM3` const tables
//! (`material/theme_data.dart`, oracle tag `3.44.0`).
//!
//! ## Deferred: `ColorScheme.fromSeed`
//!
//! Flutter derives a full palette from one seed color via the
//! `material_color_utilities` HCT/tonal-palette algorithm
//! (`ColorScheme.fromSeed`, `color_scheme.dart`). Porting that algorithm needs
//! its own crate (or a `material-colors`-equivalent dependency) validated
//! against `color_scheme_test.dart`'s literal expected-output table — out of
//! scope for this theming-foundation unit, which ships the two fixed M3
//! baseline schemes ([`ColorScheme::light`], [`ColorScheme::dark`]) only.
//! Tracked as a named follow-up gated on a standalone spike.
//!
//! ## Deprecated-but-included roles
//!
//! `background`/`on_background`/`surface_variant` are deprecated in Flutter
//! (superseded by `surface`/`on_surface`/`surface_container_highest`) but are
//! kept here as normal fields: the oracle's own default-value tables still
//! populate them, and dropping them would silently fail parity assertions
//! against `color_scheme_test.dart`.

use flui_types::platform::Brightness;
use flui_types::styling::Color;

/// The full set of Material 3 color roles
/// (<https://m3.material.io/styles/color/the-color-system/color-roles>).
///
/// Construct one of the two ported M3 baseline schemes with
/// [`ColorScheme::light`] / [`ColorScheme::dark`], then adjust individual
/// roles with [`ColorScheme::copy_with`]. `#[non_exhaustive]`: Flutter's own
/// role list has grown across releases (most recently the `*Fixed`/`*FixedDim`
/// roles), so construction always goes through a named constructor rather
/// than a struct literal.
///
/// Flutter parity: `ColorScheme` (`material/color_scheme.dart`, oracle tag
/// `3.44.0`) — 50 fields (49 color roles + `brightness`), matching the
/// oracle's constructor field count exactly, including the three deprecated
/// roles (see module docs).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorScheme {
    /// Whether this scheme is designed for a light or dark surface.
    ///
    /// Flutter parity: `ColorScheme.brightness`.
    pub brightness: Brightness,

    /// Flutter parity: `ColorScheme.primary`.
    pub primary: Color,

    /// Flutter parity: `ColorScheme.onPrimary`.
    pub on_primary: Color,

    /// Flutter parity: `ColorScheme.primaryContainer`.
    pub primary_container: Color,

    /// Flutter parity: `ColorScheme.onPrimaryContainer`.
    pub on_primary_container: Color,

    /// Flutter parity: `ColorScheme.primaryFixed`.
    pub primary_fixed: Color,

    /// Flutter parity: `ColorScheme.primaryFixedDim`.
    pub primary_fixed_dim: Color,

    /// Flutter parity: `ColorScheme.onPrimaryFixed`.
    pub on_primary_fixed: Color,

    /// Flutter parity: `ColorScheme.onPrimaryFixedVariant`.
    pub on_primary_fixed_variant: Color,

    /// Flutter parity: `ColorScheme.secondary`.
    pub secondary: Color,

    /// Flutter parity: `ColorScheme.onSecondary`.
    pub on_secondary: Color,

    /// Flutter parity: `ColorScheme.secondaryContainer`.
    pub secondary_container: Color,

    /// Flutter parity: `ColorScheme.onSecondaryContainer`.
    pub on_secondary_container: Color,

    /// Flutter parity: `ColorScheme.secondaryFixed`.
    pub secondary_fixed: Color,

    /// Flutter parity: `ColorScheme.secondaryFixedDim`.
    pub secondary_fixed_dim: Color,

    /// Flutter parity: `ColorScheme.onSecondaryFixed`.
    pub on_secondary_fixed: Color,

    /// Flutter parity: `ColorScheme.onSecondaryFixedVariant`.
    pub on_secondary_fixed_variant: Color,

    /// Flutter parity: `ColorScheme.tertiary`.
    pub tertiary: Color,

    /// Flutter parity: `ColorScheme.onTertiary`.
    pub on_tertiary: Color,

    /// Flutter parity: `ColorScheme.tertiaryContainer`.
    pub tertiary_container: Color,

    /// Flutter parity: `ColorScheme.onTertiaryContainer`.
    pub on_tertiary_container: Color,

    /// Flutter parity: `ColorScheme.tertiaryFixed`.
    pub tertiary_fixed: Color,

    /// Flutter parity: `ColorScheme.tertiaryFixedDim`.
    pub tertiary_fixed_dim: Color,

    /// Flutter parity: `ColorScheme.onTertiaryFixed`.
    pub on_tertiary_fixed: Color,

    /// Flutter parity: `ColorScheme.onTertiaryFixedVariant`.
    pub on_tertiary_fixed_variant: Color,

    /// Flutter parity: `ColorScheme.error`.
    pub error: Color,

    /// Flutter parity: `ColorScheme.onError`.
    pub on_error: Color,

    /// Flutter parity: `ColorScheme.errorContainer`.
    pub error_container: Color,

    /// Flutter parity: `ColorScheme.onErrorContainer`.
    pub on_error_container: Color,

    /// Flutter parity: `ColorScheme.surface`.
    pub surface: Color,

    /// Flutter parity: `ColorScheme.onSurface`.
    pub on_surface: Color,

    /// Flutter parity: `ColorScheme.surfaceDim`.
    pub surface_dim: Color,

    /// Flutter parity: `ColorScheme.surfaceBright`.
    pub surface_bright: Color,

    /// Flutter parity: `ColorScheme.surfaceContainerLowest`.
    pub surface_container_lowest: Color,

    /// Flutter parity: `ColorScheme.surfaceContainerLow`.
    pub surface_container_low: Color,

    /// Flutter parity: `ColorScheme.surfaceContainer`.
    pub surface_container: Color,

    /// Flutter parity: `ColorScheme.surfaceContainerHigh`.
    pub surface_container_high: Color,

    /// Flutter parity: `ColorScheme.surfaceContainerHighest`.
    pub surface_container_highest: Color,

    /// Flutter parity: `ColorScheme.onSurfaceVariant`.
    pub on_surface_variant: Color,

    /// Flutter parity: `ColorScheme.outline`.
    pub outline: Color,

    /// Flutter parity: `ColorScheme.outlineVariant`.
    pub outline_variant: Color,

    /// Flutter parity: `ColorScheme.shadow`.
    pub shadow: Color,

    /// Flutter parity: `ColorScheme.scrim`.
    pub scrim: Color,

    /// Flutter parity: `ColorScheme.inverseSurface`.
    pub inverse_surface: Color,

    /// Flutter parity: `ColorScheme.onInverseSurface`.
    pub on_inverse_surface: Color,

    /// Flutter parity: `ColorScheme.inversePrimary`.
    pub inverse_primary: Color,

    /// Flutter parity: `ColorScheme.surfaceTint`.
    ///
    /// Set to the same color as `primary` (see the oracle's inline comment on both const tables).
    pub surface_tint: Color,

    /// Flutter parity: `ColorScheme.background`.
    ///
    /// **Deprecated in Flutter** (`3.18.0-0.1.pre`) in favor of `surface`; kept here because the oracle's default-value table still asserts it.
    pub background: Color,

    /// Flutter parity: `ColorScheme.onBackground`.
    ///
    /// **Deprecated in Flutter** (`3.18.0-0.1.pre`) in favor of `on_surface`; kept for the same reason as `background`.
    pub on_background: Color,

    /// Flutter parity: `ColorScheme.surfaceVariant`.
    ///
    /// **Deprecated in Flutter** (`3.18.0-0.1.pre`) in favor of `surface_container_highest`; kept for the same reason as `background`.
    pub surface_variant: Color,
}

impl ColorScheme {
    /// The default Material 3 light color scheme.
    ///
    /// Verbatim port of the oracle's generated `_colorSchemeLightM3` const
    /// table (`material/theme_data.dart`, oracle tag `3.44.0`) — every hex
    /// value below is cited from that table, not recomputed. This is the
    /// table `ThemeData`'s factory constructor defaults to
    /// (`colorScheme ??= isDark ? _colorSchemeDarkM3 : _colorSchemeLightM3`,
    /// `theme_data.dart`, oracle tag `3.44.0`) whenever no seed color or
    /// explicit `colorScheme` is supplied — i.e. this is the scheme a plain
    /// `ThemeData()` gets in Flutter today, not the legacy M2
    /// `ColorScheme.light()` baseline.
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
            background: Color::from_argb(0xFFFE_F7FF), // background
            on_background: Color::from_argb(0xFF1D_1B20), // onBackground
            surface_variant: Color::from_argb(0xFFE7_E0EC), // surfaceVariant
        }
    }

    /// The default Material 3 dark color scheme.
    ///
    /// Verbatim port of the oracle's generated `_colorSchemeDarkM3` const
    /// table (`material/theme_data.dart`, oracle tag `3.44.0`) — see
    /// [`ColorScheme::light`]'s doc comment for the citation and defaulting
    /// rationale, mirrored here for the dark branch.
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
            background: Color::from_argb(0xFF14_1218), // background
            on_background: Color::from_argb(0xFFE6_E0E9), // onBackground
            surface_variant: Color::from_argb(0xFF49_454F), // surfaceVariant
        }
    }

    /// Return a copy of this scheme with the given roles replaced.
    ///
    /// Mirrors Flutter's `ColorScheme.copyWith(...)` (all-optional named
    /// parameters); build the patch with [`ColorSchemeOverrides::default`]
    /// and struct-update syntax:
    ///
    /// ```
    /// use flui_material::{ColorScheme, ColorSchemeOverrides};
    ///
    /// let scheme = ColorScheme::light().copy_with(ColorSchemeOverrides {
    ///     primary: Some(flui_types::styling::Color::from_argb(0xFF00_66CC)),
    ///     ..Default::default()
    /// });
    /// assert_eq!(scheme.primary, flui_types::styling::Color::from_argb(0xFF00_66CC));
    /// ```
    #[must_use]
    pub fn copy_with(&self, overrides: ColorSchemeOverrides) -> Self {
        Self {
            brightness: overrides.brightness.unwrap_or(self.brightness),
            primary: overrides.primary.unwrap_or(self.primary),
            on_primary: overrides.on_primary.unwrap_or(self.on_primary),
            primary_container: overrides
                .primary_container
                .unwrap_or(self.primary_container),
            on_primary_container: overrides
                .on_primary_container
                .unwrap_or(self.on_primary_container),
            primary_fixed: overrides.primary_fixed.unwrap_or(self.primary_fixed),
            primary_fixed_dim: overrides
                .primary_fixed_dim
                .unwrap_or(self.primary_fixed_dim),
            on_primary_fixed: overrides.on_primary_fixed.unwrap_or(self.on_primary_fixed),
            on_primary_fixed_variant: overrides
                .on_primary_fixed_variant
                .unwrap_or(self.on_primary_fixed_variant),
            secondary: overrides.secondary.unwrap_or(self.secondary),
            on_secondary: overrides.on_secondary.unwrap_or(self.on_secondary),
            secondary_container: overrides
                .secondary_container
                .unwrap_or(self.secondary_container),
            on_secondary_container: overrides
                .on_secondary_container
                .unwrap_or(self.on_secondary_container),
            secondary_fixed: overrides.secondary_fixed.unwrap_or(self.secondary_fixed),
            secondary_fixed_dim: overrides
                .secondary_fixed_dim
                .unwrap_or(self.secondary_fixed_dim),
            on_secondary_fixed: overrides
                .on_secondary_fixed
                .unwrap_or(self.on_secondary_fixed),
            on_secondary_fixed_variant: overrides
                .on_secondary_fixed_variant
                .unwrap_or(self.on_secondary_fixed_variant),
            tertiary: overrides.tertiary.unwrap_or(self.tertiary),
            on_tertiary: overrides.on_tertiary.unwrap_or(self.on_tertiary),
            tertiary_container: overrides
                .tertiary_container
                .unwrap_or(self.tertiary_container),
            on_tertiary_container: overrides
                .on_tertiary_container
                .unwrap_or(self.on_tertiary_container),
            tertiary_fixed: overrides.tertiary_fixed.unwrap_or(self.tertiary_fixed),
            tertiary_fixed_dim: overrides
                .tertiary_fixed_dim
                .unwrap_or(self.tertiary_fixed_dim),
            on_tertiary_fixed: overrides
                .on_tertiary_fixed
                .unwrap_or(self.on_tertiary_fixed),
            on_tertiary_fixed_variant: overrides
                .on_tertiary_fixed_variant
                .unwrap_or(self.on_tertiary_fixed_variant),
            error: overrides.error.unwrap_or(self.error),
            on_error: overrides.on_error.unwrap_or(self.on_error),
            error_container: overrides.error_container.unwrap_or(self.error_container),
            on_error_container: overrides
                .on_error_container
                .unwrap_or(self.on_error_container),
            surface: overrides.surface.unwrap_or(self.surface),
            on_surface: overrides.on_surface.unwrap_or(self.on_surface),
            surface_dim: overrides.surface_dim.unwrap_or(self.surface_dim),
            surface_bright: overrides.surface_bright.unwrap_or(self.surface_bright),
            surface_container_lowest: overrides
                .surface_container_lowest
                .unwrap_or(self.surface_container_lowest),
            surface_container_low: overrides
                .surface_container_low
                .unwrap_or(self.surface_container_low),
            surface_container: overrides
                .surface_container
                .unwrap_or(self.surface_container),
            surface_container_high: overrides
                .surface_container_high
                .unwrap_or(self.surface_container_high),
            surface_container_highest: overrides
                .surface_container_highest
                .unwrap_or(self.surface_container_highest),
            on_surface_variant: overrides
                .on_surface_variant
                .unwrap_or(self.on_surface_variant),
            outline: overrides.outline.unwrap_or(self.outline),
            outline_variant: overrides.outline_variant.unwrap_or(self.outline_variant),
            shadow: overrides.shadow.unwrap_or(self.shadow),
            scrim: overrides.scrim.unwrap_or(self.scrim),
            inverse_surface: overrides.inverse_surface.unwrap_or(self.inverse_surface),
            on_inverse_surface: overrides
                .on_inverse_surface
                .unwrap_or(self.on_inverse_surface),
            inverse_primary: overrides.inverse_primary.unwrap_or(self.inverse_primary),
            surface_tint: overrides.surface_tint.unwrap_or(self.surface_tint),
            background: overrides.background.unwrap_or(self.background),
            on_background: overrides.on_background.unwrap_or(self.on_background),
            surface_variant: overrides.surface_variant.unwrap_or(self.surface_variant),
        }
    }
}

impl Default for ColorScheme {
    /// Same default as Flutter's `ThemeData()`: the M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}

/// Patch for [`ColorScheme::copy_with`] — every field mirrors a
/// [`ColorScheme`] role, `None` meaning "leave unchanged".
///
/// Flutter parity: the optional-parameter list of `ColorScheme.copyWith`
/// (`material/color_scheme.dart`, oracle tag `3.44.0`), reshaped as a
/// struct because Rust has no optional named parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColorSchemeOverrides {
    /// Overrides [`ColorScheme::brightness`].
    pub brightness: Option<Brightness>,

    /// Overrides [`ColorScheme::primary`].
    pub primary: Option<Color>,

    /// Overrides [`ColorScheme::on_primary`].
    pub on_primary: Option<Color>,

    /// Overrides [`ColorScheme::primary_container`].
    pub primary_container: Option<Color>,

    /// Overrides [`ColorScheme::on_primary_container`].
    pub on_primary_container: Option<Color>,

    /// Overrides [`ColorScheme::primary_fixed`].
    pub primary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::primary_fixed_dim`].
    pub primary_fixed_dim: Option<Color>,

    /// Overrides [`ColorScheme::on_primary_fixed`].
    pub on_primary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::on_primary_fixed_variant`].
    pub on_primary_fixed_variant: Option<Color>,

    /// Overrides [`ColorScheme::secondary`].
    pub secondary: Option<Color>,

    /// Overrides [`ColorScheme::on_secondary`].
    pub on_secondary: Option<Color>,

    /// Overrides [`ColorScheme::secondary_container`].
    pub secondary_container: Option<Color>,

    /// Overrides [`ColorScheme::on_secondary_container`].
    pub on_secondary_container: Option<Color>,

    /// Overrides [`ColorScheme::secondary_fixed`].
    pub secondary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::secondary_fixed_dim`].
    pub secondary_fixed_dim: Option<Color>,

    /// Overrides [`ColorScheme::on_secondary_fixed`].
    pub on_secondary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::on_secondary_fixed_variant`].
    pub on_secondary_fixed_variant: Option<Color>,

    /// Overrides [`ColorScheme::tertiary`].
    pub tertiary: Option<Color>,

    /// Overrides [`ColorScheme::on_tertiary`].
    pub on_tertiary: Option<Color>,

    /// Overrides [`ColorScheme::tertiary_container`].
    pub tertiary_container: Option<Color>,

    /// Overrides [`ColorScheme::on_tertiary_container`].
    pub on_tertiary_container: Option<Color>,

    /// Overrides [`ColorScheme::tertiary_fixed`].
    pub tertiary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::tertiary_fixed_dim`].
    pub tertiary_fixed_dim: Option<Color>,

    /// Overrides [`ColorScheme::on_tertiary_fixed`].
    pub on_tertiary_fixed: Option<Color>,

    /// Overrides [`ColorScheme::on_tertiary_fixed_variant`].
    pub on_tertiary_fixed_variant: Option<Color>,

    /// Overrides [`ColorScheme::error`].
    pub error: Option<Color>,

    /// Overrides [`ColorScheme::on_error`].
    pub on_error: Option<Color>,

    /// Overrides [`ColorScheme::error_container`].
    pub error_container: Option<Color>,

    /// Overrides [`ColorScheme::on_error_container`].
    pub on_error_container: Option<Color>,

    /// Overrides [`ColorScheme::surface`].
    pub surface: Option<Color>,

    /// Overrides [`ColorScheme::on_surface`].
    pub on_surface: Option<Color>,

    /// Overrides [`ColorScheme::surface_dim`].
    pub surface_dim: Option<Color>,

    /// Overrides [`ColorScheme::surface_bright`].
    pub surface_bright: Option<Color>,

    /// Overrides [`ColorScheme::surface_container_lowest`].
    pub surface_container_lowest: Option<Color>,

    /// Overrides [`ColorScheme::surface_container_low`].
    pub surface_container_low: Option<Color>,

    /// Overrides [`ColorScheme::surface_container`].
    pub surface_container: Option<Color>,

    /// Overrides [`ColorScheme::surface_container_high`].
    pub surface_container_high: Option<Color>,

    /// Overrides [`ColorScheme::surface_container_highest`].
    pub surface_container_highest: Option<Color>,

    /// Overrides [`ColorScheme::on_surface_variant`].
    pub on_surface_variant: Option<Color>,

    /// Overrides [`ColorScheme::outline`].
    pub outline: Option<Color>,

    /// Overrides [`ColorScheme::outline_variant`].
    pub outline_variant: Option<Color>,

    /// Overrides [`ColorScheme::shadow`].
    pub shadow: Option<Color>,

    /// Overrides [`ColorScheme::scrim`].
    pub scrim: Option<Color>,

    /// Overrides [`ColorScheme::inverse_surface`].
    pub inverse_surface: Option<Color>,

    /// Overrides [`ColorScheme::on_inverse_surface`].
    pub on_inverse_surface: Option<Color>,

    /// Overrides [`ColorScheme::inverse_primary`].
    pub inverse_primary: Option<Color>,

    /// Overrides [`ColorScheme::surface_tint`].
    pub surface_tint: Option<Color>,

    /// Overrides [`ColorScheme::background`].
    pub background: Option<Color>,

    /// Overrides [`ColorScheme::on_background`].
    pub on_background: Option<Color>,

    /// Overrides [`ColorScheme::surface_variant`].
    pub surface_variant: Option<Color>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle citation: `_colorSchemeLightM3` (`material/theme_data.dart`,
    /// oracle tag `3.44.0`). Spot-checks the roles most likely to regress
    /// silently (the fixed/fixed-dim roles were the newest additions to the
    /// oracle's field list) plus the three deprecated roles.
    #[test]
    fn light_matches_oracle_color_scheme_light_m3() {
        let scheme = ColorScheme::light();
        assert_eq!(scheme.brightness, Brightness::Light);
        assert_eq!(scheme.primary, Color::from_argb(0xFF67_50A4));
        assert_eq!(scheme.on_primary, Color::from_argb(0xFFFF_FFFF));
        assert_eq!(scheme.primary_fixed, Color::from_argb(0xFFEA_DDFF));
        assert_eq!(scheme.primary_fixed_dim, Color::from_argb(0xFFD0_BCFF));
        assert_eq!(scheme.surface, Color::from_argb(0xFFFE_F7FF));
        assert_eq!(
            scheme.surface_container_highest,
            Color::from_argb(0xFFE6_E0E9)
        );
        assert_eq!(scheme.surface_tint, scheme.primary);
        assert_eq!(scheme.background, Color::from_argb(0xFFFE_F7FF));
        assert_eq!(scheme.on_background, Color::from_argb(0xFF1D_1B20));
        assert_eq!(scheme.surface_variant, Color::from_argb(0xFFE7_E0EC));
    }

    /// Oracle citation: `_colorSchemeDarkM3` (`material/theme_data.dart`,
    /// oracle tag `3.44.0`). Mirrors the light-scheme spot-check above.
    #[test]
    fn dark_matches_oracle_color_scheme_dark_m3() {
        let scheme = ColorScheme::dark();
        assert_eq!(scheme.brightness, Brightness::Dark);
        assert_eq!(scheme.primary, Color::from_argb(0xFFD0_BCFF));
        assert_eq!(scheme.on_primary, Color::from_argb(0xFF38_1E72));
        assert_eq!(scheme.primary_fixed, Color::from_argb(0xFFEA_DDFF));
        assert_eq!(scheme.primary_fixed_dim, Color::from_argb(0xFFD0_BCFF));
        assert_eq!(scheme.surface, Color::from_argb(0xFF14_1218));
        assert_eq!(
            scheme.surface_container_highest,
            Color::from_argb(0xFF36_343B)
        );
        assert_eq!(scheme.surface_tint, scheme.primary);
        assert_eq!(scheme.background, Color::from_argb(0xFF14_1218));
        assert_eq!(scheme.on_background, Color::from_argb(0xFFE6_E0E9));
        assert_eq!(scheme.surface_variant, Color::from_argb(0xFF49_454F));
    }

    #[test]
    fn light_and_dark_are_distinct() {
        assert_ne!(ColorScheme::light(), ColorScheme::dark());
    }

    #[test]
    fn default_is_light() {
        assert_eq!(ColorScheme::default(), ColorScheme::light());
    }

    #[test]
    fn copy_with_overrides_only_the_given_roles() {
        let base = ColorScheme::light();
        let sentinel = Color::from_argb(0xFF12_3456);
        let patched = base.copy_with(ColorSchemeOverrides {
            primary: Some(sentinel),
            ..Default::default()
        });
        assert_eq!(patched.primary, sentinel);
        assert_eq!(patched.on_primary, base.on_primary);
        assert_eq!(patched.brightness, base.brightness);
        assert_eq!(patched.surface, base.surface);
    }

    #[test]
    fn copy_with_no_overrides_is_identity() {
        let base = ColorScheme::dark();
        assert_eq!(base.copy_with(ColorSchemeOverrides::default()), base);
    }
}
