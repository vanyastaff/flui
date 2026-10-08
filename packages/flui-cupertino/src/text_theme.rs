//! [`CupertinoTextThemeData`] — the iOS text-style roles (`textStyle`,
//! `actionTextStyle`, `navTitleTextStyle`, …).
//!
//! ## Font family: alias names, not real San Francisco metrics
//!
//! The default styles use the font-family names `'CupertinoSystemText'`
//! and `'CupertinoSystemDisplay'` — aliases conventionally resolved to the
//! platform's San Francisco font on iOS/macOS. FLUI has no engine-level alias
//! table and ships no bundled SF font (license), so the alias names are kept
//! as the primary family and a fallback chain follows. This is **metrics
//! parity, not pixel parity** — sizes, weights and letter-spacing follow the
//! iOS type ramp, but off Apple platforms the glyphs come from whatever the
//! host provides.
//!
//! What resolves that name off Apple platforms is `flui_painting`'s family
//! resolution: it walks the style's own family, then each entry of
//! `font_family_fallback` in order, and degrades to the sans-serif generic
//! only when the host carries none of them.
//!
//! The chain's two leading entries — `"-apple-system"` and `"system-ui"` —
//! are deliberately *not* mapped to a generic, and fall through as ordinary
//! names the host does not carry. Mapping them would be worse than the gap it
//! closes: a generic terminates the walk, so `"-apple-system"` would resolve
//! to sans-serif on every non-Apple host and `"Segoe UI"` would never be
//! reached — on Windows that means losing the actual platform UI font to
//! whatever `common_fallback()` happens to name first. Falling through is
//! what makes the rest of the chain do its job.

use flui_sdk::painting::Color;
use flui_sdk::painting::{FontWeight, TextStyle};
use flui_sdk::view::prelude::BuildContext;

use crate::colors::{CupertinoColors, CupertinoDynamicColor};

/// The default general-content text style.
fn default_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_string()),
        font_family_fallback: system_text_fallback(),
        font_size: Some(17.0),
        letter_spacing: Some(-0.41),
        color: Some(CupertinoColors::LABEL.color),
        ..TextStyle::default()
    }
}

/// The default style of interactive text without a background.
fn default_action_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_string()),
        font_family_fallback: system_text_fallback(),
        font_size: Some(17.0),
        letter_spacing: Some(-0.41),
        color: Some(CupertinoColors::ACTIVE_BLUE.color),
        ..TextStyle::default()
    }
}

/// The default style of interactive text in a small button.
fn default_action_small_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_string()),
        font_family_fallback: system_text_fallback(),
        font_size: Some(15.0),
        letter_spacing: Some(-0.23),
        color: Some(CupertinoColors::ACTIVE_BLUE.color),
        ..TextStyle::default()
    }
}

/// The default style of tab labels.
fn default_tab_label_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_string()),
        font_family_fallback: system_text_fallback(),
        font_size: Some(10.0),
        font_weight: Some(FontWeight::W500),
        letter_spacing: Some(-0.24),
        color: Some(CupertinoColors::INACTIVE_GRAY.color),
        ..TextStyle::default()
    }
}

/// The source for [`CupertinoTextThemeData::nav_title_text_style`].
fn default_middle_title_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemText".to_string()),
        font_family_fallback: system_text_fallback(),
        font_size: Some(17.0),
        font_weight: Some(FontWeight::W600),
        letter_spacing: Some(-0.41),
        color: Some(CupertinoColors::LABEL.color),
        ..TextStyle::default()
    }
}

/// The default style of large titles.
fn default_large_title_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemDisplay".to_string()),
        font_family_fallback: system_display_fallback(),
        font_size: Some(34.0),
        font_weight: Some(FontWeight::W700),
        letter_spacing: Some(0.38),
        color: Some(CupertinoColors::LABEL.color),
        ..TextStyle::default()
    }
}

/// The default style of pickers.
fn default_picker_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemDisplay".to_string()),
        font_family_fallback: system_display_fallback(),
        font_size: Some(21.0),
        font_weight: Some(FontWeight::W400),
        letter_spacing: Some(-0.6),
        color: Some(CupertinoColors::LABEL.color),
        ..TextStyle::default()
    }
}

/// The default style of date/time pickers.
fn default_date_time_picker_text_style() -> TextStyle {
    TextStyle {
        font_family: Some("CupertinoSystemDisplay".to_string()),
        font_family_fallback: system_display_fallback(),
        font_size: Some(21.0),
        letter_spacing: Some(0.4),
        font_weight: Some(FontWeight::W400),
        color: Some(CupertinoColors::LABEL.color),
        ..TextStyle::default()
    }
}

/// Named, documented fallback chain for `'CupertinoSystemText'` — see the
/// module doc's "Font family" section.
fn system_text_fallback() -> Vec<String> {
    [
        "-apple-system",
        "system-ui",
        "Segoe UI",
        "Helvetica Neue",
        "Arial",
        "sans-serif",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Same fallback chain as [`system_text_fallback`] — the Text/Display split
/// exists only because San Francisco ships as two optical sizes; a family
/// fallback chain has no such split to mirror.
fn system_display_fallback() -> Vec<String> {
    system_text_fallback()
}

/// The action style for `primary_color`. There is always a concrete
/// [`CupertinoDynamicColor`] to read `.color` off, so this is a plain function
/// rather than a method on [`TextThemeDefaults`] (it reads none of that
/// type's fields).
fn action_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    TextStyle {
        color: Some(primary_color.color),
        ..default_action_text_style()
    }
}

/// The small-button action style for `primary_color`.
fn action_small_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    TextStyle {
        color: Some(primary_color.color),
        ..default_action_small_text_style()
    }
}

/// The navigation-bar action style — a direct delegate to the action style.
fn nav_action_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    action_text_style_for(primary_color)
}

/// Sets `style.color` to `color`, returning `style` unchanged when it already
/// has that color (a no-op short-circuit, even though FLUI's `TextStyle` is
/// cheap to clone regardless).
fn apply_label_color(style: TextStyle, color: Color) -> TextStyle {
    if style.color == Some(color) {
        style
    } else {
        TextStyle {
            color: Some(color),
            ..style
        }
    }
}

/// The label-color-driven default styles, parameterized on the two dynamic
/// colors that drive them.
///
/// The label and inactive-gray colors are typed as [`CupertinoDynamicColor`]
/// directly, since they are always dynamic in practice — see `colors.rs`'s module doc on why FLUI needs
/// [`crate::colors::CupertinoColor`] only where a field can hold *either* a
/// concrete or dynamic color, not where it is always one or the other.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TextThemeDefaults {
    label_color: CupertinoDynamicColor,
    inactive_gray_color: CupertinoDynamicColor,
}

impl TextThemeDefaults {
    const fn new(
        label_color: CupertinoDynamicColor,
        inactive_gray_color: CupertinoDynamicColor,
    ) -> Self {
        Self {
            label_color,
            inactive_gray_color,
        }
    }

    fn text_style(&self) -> TextStyle {
        apply_label_color(default_text_style(), self.label_color.color)
    }

    fn tab_label_text_style(&self) -> TextStyle {
        apply_label_color(
            default_tab_label_text_style(),
            self.inactive_gray_color.color,
        )
    }

    fn nav_title_text_style(&self) -> TextStyle {
        apply_label_color(default_middle_title_text_style(), self.label_color.color)
    }

    fn nav_large_title_text_style(&self) -> TextStyle {
        apply_label_color(default_large_title_text_style(), self.label_color.color)
    }

    fn picker_text_style(&self) -> TextStyle {
        apply_label_color(default_picker_text_style(), self.label_color.color)
    }

    fn date_time_picker_text_style(&self) -> TextStyle {
        apply_label_color(
            default_date_time_picker_text_style(),
            self.label_color.color,
        )
    }

    /// Collapses both colors using the same policy as the containing theme.
    fn resolve_colors(&self, colors: &crate::colors::ColorResolver<'_>) -> Self {
        let resolved_label = colors.dynamic(self.label_color);
        let resolved_inactive_gray = colors.dynamic(self.inactive_gray_color);
        Self::new(
            CupertinoDynamicColor::with_brightness(resolved_label, resolved_label),
            CupertinoDynamicColor::with_brightness(resolved_inactive_gray, resolved_inactive_gray),
        )
    }
}

/// Cupertino typography theme: the type-style roles a Cupertino widget tree
/// reads by name instead of hard-coding a `TextStyle`.
///
/// ## Read-time dynamic resolution
///
/// The label/action-family roles below are **not** pre-resolved: they embed
/// [`CupertinoColors::LABEL`]/[`CupertinoColors::ACTIVE_BLUE`]'s *unresolved
/// effective* color (the light-mode variant — a
/// `CupertinoDynamicColor` defaults to its `color` field until resolved).
/// [`CupertinoTextThemeData::resolve_from`] produces a copy with those roles
/// collapsed to the color actually implied by the ambient context (dark mode
/// flips `label`/`inactiveGray`/`primaryColor` to their dark variants).
/// [`crate::theme::CupertinoTheme::of`] calls this before returning, so
/// ordinary consumers always see already-resolved styles — see that
/// function's doc.
///
/// Caller-supplied overrides (`with_text_style`, …) are **not** re-resolved:
/// FLUI's `TextStyle::color` is a concrete [`Color`], never a
/// [`CupertinoDynamicColor`] (see `colors.rs`'s module doc), so an override
/// the caller passed in was already concrete when it was set.
#[derive(Debug, Clone, PartialEq)]
pub struct CupertinoTextThemeData {
    defaults: TextThemeDefaults,
    primary_color: CupertinoDynamicColor,
    text_style: Option<TextStyle>,
    action_text_style: Option<TextStyle>,
    action_small_text_style: Option<TextStyle>,
    tab_label_text_style: Option<TextStyle>,
    nav_title_text_style: Option<TextStyle>,
    nav_large_title_text_style: Option<TextStyle>,
    nav_action_text_style: Option<TextStyle>,
    picker_text_style: Option<TextStyle>,
    date_time_picker_text_style: Option<TextStyle>,
}

impl Default for CupertinoTextThemeData {
    fn default() -> Self {
        Self {
            defaults: TextThemeDefaults::new(
                CupertinoColors::LABEL,
                CupertinoColors::INACTIVE_GRAY,
            ),
            primary_color: CupertinoColors::SYSTEM_BLUE,
            text_style: None,
            action_text_style: None,
            action_small_text_style: None,
            tab_label_text_style: None,
            nav_title_text_style: None,
            nav_large_title_text_style: None,
            nav_action_text_style: None,
            picker_text_style: None,
            date_time_picker_text_style: None,
        }
    }
}

impl CupertinoTextThemeData {
    /// The default text theme.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the color [`Self::action_text_style`]/[`Self::action_small_text_style`]/
    /// [`Self::nav_action_text_style`] derive from when not overridden.
    /// Defaults to [`CupertinoColors::SYSTEM_BLUE`].
    #[must_use]
    pub fn with_primary_color(mut self, primary_color: CupertinoDynamicColor) -> Self {
        self.primary_color = primary_color;
        self
    }

    /// Overrides [`Self::text_style`].
    #[must_use]
    pub fn with_text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Overrides [`Self::action_text_style`].
    #[must_use]
    pub fn with_action_text_style(mut self, style: TextStyle) -> Self {
        self.action_text_style = Some(style);
        self
    }

    /// Overrides [`Self::action_small_text_style`].
    #[must_use]
    pub fn with_action_small_text_style(mut self, style: TextStyle) -> Self {
        self.action_small_text_style = Some(style);
        self
    }

    /// Overrides [`Self::nav_title_text_style`].
    #[must_use]
    pub fn with_nav_title_text_style(mut self, style: TextStyle) -> Self {
        self.nav_title_text_style = Some(style);
        self
    }

    /// The style of general text content.
    #[must_use]
    pub fn text_style(&self) -> TextStyle {
        self.text_style
            .clone()
            .unwrap_or_else(|| self.defaults.text_style())
    }

    /// The style of interactive text without a background (e.g.
    /// `CupertinoButton`'s large/medium text).
    #[must_use]
    pub fn action_text_style(&self) -> TextStyle {
        self.action_text_style
            .clone()
            .unwrap_or_else(|| action_text_style_for(self.primary_color))
    }

    /// The style of interactive text in a small button.
    #[must_use]
    pub fn action_small_text_style(&self) -> TextStyle {
        self.action_small_text_style
            .clone()
            .unwrap_or_else(|| action_small_text_style_for(self.primary_color))
    }

    /// The style of unselected tabs.
    #[must_use]
    pub fn tab_label_text_style(&self) -> TextStyle {
        self.tab_label_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.tab_label_text_style())
    }

    /// The style of titles in standard navigation bars.
    #[must_use]
    pub fn nav_title_text_style(&self) -> TextStyle {
        self.nav_title_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.nav_title_text_style())
    }

    /// The style of large titles in sliver navigation bars.
    #[must_use]
    pub fn nav_large_title_text_style(&self) -> TextStyle {
        self.nav_large_title_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.nav_large_title_text_style())
    }

    /// The style of interactive text in navigation bars.
    #[must_use]
    pub fn nav_action_text_style(&self) -> TextStyle {
        self.nav_action_text_style
            .clone()
            .unwrap_or_else(|| nav_action_text_style_for(self.primary_color))
    }

    /// The style of pickers.
    #[must_use]
    pub fn picker_text_style(&self) -> TextStyle {
        self.picker_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.picker_text_style())
    }

    /// The style of date/time pickers.
    #[must_use]
    pub fn date_time_picker_text_style(&self) -> TextStyle {
        self.date_time_picker_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.date_time_picker_text_style())
    }

    /// Returns a copy with every role's dynamic color resolved against
    /// `ctx` — see the type doc's "Read-time dynamic resolution" section.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Self {
        self.resolve_colors(&crate::colors::ColorResolver::new(ctx, None))
    }

    pub(crate) fn resolve_colors(&self, colors: &crate::colors::ColorResolver<'_>) -> Self {
        let resolved_primary = colors.dynamic(self.primary_color);
        Self {
            defaults: self.defaults.resolve_colors(colors),
            primary_color: CupertinoDynamicColor::with_brightness(
                resolved_primary,
                resolved_primary,
            ),
            text_style: self.text_style.clone(),
            action_text_style: self.action_text_style.clone(),
            action_small_text_style: self.action_small_text_style.clone(),
            tab_label_text_style: self.tab_label_text_style.clone(),
            nav_title_text_style: self.nav_title_text_style.clone(),
            nav_large_title_text_style: self.nav_large_title_text_style.clone(),
            nav_action_text_style: self.nav_action_text_style.clone(),
            picker_text_style: self.picker_text_style.clone(),
            date_time_picker_text_style: self.date_time_picker_text_style.clone(),
        }
    }
}
