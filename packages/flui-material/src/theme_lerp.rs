//! [`Lerp`] for the theme values, so a theme change can animate.
//!
//! Colours interpolate in premultiplied Oklab like every other colour;
//! text styles follow `Lerp for TextStyle`. A brightness, and each optional
//! component theme, switches half way: they are choices, not magnitudes.

use flui_sdk::geometry::Lerp;

use crate::color_scheme::ColorScheme;
use crate::text_theme::TextTheme;
use crate::theme_data::ThemeData;

/// Every colour role interpolates; the brightness switches half way.
macro_rules! lerp_roles {
    ($from:expr, $to:expr, $t:expr, $($role:ident),* $(,)?) => {
        ColorScheme {
            brightness: if $t < 0.5 { $from.brightness } else { $to.brightness },
            $($role: $from.$role.lerp_to(&$to.$role, $t),)*
        }
    };
}

impl Lerp for ColorScheme {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        lerp_roles!(
            self,
            other,
            t,
            primary,
            on_primary,
            primary_container,
            on_primary_container,
            primary_fixed,
            primary_fixed_dim,
            on_primary_fixed,
            on_primary_fixed_variant,
            secondary,
            on_secondary,
            secondary_container,
            on_secondary_container,
            secondary_fixed,
            secondary_fixed_dim,
            on_secondary_fixed,
            on_secondary_fixed_variant,
            tertiary,
            on_tertiary,
            tertiary_container,
            on_tertiary_container,
            tertiary_fixed,
            tertiary_fixed_dim,
            on_tertiary_fixed,
            on_tertiary_fixed_variant,
            error,
            on_error,
            error_container,
            on_error_container,
            surface,
            on_surface,
            surface_dim,
            surface_bright,
            surface_container_lowest,
            surface_container_low,
            surface_container,
            surface_container_high,
            surface_container_highest,
            on_surface_variant,
            outline,
            outline_variant,
            shadow,
            scrim,
            inverse_surface,
            on_inverse_surface,
            inverse_primary,
            surface_tint,
            background,
            on_background,
            surface_variant
        )
    }
}

/// A role set on both ends interpolates; one set on one end only switches
/// half way.
fn lerp_role(
    from: Option<&flui_sdk::painting::TextStyle>,
    to: Option<&flui_sdk::painting::TextStyle>,
    t: f64,
) -> Option<flui_sdk::painting::TextStyle> {
    match (from, to) {
        (Some(from), Some(to)) => Some(from.lerp_to(to, t)),
        _ if t < 0.5 => from.cloned(),
        _ => to.cloned(),
    }
}

/// Every type role interpolates.
macro_rules! lerp_text_roles {
    ($from:expr, $to:expr, $t:expr, $($role:ident),* $(,)?) => {
        TextTheme {
            $($role: lerp_role($from.$role.as_ref(), $to.$role.as_ref(), $t),)*
        }
    };
}

impl Lerp for TextTheme {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        lerp_text_roles!(
            self,
            other,
            t,
            display_large,
            display_medium,
            display_small,
            headline_large,
            headline_medium,
            headline_small,
            title_large,
            title_medium,
            title_small,
            label_large,
            label_medium,
            label_small,
            body_large,
            body_medium,
            body_small,
        )
    }
}

/// The colour scheme and text theme interpolate; every component theme
/// switches half way.
impl Lerp for ThemeData {
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Self {
            color_scheme: self.color_scheme.lerp_to(&other.color_scheme, t),
            text_theme: self.text_theme.lerp_to(&other.text_theme, t),
            ..if t < 0.5 { self.clone() } else { other.clone() }
        }
    }
}
