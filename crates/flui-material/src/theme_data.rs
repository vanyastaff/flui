//! [`ThemeData`] — the value [`crate::Theme`] publishes to a subtree.
//!
//! Flutter parity: `material/theme_data.dart` `ThemeData` (oracle tag
//! `3.44.0`).

use flui_types::platform::Brightness;
use flui_types::styling::Color;

use crate::color_scheme::ColorScheme;
use crate::text_theme::TextTheme;
use crate::typography;

/// Compute the M3 default [`TextTheme`]: `englishLike2021` geometry overlaid
/// with a color-only theme uniformly recolored to `on_surface`.
///
/// Flutter parity, in two parts:
///
/// - The **merge direction** — geometry as the base, a color-only theme as
///   the patch — mirrors `Theme`'s build-time localization step:
///   `ThemeData.localize(baseTheme, localTextGeometry)` sets
///   `textTheme: localTextGeometry.merge(baseTheme.textTheme)`
///   (`theme_data.dart:1762`, oracle tag `3.44.0`), i.e. `geometry.merge(color)`.
/// - The **uniform recolor** mirrors `Typography.material2021`'s
///   `base.black.apply(displayColor: dark, bodyColor: dark, ...)` /
///   `base.white.apply(displayColor: light, bodyColor: light, ...)`
///   (`typography.dart`, oracle tag `3.44.0`), where the oracle's `dark` and
///   `light` locals both reduce to `colorScheme.onSurface` regardless of
///   brightness — so every role ends up the same color, `on_surface`, not
///   the `black54`/`black87`/`black` (or `white70`/`white`) tiers
///   [`TextTheme::black_mountain_view`]/[`TextTheme::white_mountain_view`]
///   themselves carry.
///
/// **Documented divergence**: the oracle recomputes this lazily, per
/// `Theme.of` read, keyed on the ambient locale's `ScriptCategory` (English
/// vs. dense/tall scripts — ADR: see [`crate::typography`] module docs on why
/// dense/tall are deferred). FLUI V1 has no script-category-resolving
/// localization consumer yet, so this bakes the `englishLike`-only default
/// once, here, at [`ThemeData::light`]/[`ThemeData::dark`] construction time
/// instead of on every `Theme::of` read.
fn default_text_theme(brightness: Brightness, on_surface: Color) -> TextTheme {
    let geometry = typography::english_like_2021();
    let color_theme = match brightness {
        Brightness::Dark => TextTheme::white_mountain_view(),
        Brightness::Light => TextTheme::black_mountain_view(),
    };
    geometry.merge(&color_theme.apply_color(on_surface))
}

/// Visual-style configuration provided to descendants by a
/// [`Theme`](crate::Theme) ancestor.
///
/// Two ready-made M3 baselines are available: [`ThemeData::light`] and
/// [`ThemeData::dark`]. `#[non_exhaustive]`: component themes (button, input
/// decoration, …) land as additional fields alongside their owning widgets,
/// not in this theming-foundation unit — see the crate root docs' scope
/// section.
///
/// Flutter parity: `ThemeData` (`material/theme_data.dart`, oracle tag
/// `3.44.0`) — implemented subset: [`color_scheme`](Self::color_scheme) and
/// [`text_theme`](Self::text_theme). Deferred: component theme slots,
/// `iconTheme`, `extensions`, `platform`, `useMaterial3` (this crate is
/// M3-only — there is no M2 mode to switch away from).
///
/// # Example
///
/// ```rust
/// use flui_material::ThemeData;
///
/// let dark = ThemeData::dark();
/// assert_eq!(dark.brightness(), dark.color_scheme.brightness);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeData {
    /// The Material 3 color roles this theme provides.
    ///
    /// Flutter parity: `ThemeData.colorScheme`.
    pub color_scheme: ColorScheme,

    /// The type-scale roles this theme provides.
    ///
    /// Flutter parity: `ThemeData.textTheme`.
    pub text_theme: TextTheme,
}

impl ThemeData {
    /// The M3 light baseline: [`ColorScheme::light`] plus its derived
    /// default [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn light() -> Self {
        let color_scheme = ColorScheme::light();
        let text_theme = default_text_theme(color_scheme.brightness, color_scheme.on_surface);
        Self {
            color_scheme,
            text_theme,
        }
    }

    /// The M3 dark baseline: [`ColorScheme::dark`] plus its derived default
    /// [`TextTheme`] (see `default_text_theme`, this module, private).
    #[must_use]
    pub fn dark() -> Self {
        let color_scheme = ColorScheme::dark();
        let text_theme = default_text_theme(color_scheme.brightness, color_scheme.on_surface);
        Self {
            color_scheme,
            text_theme,
        }
    }

    /// This theme's brightness, read from [`ColorScheme::brightness`].
    ///
    /// Flutter parity: `ThemeData.brightness =>
    /// colorScheme.brightness` (`theme_data.dart:911`, oracle tag `3.44.0`).
    #[must_use]
    pub fn brightness(&self) -> Brightness {
        self.color_scheme.brightness
    }

    /// Return a copy of this theme with the given fields replaced.
    ///
    /// Flutter parity: `ThemeData.copyWith(colorScheme: ..., textTheme: ...)`
    /// (`theme_data.dart`, oracle tag `3.44.0`), narrowed to this crate's
    /// implemented subset.
    #[must_use]
    pub fn copy_with(
        &self,
        color_scheme: Option<ColorScheme>,
        text_theme: Option<TextTheme>,
    ) -> Self {
        Self {
            color_scheme: color_scheme.unwrap_or(self.color_scheme),
            text_theme: text_theme.unwrap_or_else(|| self.text_theme.clone()),
        }
    }
}

impl Default for ThemeData {
    /// Same default as Flutter's `ThemeData()`: the M3 light baseline.
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_light() {
        assert_eq!(ThemeData::default(), ThemeData::light());
    }

    #[test]
    fn brightness_agrees_with_color_scheme() {
        assert_eq!(ThemeData::light().brightness(), Brightness::Light);
        assert_eq!(ThemeData::dark().brightness(), Brightness::Dark);
        assert_eq!(
            ThemeData::light().brightness(),
            ThemeData::light().color_scheme.brightness
        );
    }

    #[test]
    fn light_and_dark_are_distinct() {
        assert_ne!(ThemeData::light(), ThemeData::dark());
    }

    /// The default text theme's color is always `on_surface`, regardless of
    /// brightness — the M3 uniform-recolor behavior (see
    /// [`default_text_theme`]'s doc comment).
    #[test]
    fn default_text_theme_color_is_on_surface_for_both_brightnesses() {
        let light = ThemeData::light();
        let dark = ThemeData::dark();

        for role in light.text_theme.roles() {
            assert_eq!(
                role.and_then(|s| s.color),
                Some(light.color_scheme.on_surface)
            );
        }
        for role in dark.text_theme.roles() {
            assert_eq!(
                role.and_then(|s| s.color),
                Some(dark.color_scheme.on_surface)
            );
        }
    }

    /// The default text theme still carries `englishLike2021` geometry — the
    /// color overlay must not clobber font-size/weight/etc.
    #[test]
    fn default_text_theme_keeps_englishlike_geometry() {
        let theme = ThemeData::light();
        let body_medium = theme.text_theme.body_medium.expect("body_medium is set");
        assert_eq!(body_medium.font_size, Some(14.0));
        assert_eq!(
            body_medium.font_weight,
            Some(flui_types::typography::FontWeight::W400)
        );
    }

    #[test]
    fn copy_with_overrides_only_the_given_fields() {
        let base = ThemeData::light();
        let new_scheme = ColorScheme::dark();

        let patched = base.copy_with(Some(new_scheme), None);

        assert_eq!(patched.color_scheme, new_scheme);
        assert_eq!(patched.text_theme, base.text_theme);
    }

    #[test]
    fn copy_with_no_overrides_is_identity() {
        let base = ThemeData::dark();
        assert_eq!(base.copy_with(None, None), base);
    }
}
