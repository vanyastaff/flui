//! [`Icon`] — draws a single glyph from an icon font.

use flui_objects::RenderIcon;
use flui_painting::styling::Color;
use flui_painting::typography::{FontVariation, TextStyle};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView, RenderView, impl_render_view};

use crate::icon::{IconData, IconTheme, IconThemeData};
use crate::{MediaQuery, Semantics};

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
/// - **`IconData::match_text_direction`** RTL mirroring: needs a `Transform`
///   composition step not wired into this build path yet.
/// - **Ambient `Directionality`**: `Icon` does not read
///   `Directionality::of` yet, so it always renders left-to-right
///   ([`flui_painting::typography::TextDirection::Ltr`]).
/// - **`IconThemeData::opacity`** folding into the resolved color.
/// - Font weight, blend mode, and per-call shadow/text-direction
///   overrides are not supported yet.
#[derive(Clone, Debug, Default, StatelessView)]
pub struct Icon {
    data: Option<IconData>,
    size: Option<f64>,
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
    #[must_use]
    pub fn none() -> Self {
        Self::default()
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
        TextStyle {
            color: self.color.or(theme.color),
            font_size: Some(size),
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
            semantics = semantics.label(label.clone()).image(true);
        }
        semantics.child(icon)
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
