//! The Material 3 (2021) English-like type scale.
//!
//! Every `font_size`/`font_weight`/`letter_spacing`/`height` value below is
//! the M3 spec's `englishLike` type scale, verbatim.
//!
//! ## Deferred: dense / tall script geometries
//!
//! The oracle also ships `Typography.dense2021` (CJK) and `Typography.tall2021`
//! (Farsi/Hindi/Thai/…) — alternate type-scale geometries selected by
//! `MaterialLocalizations.scriptCategory`. Both are identical in M3 (the oracle
//! comment on each says "the Material Design 3 specification does not include
//! a 'dense'/'tall' text theme, so this is just here to be consistent with the
//! API" — `typography.dart`, oracle tag `3.44.0`) and script-category
//! resolution has no consumer yet (no localizations resolve
//! `ScriptCategory`). Deferred until a localization consumer exists to pin the
//! API against, tracked alongside the crate's other named deferrals (see the
//! crate root docs).
//!
//! ## Deferred: color
//!
//! This module provides geometry only (`color: None` on every style) — same
//! contract as the oracle's `englishLike2021`. See [`crate::text_theme`] for
//! the black/white color overlays [`crate::ThemeData`]'s defaults compose
//! this geometry with.

use flui_sdk::foundation::{TextScaleProfile, TextSizingIntent};
use flui_sdk::painting::{FontWeight, TextStyle};

use crate::text_theme::TextTheme;

fn style(
    profile: TextScaleProfile,
    font_size: f64,
    font_weight: FontWeight,
    letter_spacing: f64,
    height: f64,
) -> TextStyle {
    TextStyle {
        sizing: Some(TextSizingIntent::Profile(profile)),
        font_size: Some(font_size),
        font_weight: Some(font_weight),
        letter_spacing: Some(letter_spacing),
        height: Some(height),
        ..TextStyle::default()
    }
}

/// The M3 2021 `englishLike` type scale — 15 roles, geometry only (no color).
///
/// Every geometry value below is the spec's
/// `fontSize`/`fontWeight`/`letterSpacing`/`height`, in its declared order.
/// Scaling profiles are FLUI's projection for an explicit sizing policy,
/// not part of the Material type scale. Profiles retain authored geometry;
/// equal font sizes can have distinct growth profiles.
#[must_use]
pub fn english_like_2021() -> TextTheme {
    use TextScaleProfile::{
        Body, Callout, Caption1, Caption2, Footnote, Headline, LargeTitle, Subheadline, Title1,
        Title2, Title3,
    };

    TextTheme {
        display_large: Some(style(LargeTitle, 57.0, FontWeight::W400, -0.25, 1.12)),
        display_medium: Some(style(LargeTitle, 45.0, FontWeight::W400, 0.0, 1.16)),
        display_small: Some(style(LargeTitle, 36.0, FontWeight::W400, 0.0, 1.22)),
        headline_large: Some(style(Title1, 32.0, FontWeight::W400, 0.0, 1.25)),
        headline_medium: Some(style(Title2, 28.0, FontWeight::W400, 0.0, 1.29)),
        headline_small: Some(style(Title3, 24.0, FontWeight::W400, 0.0, 1.33)),
        title_large: Some(style(Title3, 22.0, FontWeight::W400, 0.0, 1.27)),
        title_medium: Some(style(Headline, 16.0, FontWeight::W500, 0.15, 1.50)),
        title_small: Some(style(Subheadline, 14.0, FontWeight::W500, 0.1, 1.43)),
        label_large: Some(style(Callout, 14.0, FontWeight::W500, 0.1, 1.43)),
        label_medium: Some(style(Caption1, 12.0, FontWeight::W500, 0.5, 1.33)),
        label_small: Some(style(Caption2, 11.0, FontWeight::W500, 0.5, 1.45)),
        body_large: Some(style(Body, 16.0, FontWeight::W400, 0.5, 1.50)),
        body_medium: Some(style(Body, 14.0, FontWeight::W400, 0.25, 1.43)),
        body_small: Some(style(Footnote, 12.0, FontWeight::W400, 0.4, 1.33)),
    }
}
