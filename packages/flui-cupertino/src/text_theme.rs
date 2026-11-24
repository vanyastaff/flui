//! The iOS text-style roles and their default styles.
//!
//! Implementation notes: the chain's two leading entries, `"-apple-system"`
//! and `"system-ui"`, are deliberately not mapped to a generic family.
//! `flui_painting`'s family resolution walks the style's own family and then
//! each `font_family_fallback` entry in order, and a generic terminates the
//! walk, so mapping `"-apple-system"` would resolve to sans-serif on every
//! non-Apple host and `"Segoe UI"` would never be reached. Left as ordinary
//! names the host does not carry, they fall through and the rest of the chain
//! does its job.
//!
//! Each default role's constant cites, by line, its counterpart in
//! `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/text_theme.dart`.

use std::sync::LazyLock;

use flui_sdk::foundation::{TextScaleProfile, TextSizingIntent};
use flui_sdk::painting::styling::Color;
use flui_sdk::painting::typography::{FontFamilyName, FontWeight, SharedSlice, TextStyle};

use crate::colors::{ColorResolver, CupertinoColor, CupertinoColors};

/// Cupertino typography theme: the type-style roles a Cupertino widget tree
/// reads by name instead of hard-coding a `TextStyle`.
///
/// What an author writes. Each role left `None` takes its iOS default when
/// the theme resolves ([`crate::CupertinoTheme::of`]); a set role is used as
/// written. Widgets read the roles of the resolved
/// [`ResolvedCupertinoTextTheme`], never this type.
///
/// # Font family
///
/// The default styles name the families `'CupertinoSystemText'` and
/// `'CupertinoSystemDisplay'`, aliases conventionally resolved to the
/// platform's San Francisco font on iOS and macOS. FLUI has no engine-level
/// alias table and ships no bundled SF font (license), so the alias is the
/// primary family and a fallback chain follows. The match is metrics
/// parity, not pixel parity: sizes, weights and letter-spacing follow the iOS
/// type ramp, but off Apple platforms the glyphs come from whatever the host
/// provides.
///
/// # Colours
///
/// The default roles take the label colour (the general and title roles), the
/// inactive gray (the tab label) or the primary colour (the three action
/// roles), each resolved for the context's brightness and contrast. A set
/// role keeps its own colour: FLUI's `TextStyle::color` is a concrete
/// [`Color`], so it was already resolved when it was written (Flutter's
/// `resolveFrom` also resolves a dynamic colour inside a set style, which a
/// FLUI style cannot hold).
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/text_theme.dart:136`.
///
/// # Example
///
/// ```rust
/// use flui_cupertino::{CupertinoColors, CupertinoThemeData};
/// use flui_sdk::painting::typography::TextStyle;
///
/// let mut theme = CupertinoThemeData::default();
/// theme.text_theme.primary_color = Some(CupertinoColors::SYSTEM_RED.into());
/// theme.text_theme.picker_text_style = Some(TextStyle {
///     font_size: Some(19.0),
///     ..TextStyle::default()
/// });
/// assert_eq!(theme.text_theme.text_style, None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CupertinoTextThemeData {
    /// The colour of the default action roles ([`Self::action_text_style`],
    /// [`Self::action_small_text_style`], [`Self::nav_action_text_style`]).
    /// `None`: the theme's resolved
    /// [`primary_color`](crate::CupertinoThemeData::primary_color).
    pub primary_color: Option<CupertinoColor>,
    /// The style of general text content.
    pub text_style: Option<TextStyle>,
    /// The style of interactive text without a background (a large or
    /// medium `CupertinoButton`'s text).
    pub action_text_style: Option<TextStyle>,
    /// The style of interactive text in a small button.
    pub action_small_text_style: Option<TextStyle>,
    /// The style of tab labels.
    pub tab_label_text_style: Option<TextStyle>,
    /// The style of titles in standard navigation bars.
    pub nav_title_text_style: Option<TextStyle>,
    /// The style of large titles in sliver navigation bars.
    pub nav_large_title_text_style: Option<TextStyle>,
    /// The style of interactive text in navigation bars.
    pub nav_action_text_style: Option<TextStyle>,
    /// The style of pickers.
    pub picker_text_style: Option<TextStyle>,
    /// The style of date and time pickers.
    pub date_time_picker_text_style: Option<TextStyle>,
}

/// The text roles of a resolved theme: every role a concrete [`TextStyle`]
/// for the context the theme was resolved in.
///
/// Built once per resolution by [`crate::CupertinoThemeData::resolve`] and
/// read by field; a widget clones only the role it needs to own.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCupertinoTextTheme {
    /// The style of general text content.
    pub text_style: TextStyle,
    /// The style of interactive text without a background.
    pub action_text_style: TextStyle,
    /// The style of interactive text in a small button.
    pub action_small_text_style: TextStyle,
    /// The style of tab labels.
    pub tab_label_text_style: TextStyle,
    /// The style of titles in standard navigation bars.
    pub nav_title_text_style: TextStyle,
    /// The style of large titles in sliver navigation bars.
    pub nav_large_title_text_style: TextStyle,
    /// The style of interactive text in navigation bars.
    pub nav_action_text_style: TextStyle,
    /// The style of pickers.
    pub picker_text_style: TextStyle,
    /// The style of date and time pickers.
    pub date_time_picker_text_style: TextStyle,
}

impl CupertinoTextThemeData {
    /// Every role for `colors`, the default action colour being
    /// `theme_primary` unless [`Self::primary_color`] is set.
    pub(crate) fn resolve_with(
        &self,
        colors: ColorResolver,
        theme_primary: Color,
    ) -> ResolvedCupertinoTextTheme {
        let fonts: &DefaultFonts = &DEFAULT_FONTS;
        let label = colors.dynamic(CupertinoColors::LABEL);
        let primary = self
            .primary_color
            .map_or(theme_primary, |color| colors.resolve(color));
        let role = |authored: &Option<TextStyle>, default: &RoleDefault, color: Color| {
            authored
                .clone()
                .unwrap_or_else(|| default.style(fonts, color))
        };
        ResolvedCupertinoTextTheme {
            text_style: role(&self.text_style, &TEXT, label),
            action_text_style: role(&self.action_text_style, &ACTION, primary),
            action_small_text_style: role(&self.action_small_text_style, &ACTION_SMALL, primary),
            tab_label_text_style: role(
                &self.tab_label_text_style,
                &TAB_LABEL,
                colors.dynamic(CupertinoColors::INACTIVE_GRAY),
            ),
            nav_title_text_style: role(&self.nav_title_text_style, &MIDDLE_TITLE, label),
            nav_large_title_text_style: role(&self.nav_large_title_text_style, &LARGE_TITLE, label),
            // The navigation action delegates to the action style.
            nav_action_text_style: role(&self.nav_action_text_style, &ACTION, primary),
            picker_text_style: role(&self.picker_text_style, &PICKER, label),
            date_time_picker_text_style: role(
                &self.date_time_picker_text_style,
                &DATE_TIME_PICKER,
                label,
            ),
        }
    }
}

/// One default role's metrics; its colour comes from the resolution.
struct RoleDefault {
    profile: TextScaleProfile,
    family: Family,
    font_size: f64,
    font_weight: Option<FontWeight>,
    letter_spacing: f64,
}

/// Which San Francisco optical size a default role names.
#[derive(Clone, Copy)]
enum Family {
    Text,
    Display,
}

impl RoleDefault {
    fn style(&self, fonts: &DefaultFonts, color: Color) -> TextStyle {
        TextStyle {
            sizing: Some(TextSizingIntent::Profile(self.profile)),
            font_family: Some(match self.family {
                Family::Text => fonts.text.clone(),
                Family::Display => fonts.display.clone(),
            }),
            font_family_fallback: fonts.fallback.clone(),
            font_size: Some(self.font_size),
            font_weight: self.font_weight,
            letter_spacing: Some(self.letter_spacing),
            color: Some(color),
            ..TextStyle::default()
        }
    }
}

/// `_kDefaultTextStyle` (text_theme.dart:19).
const TEXT: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Body,
    family: Family::Text,
    font_size: 17.0,
    font_weight: None,
    letter_spacing: -0.41,
};

/// `_kDefaultActionTextStyle` (text_theme.dart:34).
const ACTION: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Body,
    family: Family::Text,
    font_size: 17.0,
    font_weight: None,
    letter_spacing: -0.41,
};

/// `_kDefaultActionSmallTextStyle` (text_theme.dart:49).
const ACTION_SMALL: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Callout,
    family: Family::Text,
    font_size: 15.0,
    font_weight: None,
    letter_spacing: -0.23,
};

/// `_kDefaultTabLabelTextStyle` (text_theme.dart:63).
const TAB_LABEL: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Caption2,
    family: Family::Text,
    font_size: 10.0,
    font_weight: Some(FontWeight::W500),
    letter_spacing: -0.24,
};

/// `_kDefaultMiddleTitleTextStyle` (text_theme.dart:72), the navigation
/// title.
const MIDDLE_TITLE: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Headline,
    family: Family::Text,
    font_size: 17.0,
    font_weight: Some(FontWeight::W600),
    letter_spacing: -0.41,
};

/// `_kDefaultLargeTitleTextStyle` (text_theme.dart:81).
const LARGE_TITLE: RoleDefault = RoleDefault {
    profile: TextScaleProfile::LargeTitle,
    family: Family::Display,
    font_size: 34.0,
    font_weight: Some(FontWeight::W700),
    letter_spacing: 0.38,
};

/// `_kDefaultPickerTextStyle` (text_theme.dart:101).
const PICKER: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Title2,
    family: Family::Display,
    font_size: 21.0,
    font_weight: Some(FontWeight::W400),
    letter_spacing: -0.6,
};

/// `_kDefaultDateTimePickerTextStyle` (text_theme.dart:116).
const DATE_TIME_PICKER: RoleDefault = RoleDefault {
    profile: TextScaleProfile::Title2,
    family: Family::Display,
    font_size: 21.0,
    font_weight: Some(FontWeight::W400),
    letter_spacing: 0.4,
};

/// The family names every default role shares: one copy of each name and of
/// the fallback chain for the process, so a resolution clones reference
/// counts and allocates no name. Immutable data; a `static` only because a
/// `FontFamilyName` cannot be built in a `const`.
static DEFAULT_FONTS: LazyLock<DefaultFonts> = LazyLock::new(DefaultFonts::new);

/// The family names of the default roles.
struct DefaultFonts {
    text: FontFamilyName,
    display: FontFamilyName,
    fallback: SharedSlice<FontFamilyName>,
}

impl DefaultFonts {
    fn new() -> Self {
        Self {
            text: "CupertinoSystemText".into(),
            display: "CupertinoSystemDisplay".into(),
            // One chain for both optical sizes: the Text/Display split exists
            // only because San Francisco ships as two, and a fallback chain
            // has no such split to mirror. See the notes at the top of this
            // file for its two leading entries.
            fallback: [
                "-apple-system",
                "system-ui",
                "Segoe UI",
                "Helvetica Neue",
                "Arial",
                "sans-serif",
            ]
            .into(),
        }
    }
}
