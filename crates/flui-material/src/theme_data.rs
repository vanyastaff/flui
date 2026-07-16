//! [`ThemeData`] — the value [`crate::Theme`] publishes to a subtree.
//!
//! Flutter parity: `material/theme_data.dart` `ThemeData` (oracle tag
//! `3.44.0`).

use flui_types::EdgeInsets;
use flui_types::platform::Brightness;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;

use crate::button_style::ButtonStyle;
use crate::color_scheme::ColorScheme;
use crate::shape::MaterialShape;
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
///   (`theme_data.dart`, oracle tag `3.44.0`), i.e. `geometry.merge(color)`.
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

/// Overrides [`ElevatedButton`](crate::ElevatedButton)'s default
/// [`ButtonStyle`], resolved between the widget's own `style` and
/// `ElevatedButton::default_style` — see
/// `crate::button_style_button`'s resolve-then-coalesce docs for how this
/// slot's `style` participates in the three-tier cascade.
///
/// Flutter parity: `ElevatedButtonThemeData` (`material/elevated_button_theme.dart`,
/// oracle tag `3.44.0`), which carries the identical single `style` field.
/// **Named reduction**: the oracle also has a standalone `ElevatedButtonTheme`
/// `InheritedTheme` widget (so a subtree can override the style without
/// touching the whole [`ThemeData`]); FLUI V1 has no per-widget
/// `InheritedTheme` wrappers yet, so `ElevatedButton` reads only this
/// [`ThemeData`] slot via [`Theme::of`](crate::Theme::of).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElevatedButtonThemeData {
    /// Overrides for [`ElevatedButton`](crate::ElevatedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`FilledButton`](crate::FilledButton)'s default [`ButtonStyle`]
/// (both the plain and `tonal` variants share this one slot, matching the
/// oracle). Flutter parity: `FilledButtonThemeData`
/// (`material/filled_button_theme.dart`, oracle tag `3.44.0`) — same named
/// reduction as [`ElevatedButtonThemeData`] (no standalone `InheritedTheme`
/// wrapper yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilledButtonThemeData {
    /// Overrides for [`FilledButton`](crate::FilledButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`OutlinedButton`](crate::OutlinedButton)'s default
/// [`ButtonStyle`]. Flutter parity: `OutlinedButtonThemeData`
/// (`material/outlined_button_theme.dart`, oracle tag `3.44.0`) — same named
/// reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutlinedButtonThemeData {
    /// Overrides for [`OutlinedButton`](crate::OutlinedButton)'s default
    /// style. `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`TextButton`](crate::TextButton)'s default [`ButtonStyle`].
/// Flutter parity: `TextButtonThemeData` (`material/text_button_theme.dart`,
/// oracle tag `3.44.0`) — same named reduction as [`ElevatedButtonThemeData`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextButtonThemeData {
    /// Overrides for [`TextButton`](crate::TextButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`IconButton`](crate::IconButton)'s default [`ButtonStyle`].
/// Flutter parity: `IconButtonThemeData` (`material/icon_button_theme.dart`,
/// oracle tag `3.44.0`) — same named reduction as [`ElevatedButtonThemeData`].
/// Not to be confused with [`flui_widgets::IconThemeData`], which colors an
/// `Icon` child, not an `IconButton`'s own resolved `ButtonStyle`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconButtonThemeData {
    /// Overrides for [`IconButton`](crate::IconButton)'s default style.
    /// `None` means this theme doesn't override anything.
    pub style: Option<ButtonStyle>,
}

/// Overrides [`AppBar`](crate::AppBar)'s M3 token defaults, one field at a
/// time — an unset field here still falls through to `AppBar`'s own default
/// (see that type's `resolve_style`), it does not blank the whole slot.
///
/// Flutter parity: `AppBarThemeData` (`material/app_bar_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `AppBar` actually consumes:
/// [`background_color`](Self::background_color),
/// [`foreground_color`](Self::foreground_color),
/// [`elevation`](Self::elevation), [`title_text_style`](Self::title_text_style).
/// Named deferrals (no consumer in FLUI's `AppBar` yet, so a field here would
/// have nothing to reach): `scrolled_under_elevation`, `shadow_color`,
/// `surface_tint_color`, `shape`, `icon_theme`/`actions_icon_theme`,
/// `center_title`, `title_spacing`, `leading_width`, `toolbar_height`,
/// `toolbar_text_style`, `system_overlay_style`, `actions_padding`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppBarThemeData {
    /// Overrides [`AppBar`](crate::AppBar)'s default background color
    /// (`ColorScheme.surface`).
    pub background_color: Option<Color>,
    /// Overrides [`AppBar`](crate::AppBar)'s default icon/title color
    /// (`ColorScheme.on_surface`).
    pub foreground_color: Option<Color>,
    /// Overrides [`AppBar`](crate::AppBar)'s default elevation (`0.0`).
    pub elevation: Option<f32>,
    /// Overrides the title's text style verbatim — Flutter parity:
    /// `widget.titleTextStyle ?? appBarTheme.titleTextStyle ??
    /// defaults.titleTextStyle?.copyWith(color: foregroundColor)`
    /// (`app_bar.dart`, oracle tag `3.44.0`): unlike the default tier, a
    /// theme-supplied style is used as-is, not recolored to the resolved
    /// [`foreground_color`](Self::foreground_color).
    pub title_text_style: Option<TextStyle>,
}

/// Overrides [`Card`](crate::Card)'s `_CardDefaultsM3` token defaults, one
/// field at a time.
///
/// Flutter parity: `CardThemeData` (`material/card_theme.dart`, oracle tag
/// `3.44.0`), narrowed to the fields FLUI's `Card` actually consumes:
/// [`color`](Self::color), [`elevation`](Self::elevation),
/// [`shape`](Self::shape), [`margin`](Self::margin). Named deferrals (no
/// consumer in FLUI's `Card` yet): `shadow_color`, `surface_tint_color`,
/// `clip_behavior`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardThemeData {
    /// Overrides [`Card`](crate::Card)'s default fill color
    /// (`ColorScheme.surfaceContainerLow`).
    pub color: Option<Color>,
    /// Overrides [`Card`](crate::Card)'s default elevation (`1.0`).
    pub elevation: Option<f32>,
    /// Overrides [`Card`](crate::Card)'s default shape (a 12dp rounded
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides [`Card`](crate::Card)'s default outer margin
    /// (`EdgeInsets.all(4.0)`).
    pub margin: Option<EdgeInsets>,
}

/// Overrides [`Dialog`](crate::Dialog)/[`AlertDialog`](crate::AlertDialog)'s
/// `_DialogDefaultsM3` token defaults, one field at a time.
///
/// Flutter parity: `DialogThemeData` (`material/dialog_theme.dart`, oracle
/// tag `3.44.0`), narrowed to the fields FLUI's `Dialog`/`AlertDialog`
/// actually consume: [`background_color`](Self::background_color) (`Dialog`),
/// [`elevation`](Self::elevation) (`Dialog`), [`shape`](Self::shape)
/// (`Dialog`), [`title_text_style`](Self::title_text_style) (`AlertDialog`'s
/// title), [`content_text_style`](Self::content_text_style) (`AlertDialog`'s
/// content). Named deferrals (no consumer yet): `shadow_color`,
/// `surface_tint_color`, `alignment`, `icon_color`, `actions_padding`,
/// `barrier_color`, `inset_padding`, `clip_behavior`, `constraints` (the
/// oracle's own `Dialog.build` reads `constraints ?? dialogTheme.constraints
/// ?? const BoxConstraints(minWidth: 280.0)` — FLUI's `Dialog` has a widget
/// tier for this via [`Dialog::constraints`](crate::Dialog::constraints) but
/// no theme tier yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DialogThemeData {
    /// Overrides [`Dialog`](crate::Dialog)'s default background color
    /// (`ColorScheme.surfaceContainerHigh`).
    pub background_color: Option<Color>,
    /// Overrides [`Dialog`](crate::Dialog)'s default elevation (`6.0`).
    pub elevation: Option<f32>,
    /// Overrides [`Dialog`](crate::Dialog)'s default shape (a 28dp rounded
    /// rectangle).
    pub shape: Option<MaterialShape>,
    /// Overrides [`AlertDialog`](crate::AlertDialog)'s title text style
    /// (default: `TextTheme.headlineSmall`).
    pub title_text_style: Option<TextStyle>,
    /// Overrides [`AlertDialog`](crate::AlertDialog)'s content text style
    /// (default: `TextTheme.bodyMedium`).
    pub content_text_style: Option<TextStyle>,
}

/// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
/// `_FABDefaultsM3` token defaults, one field at a time.
///
/// Flutter parity: `FloatingActionButtonThemeData`
/// (`material/floating_action_button_theme.dart`, oracle tag `3.44.0`),
/// narrowed to the fields FLUI's `FloatingActionButton` actually consumes:
/// [`background_color`](Self::background_color),
/// [`foreground_color`](Self::foreground_color),
/// [`elevation`](Self::elevation) (the enabled/disabled tier — see
/// `floating_action_button.rs`'s `resolve_elevation`). Named deferrals (no
/// override surface in FLUI's `FloatingActionButton` yet, matching that
/// module's own deferred list): `focus_color`/`hover_color`/`splash_color`,
/// `focus_elevation`/`hover_elevation`/`disabled_elevation`/
/// `highlight_elevation`, `shape`, `enable_feedback`, `icon_size`, every
/// `*_size_constraints` field, the `extended*` fields, `mouse_cursor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FabThemeData {
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// default background color (`ColorScheme.primaryContainer`).
    pub background_color: Option<Color>,
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// default foreground/icon color (`ColorScheme.onPrimaryContainer`).
    pub foreground_color: Option<Color>,
    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s
    /// enabled-tier elevation (`6.0`) — the same value the `disabled` tier
    /// falls back to (see `resolve_elevation`'s doc comment); the
    /// `pressed`/`hovered`/`focused` tiers are unaffected, matching the
    /// oracle's own independent `highlightElevation`/`hoverElevation`/
    /// `focusElevation` fields (this crate exposes no override for those).
    pub elevation: Option<f32>,
}

/// Visual-style configuration provided to descendants by a
/// [`Theme`](crate::Theme) ancestor.
///
/// Two ready-made M3 baselines are available: [`ThemeData::light`] and
/// [`ThemeData::dark`]. `#[non_exhaustive]`: further component themes (input
/// decoration and any future widget's own slot) land as additional fields
/// alongside their owning widgets, not in this theming-foundation unit — see
/// the crate root docs' scope section.
///
/// Flutter parity: `ThemeData` (`material/theme_data.dart`, oracle tag
/// `3.44.0`) — implemented subset: [`color_scheme`](Self::color_scheme),
/// [`text_theme`](Self::text_theme), and the button-family/`AppBar`/`Card`/
/// `Dialog`/`FloatingActionButton` component-theme slots below (each
/// narrowed to its owning widget's actually-consumed fields — see that
/// slot's own doc comment). Deferred: `iconTheme`, `extensions`, `platform`,
/// `useMaterial3` (this crate is M3-only — there is no M2 mode to switch
/// away from), and every other widget's component theme (`inputDecorationTheme`
/// and friends) until that widget lands.
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

    /// Overrides [`ElevatedButton`](crate::ElevatedButton)'s default style.
    /// `None` (the default): no override, `ElevatedButton` uses its own
    /// M3 token table verbatim. Flutter parity:
    /// `ThemeData.elevatedButtonTheme`.
    pub elevated_button_theme: Option<ElevatedButtonThemeData>,

    /// Overrides [`FilledButton`](crate::FilledButton)'s default style.
    /// Flutter parity: `ThemeData.filledButtonTheme`.
    pub filled_button_theme: Option<FilledButtonThemeData>,

    /// Overrides [`OutlinedButton`](crate::OutlinedButton)'s default style.
    /// Flutter parity: `ThemeData.outlinedButtonTheme`.
    pub outlined_button_theme: Option<OutlinedButtonThemeData>,

    /// Overrides [`TextButton`](crate::TextButton)'s default style. Flutter
    /// parity: `ThemeData.textButtonTheme`.
    pub text_button_theme: Option<TextButtonThemeData>,

    /// Overrides [`IconButton`](crate::IconButton)'s default style. Flutter
    /// parity: `ThemeData.iconButtonTheme`.
    pub icon_button_theme: Option<IconButtonThemeData>,

    /// Overrides [`AppBar`](crate::AppBar)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.appBarTheme`.
    pub app_bar_theme: Option<AppBarThemeData>,

    /// Overrides [`Card`](crate::Card)'s M3 token defaults, per field.
    /// Flutter parity: `ThemeData.cardTheme`.
    pub card_theme: Option<CardThemeData>,

    /// Overrides [`Dialog`](crate::Dialog)/[`AlertDialog`](crate::AlertDialog)'s
    /// M3 token defaults, per field. Flutter parity: `ThemeData.dialogTheme`.
    pub dialog_theme: Option<DialogThemeData>,

    /// Overrides [`FloatingActionButton`](crate::FloatingActionButton)'s M3
    /// token defaults, per field. Flutter parity:
    /// `ThemeData.floatingActionButtonTheme`.
    pub floating_action_button_theme: Option<FabThemeData>,
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
            elevated_button_theme: None,
            filled_button_theme: None,
            outlined_button_theme: None,
            text_button_theme: None,
            icon_button_theme: None,
            app_bar_theme: None,
            card_theme: None,
            dialog_theme: None,
            floating_action_button_theme: None,
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
            elevated_button_theme: None,
            filled_button_theme: None,
            outlined_button_theme: None,
            text_button_theme: None,
            icon_button_theme: None,
            app_bar_theme: None,
            card_theme: None,
            dialog_theme: None,
            floating_action_button_theme: None,
        }
    }

    /// This theme's brightness, read from [`ColorScheme::brightness`].
    ///
    /// Flutter parity: `ThemeData.brightness => colorScheme.brightness`
    /// (`theme_data.dart`, oracle tag `3.44.0`).
    #[must_use]
    pub fn brightness(&self) -> Brightness {
        self.color_scheme.brightness
    }

    /// Return a copy of this theme with the given overrides applied.
    ///
    /// Build the patch with [`ThemeDataOverrides::default`] and struct-update
    /// syntax — mirrors [`ColorScheme::copy_with`]'s patch-struct shape, for
    /// the same reason: `ThemeData` is `#[non_exhaustive]`, and a positional
    /// `Option<T>`-per-field signature would have to change (breaking every
    /// caller) each time a component-theme slot is added. A patch struct
    /// absorbs new fields as `..Default::default()`-compatible additions
    /// instead.
    ///
    /// Flutter parity: `ThemeData.copyWith(colorScheme: ..., textTheme: ...)`
    /// (`theme_data.dart`, oracle tag `3.44.0`), narrowed to this crate's
    /// implemented subset and reshaped as a struct (Rust has no optional
    /// named parameters).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_material::{ThemeData, ThemeDataOverrides};
    ///
    /// let base = ThemeData::light();
    /// let patched = base.copy_with(ThemeDataOverrides {
    ///     color_scheme: Some(flui_material::ColorScheme::dark()),
    ///     ..Default::default()
    /// });
    /// assert_eq!(patched.color_scheme, flui_material::ColorScheme::dark());
    /// assert_eq!(patched.text_theme, base.text_theme);
    /// ```
    #[must_use]
    pub fn copy_with(&self, overrides: ThemeDataOverrides) -> Self {
        Self {
            color_scheme: overrides.color_scheme.unwrap_or(self.color_scheme),
            text_theme: overrides
                .text_theme
                .unwrap_or_else(|| self.text_theme.clone()),
            // Each component-theme slot is itself already `Option<_>` on
            // `ThemeData`, so — unlike `color_scheme`/`text_theme` above —
            // the override field mirrors that `Option<_>` shape directly
            // rather than doubling it: `Some(_)` replaces the whole slot,
            // `None` leaves whatever this theme already had (`Some` or
            // `None`) unchanged. See `ThemeDataOverrides`'s doc comment.
            elevated_button_theme: overrides
                .elevated_button_theme
                .or_else(|| self.elevated_button_theme.clone()),
            filled_button_theme: overrides
                .filled_button_theme
                .or_else(|| self.filled_button_theme.clone()),
            outlined_button_theme: overrides
                .outlined_button_theme
                .or_else(|| self.outlined_button_theme.clone()),
            text_button_theme: overrides
                .text_button_theme
                .or_else(|| self.text_button_theme.clone()),
            icon_button_theme: overrides
                .icon_button_theme
                .or_else(|| self.icon_button_theme.clone()),
            app_bar_theme: overrides
                .app_bar_theme
                .or_else(|| self.app_bar_theme.clone()),
            card_theme: overrides.card_theme.or_else(|| self.card_theme.clone()),
            dialog_theme: overrides.dialog_theme.or_else(|| self.dialog_theme.clone()),
            floating_action_button_theme: overrides
                .floating_action_button_theme
                .or_else(|| self.floating_action_button_theme.clone()),
        }
    }
}

/// Patch for [`ThemeData::copy_with`] — every field mirrors a [`ThemeData`]
/// field, `None` meaning "leave unchanged".
///
/// Deliberately **not** `#[non_exhaustive]` (unlike [`ThemeData`] itself):
/// `#[non_exhaustive]` blocks external-crate struct-literal construction
/// even via `..Default::default()` functional update, which is the only way
/// callers build this patch — see [`ColorSchemeOverrides`](crate::ColorSchemeOverrides)'s
/// doc comment for the same reasoning. Every future component-theme slot on
/// [`ThemeData`] gets a matching field here; adding one is still additive
/// for any caller already writing `..Default::default()`, without needing
/// the `#[non_exhaustive]` ceremony.
///
/// Flutter parity: the optional-parameter list of `ThemeData.copyWith`
/// (`theme_data.dart`, oracle tag `3.44.0`), reshaped as a struct.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeDataOverrides {
    /// Overrides [`ThemeData::color_scheme`].
    pub color_scheme: Option<ColorScheme>,
    /// Overrides [`ThemeData::text_theme`].
    pub text_theme: Option<TextTheme>,
    /// Replaces [`ThemeData::elevated_button_theme`] wholesale when `Some`;
    /// `None` leaves it unchanged (see [`ThemeData::copy_with`]'s doc
    /// comment on why this mirrors `Option<ElevatedButtonThemeData>`
    /// directly rather than doubling the `Option`).
    pub elevated_button_theme: Option<ElevatedButtonThemeData>,
    /// Replaces [`ThemeData::filled_button_theme`] wholesale when `Some`.
    pub filled_button_theme: Option<FilledButtonThemeData>,
    /// Replaces [`ThemeData::outlined_button_theme`] wholesale when `Some`.
    pub outlined_button_theme: Option<OutlinedButtonThemeData>,
    /// Replaces [`ThemeData::text_button_theme`] wholesale when `Some`.
    pub text_button_theme: Option<TextButtonThemeData>,
    /// Replaces [`ThemeData::icon_button_theme`] wholesale when `Some`.
    pub icon_button_theme: Option<IconButtonThemeData>,
    /// Replaces [`ThemeData::app_bar_theme`] wholesale when `Some`.
    pub app_bar_theme: Option<AppBarThemeData>,
    /// Replaces [`ThemeData::card_theme`] wholesale when `Some`.
    pub card_theme: Option<CardThemeData>,
    /// Replaces [`ThemeData::dialog_theme`] wholesale when `Some`.
    pub dialog_theme: Option<DialogThemeData>,
    /// Replaces [`ThemeData::floating_action_button_theme`] wholesale when
    /// `Some`.
    pub floating_action_button_theme: Option<FabThemeData>,
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

        let patched = base.copy_with(ThemeDataOverrides {
            color_scheme: Some(new_scheme),
            ..Default::default()
        });

        assert_eq!(patched.color_scheme, new_scheme);
        assert_eq!(patched.text_theme, base.text_theme);
    }

    #[test]
    fn copy_with_no_overrides_is_identity() {
        let base = ThemeData::dark();
        assert_eq!(base.copy_with(ThemeDataOverrides::default()), base);
    }

    #[test]
    fn light_and_dark_leave_every_component_theme_slot_unset() {
        for theme in [ThemeData::light(), ThemeData::dark()] {
            assert!(theme.elevated_button_theme.is_none());
            assert!(theme.filled_button_theme.is_none());
            assert!(theme.outlined_button_theme.is_none());
            assert!(theme.text_button_theme.is_none());
            assert!(theme.icon_button_theme.is_none());
            assert!(theme.app_bar_theme.is_none());
            assert!(theme.card_theme.is_none());
            assert!(theme.dialog_theme.is_none());
            assert!(theme.floating_action_button_theme.is_none());
        }
    }

    #[test]
    fn copy_with_sets_the_new_component_theme_slots() {
        let base = ThemeData::light();
        let elevated_button_theme = ElevatedButtonThemeData {
            style: Some(ButtonStyle {
                elevation: Some(flui_widgets::WidgetStateProperty::all(Some(42.0))),
                ..Default::default()
            }),
        };
        let app_bar_theme = AppBarThemeData {
            elevation: Some(9.0),
            ..Default::default()
        };

        let patched = base.copy_with(ThemeDataOverrides {
            elevated_button_theme: Some(elevated_button_theme.clone()),
            app_bar_theme: Some(app_bar_theme.clone()),
            ..Default::default()
        });

        assert_eq!(patched.elevated_button_theme, Some(elevated_button_theme));
        assert_eq!(patched.app_bar_theme, Some(app_bar_theme));
        // Untouched slots stay unset, mirroring the base theme.
        assert!(patched.card_theme.is_none());
    }

    /// `None` in the override leaves an ALREADY-SET slot alone — it must not
    /// be read as "clear this slot back to `None`" (see `copy_with`'s doc
    /// comment on why the override field mirrors `Option<T>` directly).
    #[test]
    fn copy_with_none_preserves_an_already_set_component_theme_slot() {
        let card_theme = CardThemeData {
            elevation: Some(3.0),
            ..Default::default()
        };
        let base = ThemeData::light().copy_with(ThemeDataOverrides {
            card_theme: Some(card_theme.clone()),
            ..Default::default()
        });

        let patched = base.copy_with(ThemeDataOverrides::default());
        assert_eq!(patched.card_theme, Some(card_theme));
    }
}
