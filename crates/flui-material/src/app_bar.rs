//! [`AppBar`] — a Material app bar: a leading/title/actions toolbar on a
//! [`Material`] surface.
//!
//! # Flutter parity
//!
//! `material/app_bar.dart`'s `AppBar` (oracle tag `3.44.0`). Implemented
//! subset: `leading`, `title`, `actions`, `toolbar_height`, `background_color`,
//! `foreground_color`, `elevation`, and the M3 token defaults
//! (`_AppBarDefaultsM3`, `app_bar.dart:2521-2570`): `background_color` falls
//! back to `ColorScheme.surface`, `foreground_color` to `ColorScheme.on_surface`,
//! `elevation` to `0.0`, and the title's text style to `TextTheme.title_large`
//! (recolored to the resolved foreground).
//!
//! ## The app bar consumes the top inset itself
//!
//! When `widget.primary` (`app_bar.dart:1189-1191`), the oracle wraps its
//! toolbar in `SafeArea(bottom: false, child: appBar)` — the app bar pads
//! itself against `MediaQuery.paddingOf(context).top`, rather than a parent
//! adding that padding on its behalf. This substrate does the same
//! unconditionally (no `primary` toggle yet — every `AppBar` behaves as
//! `primary: true`), via [`flui_widgets::SafeArea`]. A consequence, matching
//! the oracle: a standalone `AppBar` (mounted with no `Scaffold` at all, just
//! a `MediaQuery` ancestor) already reserves the status-bar inset on its own.
//!
//! ## `centerTitle`: a platform switch, narrowed
//!
//! `_getEffectiveCenterTitle` (`app_bar.dart:805-817`) is a `TargetPlatform`
//! switch: `false` on Android/Fuchsia/Linux/Windows, `true` on iOS/macOS with
//! fewer than two actions. FLUI's desktop targets are Linux and Win32 — both
//! land on the `false` branch — so this substrate always start-aligns the
//! title (no `center_title` override, no `NavigationToolbar`-style toggle
//! yet). **Named divergence**: real macOS parity (the `true` branch) waits
//! for a platform-adaptive seam; today every platform gets the
//! Android/Linux/Windows answer.
//!
//! ## Deferred, and named
//!
//! - `center_title` / a full `NavigationToolbar` port — the title area here
//!   is a plain `Expanded` + `Align(center_left)`, not `NavigationToolbar`'s
//!   overflow-aware middle-widget layout.
//! - `scrolledUnder` — no `ScrollNotification` substrate to observe yet.
//! - `flexibleSpace`, `bottom` (and therefore `PreferredSize`'s bottom-height
//!   contribution — [`AppBar::preferred_size`] reports `toolbar_height` only).
//! - Implied leading (a `DrawerButton`/`BackButton` synthesized when
//!   `leading` is unset) — arrives with the `IconButton`/`Navigator`-aware
//!   follow-up unit that also ships a real floating action button.

use flui_types::geometry::px;
use flui_types::styling::Color;
use flui_types::typography::TextStyle;
use flui_types::{Alignment, Size};
use flui_view::prelude::*;
use flui_widgets::{
    Align, CrossAxisAlignment, DefaultTextStyle, Expanded, IconTheme, IconThemeData,
    PreferredSizeView, Row, SafeArea, SizedBox,
};

use crate::material::Material;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The default toolbar height in logical pixels.
///
/// Flutter parity: `material/constants.dart`'s `kToolbarHeight` (oracle tag
/// `3.44.0`).
pub const DEFAULT_TOOLBAR_HEIGHT: f32 = 56.0;

/// A Material app bar: a `leading` / `title` / `actions` toolbar painted on a
/// [`Material`] surface, sized to [`toolbar_height`](Self::toolbar_height) and
/// self-padded against the top safe-area inset.
///
/// See the module docs for the implemented subset, the "consumes the top
/// inset itself" contract, and the deferred list.
///
/// # Examples
///
/// ```rust
/// use flui_material::AppBar;
/// use flui_widgets::Text;
///
/// let _bar = AppBar::new().title(Text::new("FLUI")).toolbar_height(64.0);
/// ```
#[derive(Clone, StatelessView)]
pub struct AppBar {
    leading: Option<BoxedView>,
    title: Option<BoxedView>,
    actions: Vec<BoxedView>,
    toolbar_height: f32,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f32>,
}

impl AppBar {
    /// An `AppBar` with no leading/title/actions, the default toolbar height
    /// ([`DEFAULT_TOOLBAR_HEIGHT`]), and every color/elevation left to the
    /// M3 theme defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            leading: None,
            title: None,
            actions: Vec::new(),
            toolbar_height: DEFAULT_TOOLBAR_HEIGHT,
            background_color: None,
            foreground_color: None,
            elevation: None,
        }
    }

    /// Sets the widget in the leading slot (before the title).
    #[must_use]
    pub fn leading(mut self, leading: impl IntoView) -> Self {
        self.leading = Some(leading.into_view().boxed());
        self
    }

    /// Sets the title widget, start-aligned in the space between `leading`
    /// and `actions` — see the module docs' `centerTitle` note.
    #[must_use]
    pub fn title(mut self, title: impl IntoView) -> Self {
        self.title = Some(title.into_view().boxed());
        self
    }

    /// Sets the trailing action widgets, laid out in a row after the title.
    #[must_use]
    pub fn actions(mut self, actions: Vec<BoxedView>) -> Self {
        self.actions = actions;
        self
    }

    /// Sets the toolbar's height. Defaults to [`DEFAULT_TOOLBAR_HEIGHT`].
    #[must_use]
    pub fn toolbar_height(mut self, toolbar_height: f32) -> Self {
        self.toolbar_height = toolbar_height;
        self
    }

    /// Overrides the surface color. Defaults to `ColorScheme.surface`.
    #[must_use]
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Overrides the icon/title color. Defaults to `ColorScheme.on_surface`.
    #[must_use]
    pub fn foreground_color(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    /// Overrides the `Material` elevation. Defaults to `0.0`.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }
}

impl Default for AppBar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AppBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBar")
            .field("has_leading", &self.leading.is_some())
            .field("has_title", &self.title.is_some())
            .field("action_count", &self.actions.len())
            .field("toolbar_height", &self.toolbar_height)
            .finish_non_exhaustive()
    }
}

/// [`AppBar`]'s theme-resolved colors and title style — `_AppBarDefaultsM3`
/// (`app_bar.dart:2521-2570`, oracle tag `3.44.0`) applied to the caller's
/// overrides, then coalesced. Factored out of [`AppBar::build`] so the
/// resolution itself (a pure function of a [`ThemeData`] and the three
/// override fields) is directly unit-testable without mounting a widget
/// tree — see this module's tests.
struct ResolvedAppBarStyle {
    background_color: Color,
    foreground_color: Color,
    elevation: f32,
    title_style: TextStyle,
}

/// Resolve `AppBar`'s M3 defaults: `background_color` falls back to
/// `ColorScheme.surface`, `foreground_color` to `ColorScheme.on_surface`,
/// `elevation` to `0.0`, and the title style to `TextTheme.title_large`
/// recolored to the resolved foreground.
fn resolve_style(
    theme: &ThemeData,
    background_color: Option<Color>,
    foreground_color: Option<Color>,
    elevation: Option<f32>,
) -> ResolvedAppBarStyle {
    let background_color = background_color.unwrap_or(theme.color_scheme.surface);
    let foreground_color = foreground_color.unwrap_or(theme.color_scheme.on_surface);
    let elevation = elevation.unwrap_or(0.0);
    let title_style = theme
        .text_theme
        .title_large
        .clone()
        .unwrap_or_default()
        .with_color(foreground_color);

    ResolvedAppBarStyle {
        background_color,
        foreground_color,
        elevation,
        title_style,
    }
}

impl StatelessView for AppBar {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let ResolvedAppBarStyle {
            background_color,
            foreground_color,
            elevation,
            title_style,
        } = resolve_style(
            &theme,
            self.background_color,
            self.foreground_color,
            self.elevation,
        );

        let mut toolbar_children: Vec<BoxedView> = Vec::new();
        if let Some(leading) = &self.leading {
            toolbar_children.push(leading.clone());
        }
        if let Some(title) = &self.title {
            // Always start-aligned — see the module docs' `centerTitle` note.
            toolbar_children.push(
                Expanded::new(Align::new(Alignment::CENTER_LEFT).child(title.clone())).boxed(),
            );
        }
        if !self.actions.is_empty() {
            toolbar_children.push(Row::new(self.actions.clone()).boxed());
        }

        let toolbar = Row::new(toolbar_children).cross_axis_alignment(CrossAxisAlignment::Center);

        let themed_toolbar = IconTheme::new(
            IconThemeData {
                color: Some(foreground_color),
                ..IconThemeData::default()
            },
            DefaultTextStyle::new(
                title_style,
                SizedBox::height(self.toolbar_height).child(toolbar),
            ),
        );

        // The app bar pads itself against the top safe-area inset — see the
        // module docs' "consumes the top inset itself" section.
        let safe_toolbar = SafeArea::new().bottom(false).child(themed_toolbar);

        Material::new(background_color)
            .elevation(elevation)
            .child(safe_toolbar)
    }
}

impl PreferredSizeView for AppBar {
    fn preferred_size(&self) -> Size {
        // Flutter oracle: `Size.fromHeight(toolbarHeight)` (`app_bar.dart`'s
        // `_PreferredAppBarSize`, minus the `bottom` contribution — deferred,
        // see the module docs).
        Size::new(px(f32::INFINITY), px(self.toolbar_height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toolbar_height_matches_the_oracle_constant() {
        assert_eq!(DEFAULT_TOOLBAR_HEIGHT, 56.0);
    }

    #[test]
    fn preferred_size_reports_the_toolbar_height() {
        let bar = AppBar::new().toolbar_height(64.0);
        assert_eq!(bar.preferred_size().height, px(64.0));
    }

    #[test]
    fn preferred_size_defaults_to_the_default_toolbar_height() {
        let bar = AppBar::new();
        assert_eq!(bar.preferred_size().height, px(DEFAULT_TOOLBAR_HEIGHT));
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_token_table() {
        let theme = ThemeData::light();
        let resolved = resolve_style(&theme, None, None, None);

        assert_eq!(
            resolved.background_color, theme.color_scheme.surface,
            "background_color must fall back to ColorScheme.surface"
        );
        assert_eq!(
            resolved.foreground_color, theme.color_scheme.on_surface,
            "foreground_color must fall back to ColorScheme.on_surface"
        );
        assert_eq!(resolved.elevation, 0.0, "elevation must fall back to 0.0");
        assert_eq!(
            resolved.title_style,
            theme
                .text_theme
                .title_large
                .clone()
                .unwrap_or_default()
                .with_color(theme.color_scheme.on_surface),
            "the title style must be TextTheme.title_large recolored to the resolved foreground"
        );
    }

    #[test]
    fn resolve_style_honors_explicit_overrides() {
        let theme = ThemeData::light();
        let background_override = Color::rgb(1, 2, 3);
        let foreground_override = Color::rgb(4, 5, 6);
        let resolved = resolve_style(
            &theme,
            Some(background_override),
            Some(foreground_override),
            Some(8.0),
        );

        assert_eq!(resolved.background_color, background_override);
        assert_eq!(resolved.foreground_color, foreground_override);
        assert_eq!(resolved.elevation, 8.0);
        assert_eq!(resolved.title_style.color, Some(foreground_override));
    }

    #[test]
    fn builders_set_the_expected_fields() {
        let bar = AppBar::new()
            .leading(flui_widgets::SizedBox::shrink())
            .title(flui_widgets::SizedBox::shrink())
            .actions(vec![flui_widgets::SizedBox::shrink().boxed()])
            .background_color(Color::rgb(10, 20, 30))
            .foreground_color(Color::rgb(40, 50, 60))
            .elevation(4.0);

        assert!(bar.leading.is_some());
        assert!(bar.title.is_some());
        assert_eq!(bar.actions.len(), 1);
        assert_eq!(bar.background_color, Some(Color::rgb(10, 20, 30)));
        assert_eq!(bar.foreground_color, Some(Color::rgb(40, 50, 60)));
        assert_eq!(bar.elevation, Some(4.0));
    }
}
