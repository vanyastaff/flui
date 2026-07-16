//! [`TextTheme`] — the 15 M3 type-scale roles, and the color overlays
//! [`ThemeData`](crate::ThemeData) composes them with by default.
//!
//! Flutter parity: `material/text_theme.dart` `TextTheme`, and the
//! `blackMountainView`/`whiteMountainView` const tables in
//! `material/typography.dart` (oracle tag `3.44.0`).

use flui_types::styling::Color;
use flui_types::typography::TextStyle;

/// The 15 Material 3 type-scale roles, each independently overridable.
///
/// Flutter parity: `TextTheme` (`material/text_theme.dart`, oracle tag
/// `3.44.0`) — same 15 named roles, same nesting (`display_large` down to
/// `label_small`). Unset (`None`) roles mean "inherit from whatever this
/// theme is merged onto" — see [`TextTheme::merge`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextTheme {
    /// Flutter parity: `TextTheme.displayLarge`.
    pub display_large: Option<TextStyle>,
    /// Flutter parity: `TextTheme.displayMedium`.
    pub display_medium: Option<TextStyle>,
    /// Flutter parity: `TextTheme.displaySmall`.
    pub display_small: Option<TextStyle>,
    /// Flutter parity: `TextTheme.headlineLarge`.
    pub headline_large: Option<TextStyle>,
    /// Flutter parity: `TextTheme.headlineMedium`.
    pub headline_medium: Option<TextStyle>,
    /// Flutter parity: `TextTheme.headlineSmall`.
    pub headline_small: Option<TextStyle>,
    /// Flutter parity: `TextTheme.titleLarge`.
    pub title_large: Option<TextStyle>,
    /// Flutter parity: `TextTheme.titleMedium`.
    pub title_medium: Option<TextStyle>,
    /// Flutter parity: `TextTheme.titleSmall`.
    pub title_small: Option<TextStyle>,
    /// Flutter parity: `TextTheme.labelLarge`.
    pub label_large: Option<TextStyle>,
    /// Flutter parity: `TextTheme.labelMedium`.
    pub label_medium: Option<TextStyle>,
    /// Flutter parity: `TextTheme.labelSmall`.
    pub label_small: Option<TextStyle>,
    /// Flutter parity: `TextTheme.bodyLarge`.
    pub body_large: Option<TextStyle>,
    /// Flutter parity: `TextTheme.bodyMedium`.
    pub body_medium: Option<TextStyle>,
    /// Flutter parity: `TextTheme.bodySmall`.
    pub body_small: Option<TextStyle>,
}

/// Merge one role: mirrors Flutter's
/// `base?.merge(patch) ?? patch` (`TextTheme.merge`, `text_theme.dart`,
/// oracle tag `3.44.0`).
///
/// `flui_types::TextStyle::merge` already implements "each of `other`'s
/// non-`None` fields wins, field-wise, else keep `self`'s" — the same shape
/// as the oracle's own `TextStyle.merge`, minus Flutter's `inherit` flag
/// (`flui_types::TextStyle` carries no such flag, so there is nothing for it
/// to interact with). That is the one documented divergence this module
/// relies on: FLUI has no build-context-relative "inherit from ambient
/// DefaultTextStyle" concept baked into `TextStyle` itself, so merging here
/// is a pure, context-free field combination.
fn merge_role(base: Option<&TextStyle>, patch: Option<&TextStyle>) -> Option<TextStyle> {
    match (base, patch) {
        (Some(base), Some(patch)) => Some(base.merge(patch)),
        (Some(base), None) => Some(base.clone()),
        (None, patch) => patch.cloned(),
    }
}

/// Set `color` on a role if it is present, leaving `None` roles `None` —
/// mirrors the shape of Flutter's `TextStyle?.apply(color: ...)`.
fn recolor_role(role: Option<&TextStyle>, color: Color) -> Option<TextStyle> {
    role.map(|style| TextStyle {
        color: Some(color),
        ..style.clone()
    })
}

impl TextTheme {
    /// Merge `patch` onto `self`: for every role, `patch`'s non-`None` fields
    /// win field-wise (via [`TextStyle::merge`]); a role `patch` leaves
    /// entirely unset keeps `self`'s value unchanged.
    ///
    /// Flutter parity: `TextTheme.merge` (`material/text_theme.dart`, oracle
    /// tag `3.44.0`). See `merge_role`'s doc comment (this module, private) for
    /// the one documented
    /// divergence (no `inherit` flag).
    #[must_use]
    pub fn merge(&self, patch: &TextTheme) -> TextTheme {
        TextTheme {
            display_large: merge_role(self.display_large.as_ref(), patch.display_large.as_ref()),
            display_medium: merge_role(self.display_medium.as_ref(), patch.display_medium.as_ref()),
            display_small: merge_role(self.display_small.as_ref(), patch.display_small.as_ref()),
            headline_large: merge_role(self.headline_large.as_ref(), patch.headline_large.as_ref()),
            headline_medium: merge_role(
                self.headline_medium.as_ref(),
                patch.headline_medium.as_ref(),
            ),
            headline_small: merge_role(self.headline_small.as_ref(), patch.headline_small.as_ref()),
            title_large: merge_role(self.title_large.as_ref(), patch.title_large.as_ref()),
            title_medium: merge_role(self.title_medium.as_ref(), patch.title_medium.as_ref()),
            title_small: merge_role(self.title_small.as_ref(), patch.title_small.as_ref()),
            label_large: merge_role(self.label_large.as_ref(), patch.label_large.as_ref()),
            label_medium: merge_role(self.label_medium.as_ref(), patch.label_medium.as_ref()),
            label_small: merge_role(self.label_small.as_ref(), patch.label_small.as_ref()),
            body_large: merge_role(self.body_large.as_ref(), patch.body_large.as_ref()),
            body_medium: merge_role(self.body_medium.as_ref(), patch.body_medium.as_ref()),
            body_small: merge_role(self.body_small.as_ref(), patch.body_small.as_ref()),
        }
    }

    /// Set `color` uniformly on every role that is currently `Some`, leaving
    /// `None` roles `None`.
    ///
    /// Flutter's `TextTheme.apply` splits `displayColor` (applied to
    /// `display*`/`headline*`/`bodySmall`) from `bodyColor` (the rest) — see
    /// its doc comment on `text_theme.dart`, oracle tag `3.44.0`. This crate
    /// only calls `apply` from [`ThemeData`](crate::ThemeData)'s M3 default
    /// construction, where both colors are always the *same* value
    /// (`ColorScheme.onSurface` — see `Typography.material2021`'s
    /// `dark`/`light` locals in `typography.dart`, oracle tag `3.44.0`, which
    /// both reduce to `onSurface` regardless of brightness). This method is
    /// the honest simplification of that specific call site — a single
    /// uniform color — not a general port of `apply`'s `displayColor`/
    /// `bodyColor` split, which has no other caller yet.
    #[must_use]
    pub fn apply_color(&self, color: Color) -> TextTheme {
        TextTheme {
            display_large: recolor_role(self.display_large.as_ref(), color),
            display_medium: recolor_role(self.display_medium.as_ref(), color),
            display_small: recolor_role(self.display_small.as_ref(), color),
            headline_large: recolor_role(self.headline_large.as_ref(), color),
            headline_medium: recolor_role(self.headline_medium.as_ref(), color),
            headline_small: recolor_role(self.headline_small.as_ref(), color),
            title_large: recolor_role(self.title_large.as_ref(), color),
            title_medium: recolor_role(self.title_medium.as_ref(), color),
            title_small: recolor_role(self.title_small.as_ref(), color),
            label_large: recolor_role(self.label_large.as_ref(), color),
            label_medium: recolor_role(self.label_medium.as_ref(), color),
            label_small: recolor_role(self.label_small.as_ref(), color),
            body_large: recolor_role(self.body_large.as_ref(), color),
            body_medium: recolor_role(self.body_medium.as_ref(), color),
            body_small: recolor_role(self.body_small.as_ref(), color),
        }
    }

    /// All 15 roles, in the oracle's declared order, for iteration in tests
    /// and diagnostics.
    pub fn roles(&self) -> [Option<&TextStyle>; 15] {
        [
            self.display_large.as_ref(),
            self.display_medium.as_ref(),
            self.display_small.as_ref(),
            self.headline_large.as_ref(),
            self.headline_medium.as_ref(),
            self.headline_small.as_ref(),
            self.title_large.as_ref(),
            self.title_medium.as_ref(),
            self.title_small.as_ref(),
            self.label_large.as_ref(),
            self.label_medium.as_ref(),
            self.label_small.as_ref(),
            self.body_large.as_ref(),
            self.body_medium.as_ref(),
            self.body_small.as_ref(),
        ]
    }

    /// A color-only text theme with dark ("black") glyphs, for use on light
    /// surfaces — Roboto family, no geometry (`font_size`/`font_weight`/…
    /// left `None`).
    ///
    /// Flutter parity: `Typography.blackMountainView`
    /// (`material/typography.dart`, oracle tag `3.44.0`) — every
    /// `Colors.black*` value below is that const table's `color`, cited by
    /// name (`Colors.black54` = `0x8A000000`, `Colors.black87` = `0xDD000000`,
    /// `Colors.black` = `0xFF000000` — `material/colors.dart`, oracle tag
    /// `3.44.0`).
    #[must_use]
    pub fn black_mountain_view() -> TextTheme {
        let black54 = Color::from_argb(0x8A00_0000);
        let black87 = Color::from_argb(0xDD00_0000);
        let black = Color::from_argb(0xFF00_0000);
        mountain_view(black54, black87, black)
    }

    /// A color-only text theme with light ("white") glyphs, for use on dark
    /// surfaces — the [`black_mountain_view`](Self::black_mountain_view)
    /// counterpart.
    ///
    /// Flutter parity: `Typography.whiteMountainView`
    /// (`material/typography.dart`, oracle tag `3.44.0`) — `Colors.white70` =
    /// `0xB3FFFFFF`, `Colors.white` = `0xFFFFFFFF` (`material/colors.dart`,
    /// oracle tag `3.44.0`).
    #[must_use]
    pub fn white_mountain_view() -> TextTheme {
        let white70 = Color::from_argb(0xB3FF_FFFF);
        let white = Color::from_argb(0xFFFF_FFFF);
        // `whiteMountainView` uses only two distinct colors (no third,
        // `black`-only-styled tier) — pass `white70` as the "emphasis" color
        // too so `mountain_view`'s three-tier shape degenerates correctly.
        mountain_view(white70, white, white)
    }
}

/// Shared shape behind [`TextTheme::black_mountain_view`] /
/// [`TextTheme::white_mountain_view`]: both oracle tables assign each of the
/// 15 roles one of exactly three colors, in the same per-role pattern —
/// `low` for `display*`/`headline{Large,Medium}`/`bodySmall`, `mid` for
/// `headlineSmall`/`title{Large,Medium}`/`bodyLarge`/`bodyMedium`/
/// `labelLarge`, `high` for `titleSmall`/`labelMedium`/`labelSmall`.
fn mountain_view(low: Color, mid: Color, high: Color) -> TextTheme {
    let roboto = |color: Color| TextStyle {
        font_family: Some("Roboto".to_owned()),
        color: Some(color),
        ..TextStyle::default()
    };
    TextTheme {
        display_large: Some(roboto(low)),
        display_medium: Some(roboto(low)),
        display_small: Some(roboto(low)),
        headline_large: Some(roboto(low)),
        headline_medium: Some(roboto(low)),
        headline_small: Some(roboto(mid)),
        title_large: Some(roboto(mid)),
        title_medium: Some(roboto(mid)),
        title_small: Some(roboto(high)),
        label_large: Some(roboto(mid)),
        label_medium: Some(roboto(high)),
        label_small: Some(roboto(high)),
        body_large: Some(roboto(mid)),
        body_medium: Some(roboto(mid)),
        body_small: Some(roboto(low)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::english_like_2021;

    #[test]
    fn merge_prefers_patch_fields_and_keeps_base_when_patch_role_is_none() {
        let base = english_like_2021();
        let patch = TextTheme {
            display_large: Some(TextStyle {
                color: Some(Color::from_argb(0xFF11_2233)),
                ..TextStyle::default()
            }),
            ..TextTheme::default()
        };

        let merged = base.merge(&patch);

        let display_large = merged.display_large.expect("display_large is set");
        assert_eq!(display_large.color, Some(Color::from_argb(0xFF11_2233)));
        // Geometry from `base` must survive: `patch`'s displayLarge only set
        // color, and TextStyle::merge keeps a field when `other`'s is None.
        assert_eq!(display_large.font_size, Some(57.0));

        // A role `patch` never touched keeps `base`'s value unchanged.
        assert_eq!(merged.body_small, base.body_small);
    }

    #[test]
    fn merge_onto_empty_base_yields_the_patch() {
        let base = TextTheme::default();
        let patch = english_like_2021();
        assert_eq!(base.merge(&patch), patch);
    }

    #[test]
    fn apply_color_sets_every_present_role_and_skips_none() {
        let mut theme = english_like_2021();
        theme.headline_small = None;
        let sentinel = Color::from_argb(0xFF00_99FF);

        let recolored = theme.apply_color(sentinel);

        assert_eq!(recolored.headline_small, None);
        for style in recolored.roles().into_iter().flatten() {
            assert_eq!(style.color, Some(sentinel));
        }
    }

    /// Oracle citation: `Typography.blackMountainView`
    /// (`material/typography.dart`, oracle tag `3.44.0`).
    #[test]
    fn black_mountain_view_matches_oracle_color_tiers() {
        let theme = TextTheme::black_mountain_view();
        let black54 = Color::from_argb(0x8A00_0000);
        let black87 = Color::from_argb(0xDD00_0000);
        let black = Color::from_argb(0xFF00_0000);

        assert_eq!(theme.display_large.unwrap().color, Some(black54));
        assert_eq!(theme.headline_small.unwrap().color, Some(black87));
        assert_eq!(theme.title_small.unwrap().color, Some(black));
        assert_eq!(theme.body_small.unwrap().color, Some(black54));
        assert_eq!(theme.label_large.unwrap().color, Some(black87));
        assert_eq!(theme.label_medium.unwrap().color, Some(black));
    }

    /// Oracle citation: `Typography.whiteMountainView`
    /// (`material/typography.dart`, oracle tag `3.44.0`).
    #[test]
    fn white_mountain_view_matches_oracle_color_tiers() {
        let theme = TextTheme::white_mountain_view();
        let white70 = Color::from_argb(0xB3FF_FFFF);
        let white = Color::from_argb(0xFFFF_FFFF);

        assert_eq!(theme.display_large.unwrap().color, Some(white70));
        assert_eq!(theme.headline_small.unwrap().color, Some(white));
        assert_eq!(theme.body_small.unwrap().color, Some(white70));
        assert_eq!(theme.label_small.unwrap().color, Some(white));
    }
}
