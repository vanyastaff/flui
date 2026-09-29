//! The Material 3 (2021) English-like type scale.
//!
//! Flutter parity: `_M3Typography.englishLike` (`material/typography.dart`,
//! oracle tag `3.44.0`), exposed there as `Typography.englishLike2021`. Every
//! `font_size`/`font_weight`/`letter_spacing`/`height` value below is copied
//! verbatim from that const table.
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

use flui_sdk::painting::{FontWeight, TextStyle};

use crate::text_theme::TextTheme;

fn style(font_size: f64, font_weight: FontWeight, letter_spacing: f64, height: f64) -> TextStyle {
    TextStyle {
        font_size: Some(font_size),
        font_weight: Some(font_weight),
        letter_spacing: Some(letter_spacing),
        height: Some(height),
        ..TextStyle::default()
    }
}

/// The M3 2021 `englishLike` type scale — 15 roles, geometry only (no color).
///
/// Flutter parity: `Typography.englishLike2021` (`material/typography.dart`,
/// oracle tag `3.44.0`), which is `_M3Typography.englishLike` — every value
/// below is that const table's `fontSize`/`fontWeight`/`letterSpacing`/
/// `height`, in the oracle's declared order.
#[must_use]
pub fn english_like_2021() -> TextTheme {
    TextTheme {
        display_large: Some(style(57.0, FontWeight::W400, -0.25, 1.12)),
        display_medium: Some(style(45.0, FontWeight::W400, 0.0, 1.16)),
        display_small: Some(style(36.0, FontWeight::W400, 0.0, 1.22)),
        headline_large: Some(style(32.0, FontWeight::W400, 0.0, 1.25)),
        headline_medium: Some(style(28.0, FontWeight::W400, 0.0, 1.29)),
        headline_small: Some(style(24.0, FontWeight::W400, 0.0, 1.33)),
        title_large: Some(style(22.0, FontWeight::W400, 0.0, 1.27)),
        title_medium: Some(style(16.0, FontWeight::W500, 0.15, 1.50)),
        title_small: Some(style(14.0, FontWeight::W500, 0.1, 1.43)),
        label_large: Some(style(14.0, FontWeight::W500, 0.1, 1.43)),
        label_medium: Some(style(12.0, FontWeight::W500, 0.5, 1.33)),
        label_small: Some(style(11.0, FontWeight::W500, 0.5, 1.45)),
        body_large: Some(style(16.0, FontWeight::W400, 0.5, 1.50)),
        body_medium: Some(style(14.0, FontWeight::W400, 0.25, 1.43)),
        body_small: Some(style(12.0, FontWeight::W400, 0.4, 1.33)),
    }
}
