//! A single icon glyph and its square share one admitted logical size.

use flui_foundation::geometry::{Offset, Size};
use flui_foundation::{
    Diagnosticable, Leaf, TextScaleProfile, TextSize, TextSizeRequest, TextSizingIntent,
};
use flui_painting::typography::{InlineSpan, TextDirection, TextSpan, TextStyle};
use flui_painting::{TextBaseline as PainterBaseline, TextPainter, TextResolvedSize, TextSizing};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::context::{
    BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx,
};
use flui_rendering::parent_data::BoxParentData;
use flui_rendering::traits::{RenderBox, TextBaseline};
use flui_rendering::{RenderResult, RenderUpdateImpact, TextCx};

/// A leaf icon whose authored side resolves during measurement, even without a glyph.
#[derive(Debug)]
pub struct RenderIcon {
    side: f64,
    glyph: Option<char>,
    style: TextStyle,
    sizing: Option<TextSizing>,
    weight_adjustment: i32,
    painter: TextPainter,
    glyph_offset: Offset,
    resolved_side: Option<TextResolvedSize>,
}

impl RenderIcon {
    /// Construct an icon with authored logical dimensions and optional glyph.
    ///
    /// `side` determines the glyph size, overriding `style.font_size`. The style's
    /// sizing intent selects its growth profile, or preserves the authored side
    /// when fixed. An absent intent uses the body profile. A zero side without
    /// a glyph remains empty and requires no text-sizing answer.
    pub fn new(side: f64, glyph: Option<char>, style: TextStyle) -> Self {
        Self {
            side,
            glyph,
            style,
            sizing: None,
            weight_adjustment: 0,
            painter: TextPainter::new()
                .with_text_direction(TextDirection::Ltr)
                .with_text_sizing(Some(TextSizing::fixed())),
            glyph_offset: Offset::ZERO,
            resolved_side: None,
        }
    }

    /// Select an explicit sizing authority, or inherit the presentation's policy.
    #[must_use]
    pub fn with_text_sizing(mut self, sizing: Option<TextSizing>) -> Self {
        self.sizing = sizing;
        self
    }

    /// Apply inherited weight preferences to the glyph independently of sizing.
    #[must_use]
    pub fn with_font_weight_adjustment(mut self, adjustment: i32) -> Self {
        self.weight_adjustment = adjustment;
        self
    }

    /// Replace authored inputs while retaining this mounted render object.
    pub fn update(
        &mut self,
        side: f64,
        glyph: Option<char>,
        style: TextStyle,
        sizing: Option<TextSizing>,
        weight_adjustment: i32,
    ) -> RenderUpdateImpact {
        let unchanged = self.side == side
            && self.glyph == glyph
            && self.style == style
            && self.sizing == sizing
            && self.weight_adjustment == weight_adjustment;
        self.sizing = sizing;
        if unchanged {
            return RenderUpdateImpact::NONE;
        }
        self.side = side;
        self.glyph = glyph;
        self.style = style;
        self.weight_adjustment = weight_adjustment;
        RenderUpdateImpact::LAYOUT
    }

    fn resolve_side(&self, text: &TextCx<'_>) -> RenderResult<Option<TextResolvedSize>> {
        if self.glyph.is_none() && self.side == 0.0 {
            return Ok(None);
        }
        let size = TextSize::new(self.side)
            .map_err(|_| flui_painting::TextLayoutError::InvalidFontSize { size: self.side })?;
        let profile = if let Some(TextSizingIntent::Profile(profile)) = self.style.sizing {
            profile
        } else {
            TextScaleProfile::Body
        };
        let fixed = TextSizing::fixed();
        let sizing = if self.style.sizing == Some(TextSizingIntent::Fixed) {
            Some(&fixed)
        } else {
            self.sizing.as_ref()
        };
        Ok(Some(
            text.resolve_size(TextSizeRequest { size, profile }, sizing)?,
        ))
    }

    fn glyph_span(&self, side: f64) -> Option<InlineSpan> {
        self.glyph.map(|glyph| {
            let mut style = self.style.clone();
            style.font_size = Some(side);
            TextSpan::styled(glyph.to_string(), style).into()
        })
    }

    fn glyph_painter(&self, side: f64) -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text_direction(TextDirection::Ltr)
            .with_text_sizing(Some(TextSizing::fixed()));
        painter.set_font_weight_adjustment(self.weight_adjustment);
        painter.set_text(self.glyph_span(side));
        painter
    }

    fn centered_offset(size: Size, glyph: Size) -> Offset {
        Offset::new(
            (size.width - glyph.width.min(size.width)) * 0.5,
            (size.height - glyph.height.min(size.height)) * 0.5,
        )
    }
}

impl Diagnosticable for RenderIcon {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add("side", self.side.to_string());
        if let Some(glyph) = self.glyph {
            properties.add("text", glyph.to_string());
        }
        properties.add("style", format!("{:?}", self.style));
    }
}

impl RenderBox for RenderIcon {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> RenderResult<Size> {
        // A failed current layout cannot expose geometry from an older policy.
        // Dry and intrinsic queries leave the committed painter untouched.
        self.painter.mark_needs_layout();
        self.glyph_offset = Offset::ZERO;
        self.resolved_side = None;
        let constraints = *ctx.constraints();
        let mut text = ctx.text()?;
        let resolved_side = self.resolve_side(&text)?;
        let side = resolved_side.as_ref().map_or(0.0, TextResolvedSize::value);
        let size = constraints.constrain(Size::new(side, side));
        self.painter.set_text(self.glyph_span(side));
        self.painter
            .set_font_weight_adjustment(self.weight_adjustment);
        if self.glyph.is_some() {
            text.measurement()
                .layout(&mut self.painter, 0.0, size.width)?;
            self.glyph_offset = Self::centered_offset(size, self.painter.size());
        }
        // The committed square also owns its answer when no glyph is painted.
        self.resolved_side = resolved_side;
        Ok(size)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> RenderResult<Size> {
        let side = self
            .resolve_side(&ctx.text()?)?
            .as_ref()
            .map_or(0.0, TextResolvedSize::value);
        Ok(constraints.constrain(Size::new(side, side)))
    }

    fn compute_min_intrinsic_width(
        &self,
        _height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(self
            .resolve_side(&ctx.text()?)?
            .as_ref()
            .map_or(0.0, TextResolvedSize::value))
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(self
            .resolve_side(&ctx.text()?)?
            .as_ref()
            .map_or(0.0, TextResolvedSize::value))
    }

    fn compute_min_intrinsic_height(
        &self,
        _width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(self
            .resolve_side(&ctx.text()?)?
            .as_ref()
            .map_or(0.0, TextResolvedSize::value))
    }

    fn compute_max_intrinsic_height(
        &self,
        _width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> RenderResult<f64> {
        Ok(self
            .resolve_side(&ctx.text()?)?
            .as_ref()
            .map_or(0.0, TextResolvedSize::value))
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> RenderResult<Option<f64>> {
        let mut text = ctx.text()?;
        let resolved_side = self.resolve_side(&text)?;
        let side = resolved_side.as_ref().map_or(0.0, TextResolvedSize::value);
        if self.glyph.is_none() {
            return Ok(None);
        }
        let size = constraints.constrain(Size::new(side, side));
        let painter = self.glyph_painter(side);
        let glyph_size = text.measurement().dry_size(&painter, 0.0, size.width)?;
        let offset = Self::centered_offset(size, glyph_size);
        Ok(text
            .measurement()
            .dry_baseline(&painter, 0.0, size.width, painter_baseline(baseline))?
            .map(|distance| distance + offset.dy))
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> RenderResult<Option<f64>> {
        Ok(self.painter.has_layout().then(|| {
            self.painter
                .compute_distance_to_actual_baseline(painter_baseline(baseline))
                + self.glyph_offset.dy
        }))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        if self.painter.has_layout() {
            self.painter.paint(ctx.canvas(), self.glyph_offset);
        }
    }
}

fn painter_baseline(baseline: TextBaseline) -> PainterBaseline {
    match baseline {
        TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
        TextBaseline::Ideographic => PainterBaseline::Ideographic,
    }
}
