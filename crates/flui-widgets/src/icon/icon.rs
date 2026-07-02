//! [`Icon`] — draws a single glyph from an icon font.

use flui_types::Color;
use flui_types::typography::{FontVariation, TextDirection, TextSpan, TextStyle};
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView};

use crate::icon::{IconData, IconTheme, IconThemeData};
use crate::layout::{Center, SizedBox};
use crate::text::RichText;

/// A graphical icon drawn from a glyph in an icon font, described by an
/// [`IconData`].
///
/// Flutter parity: `widgets/icon.dart` `Icon`. `build` resolves the ambient
/// [`IconTheme`], picks an effective size, and — when an icon is set —
/// composes `SizedBox::square(size) → Center → RichText(TextSpan(codepoint))`
/// exactly as `icon.dart:260-357` does.
///
/// # Glyph rendering is not yet faithful
///
/// This slice wires the codepoint all the way to a [`RichText`] /
/// `RenderParagraph`, and the bounded `size × size` box is exact and
/// font-independent. **It does not assert, and cannot yet guarantee, that the
/// codepoint shapes to a real icon glyph.** FLUI ships no bundled icon font,
/// and the two independent font-shaping systems (layout-time measurement and
/// render-time painting) have no public font-registration API today — an
/// icon-font codepoint shapes to tofu (the "missing glyph" box) until that
/// infrastructure lands. See `docs/research/2026-07-02-icon-widget-plan.md`
/// §"THE GAP" for the tracked follow-up (a font-registration ADR).
///
/// # Deferred from the oracle
///
/// - **`IconData::match_text_direction`** RTL mirroring: needs a `Transform`
///   composition step not wired into this build path yet.
/// - **Ambient `Directionality`**: FLUI has no `Directionality` inherited
///   widget yet, so `Icon` always renders left-to-right
///   ([`TextDirection::Ltr`]) rather than resolving one.
/// - **`Semantics`/`ExcludeSemantics`** wrapping (`semantic_label` is stored
///   but not yet surfaced to the accessibility tree).
/// - **`IconThemeData::opacity`** folding into the resolved color.
/// - `fontWeight`, `blendMode`, and per-call `shadows`/`textDirection`
///   overrides from the oracle's constructor are not ported in this slice.
#[derive(Clone, Debug, Default, StatelessView)]
pub struct Icon {
    data: Option<IconData>,
    size: Option<f32>,
    color: Option<Color>,
    semantic_label: Option<String>,
}

impl Icon {
    /// An icon drawing the glyph described by `data`.
    #[must_use]
    pub fn new(data: IconData) -> Self {
        Self {
            data: Some(data),
            ..Self::default()
        }
    }

    /// An icon with no glyph: reserves an empty `size × size` square and
    /// draws nothing.
    ///
    /// Flutter parity: `Icon(null)` — the `icon` constructor argument is
    /// nullable in the oracle.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Override the icon's side length in logical pixels. Defaults to the
    /// ambient [`IconTheme`]'s size, or `24.0` with no ancestor theme.
    #[must_use]
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    /// Override the icon's color. Defaults to the ambient [`IconTheme`]'s
    /// color, or opaque black with no ancestor theme.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the accessibility label announced for this icon.
    ///
    /// Stored on the widget even though the `Semantics` wrapper that would
    /// surface it to the accessibility tree is deferred in this slice (see
    /// the type docs).
    #[must_use]
    pub fn semantic_label(mut self, semantic_label: impl Into<String>) -> Self {
        self.semantic_label = Some(semantic_label.into());
        self
    }

    /// The font-variation axes this icon's [`TextStyle`] carries, built from
    /// the resolved theme's `fill`/`weight`/`grade`/`optical_size`.
    fn font_variations(theme: &IconThemeData) -> Vec<FontVariation> {
        [
            theme.fill.map(|fill| FontVariation::new("FILL", fill)),
            theme
                .weight
                .map(|weight| FontVariation::new("wght", weight)),
            theme.grade.map(|grade| FontVariation::new("GRAD", grade)),
            theme
                .optical_size
                .map(|optical_size| FontVariation::new("opsz", optical_size)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Build the [`TextStyle`] `icon`'s glyph paints with, at the resolved
    /// `size`, against the resolved ambient `theme`.
    ///
    /// Split out from `build` so the style-construction logic (`font_size`,
    /// `height`, color resolution, font-variation axes) is unit-testable
    /// without a live [`BuildContext`].
    ///
    /// Oracle: `icon.dart:305-319`.
    fn style_for(&self, icon: &IconData, size: f32, theme: &IconThemeData) -> TextStyle {
        TextStyle {
            color: self.color.or(theme.color),
            font_size: Some(f64::from(size)),
            font_family: icon.font_family.clone(),
            font_family_fallback: icon.font_family_fallback.clone(),
            height: Some(1.0),
            font_variations: Self::font_variations(theme),
            shadows: theme.shadows.clone().unwrap_or_default(),
            ..TextStyle::default()
        }
    }
}

impl StatelessView for Icon {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // Oracle: icon.dart:260-357.
        let theme = IconTheme::of(ctx);
        let size = self.size.or(theme.size).unwrap_or(24.0);

        // `icon.dart:285-289`: a null `icon` renders as empty `size × size`
        // space. Same shape for a codepoint that isn't a valid Unicode scalar
        // value (see `IconData::code_point_string`) — there is nothing to
        // shape either way.
        let Some(icon) = self.data.as_ref() else {
            return SizedBox::square(size);
        };
        let Some(code_point_string) = icon.code_point_string() else {
            return SizedBox::square(size);
        };

        let style = self.style_for(icon, size, &theme);

        // Ambient `Directionality` is not yet modelled (see type docs) — a
        // faithful port would resolve `Directionality::of(ctx)` here.
        let rich_text =
            RichText::new(TextSpan::styled(code_point_string, style)).direction(TextDirection::Ltr);

        SizedBox::square(size).child(Center::new().child(rich_text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_icon() -> IconData {
        IconData::new(0xE87D)
    }

    #[test]
    fn new_stores_the_icon_and_leaves_overrides_unset() {
        let icon = Icon::new(sample_icon());
        assert_eq!(icon.data, Some(sample_icon()));
        assert_eq!(icon.size, None);
        assert_eq!(icon.color, None);
        assert_eq!(icon.semantic_label, None);
    }

    #[test]
    fn none_has_no_icon() {
        assert_eq!(Icon::none().data, None);
    }

    #[test]
    fn builders_override_size_color_and_semantic_label() {
        let icon = Icon::new(sample_icon())
            .size(36.0)
            .color(Color::rgb(255, 0, 0))
            .semantic_label("close");
        assert_eq!(icon.size, Some(36.0));
        assert_eq!(icon.color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(icon.semantic_label.as_deref(), Some("close"));
    }

    #[test]
    fn font_variations_only_includes_axes_the_theme_set() {
        let theme = IconThemeData {
            fill: Some(0.5),
            grade: Some(-25.0),
            ..IconThemeData::default()
        };
        assert_eq!(
            Icon::font_variations(&theme),
            vec![
                FontVariation::new("FILL", 0.5),
                FontVariation::new("GRAD", -25.0),
            ]
        );
    }

    #[test]
    fn font_variations_is_empty_when_theme_sets_no_axis() {
        assert!(Icon::font_variations(&IconThemeData::default()).is_empty());
    }

    #[test]
    fn style_for_uses_the_resolved_size_for_font_size_and_pins_height_to_one() {
        let icon = Icon::new(sample_icon());
        let style = icon.style_for(&sample_icon(), 36.0, &IconThemeData::fallback());
        assert_eq!(style.font_size, Some(36.0));
        assert_eq!(style.height, Some(1.0));
    }

    #[test]
    fn style_for_prefers_the_icon_color_over_the_theme_color() {
        let red = Color::rgb(255, 0, 0);
        let icon = Icon::new(sample_icon()).color(red);
        let style = icon.style_for(&sample_icon(), 24.0, &IconThemeData::fallback());
        assert_eq!(style.color, Some(red));
    }

    #[test]
    fn style_for_falls_back_to_the_theme_color_when_unset() {
        let icon = Icon::new(sample_icon());
        let style = icon.style_for(&sample_icon(), 24.0, &IconThemeData::fallback());
        assert_eq!(style.color, IconThemeData::fallback().color);
    }

    #[test]
    fn style_for_carries_the_icon_font_family_and_fallback_list() {
        let icon_data = sample_icon().with_font_family("CustomIcons");
        let icon = Icon::new(icon_data.clone());
        let style = icon.style_for(&icon_data, 24.0, &IconThemeData::fallback());
        assert_eq!(style.font_family.as_deref(), Some("CustomIcons"));
        assert_eq!(style.font_family_fallback, icon_data.font_family_fallback);
    }
}
