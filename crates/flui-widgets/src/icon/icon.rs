//! [`Icon`] — draws a single glyph from an icon font.

use flui_objects::RenderIcon;
use flui_painting::styling::Color;
use flui_painting::typography::{FontVariation, TextDirection, TextStyle};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, CreationSite, IntoView, RenderView, ViewExt, impl_render_view};

use crate::icon::symbol_effect::{ContentKey, SymbolEffectHost};
use crate::icon::{IconData, IconTheme, IconThemeData, SymbolEffect};
use crate::{Directionality, Semantics, Transform};
use flui_view::environment::MediaQuery;

/// A graphical icon drawn from a glyph in an icon font, described by an
/// [`IconData`].
///
/// `build` resolves the ambient
/// [`IconTheme`] and supplies authored inputs to a leaf [`RenderIcon`].
/// The box and glyph keep that logical size by default. With
/// [`IconThemeData::apply_text_scaling`] enabled, both use the nearest
/// [`MediaQuery`]'s text sizing policy once, during measurement.
///
/// # Glyphs
///
/// The codepoint reaches [`RenderIcon`], and the bounded
/// `size × size` box is exact and font-independent. The glyph comes from
/// [`IconData::font_family`]. With its default `bundled-fonts` feature,
/// `flui-painting` embeds the Material Icons (family `"Material Icons"`) and
/// Cupertino Icons (family `"CupertinoIcons"`) faces and installs each one
/// in every `FontCollection` (`flui_painting::fonts`), so those families'
/// codepoints shape to real glyphs in measurement and paint alike. Any other
/// icon font must be registered first with `flui::register_font`, which
/// loads it for measurement, paint and carets alike and lays laid-out text
/// out again on the next frame. Without a registration its codepoints shape
/// to tofu (the "missing glyph" box).
///
/// # Deferred
///
/// - Font weight, blend mode, and per-call shadow/text-direction
///   overrides are not supported yet.
#[derive(Clone, Debug, StatelessView)]
pub struct Icon {
    data: Option<IconData>,
    size: Option<f64>,
    color: Option<Color>,
    semantic_label: Option<String>,
    effect: Option<SymbolEffect>,
    #[view(site)]
    site: CreationSite,
}

impl Default for Icon {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::empty(CreationSite::here())
    }
}

impl Icon {
    fn empty(site: CreationSite) -> Self {
        Self {
            data: None,
            size: None,
            color: None,
            semantic_label: None,
            effect: None,
            site,
        }
    }

    /// An icon drawing the glyph described by `data`.
    #[must_use]
    #[track_caller]
    pub fn new(data: IconData) -> Self {
        Self {
            data: Some(data),
            ..Self::empty(CreationSite::here())
        }
    }

    /// An icon with no glyph: reserves an empty `size × size` square and
    /// draws nothing.
    #[must_use]
    #[track_caller]
    pub fn none() -> Self {
        Self::empty(CreationSite::here())
    }

    /// Override the icon's side length in logical pixels. Defaults to the
    /// ambient [`IconTheme`]'s size, or `24.0` with no ancestor theme.
    #[must_use]
    pub fn size(mut self, size: f64) -> Self {
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
    /// A labelled icon contributes image semantics, including when no glyph is
    /// set. An unlabelled icon is decorative and contributes no glyph label.
    #[must_use]
    pub fn semantic_label(mut self, semantic_label: impl Into<String>) -> Self {
        self.semantic_label = Some(semantic_label.into());
        self
    }

    /// Moves the icon with `effect`: a bounce, a pulse, a cross-fade when its glyph
    /// changes; see [`SymbolEffect`]. A glyph has no paths to draw on, so an icon takes no
    /// [`PathEffect`](super::PathEffect); [`AnimatedIcon`](super::AnimatedIcon) does.
    /// It takes the place of the ambient [`IconThemeData::symbol_effect`].
    #[must_use]
    pub fn symbol_effect(mut self, effect: SymbolEffect) -> Self {
        self.effect = Some(effect);
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
    /// Numeric growth is deferred to the render object; this retains the authored size.
    fn style_for(&self, icon: &IconData, size: f64, theme: &IconThemeData) -> TextStyle {
        let color = self.color.or(theme.color);
        // The theme's opacity scales the resolved colour's alpha; an icon
        // with no colour paints in the glyph default, black.
        let color = match theme.opacity.filter(|opacity| opacity.is_finite()) {
            Some(opacity) if opacity < 1.0 => {
                let color = color.unwrap_or(Color::BLACK);
                Some(color.with_opacity(f64::from(color.opacity()) * opacity.clamp(0.0, 1.0)))
            }
            _ => color,
        };
        TextStyle {
            color,
            font_size: Some(size),
            font_family: icon.font_family.clone(),
            font_family_fallback: icon.font_family_fallback.clone(),
            height: Some(1.0),
            font_variations: Self::font_variations(theme),
            shadows: theme.shadows.clone(),
            ..TextStyle::default()
        }
    }
}

impl StatelessView for Icon {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = IconTheme::of(ctx);
        let size = self.size.or(theme.size).unwrap_or(24.0);
        let icon = ResolvedIcon {
            size,
            glyph: self
                .data
                .as_ref()
                .and_then(|icon| char::from_u32(icon.code_point)),
            style: self.data.as_ref().map_or_else(TextStyle::default, |icon| {
                self.style_for(icon, size, &theme)
            }),
            sizing: if theme.apply_text_scaling.unwrap_or(false) {
                MediaQuery::text_sizing_of(ctx)
            } else {
                Some(flui_painting::TextSizing::fixed())
            },
            weight_adjustment: MediaQuery::font_weight_adjustment_of(ctx).unwrap_or(0),
        };
        let mut semantics = Semantics::new();
        if let Some(label) = self.semantic_label.as_ref() {
            semantics = semantics
                .label(label.clone())
                .role(crate::SemanticsRole::Image);
        }
        // `IconData::match_text_direction` mirrors the glyph horizontally under
        // right-to-left text (a "back" arrow): the glyph itself is directionless
        // (a `RenderIcon` text run), so the flip is a `Transform` composition over
        // it, about its centre — mirroring paint and hit-testing alike.
        let icon = if self.data.as_ref().is_some_and(|d| d.match_text_direction)
            && Directionality::of(ctx) == TextDirection::Rtl
        {
            Transform::scale(-1.0, 1.0).child(icon).boxed()
        } else {
            icon.boxed()
        };
        let built = semantics.child(icon).boxed();
        match self.effect.as_ref().or(theme.symbol_effect.as_ref()) {
            Some(effect) => {
                SymbolEffectHost::new(effect.clone(), ContentKey::Glyph(self.data.clone()), built)
                    .boxed()
            }
            None => built,
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedIcon {
    size: f64,
    glyph: Option<char>,
    style: TextStyle,
    sizing: Option<flui_painting::TextSizing>,
    weight_adjustment: i32,
}

impl RenderView for ResolvedIcon {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIcon;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderIcon {
        RenderIcon::new(self.size, self.glyph, self.style.clone())
            .with_text_sizing(self.sizing.clone())
            .with_font_weight_adjustment(self.weight_adjustment)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        object: &mut RenderIcon,
    ) -> flui_rendering::RenderUpdateImpact {
        object.update(
            self.size,
            self.glyph,
            self.style.clone(),
            self.sizing.clone(),
            self.weight_adjustment,
        )
    }
}

impl_render_view!(ResolvedIcon);
