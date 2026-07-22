//! [`CupertinoTextThemeData`] — the iOS text-style roles (`textStyle`,
//! `actionTextStyle`, `navTitleTextStyle`, …).
//!
//! Flutter parity: `cupertino/text_theme.dart` (oracle tag `3.44.0`).
//!
//! ## Font family: verbatim oracle strings, not real San Francisco metrics
//!
//! The oracle's default styles use the font-family names `'CupertinoSystemText'`
//! and `'CupertinoSystemDisplay'` — internal aliases the Flutter *engine*
//! resolves to the platform's San Francisco font on iOS/macOS. FLUI has no
//! engine-level alias table and ships no bundled SF font (license), so this
//! port keeps the oracle's exact family name (for citation fidelity — a
//! reader diffing against `text_theme.dart` sees the same string) and adds a
//! `font_family_fallback` chain of common system sans-serif fonts for
//! `cosmic-text` to actually resolve against. This is **metrics parity, not
//! pixel parity**: sizes/weights/letter-spacing match the oracle's tables
//! exactly, but the rendered glyphs come from whatever system font
//! `cosmic-text` finds, not San Francisco.

use flui_types::Color;
use flui_types::typography::{FontWeight, TextStyle};
use flui_view::prelude::BuildContext;

use crate::colors::{CupertinoColors, CupertinoDynamicColor};

/// `_kDefaultTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultActionTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultActionSmallTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultTabLabelTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultMiddleTitleTextStyle` (`text_theme.dart`, oracle tag `3.44.0`) —
/// the source for [`CupertinoTextThemeData::nav_title_text_style`].
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

/// `_kDefaultLargeTitleTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultPickerTextStyle` (`text_theme.dart`, oracle tag `3.44.0`).
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

/// `_kDefaultDateTimePickerTextStyle` (`text_theme.dart`, oracle tag
/// `3.44.0`).
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

/// Same fallback chain as [`system_text_fallback`] — the oracle's Text/
/// Display split exists only because San Francisco ships as two optical
/// sizes; a `cosmic-text` fallback has no such split to mirror.
fn system_display_fallback() -> Vec<String> {
    system_text_fallback()
}

/// Flutter parity: `_TextThemeDefaultsBuilder.actionTextStyle({Color?
/// primaryColor})` — the oracle's `primaryColor` is nullable there only
/// because the private builder can be called before the owning
/// `CupertinoTextThemeData` resolves its own (non-nullable, defaulted)
/// `primaryColor`; this port always has a concrete [`CupertinoDynamicColor`]
/// to read `.color` off, so this is a plain function rather than a method on
/// [`TextThemeDefaults`] (it reads none of that type's fields).
fn action_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    TextStyle {
        color: Some(primary_color.color),
        ..default_action_text_style()
    }
}

/// Flutter parity: `_TextThemeDefaultsBuilder.actionSmallTextStyle`.
fn action_small_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    TextStyle {
        color: Some(primary_color.color),
        ..default_action_small_text_style()
    }
}

/// Flutter parity: `navActionTextStyle` — the oracle defines it as a direct
/// delegate to `actionTextStyle`.
fn nav_action_text_style_for(primary_color: CupertinoDynamicColor) -> TextStyle {
    action_text_style_for(primary_color)
}

/// Sets `style.color` to `color`, matching the oracle's
/// `_applyLabelColor`'s `original.color == color ? original :
/// original.copyWith(color: color)` short-circuit (kept for parity with the
/// oracle's own no-op-avoidance, even though FLUI's `TextStyle` is cheap to
/// clone regardless).
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
/// colors that drive them. Flutter parity: `_TextThemeDefaultsBuilder`
/// (`text_theme.dart`, oracle tag `3.44.0`).
///
/// The oracle types `labelColor`/`inactiveGrayColor` as plain `Color` (able
/// to hold a `CupertinoDynamicColor` polymorphically); this port types them
/// as [`CupertinoDynamicColor`] directly, since they are always dynamic in
/// practice — see `colors.rs`'s module doc on why FLUI needs
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

    /// Flutter parity: `_TextThemeDefaultsBuilder.resolveFrom`.
    fn resolve_from(&self, ctx: &dyn BuildContext) -> Self {
        let resolved_label = self.label_color.resolve_from(ctx);
        let resolved_inactive_gray = self.inactive_gray_color.resolve_from(ctx);
        Self::new(
            CupertinoDynamicColor::with_brightness(resolved_label, resolved_label),
            CupertinoDynamicColor::with_brightness(resolved_inactive_gray, resolved_inactive_gray),
        )
    }
}

/// Cupertino typography theme: the type-style roles a Cupertino widget tree
/// reads by name instead of hard-coding a `TextStyle`.
///
/// Flutter parity: `CupertinoTextThemeData` (`cupertino/text_theme.dart`,
/// oracle tag `3.44.0`).
///
/// ## Read-time dynamic resolution
///
/// The label/action-family roles below are **not** pre-resolved: they embed
/// [`CupertinoColors::LABEL`]/[`CupertinoColors::ACTIVE_BLUE`]'s *unresolved
/// effective* color (the light-mode variant — Flutter parity: a
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
    /// The default text theme — Flutter parity: `CupertinoTextThemeData()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the color [`Self::action_text_style`]/[`Self::action_small_text_style`]/
    /// [`Self::nav_action_text_style`] derive from when not overridden.
    /// Defaults to [`CupertinoColors::SYSTEM_BLUE`]. Flutter parity:
    /// `CupertinoTextThemeData(primaryColor: ...)`.
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

    /// The style of general text content. Flutter parity: `textStyle`.
    #[must_use]
    pub fn text_style(&self) -> TextStyle {
        self.text_style
            .clone()
            .unwrap_or_else(|| self.defaults.text_style())
    }

    /// The style of interactive text without a background (e.g.
    /// `CupertinoButton`'s large/medium text). Flutter parity:
    /// `actionTextStyle`.
    #[must_use]
    pub fn action_text_style(&self) -> TextStyle {
        self.action_text_style
            .clone()
            .unwrap_or_else(|| action_text_style_for(self.primary_color))
    }

    /// The style of interactive text in a small button. Flutter parity:
    /// `actionSmallTextStyle`.
    #[must_use]
    pub fn action_small_text_style(&self) -> TextStyle {
        self.action_small_text_style
            .clone()
            .unwrap_or_else(|| action_small_text_style_for(self.primary_color))
    }

    /// The style of unselected tabs. Flutter parity: `tabLabelTextStyle`.
    #[must_use]
    pub fn tab_label_text_style(&self) -> TextStyle {
        self.tab_label_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.tab_label_text_style())
    }

    /// The style of titles in standard navigation bars. Flutter parity:
    /// `navTitleTextStyle`.
    #[must_use]
    pub fn nav_title_text_style(&self) -> TextStyle {
        self.nav_title_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.nav_title_text_style())
    }

    /// The style of large titles in sliver navigation bars. Flutter parity:
    /// `navLargeTitleTextStyle`.
    #[must_use]
    pub fn nav_large_title_text_style(&self) -> TextStyle {
        self.nav_large_title_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.nav_large_title_text_style())
    }

    /// The style of interactive text in navigation bars. Flutter parity:
    /// `navActionTextStyle`.
    #[must_use]
    pub fn nav_action_text_style(&self) -> TextStyle {
        self.nav_action_text_style
            .clone()
            .unwrap_or_else(|| nav_action_text_style_for(self.primary_color))
    }

    /// The style of pickers. Flutter parity: `pickerTextStyle`.
    #[must_use]
    pub fn picker_text_style(&self) -> TextStyle {
        self.picker_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.picker_text_style())
    }

    /// The style of date/time pickers. Flutter parity:
    /// `dateTimePickerTextStyle`.
    #[must_use]
    pub fn date_time_picker_text_style(&self) -> TextStyle {
        self.date_time_picker_text_style
            .clone()
            .unwrap_or_else(|| self.defaults.date_time_picker_text_style())
    }

    /// Returns a copy with every role's dynamic color resolved against
    /// `ctx` — see the type doc's "Read-time dynamic resolution" section.
    /// Flutter parity: `CupertinoTextThemeData.resolveFrom`.
    #[must_use]
    pub fn resolve_from(&self, ctx: &dyn BuildContext) -> Self {
        let resolved_primary = self.primary_color.resolve_from(ctx);
        Self {
            defaults: self.defaults.resolve_from(ctx),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_text_style_matches_the_oracle_const_table() {
        let style = CupertinoTextThemeData::default().text_style();
        assert_eq!(style.font_family.as_deref(), Some("CupertinoSystemText"));
        assert_eq!(style.font_size, Some(17.0));
        assert_eq!(style.letter_spacing, Some(-0.41));
        assert_eq!(style.color, Some(CupertinoColors::LABEL.color));
    }

    #[test]
    fn default_action_text_style_uses_active_blue_and_the_oracle_metrics() {
        let style = CupertinoTextThemeData::default().action_text_style();
        assert_eq!(style.font_size, Some(17.0));
        assert_eq!(style.letter_spacing, Some(-0.41));
        assert_eq!(style.color, Some(CupertinoColors::ACTIVE_BLUE.color));
    }

    #[test]
    fn action_small_text_style_matches_the_oracle_const_table() {
        let style = CupertinoTextThemeData::default().action_small_text_style();
        assert_eq!(style.font_size, Some(15.0));
        assert_eq!(style.letter_spacing, Some(-0.23));
        assert_eq!(style.color, Some(CupertinoColors::ACTIVE_BLUE.color));
    }

    #[test]
    fn nav_large_title_text_style_uses_the_display_family_and_oracle_metrics() {
        let style = CupertinoTextThemeData::default().nav_large_title_text_style();
        assert_eq!(style.font_family.as_deref(), Some("CupertinoSystemDisplay"));
        assert_eq!(style.font_size, Some(34.0));
        assert_eq!(style.font_weight, Some(FontWeight::W700));
        assert_eq!(style.letter_spacing, Some(0.38));
    }

    #[test]
    fn with_primary_color_changes_action_text_style_color_not_the_base_text_style() {
        let theme =
            CupertinoTextThemeData::default().with_primary_color(CupertinoColors::SYSTEM_RED);
        assert_eq!(
            theme.action_text_style().color,
            Some(CupertinoColors::SYSTEM_RED.color)
        );
        // The base `textStyle` role is label-driven, not primary-color-driven —
        // a mutation collapsing this distinction would make both reads agree.
        assert_eq!(theme.text_style().color, Some(CupertinoColors::LABEL.color));
    }

    #[test]
    fn nav_action_text_style_delegates_to_action_text_style() {
        let theme =
            CupertinoTextThemeData::default().with_primary_color(CupertinoColors::SYSTEM_GREY);
        assert_eq!(theme.nav_action_text_style(), theme.action_text_style());
    }

    #[test]
    fn explicit_override_wins_over_the_default() {
        let overridden = TextStyle {
            font_size: Some(99.0),
            ..TextStyle::default()
        };
        let theme = CupertinoTextThemeData::default().with_text_style(overridden.clone());
        assert_eq!(theme.text_style(), overridden);
    }
}
