//! [`ColorScheme`] — the full Material 3 color-role palette.
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
//! ## Deprecated-but-included roles
//!
//! `background`/`on_background`/`surface_variant` are deprecated in the M3
//! spec (superseded by `surface`/`on_surface`/`surface_container_highest`) but
//! are kept here as normal fields: the M3 baseline tables still populate
//! them, and dropping them would silently break consumers that read them.

use flui_sdk::painting::Color;
use flui_sdk::platform::Brightness;

/// The full set of Material 3 color roles
/// (<https://m3.material.io/styles/color/the-color-system/color-roles>).
///
/// Construct one of the two M3 baseline schemes with
/// [`ColorScheme::light`] / [`ColorScheme::dark`], then adjust individual
/// roles with [`ColorScheme::copy_with`]. `#[non_exhaustive]`: the M3 role
/// list has grown over time (most recently the `*Fixed`/`*FixedDim` roles),
/// so construction always goes through a named constructor rather than a
/// struct literal.
///
/// 50 fields: 49 color roles plus `brightness`, including the three
/// deprecated roles (see module docs).
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

    /// The background color behind scrollable content.
    ///
    /// **Deprecated** in the M3 spec in favor of `surface`; kept because the
    /// baseline tables still populate it.
    pub background: Color,

    /// Content drawn on [`background`](Self::background).
    ///
    /// **Deprecated** in the M3 spec in favor of `on_surface`; kept for the
    /// same reason as `background`.
    pub on_background: Color,

    /// A surface variant used for lower-emphasis surfaces.
    ///
    /// **Deprecated** in the M3 spec in favor of `surface_container_highest`;
    /// kept for the same reason as `background`.
    pub surface_variant: Color,
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
            background: Color::from_argb(0xFFFE_F7FF), // background
            on_background: Color::from_argb(0xFF1D_1B20), // onBackground
            surface_variant: Color::from_argb(0xFFE7_E0EC), // surfaceVariant
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
            background: Color::from_argb(0xFF14_1218), // background
            on_background: Color::from_argb(0xFFE6_E0E9), // onBackground
            surface_variant: Color::from_argb(0xFF49_454F), // surfaceVariant
        }
    }

    /// Return a copy of this scheme with the given roles replaced.
    ///
    /// Build the patch with [`ColorSchemeOverrides::default`] and
    /// struct-update syntax:
    ///
    /// ```
    /// use flui_material::{ColorScheme, ColorSchemeOverrides};
    ///
    /// let scheme = ColorScheme::light().copy_with(ColorSchemeOverrides {
    ///     primary: Some(flui_sdk::painting::Color::from_argb(0xFF00_66CC)),
    ///     ..Default::default()
    /// });
    /// assert_eq!(scheme.primary, flui_sdk::painting::Color::from_argb(0xFF00_66CC));
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
    /// The M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}

/// Patch for [`ColorScheme::copy_with`] — every field mirrors a
/// [`ColorScheme`] role, `None` meaning "leave unchanged".
///
/// Deliberately **not** `#[non_exhaustive]` (unlike [`ColorScheme`] itself):
/// `#[non_exhaustive]` blocks external-crate struct-literal construction
/// even via `..Default::default()` functional update, which is the only way
/// callers build this patch. A future role added to [`ColorScheme`] still
/// gets a matching field here additively, without needing that ceremony.
///
/// A struct rather than an optional-parameter list, because Rust has no
/// optional named parameters.
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
