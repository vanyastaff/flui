//! Explicit numeric preparation over caller-owned shaping resources.

use flui_foundation::geometry::Size;
use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest, TextSizingIntent};

use super::measure::{LineOverflow, ResolvedText, collect_authored_spans, shaping_widths};
use super::sizing::Policy;
use super::{DEFAULT_FONT_SIZE, TextBaseline, TextMeasurementError, TextPainter, TextSizing};
use crate::{TextContext, TextLayoutError};

/// One borrowed shaping context and an explicitly selected numeric authority.
/// The context's shared font resources never carry presentation preferences.
#[derive(Debug)]
pub struct TextMeasurement<'a> {
    context: &'a mut TextContext,
    sizing: &'a TextSizing,
}

impl<'a> TextMeasurement<'a> {
    /// Bind a measurement to its caller's resources and sizing authority.
    pub fn new(context: &'a mut TextContext, sizing: &'a TextSizing) -> Self {
        Self { context, sizing }
    }

    fn effective_sizing(&self, painter: &TextPainter) -> TextSizing {
        self.sizing.selected(painter.text_sizing.as_ref())
    }

    /// Commit the shaped geometry used by paint, caret and selection queries.
    pub fn layout(
        &mut self,
        painter: &mut TextPainter,
        min_width: f64,
        max_width: f64,
    ) -> Result<(), TextMeasurementError> {
        self.context.note_lent();
        if let Err(error) = shaping_widths(min_width, max_width) {
            painter.layout_cache = None;
            return Err(error.into());
        }
        let sizing = self.effective_sizing(painter);
        let fonts = TextPainter::font_key(self.context);
        if let Some(cache) = painter.layout_cache.as_ref()
            && cache.fonts.matches(&fonts)
            && cache.sizing.matches(&sizing)
            && (cache.min_width == min_width || (cache.min_width - min_width).abs() < f64::EPSILON)
            && (cache.max_width == max_width || (cache.max_width - max_width).abs() < f64::EPSILON)
        {
            sizing.retain_resolved(&cache.answers);
            return Ok(());
        }
        painter.layout_cache = None;
        let text = painter.text.as_ref().ok_or(TextLayoutError::TextNotSet)?;
        painter
            .text_direction
            .ok_or(TextLayoutError::TextDirectionNotSet)?;
        let resolved = prepare(text, &sizing)?;
        let (shape_min, shape_max) = shaping_widths(min_width, max_width)?;
        painter.commit_layout(
            self.context,
            resolved,
            min_width,
            max_width,
            (shape_min, shape_max),
            sizing,
        )?;
        Ok(())
    }

    fn widths(&mut self, painter: &TextPainter) -> Result<(f64, f64), TextMeasurementError> {
        self.context.note_lent();
        let sizing = self.effective_sizing(painter);
        let fonts = TextPainter::font_key(self.context);
        if let Some(cache) = painter.layout_cache.as_ref()
            && cache.fonts.matches(&fonts)
            && cache.sizing.matches(&sizing)
        {
            sizing.retain_resolved(&cache.answers);
            return Ok((cache.min_intrinsic_width, cache.max_intrinsic_width));
        }
        let Some(text) = painter.text.as_ref() else {
            return Ok((0.0, 0.0));
        };
        let resolved = prepare(text, &sizing)?;
        Ok(painter.intrinsic_widths(self.context, &resolved)?)
    }

    /// Widest unbreakable content, including a configured ellipsis floor.
    pub fn min_intrinsic_width(
        &mut self,
        painter: &TextPainter,
    ) -> Result<f64, TextMeasurementError> {
        Ok(self.widths(painter)?.0)
    }

    /// Unwrapped content width, including a configured ellipsis floor.
    pub fn max_intrinsic_width(
        &mut self,
        painter: &TextPainter,
    ) -> Result<f64, TextMeasurementError> {
        Ok(self.widths(painter)?.1)
    }

    fn metrics(
        &mut self,
        painter: &TextPainter,
        min_width: f64,
        max_width: f64,
    ) -> Result<Option<super::LayoutMetrics>, TextMeasurementError> {
        self.context.note_lent();
        let Some(text) = painter.text.as_ref() else {
            return Ok(None);
        };
        crate::text_layout::error::width(min_width)?;
        let resolved = prepare(text, &self.effective_sizing(painter))?;
        let (shape_min, shape_max) = shaping_widths(0.0, max_width)?;
        let result = painter
            .parley_paragraph(
                self.context,
                &resolved,
                shape_min,
                shape_max,
                LineOverflow::Enforce,
            )?
            .metrics();
        Ok(Some(TextPainter::metrics_from(&result, min_width)))
    }

    /// Both intrinsic heights use the same resolved paragraph at `width`.
    pub fn intrinsic_height(
        &mut self,
        painter: &TextPainter,
        width: f64,
    ) -> Result<f64, TextMeasurementError> {
        Ok(self
            .metrics(painter, 0.0, width)?
            .map_or(0.0, |m| m.size.height))
    }

    /// Measure without replacing committed geometry.
    pub fn dry_size(
        &mut self,
        painter: &TextPainter,
        min_width: f64,
        max_width: f64,
    ) -> Result<Size<f64>, TextMeasurementError> {
        Ok(self
            .metrics(painter, min_width, max_width)?
            .map_or(Size::ZERO, |m| m.size))
    }

    /// Read a shaped baseline without replacing committed geometry.
    pub fn dry_baseline(
        &mut self,
        painter: &TextPainter,
        min_width: f64,
        max_width: f64,
        baseline: TextBaseline,
    ) -> Result<Option<f64>, TextMeasurementError> {
        Ok(self
            .metrics(painter, min_width, max_width)?
            .map(|m| match baseline {
                TextBaseline::Alphabetic => m.alphabetic_baseline,
                TextBaseline::Ideographic => m.ideographic_baseline,
            }))
    }
}

fn prepare(
    text: &crate::typography::InlineSpan,
    sizing: &TextSizing,
) -> Result<ResolvedText, TextMeasurementError> {
    if let Policy::Linear(factor) = sizing.0
        && (!factor.is_finite() || factor <= 0.0)
    {
        return Err(TextLayoutError::InvalidScale { factor }.into());
    }
    let mut spans = collect_authored_spans(text);
    let mut default_style = text.style().cloned();
    // Validate/flatten all authored inputs before opening the one coherent read.
    let mut inputs = Vec::with_capacity(spans.len() + 1);
    inputs.push(authored_request(default_style.as_ref())?);
    for (_, style) in &spans {
        inputs.push(authored_request(style.as_ref())?);
    }
    let requests: Vec<_> = inputs
        .iter()
        .filter(|(_, intent)| *intent != TextSizingIntent::Fixed)
        .map(|(request, _)| *request)
        .collect();
    let answers = sizing.resolve_requests(&requests)?;
    // Resolution returns one ordered answer for every non-fixed request.
    // Fixed spans keep their authored sizes without entering native demand.
    let mut sizes: Vec<_> = inputs
        .iter()
        .map(|(request, _)| request.size.value())
        .collect();
    for ((_, size), answer) in inputs
        .iter()
        .zip(&mut sizes)
        .filter(|((_, intent), _)| *intent != TextSizingIntent::Fixed)
        .zip(&answers)
    {
        *size = answer.value();
    }
    apply_resolved_style(default_style.as_mut(), inputs[0], sizes[0], sizing);
    for ((_, style), (input, size)) in spans
        .iter_mut()
        .zip(inputs[1..].iter().copied().zip(sizes[1..].iter().copied()))
    {
        apply_resolved_style(style.as_mut(), input, size, sizing);
    }
    Ok(ResolvedText {
        spans,
        default_style,
        font_size: crate::text_layout::error::font_size(sizes[0])?,
        answers,
    })
}

fn authored_request(
    style: Option<&crate::typography::TextStyle>,
) -> Result<(TextSizeRequest, TextSizingIntent), TextLayoutError> {
    let authored = style.and_then(|s| s.font_size).unwrap_or(DEFAULT_FONT_SIZE);
    let size =
        TextSize::new(authored).map_err(|_| TextLayoutError::InvalidFontSize { size: authored })?;
    let intent = style
        .and_then(|s| s.sizing)
        .unwrap_or(TextSizingIntent::Profile(TextScaleProfile::Body));
    let profile = match intent {
        TextSizingIntent::Profile(profile) => profile,
        _ => TextScaleProfile::Body,
    };
    Ok((TextSizeRequest { size, profile }, intent))
}

fn apply_resolved_style(
    style: Option<&mut crate::typography::TextStyle>,
    (request, intent): (TextSizeRequest, TextSizingIntent),
    resolved: f64,
    sizing: &TextSizing,
) {
    if let Some(style) = style {
        style.font_size = Some(resolved);
        if let Some(spacing) = style.letter_spacing {
            let factor = match (&sizing.0, intent) {
                (_, TextSizingIntent::Fixed) | (Policy::Fixed, _) => 1.0,
                (Policy::Linear(factor), _) => *factor,
                (Policy::Exact(_), _) => resolved / request.size.value(),
            };
            style.letter_spacing = Some(spacing * factor);
        }
    }
}
